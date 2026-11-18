//! A window a person signs in in (decision 0254): started afresh with a
//! screen, its first tab the engine's notice, the browser's own first
//! window closed, and a sign-in tab held to the origins it was given whose
//! helper looks only at the application's pages.
//!
//! On Linux the test starts a virtual display of its own. Where none can
//! be had it says so and passes, unless `A2M_REQUIRE_DESKTOP=1` says a
//! screen must be had: then it fails.

use crate::protocol::GetTargets;
use crate::test_support::{Screen, screen_required};
use crate::{Primitive, TextMatch};

use super::*;

/// A screen, or `None` when this machine has none and none is required.
fn screen() -> Option<Screen> {
    match Screen::start() {
        Ok(screen) => Some(screen),
        Err(why) if !screen_required() => {
            eprintln!("NOT RUN, no screen on this machine: {why}");
            None
        }
        Err(why) => panic!("no screen, and A2M_REQUIRE_DESKTOP=1 says one must be had: {why}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_window_shows_its_notice_and_holds_its_tab_to_the_origins_given() {
    let Some(screen) = screen() else {
        return;
    };
    let app = TestApp::start().await.unwrap();
    let stand = Stand::new("window", screen.settings(), &[]);
    let person = profile("person");
    let notice = "Sign in to example (test) for <the person>".to_owned();
    let browser = must(
        stand
            .browsers
            .open(
                &person,
                Visibility::Window {
                    notice: notice.clone(),
                },
            )
            .await,
    );
    assert!(browser.shows_window());

    // The application's origin is one the helper looks at; an identity
    // provider's, here the same application under another host name, is
    // one the tab may pass through and where nothing is looked at.
    let app_origins = Origins::of([&Address::new(&app.url("/")).unwrap()]);
    let elsewhere = app.url("/secure/sign-in").replace("127.0.0.1", "localhost");
    let passing = Origins::of([&Address::new(&elsewhere).unwrap()]);
    let tab = must(
        browser
            .sign_in_tab(&environment(), &app_origins, &passing)
            .await,
    );
    must(
        tab.goto(&Address::new(&app.url("/secure/sign-in")).unwrap(), soon())
            .await,
    );
    let heading = Primitive::Visible(Query::role("heading", "Sign in"));
    must(tab.wait(&heading, soon()).await);

    // Every page of the browser is the engine's, in its own context: the
    // notice and the sign-in tab. The browser's own first window is gone.
    let targets = browser
        .inner
        .client
        .call(None, &GetTargets {}, soon())
        .await
        .unwrap();
    let pages: Vec<_> = targets
        .target_infos
        .iter()
        .filter(|target| target.kind == "page")
        .collect();
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert!(
        pages
            .iter()
            .all(|page| page.browser_context_id.as_deref() == Some(browser.inner.context.as_str()))
    );

    // On the identity provider's origin the tab may be, and its helper
    // looks at nothing there.
    let seen = tab.changes();
    must(tab.goto(&Address::new(&elsewhere).unwrap(), soon()).await);
    must(tab.changed_since(seen, soon()).await);
    let looked = tab.wait(&heading, soon()).await.unwrap_err();
    assert!(
        matches!(looked, BrowserError::ForeignOrigin(_)),
        "{looked:?}"
    );

    // A move to an origin it was not given, which the page itself
    // starts, is ended in the browser, and the wait for the next change
    // hears the refusal.
    must(
        tab.goto(&Address::new(&app.url("/secure/sign-in")).unwrap(), soon())
            .await,
    );
    let _ = tab.blocked();
    let seen = tab.changes();
    let _ = tab
        .page
        .as_the_page(
            "location.href = 'http://elsewhere.invalid/sign-in'; 'moved'",
            soon(),
        )
        .await;
    must(tab.changed_since(seen, soon()).await);
    let blocked = tab.blocked();
    assert!(
        blocked.iter().any(|shape| shape.method == "NAVIGATION"),
        "{blocked:?}"
    );
    let text = TextMatch {
        text: "Sign in".to_owned(),
        contains: true,
    };
    must(
        tab.wait(
            &Primitive::Text(Query::role("heading", "Sign in"), text),
            soon(),
        )
        .await,
    );

    // A later open in the background closes the window and starts a
    // browser with none.
    drop(tab);
    drop(browser);
    let background = stand.open(&person).await;
    assert!(!background.shows_window());
    assert_eq!(stand.browsers.list().len(), 1);
    drop(background);
    stand.end().await;
    drop(screen);
}
