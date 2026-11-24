//! The Phase 1 cases of the interface driver, against the test application's
//! pages under `/cases` (cases 6 to 9, 11, and 12). Every proof asks the
//! application what its page was told and did, through `case_events`, and
//! never the engine's own reading.

use crate::{DialogAnswer, Input, Primitive, RawValue, ReadWhat, TextMatch};

use super::*;

const RENDER: &str = "/cases/rerender";
const TYPED: &str = "/cases/typed-input";
const IN_PAGE: &str = "/cases/in-page";
const GRID: &str = "/cases/grid";
const CONFIRM: &str = "/cases/confirm";
const FRAME: &str = "/cases/frame";

async fn opened(name: &str, app: &TestApp, path: &str) -> (Stand, crate::ActTab) {
    let stand = Stand::with_browser(name);
    let browser = stand.open(&profile("clerk")).await;
    let tab = must(browser.acting_tab(&environment(), &origins(app)).await);
    must(
        tab.goto(&Address::new(&app.url(path)).unwrap(), soon())
            .await,
    );
    (stand, tab)
}

async fn id_of(tab: &crate::ActTab, element: &crate::Element) -> RawValue {
    let attribute = ReadWhat::Attribute {
        name: "id".to_owned(),
    };
    must(tab.read(element, &attribute, soon()).await)
}

async fn value_of(tab: &crate::ActTab, element: &crate::Element) -> RawValue {
    must(tab.read(element, &ReadWhat::Value, soon()).await)
}

/// What the application was told, once it has been told at least `count`
/// things. A page's tell is a request of its own and may land a moment after
/// the page shows its result, so the test asks the application until it has
/// heard, within a bound; it never takes silence for an answer.
async fn told(app: &TestApp, count: usize) -> Vec<String> {
    let started = std::time::Instant::now();
    loop {
        let events = app.case_events().await.unwrap();
        if events.len() >= count || started.elapsed() > Duration::from_secs(20) {
            return events;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn text_is(query: Query, text: &str) -> Primitive {
    Primitive::Text(
        query,
        TextMatch {
            text: text.to_owned(),
            contains: false,
        },
    )
}

/// Case 6: the page draws its list again on every change, with identifiers
/// that differ on each draw. Role and name find the same button each time; an
/// element found before a draw is stale after it and is never looked for
/// again silently.
#[tokio::test(flavor = "multi_thread")]
async fn an_element_found_before_the_page_draws_again_is_stale_and_role_and_name_find_it_again() {
    let app = TestApp::start().await.unwrap();
    let (stand, tab) = opened("case-rerender", &app, RENDER).await;

    let crate_button = Query::role("button", "Archive Crate");
    let before = must(tab.locate(&crate_button, soon()).await);
    let first_id = id_of(&tab, &before).await;

    let pallet = must(
        tab.locate(&Query::role("button", "Archive Pallet"), soon())
            .await,
    );
    must(tab.input(&pallet, &Input::Click, soon()).await);
    must(
        tab.wait(&text_is(css("#status"), "Archived Pallet."), soon())
            .await,
    );

    // The element of before the draw is gone; the page is not asked again
    // in its place.
    let stale = tab.input(&before, &Input::Click, soon()).await.unwrap_err();
    assert!(matches!(stale, BrowserError::NotFound), "{stale:?}");

    let after = must(tab.locate(&crate_button, soon()).await);
    let second_id = id_of(&tab, &after).await;
    assert_ne!(
        first_id, second_id,
        "the page was to draw the button again with a new identifier"
    );
    must(tab.input(&after, &Input::Click, soon()).await);
    must(
        tab.wait(&text_is(css("#status"), "Archived Crate."), soon())
            .await,
    );

    // What the page was told is what was pressed, in order, and the row
    // that was archived is no longer drawn.
    assert_eq!(told(&app, 2).await, ["archived: Pallet", "archived: Crate"]);
    must(
        tab.wait(
            &Primitive::Hidden(Query::role("button", "Archive Crate")),
            soon(),
        )
        .await,
    );
    drop(tab);
    stand.end().await;
}

/// Case 7: the page keeps a value only when real key presses made it. The
/// three strategies of the base each reach the page, and only keys keep it.
#[tokio::test(flavor = "multi_thread")]
async fn a_value_the_page_keeps_only_for_real_key_presses_is_held_by_typing_alone() {
    let app = TestApp::start().await.unwrap();
    let (stand, tab) = opened("case-typed", &app, TYPED).await;
    let field = must(tab.locate(&textbox("Reference"), soon()).await);

    must(
        tab.input(&field, &Input::Fill("R-100".to_owned()), soon())
            .await,
    );
    assert_eq!(value_of(&tab, &field).await, RawValue::Text(String::new()));
    must(
        tab.input(&field, &Input::NativeSet("R-100".to_owned()), soon())
            .await,
    );
    assert_eq!(value_of(&tab, &field).await, RawValue::Text(String::new()));
    must(
        tab.input(&field, &Input::Type("R-100".to_owned()), soon())
            .await,
    );
    assert_eq!(
        value_of(&tab, &field).await,
        RawValue::Text("R-100".to_owned())
    );

    let save = must(
        tab.locate(&Query::role("button", "Save reference"), soon())
            .await,
    );
    must(tab.input(&save, &Input::Click, soon()).await);
    must(
        tab.wait(&text_is(css("#status"), "Saved R-100."), soon())
            .await,
    );
    // The page itself says what it ignored, and what it kept.
    assert_eq!(
        told(&app, 3).await,
        [
            "ignored: not typed",
            "ignored: not real",
            "reference: R-100"
        ]
    );
    drop(tab);
    stand.end().await;
}

/// Case 8: the address changes with no new document, and the page shows
/// its answer a moment later. A wait on the address alone is too early; a
/// wait on what the page then shows is the readiness.
#[tokio::test(flavor = "multi_thread")]
async fn a_page_that_changes_its_address_without_loading_is_ready_when_what_it_shows_is() {
    let app = TestApp::start().await.unwrap();
    let (stand, tab) = opened("case-in-page", &app, IN_PAGE).await;
    must(
        tab.wait(&text_is(css("#shown"), "Orders shown."), soon())
            .await,
    );
    let customers = must(
        tab.locate(&Query::role("button", "Customers"), soon())
            .await,
    );
    must(tab.input(&customers, &Input::Click, soon()).await);

    // The address is the first thing that changes; the heading is not yet.
    must(
        tab.wait(
            &Primitive::Url(TextMatch {
                text: "/cases/in-page/customers".to_owned(),
                contains: true,
            }),
            soon(),
        )
        .await,
    );
    let title = must(tab.locate(&css("#title"), soon()).await);
    assert_eq!(
        must(tab.read(&title, &ReadWhat::Text, soon()).await),
        RawValue::Text("Orders".to_owned()),
        "the page was to show its answer after the address changed"
    );
    must(
        tab.wait(&text_is(css("#shown"), "Customers shown."), soon())
            .await,
    );
    assert_eq!(
        must(tab.read(&title, &ReadWhat::Text, soon()).await),
        RawValue::Text("Customers".to_owned())
    );

    // One document loaded, and the move was the page's own.
    assert_eq!(
        told(&app, 2).await,
        ["loaded: /cases/in-page", "moved: /cases/in-page/customers"]
    );
    drop(tab);
    stand.end().await;
}

/// Case 9: ten thousand rows, about fifteen of them in the page. A row is
/// found by its key and never by its position: the row of a key that is not
/// drawn is not found, and is found once the page is asked to show it.
#[tokio::test(flavor = "multi_thread")]
async fn a_row_of_a_grid_that_draws_only_what_shows_is_found_by_its_key() {
    let app = TestApp::start().await.unwrap();
    let (stand, tab) = opened("case-grid", &app, GRID).await;
    let row_of = |key: &str| {
        Query::role(
            "row",
            &format!("{key} Line {} {}", number(key), quantity(key)),
        )
    };
    fn number(key: &str) -> u32 {
        key.trim_start_matches("R-").parse().unwrap()
    }
    fn quantity(key: &str) -> u32 {
        (number(key) * 7) % 90 + 1
    }

    // Only the first rows exist; the page says how few.
    must(
        tab.wait(&Primitive::Visible(row_of("R-00001")), soon())
            .await,
    );
    let body = must(tab.locate(&css("body"), soon()).await);
    let drawn = must(
        tab.read(
            &body,
            &ReadWhat::Attribute {
                name: "data-rows-in-page".to_owned(),
            },
            soon(),
        )
        .await,
    );
    let RawValue::Text(drawn) = drawn else {
        panic!("{drawn:?}")
    };
    assert!(drawn.parse::<u32>().unwrap() < 20, "{drawn}");
    assert!(tab.locate(&row_of("R-07342"), soon()).await.is_err());

    // Asking the page to show the row of a key puts it in the page.
    let find = must(tab.locate(&textbox("Find row"), soon()).await);
    must(
        tab.input(&find, &Input::Fill("R-07342".to_owned()), soon())
            .await,
    );
    must(
        tab.input(&find, &Input::Press(crate::Key::Enter), soon())
            .await,
    );
    must(
        tab.wait(&Primitive::Visible(row_of("R-07342")), soon())
            .await,
    );
    let row = must(tab.locate(&row_of("R-07342"), soon()).await);
    let cells = must(tab.read(&row, &ReadWhat::Text, soon()).await);
    assert_eq!(
        cells,
        RawValue::Text(format!("R-07342 Line 7342 {}", quantity("R-07342")))
    );
    assert_eq!(told(&app, 1).await, ["find: R-07342"]);
    drop(tab);
    stand.end().await;
}

/// Case 11: a confirmation the page shows on some presses, in its own
/// dialog, and on some in the browser's. One that is not there is not
/// waited for, and the browser's own, when nobody declared it, stops the
/// tab until it is answered.
#[tokio::test(flavor = "multi_thread")]
async fn a_confirmation_that_comes_only_sometimes_is_answered_when_it_comes_and_never_waited_for() {
    let app = TestApp::start().await.unwrap();
    let (stand, tab) = opened("case-confirm", &app, CONFIRM).await;
    let archive = must(
        tab.locate(&Query::role("button", "Archive order"), soon())
            .await,
    );
    let ask = Query::role("dialog", "Archive this order?");
    let status = text_is(css("#status"), "Archived.");

    // First press: the page asks, and nothing is done before it is answered.
    must(tab.input(&archive, &Input::Click, soon()).await);
    must(tab.wait(&Primitive::Visible(ask.clone()), soon()).await);
    assert_eq!(told(&app, 1).await, ["pressed: 1"]);
    let yes = must(
        tab.locate(
            &Query::role("button", "Confirm").within(ask.clone()),
            soon(),
        )
        .await,
    );
    must(tab.input(&yes, &Input::Click, soon()).await);
    must(tab.wait(&status, soon()).await);
    must(tab.wait(&Primitive::Hidden(ask.clone()), soon()).await);

    // Second press: no question at all; the page does it.
    must(tab.input(&archive, &Input::Click, soon()).await);
    must(tab.wait(&status, soon()).await);
    assert!(tab.locate(&ask, soon()).await.is_err());

    // Third press: asked again, and this time declined.
    must(tab.input(&archive, &Input::Click, soon()).await);
    must(tab.wait(&Primitive::Visible(ask.clone()), soon()).await);
    let no = must(
        tab.locate(&Query::role("button", "Cancel").within(ask.clone()), soon())
            .await,
    );
    must(tab.input(&no, &Input::Click, soon()).await);
    must(tab.wait(&Primitive::Hidden(ask.clone()), soon()).await);

    // Fourth press: the browser's own dialog, which nobody declared.
    let stopped = tab
        .input(&archive, &Input::Click, soon())
        .await
        .unwrap_err();
    assert_eq!(
        stopped,
        BrowserError::UnexpectedDialog {
            dispatch: Dispatch::Sent
        }
    );
    must(tab.answer_dialog(&DialogAnswer::Dismiss, soon()).await);

    assert_eq!(
        told(&app, 8).await,
        [
            "pressed: 1",
            "archived: press 1",
            "pressed: 2",
            "archived: press 2",
            "pressed: 3",
            "cancelled: press 3",
            "pressed: 4",
            "cancelled: press 4",
        ]
    );
    drop(tab);
    stand.end().await;
}

/// Case 12, the shadow tree: a control inside an open shadow root is found by
/// role and name, set, and its press reaches the page.
#[tokio::test(flavor = "multi_thread")]
async fn a_control_inside_an_open_shadow_tree_is_found_by_role_and_name() {
    let app = TestApp::start().await.unwrap();
    let (stand, tab) = opened("case-shadow", &app, FRAME).await;
    let note = must(tab.locate(&textbox("Shadow note"), soon()).await);
    must(
        tab.input(&note, &Input::Fill("left at the dock".to_owned()), soon())
            .await,
    );
    let send = must(
        tab.locate(&Query::role("button", "Send note"), soon())
            .await,
    );
    must(tab.input(&send, &Input::Click, soon()).await);
    must(
        tab.wait(&Primitive::Visible(Query::role("status", "")), soon())
            .await,
    );
    assert_eq!(told(&app, 1).await, ["shadow-note: left at the dock"]);
    drop(tab);
    stand.end().await;
}

/// Case 12, the frame: a form inside a frame of the page's own origin is
/// found only through the frame, which is named as the scope of the query.
/// Outside it the same query finds nothing, so a control of a frame is never
/// taken for one of the page. What is typed and pressed reaches the form,
/// and the wait on its answer sees the frame's own change.
#[tokio::test(flavor = "multi_thread")]
async fn a_form_inside_a_frame_is_found_through_the_frame_and_through_nothing_else() {
    let app = TestApp::start().await.unwrap();
    let (stand, tab) = opened("case-frame", &app, FRAME).await;
    let frame = Query::new(By::Title {
        text: "Dispatch form".to_owned(),
    });
    let code = textbox("Dispatch code");
    assert!(tab.locate(&code, soon()).await.is_err());

    let field = must(
        tab.locate(&code.clone().within(frame.clone()), soon())
            .await,
    );
    must(
        tab.input(&field, &Input::Type("D-4471".to_owned()), soon())
            .await,
    );
    assert_eq!(
        value_of(&tab, &field).await,
        RawValue::Text("D-4471".to_owned())
    );
    let send = must(
        tab.locate(
            &Query::role("button", "Send code").within(frame.clone()),
            soon(),
        )
        .await,
    );
    must(tab.input(&send, &Input::Click, soon()).await);
    must(
        tab.wait(
            &text_is(css("#status").within(frame.clone()), "Code sent."),
            soon(),
        )
        .await,
    );
    assert_eq!(told(&app, 1).await, ["frame-code: D-4471"]);
    drop(tab);
    stand.end().await;
}
