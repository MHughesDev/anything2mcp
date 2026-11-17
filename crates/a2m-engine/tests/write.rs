//! Build-order steps 5 to 7: the first verified write, normalization and
//! the preview, and reconcile and resume.
//!
//! Every test asks the test application what it stored; none proves a write
//! by the engine's own read path alone.

mod common;

use std::sync::atomic::Ordering;

use a2m_core::{FailureClass, RecordState, RunState, Value};
use a2m_engine::{JobKind, JobOutcome};
use common::{ITEM_UPSERT, ITEM_UPSERT_EXACT, Scene, item};

fn completed(outcome: &JobOutcome) -> bool {
    matches!(
        outcome,
        JobOutcome::Finished {
            state: RunState::Completed,
            failure: None
        }
    )
}

fn waits(outcome: &JobOutcome) -> bool {
    matches!(
        outcome,
        JobOutcome::Rests {
            state: RunState::NeedsApproval,
            failure: None
        }
    )
}

/// The position of the first entry of the log that is `what`.
fn at(log: &[String], what: &str) -> usize {
    log.iter()
        .position(|entry| entry == what)
        .unwrap_or_else(|| panic!("the log has no `{what}`: {log:?}"))
}

/// Previews a write, has a person approve its diff, and leaves the run
/// waiting for the execute.
async fn approved(scene: &Scene, tool: &str, input: &a2m_core::Values) {
    let outcome = scene.work(JobKind::Preview, tool, input.clone()).await;
    assert!(waits(&outcome), "{outcome:?}");
    let diff = scene
        .run_row()
        .await
        .diff_hash
        .expect("a preview stores a diff");
    scene.control.approve(diff).await;
}

// ---- step 5: intend, act, read back, record ----

#[tokio::test]
async fn a_write_is_previewed_approved_intended_sent_read_back_and_verified() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    let input = item("A-102", "L-300", "New rail", 3, "150.5");

    // The preview plans the record and stores the diff, and writes nothing.
    let outcome = scene
        .work(JobKind::Preview, ITEM_UPSERT, input.clone())
        .await;
    assert!(waits(&outcome), "{outcome:?}");
    assert_eq!(scene.record_states().await, [RecordState::Planned]);
    assert_eq!(scene.hooks.count("put_item"), 0);
    assert!(scene.app.writes().await.unwrap().is_empty());
    let diff = scene
        .run_row()
        .await
        .diff_hash
        .expect("a preview stores a diff");

    // An execute with no approval waits, and still writes nothing.
    let outcome = scene
        .work(JobKind::Execute, ITEM_UPSERT, input.clone())
        .await;
    assert!(waits(&outcome), "{outcome:?}");
    assert_eq!(scene.hooks.count("put_item"), 0);

    scene.control.approve(diff).await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, input).await;
    assert!(completed(&outcome), "{outcome:?}");

    // The test application holds the line item, written once.
    let stored = scene
        .app
        .stored_item("A-102", "L-300")
        .await
        .unwrap()
        .expect("the item was stored");
    assert_eq!(stored.description, "New rail");
    assert_eq!(stored.quantity, 3);
    assert_eq!(stored.length_sixteenths, 2408);
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);

    // The ledger holds the record as verified, and the run as completed.
    assert_eq!(scene.record_states().await, [RecordState::Verified]);
    assert_eq!(scene.run_row().await.state, RunState::Completed);

    // Ledger rule 1: the lock and the intent were stored before the write
    // reached the driver; the write was marked before it was read back.
    let log = scene.hooks.seen();
    let sent = at(&log, "put_item");
    assert!(at(&log, "lock") < at(&log, "intend"), "{log:?}");
    assert!(at(&log, "intend") < sent, "{log:?}");
    assert!(sent < at(&log, "acted"), "{log:?}");
    assert!(at(&log, "acted") < at(&log, "verified"), "{log:?}");
    let read_back = log
        .iter()
        .rposition(|entry| entry == "get_item")
        .expect("the record was read back");
    assert!(
        sent < read_back && read_back < at(&log, "verified"),
        "{log:?}"
    );
    assert_eq!(
        scene.last_result().get("version"),
        Some(&Value::from(1_i64))
    );
}

#[tokio::test]
async fn an_approval_of_another_diff_does_not_let_the_run_write() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    let input = item("A-102", "L-300", "New rail", 3, "150.5");
    let outcome = scene
        .work(JobKind::Preview, ITEM_UPSERT, input.clone())
        .await;
    assert!(waits(&outcome), "{outcome:?}");
    scene
        .control
        .approve(a2m_core::Digest::of(b"some other diff"))
        .await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, input).await;
    assert!(outcome.failure().is_some(), "{outcome:?}");
    assert_eq!(scene.hooks.count("put_item"), 0);
    assert!(scene.app.writes().await.unwrap().is_empty());
    assert_eq!(scene.record_states().await, [RecordState::Planned]);
}

// ---- step 6: normalization, comparison, the preview ----

#[tokio::test]
async fn values_are_compared_after_normalization() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    // The description is trimmed by its field type, and the length is an
    // exact number: `150.50` is what the application shows as `150.5`.
    let input = item("A-102", "L-300", "  New rail ", 3, "150.50");
    approved(&scene, ITEM_UPSERT, &input).await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, input).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.record_states().await, [RecordState::Verified]);
    let stored = scene
        .app
        .stored_item("A-102", "L-300")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.description, "New rail");
}

#[tokio::test]
async fn a_value_that_reads_back_different_is_a_mismatch_and_is_not_verified() {
    let scene = Scene::new(ITEM_UPSERT_EXACT, |_| {}).await;
    // This field type compares text as written, and the application trims
    // what it stores: the write lands, and does not read back equal.
    let input = item("A-102", "L-300", "New rail  ", 3, "150.5");
    approved(&scene, ITEM_UPSERT_EXACT, &input).await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT_EXACT, input).await;
    let JobOutcome::Stopped(failure) = &outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(failure.class(), FailureClass::Mismatch);
    assert_eq!(scene.record_states().await, [RecordState::Failed]);
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);
    assert_eq!(scene.run_row().await.state, RunState::Stopped);
}

#[tokio::test]
async fn a_record_that_already_holds_the_values_is_unchanged_and_not_written() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    // What the test application is seeded with: 2408 sixteenths is 150.5.
    let input = item("A-100", "L-101", "Left rail", 2, "150.5");
    let outcome = scene
        .work(JobKind::Preview, ITEM_UPSERT, input.clone())
        .await;
    assert_eq!(
        scene.record_states().await,
        [RecordState::Unchanged],
        "{outcome:?}"
    );
    assert_eq!(scene.hooks.count("put_item"), 0);
    assert!(scene.app.writes().await.unwrap().is_empty());
}

#[tokio::test]
async fn an_existing_record_is_updated_at_the_version_it_was_read_at() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    let input = item("A-100", "L-101", "Left rail", 5, "150.5");
    approved(&scene, ITEM_UPSERT, &input).await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, input).await;
    assert!(completed(&outcome), "{outcome:?}");
    let stored = scene
        .app
        .stored_item("A-100", "L-101")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.quantity, 5);
    assert_eq!(stored.version, 2);
    assert_eq!(scene.record_states().await, [RecordState::Verified]);
}

#[tokio::test]
async fn a_record_changed_since_its_preview_is_stale_and_is_not_written() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    let input = item("A-100", "L-101", "Left rail", 5, "150.5");
    approved(&scene, ITEM_UPSERT, &input).await;
    // Someone else changes the record after the diff was approved.
    let changed = scene
        .app
        .http()
        .put(scene.app.url("/api/orders/A-100/items/L-101"))
        .json(&serde_json::json!({
            "description": "Left rail", "quantity": 9, "length_in": "150.5", "version": 1
        }))
        .send()
        .await
        .unwrap();
    assert!(changed.status().is_success());
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, input).await;
    let failure = outcome.failure().expect("the run does not complete");
    assert_eq!(failure.class(), FailureClass::Stale, "{outcome:?}");
    assert_eq!(scene.hooks.count("put_item"), 0);
    // The one write the application saw is the other person's.
    assert_eq!(scene.app.write_count("A-100", "L-101").await.unwrap(), 1);
    let stored = scene
        .app
        .stored_item("A-100", "L-101")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.quantity, 9);
}

// ---- step 7: reconcile and resume ----

#[tokio::test]
async fn a_write_whose_answer_was_lost_is_reconciled_and_never_sent_twice() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    let input = item("A-102", "L-300", "New rail", 3, "150.5");
    approved(&scene, ITEM_UPSERT, &input).await;
    scene.hooks.lose_answer.store(true, Ordering::SeqCst);
    let outcome = scene
        .work(JobKind::Execute, ITEM_UPSERT, input.clone())
        .await;
    // The write may have landed: the run stops, and the write is not tried
    // again by this attempt.
    let JobOutcome::Stopped(failure) = &outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(failure.class(), FailureClass::PossibleCommit);
    assert_eq!(scene.hooks.count("put_item"), 1);
    assert_ne!(scene.record_states().await, [RecordState::Verified]);

    // Another runner claims the run. Reconcile reads the record, finds the
    // write landed, and settles it without writing.
    scene.new_lease().await;
    let outcome = scene
        .work(JobKind::Reconcile, ITEM_UPSERT, input.clone())
        .await;
    assert_eq!(outcome, JobOutcome::Settled { remaining: 0 });
    assert_eq!(scene.record_states().await, [RecordState::Verified]);
    assert_eq!(scene.hooks.count("put_item"), 1);
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);

    // Resumed, the run completes with nothing left to write.
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, input).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.hooks.count("put_item"), 1);
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);
}

#[tokio::test]
async fn a_resume_reconciles_before_it_writes_and_writes_what_did_not_land() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    let input = item("A-102", "L-300", "New rail", 3, "150.5");
    approved(&scene, ITEM_UPSERT, &input).await;
    // The intent is stored and the answer is lost: nothing was sent, and
    // the ledger holds the record in doubt.
    scene
        .control
        .faults
        .intend_answer_lost
        .store(true, Ordering::SeqCst);
    let outcome = scene
        .work(JobKind::Execute, ITEM_UPSERT, input.clone())
        .await;
    assert!(outcome.failure().is_some(), "{outcome:?}");
    assert_eq!(scene.hooks.count("put_item"), 0);
    assert_eq!(scene.record_states().await, [RecordState::Intent]);

    scene.new_lease().await;
    let before = scene.hooks.seen().len();
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, input).await;
    assert!(completed(&outcome), "{outcome:?}");
    // Ledger rule 5: on this attempt the record was read before it was
    // written, and it was written once.
    let log = scene.hooks.seen()[before..].to_vec();
    assert!(at(&log, "get_item") < at(&log, "put_item"), "{log:?}");
    assert!(
        log.iter().any(|entry| entry.starts_with("settle:")),
        "{log:?}"
    );
    assert_eq!(scene.hooks.count("put_item"), 1);
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);
    assert_eq!(scene.record_states().await, [RecordState::Verified]);
}

#[tokio::test]
async fn an_intent_that_cannot_be_stored_sends_nothing() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    let input = item("A-102", "L-300", "New rail", 3, "150.5");
    approved(&scene, ITEM_UPSERT, &input).await;
    scene
        .control
        .faults
        .intend_refused
        .store(true, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, input).await;
    // No ledger, no write.
    assert!(outcome.failure().is_some(), "{outcome:?}");
    assert_eq!(scene.hooks.count("put_item"), 0);
    assert!(scene.app.writes().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_write_that_was_never_sent_fails_cleanly() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    let input = item("A-102", "L-300", "New rail", 3, "150.5");
    approved(&scene, ITEM_UPSERT, &input).await;
    scene.hooks.fail_unsent.store(true, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, input).await;
    let failure = outcome.failure().expect("the run does not complete");
    // The driver says nothing was sent, so the failure keeps its own class
    // and is not a possible commit.
    assert_eq!(failure.class(), FailureClass::UnknownVariant, "{outcome:?}");
    assert_eq!(scene.record_states().await, [RecordState::Failed]);
    assert!(scene.app.writes().await.unwrap().is_empty());
}
