//! A secret planted in the application's own answer: the value is
//! registered with the redactor as the product registers a secret, the
//! application answers with it, and it appears in no result, no log line,
//! no trace, no capture, and no event the engine hands out.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::sync::atomic::Ordering;

use a2m_core::{Secret, Values};
use a2m_engine::JobKind;
use a2m_telemetry::CaptureLevel;
use common::{ORDER_GET, Scene};

/// The invented value the test application's hand-off answers as its
/// `client_secret`.
const PLANTED: &str = example_app::answers::HANDOFF_SECRET;

#[tokio::test]
async fn a_secret_planted_in_the_applications_answer_appears_in_no_result_log_or_trace() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    scene.vault.register(&Secret::new(PLANTED));
    scene.hooks.echoes_handoff.store(true, Ordering::SeqCst);
    scene.hooks.tells_acting.store(true, Ordering::SeqCst);
    scene.control.keeps_captures.store(true, Ordering::SeqCst);

    // The application does answer with the value: the proof below is not
    // of an answer that never held it.
    let answered = scene
        .app
        .http()
        .get(scene.app.url("/api/orders/A-100/handoff"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(answered.contains(PLANTED), "{answered}");

    // At the full level, so that the exchange itself is kept.
    let mut job = scene
        .job(
            JobKind::Execute,
            ORDER_GET,
            Values::new().with("order_id", "A-100"),
        )
        .await;
    job.capture = CaptureLevel::Full;
    let outcome = scene.engine.run_job(&job, scene.control.as_ref()).await;
    scene.apply(&outcome).await;
    assert!(common::completed(&outcome), "{outcome:?}");

    // The result holds a mask where the value was.
    let result = format!("{:?}", scene.last_result());
    assert!(!result.contains(PLANTED), "{result}");
    assert!(result.contains(a2m_core::REDACTED), "{result}");
    assert!(!format!("{outcome:?}").contains(PLANTED));

    // No trace, and none of what it names, holds it.
    let traces = scene.control.traces.lock().unwrap().clone();
    assert!(!traces.is_empty());
    for trace in &traces {
        let lines = String::from_utf8_lossy(&trace.bytes);
        assert!(!lines.contains(PLANTED), "{lines}");
    }
    let kept = scene.control.captures.lock().unwrap().clone();
    assert!(!kept.is_empty());
    for (capture, _) in &kept {
        let bytes = String::from_utf8_lossy(&capture.bytes);
        assert!(!bytes.contains(PLANTED), "{} {bytes}", capture.name);
    }

    // No event, no span, and no log line, in memory or in the file.
    let events = format!("{:?}", scene.control.events.lock().unwrap());
    assert!(!events.contains(PLANTED), "{events}");
    let spans = format!("{:?}", scene.telemetry.spans());
    assert!(!spans.contains(PLANTED), "{spans}");
    assert!(!scene.logged().contains(PLANTED));
}
