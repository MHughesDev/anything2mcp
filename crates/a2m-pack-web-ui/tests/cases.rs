//! The Phase 1 cases of the interface driver, through the driver and the
//! screens of `tests/fixtures/cases`, against the test application's pages
//! under `/cases` (cases 6 to 8, 11, and 12). Every proof asks the
//! application what its page was told, and never the driver's own account.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{FailureClass, NoSecrets, Value, Values};
use a2m_provider::{DriverHandle, Posture, Prepared, StepFailure};
use common::{Scene, Stand, Write, compile};
use example_app::harness::TestApp;

async fn scene(name: &str, app: &TestApp) -> Scene {
    let stand = Stand::new(name, Arc::new(NoSecrets));
    let compiled_target = compile(&stand.registry, "cases").compiled_target;
    Scene::over(stand, compiled_target, &app.url(""))
}

/// What the application was told, once it has been told at least `count`
/// things: a page's tell is a request of its own and may land a moment after
/// the page shows its result.
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

async fn writing(scene: &Scene) -> (DriverHandle, Prepared, Write) {
    let write = Write::new(&["cases"]).await;
    let (handle, prepared) = scene
        .try_prepare(Some(write.fence), Posture::Live)
        .await
        .unwrap();
    (handle, prepared, write)
}

/// Runs steps of an operation's binding on the page the driver is on.
async fn run(
    scene: &Scene,
    handle: &DriverHandle,
    prepared: &mut Prepared,
    write: &Write,
    steps: &[a2m_core::Step],
    inputs: &Values,
) -> Result<common::Ran, StepFailure> {
    scene
        .run(handle, prepared, steps, inputs, Some(write.authority()))
        .await
}

const NOTHING: fn() -> Values = Values::new;

/// Case 6: the page draws its list again after the first press, with new
/// generated identifiers. The second press is found by role and name on the
/// page as it is then.
#[tokio::test(flavor = "multi_thread")]
async fn a_button_is_found_again_by_role_and_name_after_the_page_draws_its_list_again() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("case-rerender", &app).await;
    let (handle, mut prepared, write) = writing(&scene).await;
    let pallet = &scene.binding("example_pallet_archive").act;
    let crate_steps = &scene.binding("example_crate_archive").act;
    run(&scene, &handle, &mut prepared, &write, pallet, &NOTHING())
        .await
        .unwrap();
    // The second action's own `goto` and `wait` are left out: the page it
    // acts on is the one the first left, drawn again.
    let second = Write::new(&["cases-2"]).await;
    run(
        &scene,
        &handle,
        &mut prepared,
        &second,
        &crate_steps[2..],
        &NOTHING(),
    )
    .await
    .unwrap();
    assert!(handle.release(prepared, false).await.complete);
    assert_eq!(told(&app, 2).await, ["archived: Pallet", "archived: Crate"]);
    scene.stand.end().await;
}

/// Case 7: the page keeps a value only for real key presses. The driver's
/// text rule fills first, then types, and the page is told of one value.
#[tokio::test(flavor = "multi_thread")]
async fn a_value_that_only_key_presses_make_is_set_by_typing_and_saved_once() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("case-typed", &app).await;
    let (handle, mut prepared, write) = writing(&scene).await;
    let steps = &scene.binding("example_reference_submit").act;
    let ran = run(
        &scene,
        &handle,
        &mut prepared,
        &write,
        steps,
        &Values::new().with("reference", "R-100"),
    )
    .await
    .unwrap();
    assert!(handle.release(prepared, false).await.complete);
    let means = ran.means_text();
    assert!(
        means.contains("strategy") && means.contains("type"),
        "{means}"
    );
    assert!(!means.contains("R-100"), "{means}");
    assert_eq!(
        told(&app, 2).await,
        ["ignored: not typed", "reference: R-100"]
    );
    scene.stand.end().await;
}

/// Case 8: the address changes with no new document, and the page shows
/// its answer a moment later. The action is done when the page shows what
/// its confirmation names, and the page loaded one document.
#[tokio::test(flavor = "multi_thread")]
async fn an_action_that_moves_the_page_without_loading_is_done_when_the_page_shows_its_answer() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("case-in-page", &app).await;
    let (handle, mut prepared, write) = writing(&scene).await;
    let steps = &scene.binding("example_customers_open").act;
    run(&scene, &handle, &mut prepared, &write, steps, &NOTHING())
        .await
        .unwrap();
    assert!(handle.release(prepared, false).await.complete);
    assert_eq!(
        told(&app, 2).await,
        ["loaded: /cases/in-page", "moved: /cases/in-page/customers"]
    );
    scene.stand.end().await;
}

/// Case 11: a question the page asks on some presses. A declared one is
/// answered by the action it names and the step goes on; one the page does
/// not ask costs nothing; the browser's own, which no screen declares, stops
/// the run and says that input reached the page.
#[tokio::test(flavor = "multi_thread")]
async fn a_declared_question_is_answered_when_asked_and_an_undeclared_one_stops_the_run() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("case-confirm", &app).await;
    let (handle, mut prepared, write) = writing(&scene).await;
    let steps = &scene.binding("example_order_archive").act;
    // Press 1: asked, and answered.
    let first = run(&scene, &handle, &mut prepared, &write, steps, &NOTHING())
        .await
        .unwrap();
    assert!(
        first.means_text().contains("asks") || first.means_text().contains("invoke"),
        "{}",
        first.means_text()
    );
    // Press 2: not asked. Press 3: asked again.
    for count in [2_u32, 3] {
        let again = Write::new(&["cases", &count.to_string()]).await;
        run(&scene, &handle, &mut prepared, &again, steps, &NOTHING())
            .await
            .unwrap();
    }
    // Press 4: the browser's own dialog.
    let last = Write::new(&["cases", "4"]).await;
    let stopped = run(&scene, &handle, &mut prepared, &last, steps, &NOTHING())
        .await
        .unwrap_err();
    assert_eq!(
        stopped.failure.class(),
        FailureClass::PossibleCommit,
        "{stopped}"
    );
    handle.release(prepared, true).await;
    let events = told(&app, 6).await;
    assert_eq!(
        events[..6],
        [
            "pressed: 1",
            "archived: press 1",
            "pressed: 2",
            "archived: press 2",
            "pressed: 3",
            "archived: press 3",
        ],
        "{events:?}"
    );
    scene.stand.end().await;
}

/// Case 11: a notice that covers the form is closed by the action the screen
/// names for it, before the click that would have met the notice.
#[tokio::test(flavor = "multi_thread")]
async fn a_notice_that_covers_the_form_is_closed_by_the_action_the_screen_names() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("case-notice", &app).await;
    let (handle, mut prepared, write) = writing(&scene).await;
    let steps = &scene.binding("example_report_send").act;
    let ran = run(&scene, &handle, &mut prepared, &write, steps, &NOTHING())
        .await
        .unwrap();
    assert!(handle.release(prepared, false).await.complete);
    assert!(
        ran.means_text().contains("dismissed"),
        "{}",
        ran.means_text()
    );
    assert_eq!(told(&app, 2).await, ["closed: notice", "sent: report"]);
    scene.stand.end().await;
}

/// Case 12: a form inside a frame is filled and sent through the frame the
/// screen declares, and the page of the frame is told of exactly that.
#[tokio::test(flavor = "multi_thread")]
async fn a_form_inside_a_frame_is_set_and_sent_through_the_frame_the_screen_declares() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("case-frame", &app).await;
    let (handle, mut prepared, write) = writing(&scene).await;
    let steps = &scene.binding("example_dispatch_code_send").act;
    run(
        &scene,
        &handle,
        &mut prepared,
        &write,
        steps,
        &Values::new().with("code", "D-4471"),
    )
    .await
    .unwrap();
    assert!(handle.release(prepared, false).await.complete);
    assert_eq!(told(&app, 1).await, ["frame-code: D-4471"]);
    scene.stand.end().await;
}

/// Case 33: the browser's own controls, each set and read back through the
/// driver: a select, a check box, a radio group, a date field, and a text
/// area. Save is disabled until the date is valid, and the browser asks
/// before it saves, which the action declares. The application says what it
/// stored, and how many times.
#[tokio::test(flavor = "multi_thread")]
async fn the_browsers_own_controls_are_set_confirmed_saved_and_read_back() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("case-delivery", &app).await;
    let (handle, mut prepared, write) = writing(&scene).await;
    let binding = scene.binding("example_order_delivery_update");
    let inputs = Values::new()
        .with("order_id", "A-100")
        .with("shipping", "express")
        .with("gift_wrap", true)
        .with("priority", "high")
        .with("deliver_on", "2026-12-01")
        .with("notes", "Leave at the dock");
    run(
        &scene,
        &handle,
        &mut prepared,
        &write,
        &binding.act,
        &inputs,
    )
    .await
    .unwrap();
    let back = scene
        .run(&handle, &mut prepared, &binding.read_back, &inputs, None)
        .await
        .unwrap();
    assert!(handle.release(prepared, false).await.complete);

    let stored =
        serde_json::to_value(app.stored_delivery("A-100").await.unwrap().unwrap()).unwrap();
    assert_eq!(app.delivery_write_count("A-100").await.unwrap(), 1);
    for (name, wanted) in [
        ("shipping", serde_json::json!("express")),
        ("gift_wrap", serde_json::json!(true)),
        ("priority", serde_json::json!("high")),
        ("deliver_on", serde_json::json!("2026-12-01")),
        ("notes", serde_json::json!("Leave at the dock")),
    ] {
        assert_eq!(stored[name], wanted, "{name}");
    }
    for (name, wanted) in [
        ("shipping", Value::from("express")),
        ("gift_wrap", Value::from(true)),
        ("priority", Value::from("high")),
        ("deliver_on", Value::from("2026-12-01")),
        ("notes", Value::from("Leave at the dock")),
    ] {
        assert_eq!(back.values.get(name), Some(&wanted), "{name}");
    }
    scene.stand.end().await;
}

/// Case 33: a code the radio group does not list, and a date the page does
/// not take, store nothing: the first is refused before anything is sent,
/// and the second leaves Save disabled.
#[tokio::test(flavor = "multi_thread")]
async fn a_choice_the_screen_does_not_list_and_a_date_the_page_refuses_store_nothing() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("case-delivery-refused", &app).await;
    let binding = scene.binding("example_order_delivery_update");
    let base = Values::new()
        .with("order_id", "A-100")
        .with("shipping", "express")
        .with("gift_wrap", false)
        .with("priority", "high")
        .with("deliver_on", "2026-12-01")
        .with("notes", "");
    for (changed, value) in [("priority", "urgent"), ("deliver_on", "2101-01-01")] {
        let inputs = base.clone().with(changed, value);
        let (handle, mut prepared, write) = writing(&scene).await;
        let refused = run(
            &scene,
            &handle,
            &mut prepared,
            &write,
            &binding.act,
            &inputs,
        )
        .await
        .unwrap_err();
        assert_eq!(
            refused.sent,
            a2m_provider::Sent::NotSent,
            "{changed}: {refused}"
        );
        handle.release(prepared, true).await;
    }
    assert_eq!(app.delivery_write_count("A-100").await.unwrap(), 0);
    scene.stand.end().await;
}
