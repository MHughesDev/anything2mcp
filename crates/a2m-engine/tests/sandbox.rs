//! Build-order step 10: the sandbox. A discovery job hands every suitable
//! discoverer a port through which a write is impossible, and what each
//! finds reaches `Control`; a `Try` job runs a draft where only declared
//! test records may be written; a `VerifyAfterLoss` job reads what a run
//! verified once more and changes nothing.
//!
//! "Cannot write" has two halves. The kernel's is the token: no intent is
//! asked for, so no step that commits can pass the port or the handle. The
//! pack's is its driver's own guard outside a posture that may write. The
//! test application's control interface shows what both let through.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::sync::atomic::Ordering;

use a2m_core::{BindingStatus, FailureClass, RecordState, RunState};
use a2m_engine::{EngineEvent, JobKind, JobOutcome};
use a2m_provider::{Sent, StepOutcome};
use common::{
    ITEM_UPSERT, ORDER_GET, Scene, approved, completed, failed_as, item, item_key, waits,
};

fn order() -> a2m_core::Values {
    a2m_core::Values::new().with("order_id", "A-100")
}

/// What the discoverers of the scene found, as it reached `Control`.
fn discovered(scene: &Scene) -> Vec<(String, String, usize, Result<(), String>)> {
    scene
        .control
        .events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            EngineEvent::Discovered {
                surface,
                provider,
                discoverer,
                findings,
                report,
            } => Some((
                format!("{surface}/{provider}"),
                discoverer.clone(),
                findings.len(),
                report
                    .as_ref()
                    .map(|_| ())
                    .map_err(|failure| failure.code().to_string()),
            )),
            _ => None,
        })
        .collect()
}

// ---- discovery ----

#[tokio::test]
async fn a_discoverer_runs_through_the_sandbox_and_its_findings_reach_control() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    let outcome = scene.discover("app").await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(
        discovered(&scene),
        [("app/example_json".to_owned(), "look".to_owned(), 1, Ok(()))]
    );
    // The driver behind the port was prepared for discovery, and policy
    // was asked about a job that writes nothing.
    assert_eq!(*scene.hooks.postures.lock().unwrap(), ["Discovery"]);
    let asked = scene.control.questions.lock().unwrap().last().cloned();
    assert!(!asked.unwrap().writes);
}

#[tokio::test]
async fn a_discoverer_that_tries_to_write_is_stopped_and_nothing_is_stored() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    let outcome = scene.discover("app").await;
    assert!(completed(&outcome), "{outcome:?}");

    let results = scene.discoverer.results.lock().unwrap().clone();
    // The planted step that commits was refused by the port, unsent,
    // before the driver saw it.
    let planted = results[0].as_ref().unwrap_err();
    assert_eq!(planted.failure.class(), FailureClass::Policy);
    assert_eq!(planted.failure.code().as_str(), "sandbox_read_only");
    assert_eq!(planted.sent, Sent::NotSent);
    // The same write, dressed as a step that commits nothing, passed the
    // kernel's half, which has only the token to go by, and was refused
    // by the driver's own guard for the posture: the pack's half.
    let dressed = results[1].as_ref().unwrap_err();
    assert_eq!(dressed.failure.code().as_str(), "read_only_posture");
    assert_eq!(dressed.sent, Sent::NotSent);
    // The read went through.
    assert!(
        matches!(results[2], Ok(StepOutcome::Absent(_))),
        "{results:?}"
    );
    assert_eq!(
        scene.steps(&["put_item", "get_item"]),
        ["put_item", "get_item"]
    );

    // No lock and no intent were asked for, and the test application's
    // control interface shows nothing written.
    let log = scene.hooks.seen();
    assert!(!log.iter().any(|entry| entry == "lock" || entry == "intend"));
    assert!(scene.app.writes().await.unwrap().is_empty());
    assert!(
        scene
            .app
            .stored_item("A-100", "L-900")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_discoverer_that_never_ends_is_ended_by_its_declared_bound_and_its_findings_stand() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    scene.discoverer.hangs.store(true, Ordering::SeqCst);
    let outcome = scene.discover("app").await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(
        discovered(&scene),
        [(
            "app/example_json".to_owned(),
            "look".to_owned(),
            1,
            Err("run_time_exceeded".to_owned())
        )]
    );
}

#[tokio::test]
async fn a_discoverer_that_panics_is_named_and_the_job_ends() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    scene.discoverer.panics.store(true, Ordering::SeqCst);
    let outcome = scene.discover("app").await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(
        discovered(&scene),
        [(
            "app/example_json".to_owned(),
            "look".to_owned(),
            1,
            Err("driver_panicked".to_owned())
        )]
    );
}

#[tokio::test]
async fn a_discovery_job_for_a_surface_the_plan_does_not_hold_leaves_the_run_as_it_was() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    let outcome = scene.discover("elsewhere").await;
    let JobOutcome::Left(failure) = &outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(failure.code().as_str(), "job_invalid");
    assert!(discovered(&scene).is_empty());
    assert!(scene.hooks.postures.lock().unwrap().is_empty());
}

// ---- a try: a draft in the sandbox ----

#[tokio::test]
async fn a_try_runs_a_draft_read_that_may_not_act_live() {
    let scene = Scene::new(ORDER_GET, |plan| {
        for binding in &mut plan.operations[0].bindings {
            binding.status = BindingStatus::Draft;
        }
    })
    .await;
    // Live, a draft binding is never chosen.
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Busy, "no_binding".to_owned())
    );
    assert!(scene.hooks.seen().iter().all(|entry| entry != "get_order"));

    // As a try it runs, in the sandbox's posture.
    scene.another_run(ORDER_GET).await;
    let outcome = scene.work(JobKind::Try, ORDER_GET, order()).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(
        *scene.hooks.postures.lock().unwrap(),
        ["Sandbox { test_records: false }"]
    );
    assert_eq!(
        scene.last_result().get("customer"),
        Some(&a2m_core::Value::from("Example Customer One"))
    );
}

#[tokio::test]
async fn a_try_of_a_write_is_refused_before_the_driver_unless_its_record_is_a_test_record() {
    let scene = Scene::new(ITEM_UPSERT, |plan| {
        plan.environments[0].test_records = vec![item_key("A-102", "T-1")];
    })
    .await;
    // A record that is not a declared test record: refused before any
    // driver is prepared.
    let other = item("A-102", "L-300", "New rail", 3, "150.5");
    let outcome = scene.work(JobKind::Try, ITEM_UPSERT, other).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Policy, "sandbox_read_only".to_owned())
    );
    assert!(scene.hooks.postures.lock().unwrap().is_empty());
    assert!(scene.app.writes().await.unwrap().is_empty());

    // The declared test record: previewed, approved, and written, with
    // its intent stored first, as any write is.
    scene.another_run(ITEM_UPSERT).await;
    let test_record = item("A-102", "T-1", "Trial", 1, "1");
    let outcome = scene
        .work(JobKind::Try, ITEM_UPSERT, test_record.clone())
        .await;
    assert!(waits(&outcome), "{outcome:?}");
    let diff = scene.run_row().await.diff_hash.unwrap();
    scene.control.approve(diff).await;
    let outcome = scene.work(JobKind::Try, ITEM_UPSERT, test_record).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.record_states().await, [RecordState::Verified]);
    assert_eq!(scene.app.write_count("A-102", "T-1").await.unwrap(), 1);
    assert_eq!(scene.app.writes().await.unwrap().len(), 1);
    assert!(
        scene
            .hooks
            .postures
            .lock()
            .unwrap()
            .iter()
            .all(|posture| posture == "Sandbox { test_records: true }")
    );
}

// ---- a second look after a lost lease ----

#[tokio::test]
async fn a_second_look_reports_a_verified_record_that_reads_differently_and_changes_nothing() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    let input = item("A-102", "L-300", "New rail", 3, "150.5");
    approved(&scene, ITEM_UPSERT, &input).await;
    let outcome = scene
        .work(JobKind::Execute, ITEM_UPSERT, input.clone())
        .await;
    assert!(completed(&outcome), "{outcome:?}");
    let differs = |scene: &Scene| {
        scene
            .control
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| matches!(event, EngineEvent::VerifiedDiffers { .. }))
            .count()
    };

    // The record still reads as it was written: nothing to report.
    let outcome = scene
        .work(JobKind::VerifyAfterLoss, ITEM_UPSERT, input.clone())
        .await;
    assert_eq!(outcome, JobOutcome::Settled { remaining: 0 });
    assert_eq!(differs(&scene), 0);

    // Someone else changes it. The second look reports the difference,
    // with evidence, and changes no state and writes nothing.
    let changed = scene
        .app
        .http()
        .put(scene.app.url("/api/orders/A-102/items/L-300"))
        .json(&serde_json::json!({
            "description": "New rail", "quantity": 9, "length_in": "150.5", "version": 1
        }))
        .send()
        .await
        .unwrap();
    assert!(changed.status().is_success());
    let outcome = scene
        .work(JobKind::VerifyAfterLoss, ITEM_UPSERT, input)
        .await;
    assert_eq!(outcome, JobOutcome::Settled { remaining: 0 });
    assert_eq!(differs(&scene), 1);
    assert_eq!(scene.record_states().await, [RecordState::Verified]);
    assert_eq!(scene.run_row().await.state, RunState::Completed);
    assert_eq!(scene.hooks.count("put_item"), 1);
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 2);
}
