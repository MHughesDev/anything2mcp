//! The desktop driver against the stand, on a session of the test's own
//! (Linux, Windows, and macOS), through the registry's driver handle. Every test asks the stand
//! what it holds and what it was asked: the reads return its values; the
//! late value is waited for by its event; the two same-named controls and
//! the password field stop by name; nothing pressed Save; and an
//! application that is not running is a named stop, with nothing started.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::time::Duration;

use a2m_core::{FailureClass, Value};
use common::Scene;
use example_desk::{LATE, Order, SECRET};

fn text(values: &a2m_core::Values, name: &str) -> String {
    values
        .get(name)
        .and_then(Value::as_text)
        .unwrap_or_else(|| panic!("no text `{name}` in {values:?}"))
        .to_owned()
}

fn list(values: &a2m_core::Values, name: &str) -> Vec<String> {
    values
        .get(name)
        .and_then(Value::as_list)
        .unwrap_or_else(|| panic!("no list `{name}` in {values:?}"))
        .iter()
        .map(|value| value.as_text().unwrap().to_owned())
        .collect()
}

fn stopped(failure: &a2m_provider::StepFailure) -> (FailureClass, String) {
    (
        failure.failure.class(),
        failure.failure.code().as_str().to_owned(),
    )
}

#[tokio::test]
async fn a_read_returns_what_the_window_holds_and_presses_nothing() {
    let Some(scene) = Scene::start("read").await else {
        return;
    };
    let order = Order::example();
    let values = scene
        .read_or_explain("example_order_get", Duration::from_secs(10))
        .await;
    assert_eq!(text(&values, "order_number"), order.number);
    assert_eq!(text(&values, "customer"), order.customer);
    assert_eq!(text(&values, "status"), order.status);
    let lines = scene
        .read_or_explain("example_order_line_list", Duration::from_secs(10))
        .await;
    let codes: Vec<String> = order.lines.iter().map(|line| line.code.clone()).collect();
    let quantities: Vec<String> = order
        .lines
        .iter()
        .map(|line| line.quantity.clone())
        .collect();
    assert_eq!(list(&lines, "item_code"), codes);
    assert_eq!(list(&lines, "quantity"), quantities);
    // The stand: nothing pressed, set, typed, or focused, and its password
    // field never asked.
    let truth = scene.desk().truth();
    assert_eq!(truth.writes, 0);
    assert!(!truth.acted(), "{:?}", truth.asked);
    assert_eq!(truth.secret_reads, 0);
}

#[tokio::test]
async fn a_value_that_appears_late_is_waited_for_by_its_event() {
    let Some(scene) = Scene::start("late").await else {
        return;
    };
    // With nothing to end the wait, it stops at its deadline, by name. The
    // deadline leaves a walk of the window time to finish on a slow
    // machine, so that it is the wait that stops.
    let early = scene
        .read("example_order_tracking_get", Duration::from_millis(1500))
        .await
        .unwrap_err();
    assert_eq!(
        stopped(&early),
        (FailureClass::UnknownVariant, "unexpected_page".to_owned())
    );
    assert_eq!(scene.desk().truth().tracking, "");
    // The stand shows the value a moment after the wait begins; the
    // application's event ends the wait, and the read returns the value.
    scene.desk().show_late(Duration::from_millis(300));
    let values = scene
        .read_or_explain("example_order_tracking_get", Duration::from_secs(10))
        .await;
    assert_eq!(text(&values, "tracking"), LATE);
    assert_eq!(scene.desk().truth().tracking, LATE);
    assert_eq!(scene.desk().truth().writes, 0);
}

#[tokio::test]
async fn two_controls_of_one_role_and_name_stop_by_name() {
    let Some(scene) = Scene::start("twice").await else {
        return;
    };
    let failure = scene
        .read("example_order_reference_get", Duration::from_secs(10))
        .await
        .unwrap_err();
    assert_eq!(
        stopped(&failure),
        (FailureClass::Drift, "locator_ambiguous".to_owned())
    );
    assert!(!failure.failure.to_string().contains("R-1"));
}

#[tokio::test]
async fn a_password_field_is_refused_and_never_asked() {
    let Some(scene) = Scene::start("secret").await else {
        return;
    };
    let failure = scene
        .read("example_order_access_get", Duration::from_secs(10))
        .await
        .unwrap_err();
    assert_eq!(
        stopped(&failure),
        (FailureClass::Policy, "password_read_refused".to_owned())
    );
    assert!(!failure.failure.to_string().contains(SECRET));
    let truth = scene.desk().truth();
    assert_eq!(truth.secret_reads, 0);
    assert_eq!(truth.writes, 0);
}

/// A program that exists and that nothing runs.
fn not_running(scene: &Scene) -> String {
    let path = scene.folder.path().join("other-program");
    std::fs::write(&path, b"not a program").unwrap();
    std::fs::canonicalize(path)
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

#[tokio::test]
async fn an_application_that_is_not_running_is_a_named_stop() {
    // On Linux the program runs (it is this test's) and publishes nothing,
    // and the session's accessibility switch is off, as the test's own
    // session has it: the stop names the setting, and the pack changes
    // none. Windows and macOS have no such switch: the stand's program,
    // which nothing runs yet, is not running.
    let Some(scene) = Scene::without_application("silent").await else {
        return;
    };
    let failure = scene
        .read("example_order_get", Duration::from_secs(10))
        .await
        .unwrap_err();
    let silent = if cfg!(any(windows, target_os = "macos")) {
        "application_not_running"
    } else {
        "accessibility_off"
    };
    assert_eq!(stopped(&failure), (FailureClass::Busy, silent.to_owned()));
    // A program nobody runs: not running, and nothing is started.
    let mut scene = scene;
    scene.surface.surface.address = not_running(&scene);
    let failure = scene
        .read("example_order_get", Duration::from_secs(10))
        .await
        .unwrap_err();
    assert_eq!(
        stopped(&failure),
        (FailureClass::Busy, "application_not_running".to_owned())
    );
    // The same, with the stand running and the surface naming a program
    // that is not the one that runs it: no other application is read. On
    // Windows and macOS a test process has one desktop at a time, which
    // the scene above holds until it is gone.
    drop(scene);
    let Some(mut scene) = Scene::start("other-program").await else {
        return;
    };
    scene.surface.surface.address = not_running(&scene);
    let failure = scene
        .read("example_order_get", Duration::from_secs(10))
        .await
        .unwrap_err();
    assert_eq!(
        stopped(&failure),
        (FailureClass::Busy, "application_not_running".to_owned())
    );
    let truth = scene.desk().truth();
    assert!(truth.asked.is_empty(), "{:?}", truth.asked);
}
