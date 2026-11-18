//! What crosses from the browser into the engine's process while an acting
//! tab signs in and writes (decision 0254, audit finding M7), by the
//! message that carries it: the body of a write from inside the page in
//! no event, and a posted form, the cookies, and the token only in the
//! events named here, so that a new carrier fails this test.

use example_app::forgery::TOKEN_PREFIX;
use example_app::harness::{SESSION_COOKIE, USERNAME};

use crate::client::witness;
use crate::{Input, Primitive};

use super::*;

/// A password the application refuses, made up for this test alone, so
/// that no other test's messages hold it.
const TYPED: &str = "secret-for-tests-typed-into-the-page-5e71";

/// The body of a write from inside the page, made up for this test alone.
const NOTE: &str = "Quiet-note-of-a-write-7a3";

#[tokio::test(flavor = "multi_thread")]
async fn an_acting_tab_hears_no_body_of_a_write_and_its_cookies_and_token_only_where_named() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("quiet");
    let origins = origins(&app);
    let browser = stand.open(&profile("quiet")).await;
    let tab = must(browser.acting_tab(&environment(), &origins).await);
    let armed = witness::arm();

    // A sign-in through the application's own page, with a password it
    // refuses: the password goes into the page and is posted by its form.
    let sign_in = Address::new(&app.url("/secure/sign-in")).unwrap();
    let fill = |label: &'static str, value: &'static str| {
        let tab = &tab;
        async move {
            let field = must(
                tab.locate(
                    &Query::new(By::Label {
                        text: label.to_owned(),
                    }),
                    soon(),
                )
                .await,
            );
            must(
                tab.input(&field, &Input::Fill(value.to_owned()), soon())
                    .await,
            );
        }
    };
    must(tab.goto(&sign_in, soon()).await);
    fill("Username", USERNAME).await;
    fill("Password", TYPED).await;
    let button = must(tab.locate(&Query::role("button", "Sign in"), soon()).await);
    must(tab.input(&button, &Input::Click, soon()).await);
    let refused = Primitive::Text(
        Query::new(By::Css {
            selector: "#message".to_owned(),
        }),
        crate::TextMatch {
            text: "not right".to_owned(),
            contains: true,
        },
    );
    must(tab.wait(&refused, soon()).await);
    assert_eq!(app.auth_report().await.unwrap().failed_sign_ins, 1);

    // The right password: the application sets the session's cookie, and
    // the next page is asked for with it.
    must(tab.goto(&sign_in, soon()).await);
    fill("Username", USERNAME).await;
    fill("Password", example_app::harness::PASSWORD).await;
    let button = must(tab.locate(&Query::role("button", "Sign in"), soon()).await);
    must(tab.input(&button, &Input::Click, soon()).await);
    must(
        tab.wait(
            &Primitive::Visible(Query::role("heading", "Orders")),
            soon(),
        )
        .await,
    );
    must(
        tab.goto(&Address::new(&app.url("/secure/orders")).unwrap(), soon())
            .await,
    );

    // A write from inside the page, with its token in a header and in the
    // body, from the meta element and from the cookie the page sets.
    let page = Address::new(&app.url("/orders/A-100/note")).unwrap();
    for (from, to) in [
        (
            crate::TokenFrom::Meta("csrf-token".to_owned()),
            crate::TokenTo::Header("X-CSRF-Token".to_owned()),
        ),
        (
            crate::TokenFrom::Cookie("example_xsrf".to_owned()),
            crate::TokenTo::Field("authenticity_token".to_owned()),
        ),
    ] {
        must(tab.goto(&page, soon()).await);
        let answer = must(
            tab.write_from_page(&super::acting::note_write(NOTE, from, to), soon())
                .await,
        );
        assert_eq!(answer.status, 200, "{}", answer.body);
    }
    assert_eq!(app.stored_notes().await.unwrap().notes["A-100"], NOTE);

    // What crossed, by the message that carried it. The body of a write
    // the page's script sends: in no event, only in the helper's reply
    // with the application's answer, which quotes it. The body of a form
    // a document posts, the typed password in it: in the pause of that
    // document request, which the hold on the tab's origins needs, and in
    // the browser's event for that request, which the browser fills
    // whatever bound it is given. The token and the session's cookie: in
    // the cookies of requests and answers, and in a header a write sets,
    // which the network events carry, and which the acting tab needs on to
    // pass a service worker by; and in the one reply that hands the
    // session's state out, on purpose.
    let only = |needle: &str, allowed: &[&str]| {
        let heard = witness::holding(needle);
        let other: Vec<&String> = heard
            .iter()
            .filter(|method| !allowed.contains(&method.as_str()))
            .collect();
        assert!(other.is_empty(), "heard in {other:?}");
        heard
    };
    assert!(!only(NOTE, &["reply"]).is_empty());
    let form = ["Fetch.requestPaused", "Network.requestWillBeSent"];
    assert!(!only(TYPED, &form).is_empty());
    let carried = [
        "Fetch.requestPaused",
        "Network.requestWillBeSent",
        "Network.requestWillBeSentExtraInfo",
        "Network.responseReceivedExtraInfo",
    ];
    only(TOKEN_PREFIX, &carried);
    let cookie = must(browser.export_state(&origins, soon()).await)
        .cookies
        .into_iter()
        .find(|cookie| cookie.name == SESSION_COOKIE)
        .expect("the session cookie is held")
        .value;
    let mut with_reply = carried.to_vec();
    with_reply.push("reply");
    let heard = only(cookie.expose(), &with_reply);
    assert_eq!(
        heard.iter().filter(|method| *method == "reply").count(),
        1,
        "{heard:?}"
    );
    drop(armed);
    drop(tab);
    stand.end().await;
}
