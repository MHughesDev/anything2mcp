//! Input, in-page requests, dialogs, and a browser that dies under a call.

use example_app::faults::{Fault, FaultKind};

use crate::{
    DialogAnswer, Input, Key, PageRequest, Primitive, RawValue, ReadWhat, Reason, TextMatch,
};

use super::*;

const ITEM: &str = "/api/orders/A-100/items/L-101";

#[tokio::test(flavor = "multi_thread")]
async fn a_form_is_filled_and_saved_and_the_stand_stored_exactly_one_write() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("acting");
    let browser = stand.open(&profile("clerk")).await;
    let tab = must(browser.acting_tab(&environment(), &origins(&app)).await);
    must(
        tab.goto(&Address::new(&app.url(FORM)).unwrap(), soon())
            .await,
    );

    // Three strategies, each one step: an edit, the native setter, keys.
    let description = must(tab.locate(&textbox("Description"), soon()).await);
    must(
        tab.input(&description, &Input::Fill("Blue rail".to_owned()), soon())
            .await,
    );
    let quantity = must(tab.locate(&textbox("Quantity"), soon()).await);
    must(
        tab.input(&quantity, &Input::NativeSet("7".to_owned()), soon())
            .await,
    );
    let length = must(tab.locate(&textbox("Length"), soon()).await);
    must(
        tab.input(&length, &Input::Type("12.5".to_owned()), soon())
            .await,
    );
    assert_eq!(
        must(tab.read(&length, &ReadWhat::Value, soon()).await),
        RawValue::Text("12.5".to_owned())
    );

    // A field that cannot be edited is refused before anything is sent.
    let code = must(tab.locate(&textbox("Item code"), soon()).await);
    let refused = tab
        .input(&code, &Input::Fill("L-999".to_owned()), soon())
        .await
        .unwrap_err();
    assert_eq!(refused, BrowserError::NotActionable(Reason::NotEditable));
    assert_eq!(refused.dispatch(), Dispatch::NotSent);
    assert!(app.writes().await.unwrap().is_empty());

    // Save, and wait for what the page says.
    let save = must(tab.locate(&Query::role("button", "Save"), soon()).await);
    must(tab.input(&save, &Input::Click, soon()).await);
    let saved = Primitive::Text(
        css("#status"),
        TextMatch {
            text: "Saved.".to_owned(),
            contains: false,
        },
    );
    must(tab.wait(&saved, soon()).await);

    // The stand says what it stored: one write, with the three values.
    let stored = app.stored_item("A-100", "L-101").await.unwrap().unwrap();
    assert_eq!(
        (
            stored.description.as_str(),
            stored.quantity,
            stored.length_sixteenths
        ),
        ("Blue rail", 7, 200)
    );
    assert_eq!(app.write_count("A-100", "L-101").await.unwrap(), 1);

    // An empty fill clears the field.
    must(
        tab.input(&description, &Input::Fill(String::new()), soon())
            .await,
    );
    assert_eq!(
        must(tab.read(&description, &ReadWhat::Value, soon()).await),
        RawValue::Text(String::new())
    );

    // A request from inside the page, with the page's own session.
    let response = must(
        tab.fetch(
            &PageRequest {
                method: "GET".to_owned(),
                address: Address::new(&app.url(ITEM)).unwrap(),
                headers: vec![("Accept".to_owned(), "application/json".to_owned())],
                body: None,
            },
            soon(),
        )
        .await,
    );
    assert_eq!(response.status, 200);
    assert!(response.body.contains("Blue rail"), "{}", response.body);
    assert!(
        response
            .headers
            .iter()
            .any(|(name, value)| name == "content-type" && value.starts_with("application/json"))
    );
    drop(tab);
    stand.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn each_control_takes_its_own_input_and_an_element_that_cannot_is_refused() {
    let stand = Stand::with_browser("controls");
    let browser = stand.open(&profile("clerk")).await;
    let tab = must(browser.acting_tab(&environment(), &data_pages()).await);
    let page = page_of(
        "<!doctype html><title>Controls</title>\
         <select aria-label=Colour><option value=r>Red<option value=g>Green</select>\
         <label><input type=checkbox id=rush> Rush</label>\
         <form onsubmit=\"document.title = 'Submitted'; return false\"><input aria-label=Search></form>\
         <button disabled>Locked</button>\
         <div style=\"position:relative\"><button id=under>Covered</button>\
         <div style=\"position:absolute;inset:0;background:white\"></div></div>\
         <button onclick=\"alert('Are you sure?')\">Ask</button>\
         <button onclick=\"document.title = 'Clicked'\">Plain</button>",
    );
    must(tab.page.goto_text(&page, soon()).await);

    let colour = must(tab.locate(&Query::role("combobox", "Colour"), soon()).await);
    must(
        tab.input(&colour, &Input::SelectByValue("g".to_owned()), soon())
            .await,
    );
    assert_eq!(
        must(tab.read(&colour, &ReadWhat::Value, soon()).await),
        RawValue::Text("g".to_owned())
    );
    assert_eq!(
        must(tab.read(&colour, &ReadWhat::SelectedText, soon()).await),
        RawValue::Text("Green".to_owned())
    );
    assert_eq!(
        tab.input(&colour, &Input::SelectByValue("b".to_owned()), soon())
            .await
            .unwrap_err(),
        BrowserError::NotActionable(Reason::NoSuchOption)
    );
    assert_eq!(
        tab.input(&colour, &Input::Check(true), soon())
            .await
            .unwrap_err(),
        BrowserError::NotActionable(Reason::WrongControl)
    );

    let rush = must(tab.locate(&Query::role("checkbox", "Rush"), soon()).await);
    for _ in 0..2 {
        // The second time it is already on, and nothing is clicked.
        must(tab.input(&rush, &Input::Check(true), soon()).await);
        assert_eq!(
            must(tab.read(&rush, &ReadWhat::Checked, soon()).await),
            RawValue::Flag(true)
        );
    }
    must(tab.input(&rush, &Input::Check(false), soon()).await);
    assert_eq!(
        must(tab.read(&rush, &ReadWhat::Checked, soon()).await),
        RawValue::Flag(false)
    );

    let locked = must(tab.locate(&Query::role("button", "Locked"), soon()).await);
    assert_eq!(
        tab.input(&locked, &Input::Click, soon()).await.unwrap_err(),
        BrowserError::NotActionable(Reason::Disabled)
    );
    let covered = must(tab.locate(&Query::role("button", "Covered"), soon()).await);
    assert_eq!(
        tab.input(&covered, &Input::Click, soon())
            .await
            .unwrap_err(),
        BrowserError::NotActionable(Reason::Obscured)
    );

    // A real click and a real key reach the page's own handlers.
    let plain = must(tab.locate(&Query::role("button", "Plain"), soon()).await);
    must(tab.input(&plain, &Input::Click, soon()).await);
    assert_eq!(must(tab.page.address_and_title(soon()).await).1, "Clicked");
    let search = must(tab.locate(&textbox("Search"), soon()).await);
    must(
        tab.input(&search, &Input::Type("rails".to_owned()), soon())
            .await,
    );
    must(tab.input(&search, &Input::Press(Key::Enter), soon()).await);
    assert_eq!(
        must(tab.page.address_and_title(soon()).await).1,
        "Submitted"
    );

    // A dialog nobody declared stops everything until it is answered.
    let ask = must(tab.locate(&Query::role("button", "Ask"), soon()).await);
    let stopped = tab.input(&ask, &Input::Click, soon()).await.unwrap_err();
    assert_eq!(
        stopped,
        BrowserError::UnexpectedDialog {
            dispatch: Dispatch::Sent
        }
    );
    assert_eq!(
        tab.read(&colour, &ReadWhat::Value, soon())
            .await
            .unwrap_err(),
        BrowserError::UnexpectedDialog {
            dispatch: Dispatch::NotSent
        }
    );
    must(tab.answer_dialog(&DialogAnswer::Accept, soon()).await);
    assert_eq!(
        must(tab.read(&colour, &ReadWhat::Value, soon()).await),
        RawValue::Text("g".to_owned())
    );
    drop(tab);
    stand.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_browser_killed_under_a_call_says_what_was_sent_and_leaves_no_process() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("killed");
    let clerk = profile("clerk");
    let browser = stand.open(&clerk).await;
    let tab = Arc::new(must(
        browser.acting_tab(&environment(), &origins(&app)).await,
    ));
    must(
        tab.goto(&Address::new(&app.url(FORM)).unwrap(), soon())
            .await,
    );

    // A write the application will hold for a long time once it arrives.
    app.set_fault(&Fault::once(FaultKind::Delay { ms: 600_000 }).on_paths(ITEM))
        .await
        .unwrap();
    let write = PageRequest {
        method: "PUT".to_owned(),
        address: Address::new(&app.url(ITEM)).unwrap(),
        headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
        body: Some(
            r#"{"description":"Sent before the crash","quantity":1,"length_in":"1","version":1}"#
                .to_owned(),
        ),
    };
    let pending = tokio::spawn({
        let tab = Arc::clone(&tab);
        async move {
            tab.fetch(&write, Deadline::after(Duration::from_secs(120)))
                .await
        }
    });
    // The fault is used up when the request arrives: the stand, not a
    // guess at timing, says the write is on its way.
    let arrived = async {
        while !app.faults().await.unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(60), arrived)
        .await
        .unwrap();
    // A second call, written and never answered, at the level of the link.
    let mut endless = tab.page.begin_endless().unwrap();
    let description = must(tab.locate(&textbox("Description"), soon()).await);
    assert_eq!(endless.dispatch(), Dispatch::Sent);

    let running = stand.supervisor.list();
    assert_eq!(running.len(), 1);
    kill(running[0].process);

    // The call that was under way: the browser is gone, and it was sent.
    // For a write that is all the crate above needs to know that the
    // write may have happened.
    let lost = pending.await.unwrap().unwrap_err();
    assert_eq!(
        lost,
        BrowserError::Disconnected {
            dispatch: Dispatch::Sent
        }
    );
    assert_eq!(lost.failure(), None);
    assert!(matches!(
        endless.finish(soon()).await.unwrap_err(),
        crate::client::ClientError::Closed {
            dispatch: Dispatch::Sent,
            ..
        }
    ));
    // A call after the kill was never sent, and that is transient.
    let later = tab
        .input(&description, &Input::Click, soon())
        .await
        .unwrap_err();
    assert_eq!(
        later,
        BrowserError::Disconnected {
            dispatch: Dispatch::NotSent
        }
    );
    assert_eq!(
        tab.locate(&textbox("Description"), soon())
            .await
            .unwrap_err(),
        BrowserError::Disconnected {
            dispatch: Dispatch::NotSent
        }
    );
    assert_eq!(later.failure().unwrap().code(), &codes::CONNECTION_LOST);
    assert_eq!(browser.health(), BrowserHealth::Exited);

    // The profile's browser is started afresh on the next use, and the
    // old one is cleaned up first.
    let fresh = stand.open(&clerk).await;
    assert_ne!(fresh.subsystem(), browser.subsystem());
    assert_eq!(stand.supervisor.list().len(), 1);
    let again = must(fresh.acting_tab(&environment(), &origins(&app)).await);
    must(
        again
            .goto(&Address::new(&app.url(FORM)).unwrap(), soon())
            .await,
    );
    must(again.locate(&textbox("Description"), soon()).await);
    drop((again, tab));
    stand.end().await;
}

/// A write of an order's note from inside the page of that note, with the
/// page's own anti-forgery token read from `from` and sent to `to`.
pub(super) fn note_write(note: &str, from: crate::TokenFrom, to: crate::TokenTo) -> crate::PageWrite {
    note_write_as(
        crate::BodyEncoding::Json,
        note,
        Some(crate::AntiForgery { from, to }),
    )
}

fn note_write_as(
    encoding: crate::BodyEncoding,
    note: &str,
    token: Option<crate::AntiForgery>,
) -> crate::PageWrite {
    let path = crate::PagePath::fill(
        "/api/orders/{order_id}/note",
        &[("order_id".to_owned(), "A-100".to_owned())]
            .into_iter()
            .collect(),
    )
    .unwrap();
    crate::PageWrite::new(
        crate::WriteMethod::Put,
        path,
        encoding,
        [("note".to_owned(), crate::FieldValue::Text(note.to_owned()))]
            .into_iter()
            .collect(),
        token,
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_acting_tab_writes_from_inside_its_page_with_the_page_s_own_token_which_never_leaves_it()
{
    use crate::{BodyEncoding, TokenFrom, TokenProblem, TokenTo};
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("page-write");
    let browser = stand.open(&profile("clerk")).await;
    let tab = must(browser.acting_tab(&environment(), &origins(&app)).await);
    let page = Address::new(&app.url("/orders/A-100/note")).unwrap();
    let puts = || async {
        app.seen()
            .await
            .unwrap()
            .into_iter()
            .filter(|line| line.starts_with("PUT"))
            .count()
    };

    // From each place the page holds its token, to each place it may go,
    // in a JSON and a form body: each write lands once, and the stand
    // stored exactly what was sent.
    let ways = [
        (
            BodyEncoding::Json,
            TokenFrom::Meta("csrf-token".to_owned()),
            TokenTo::Header("X-CSRF-Token".to_owned()),
            "Side door, by the meta element",
        ),
        (
            BodyEncoding::Json,
            TokenFrom::Input("authenticity_token".to_owned()),
            TokenTo::Field("authenticity_token".to_owned()),
            "Side door, by the hidden input",
        ),
        (
            BodyEncoding::Form,
            TokenFrom::Cookie("example_xsrf".to_owned()),
            TokenTo::Header("X-CSRF-Token".to_owned()),
            "Side door, by the cookie",
        ),
    ];
    for (count, (encoding, from, to, note)) in ways.into_iter().enumerate() {
        must(tab.goto(&page, soon()).await);
        let write = note_write_as(encoding, note, Some(crate::AntiForgery { from, to }));
        let answer = must(tab.write_from_page(&write, soon()).await);
        assert_eq!(answer.status, 200, "{}", answer.body);
        let stored = app.stored_notes().await.unwrap();
        assert_eq!(stored.notes["A-100"], note);
        assert_eq!(stored.writes["A-100"], count as u64 + 1);
        assert_eq!(puts().await, count + 1);
        assert!(!format!("{answer:?}").contains(example_app::forgery::TOKEN_PREFIX));
    }

    // The application quotes a refused body back, the token with it: the
    // answer is withheld whole in the page, whatever the spelling of the
    // token in it (decision 0245, audit finding S4).
    must(tab.goto(&page, soon()).await);
    let refused = must(
        tab.write_from_page(
            &note_write(
                "",
                TokenFrom::Meta("csrf-token".to_owned()),
                TokenTo::Field("authenticity_token".to_owned()),
            ),
            soon(),
        )
        .await,
    );
    assert_eq!(refused.status, 422);
    assert!(refused.withheld);
    assert!(refused.body.is_empty(), "{}", refused.body);
    assert!(
        !refused.body.contains(example_app::forgery::TOKEN_PREFIX),
        "{}",
        refused.body
    );

    // Without its token the application refuses the write: the stand's own
    // guard, with the token's step taken out.
    must(tab.goto(&page, soon()).await);
    let bare = must(
        tab.write_from_page(&note_write_as(BodyEncoding::Json, "No token", None), soon())
            .await,
    );
    assert_eq!(bare.status, 403);
    assert_eq!(app.stored_notes().await.unwrap().forgery_refused, 1);

    // A token the page does not hold where the write says: nothing is sent.
    let before = puts().await;
    let missing = tab
        .write_from_page(
            &note_write(
                "Never sent",
                TokenFrom::Meta("no-such-token".to_owned()),
                TokenTo::Header("X-CSRF-Token".to_owned()),
            ),
            soon(),
        )
        .await
        .unwrap_err();
    assert_eq!(missing, BrowserError::Token(TokenProblem::Missing));
    assert_eq!(missing.dispatch(), Dispatch::NotSent);
    assert_eq!(puts().await, before);

    // A page with no origin of its own has no session to write with.
    must(
        tab.page
            .goto_text(
                &page_of("<meta name=\"csrf-token\" content=\"example-token-abcdefgh\">"),
                soon(),
            )
            .await,
    );
    assert!(
        tab.write_from_page(
            &note_write(
                "Never sent",
                TokenFrom::Meta("csrf-token".to_owned()),
                TokenTo::Header("X-CSRF-Token".to_owned())
            ),
            soon()
        )
        .await
        .is_err()
    );
    assert_eq!(puts().await, before);
    assert_eq!(
        app.stored_notes().await.unwrap().notes["A-100"],
        "Side door, by the cookie"
    );
    drop(tab);
    stand.end().await;
}
