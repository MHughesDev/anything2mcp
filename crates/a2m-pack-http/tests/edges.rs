//! Case 23, for the HTTP pack (decision 0357): an answer that comes from the
//! edge in front of a system is recognized by its status and its content
//! type, stops with its own named code, is never sent again, and never
//! hides what an endpoint declares for itself. Each is proven against the
//! test application, asked how often it was reached.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use a2m_core::{FailureClass, Values};
use a2m_provider::{Sent, StepFailure};
use common::Scene;
use example_app::faults::{Fault, FaultKind};
use example_app::harness::TestApp;

fn class_and_code(failure: &StepFailure) -> (FailureClass, String) {
    (
        failure.failure.class(),
        failure.failure.code().as_str().to_owned(),
    )
}

async fn read(scene: &Scene, operation: &str, inputs: &Values) -> Result<(), StepFailure> {
    let (handle, mut prepared) = scene.reader().await;
    let read = scene.read(&handle, &mut prepared, operation, inputs).await;
    handle.release(prepared, false).await;
    read.map(|_| ())
}

#[tokio::test]
async fn an_answer_of_the_edge_stops_with_its_own_code_and_is_not_sent_again() {
    let app = TestApp::start().await.unwrap();
    let scene = Scene::new("edges", &app.url(""));
    let none = Values::new();

    // A page with status 403 where data was asked for.
    let blocked = read(&scene, "example_edge_blocked_get", &none)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&blocked),
        (FailureClass::EdgeBlocked, "forbidden_by_edge".to_owned())
    );
    assert_eq!(blocked.sent, Sent::Answered);
    assert!(
        !blocked.failure.message().contains("Access denied"),
        "the page's words are not quoted: {blocked}"
    );

    // A limit with a time to wait, which the message passes on as a number.
    let limited = read(&scene, "example_edge_limited_get", &none)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&limited),
        (FailureClass::EdgeBlocked, "rate_limited_by_edge".to_owned())
    );
    assert!(
        limited.failure.message().contains("wait 30 seconds"),
        "{limited}"
    );

    // Neither is retried: the application was reached once for each.
    assert_eq!(
        app.seen().await.unwrap(),
        ["GET /edge/blocked", "GET /edge/limited"],
        "each edge answer was asked for once"
    );
}

#[tokio::test]
async fn what_an_endpoint_declares_keeps_its_meaning_and_a_status_of_the_data_stays_unknown() {
    let app = TestApp::start().await.unwrap();
    let scene = Scene::new("edges", &app.url(""));

    // The endpoint that declares the limit as its own "not now".
    let declared = read(&scene, "example_edge_limited_declared_get", &Values::new())
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&declared),
        (FailureClass::Transient, "upstream_unavailable".to_owned())
    );

    // A 403 that is not a page is the application's own answer, not an edge's.
    app.set_fault(&Fault::once(FaultKind::Status { status: 403 }).on_paths("/api/orders"))
        .await
        .unwrap();
    let plain = read(
        &scene,
        "example_order_get",
        &Values::new().with("order_id", "A-100"),
    )
    .await
    .unwrap_err();
    assert_eq!(
        class_and_code(&plain),
        (FailureClass::UnknownVariant, "unexpected_status".to_owned())
    );
}
