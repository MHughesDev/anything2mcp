//! Build-order steps 5 to 7: the first verified write, normalization and
//! the preview, and reconcile and resume.
//!
//! Every test asks the test application what it stored; none proves a write
//! by the engine's own read path alone.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

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

// ---- what is stored and shown passes the redactor ----

/// Whether any piece of evidence the engine handed over holds `text`.
fn evidence_holds(scene: &Scene, text: &str) -> bool {
    scene
        .control
        .evidence_given
        .lock()
        .unwrap()
        .iter()
        .any(|bytes| String::from_utf8_lossy(bytes).contains(text))
}

#[tokio::test]
async fn a_secret_the_system_shows_is_in_nothing_the_engine_stores_or_shows() {
    let shown = "secret-for-tests-7c41e9";
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    scene.vault.register(&a2m_core::Secret::new(shown));

    // The system shows a registered secret in a field that carries no
    // label: the description of a line item that exists.
    let seeded = scene
        .app
        .http()
        .put(scene.app.url("/api/orders/A-102/items/L-300"))
        .json(&serde_json::json!({
            "description": format!("Rail {shown}"), "quantity": 3, "length_in": "150.5"
        }))
        .send()
        .await
        .unwrap();
    assert!(seeded.status().is_success());

    // The record is previewed for an update. Its diff, the values seen
    // before, and the intended values hold the secret nowhere, and the
    // diff shows that something was redacted.
    let input = item("A-102", "L-300", "New rail", 3, "150.5");
    approved(&scene, ITEM_UPSERT, &input).await;
    assert!(!evidence_holds(&scene, shown), "after the preview");
    assert!(evidence_holds(&scene, "[redacted]"));

    // This binding hands the save the record as it was last read, and
    // that now holds a mask where the secret was. A mask is never sent to
    // the system: the save is refused before it reaches the driver, and
    // the record stays as it was.
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, input).await;
    assert_eq!(
        common::failed_as(&outcome),
        (FailureClass::UnknownVariant, "argument_masked".to_owned())
    );
    assert_eq!(scene.hooks.count("put_item"), 0);
    let stored = scene.app.stored_item("A-102", "L-300").await.unwrap();
    assert_eq!(stored.unwrap().description, format!("Rail {shown}"));
    assert!(
        !evidence_holds(&scene, shown),
        "after the write was refused"
    );
    assert!(!format!("{outcome:?}").contains(shown));
}

#[tokio::test]
async fn a_write_that_carries_a_registered_secret_is_refused_before_anything_is_sent() {
    let written = "secret-for-tests-b20f55";
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    scene.vault.register(&a2m_core::Secret::new(written));
    let input = item("A-102", "L-301", &format!("Rail {written}"), 1, "20");
    let outcome = scene.work(JobKind::Preview, ITEM_UPSERT, input).await;
    // What is read back passes the redactor, so such a value could never
    // be compared with what the system holds.
    assert_eq!(
        common::failed_as(&outcome),
        (FailureClass::Input, "input_invalid".to_owned())
    );
    assert!(!format!("{outcome:?}").contains(written));
    assert!(scene.hooks.postures.lock().unwrap().is_empty());
    assert!(!evidence_holds(&scene, written));
    assert!(scene.app.writes().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_value_the_redactor_masked_is_never_sent_to_the_system() {
    // The code the system hands out before the save holds a registered
    // secret, so what the step returned is its mask. The save would take
    // the code as an argument.
    let scene = Scene::new(common::SLOT_ADD, |_| {}).await;
    scene.vault.register(&a2m_core::Secret::new("N-001"));
    let input = common::slot("A-102", 4, "12");
    approved(&scene, common::SLOT_ADD, &input).await;
    let outcome = scene.work(JobKind::Execute, common::SLOT_ADD, input).await;
    assert_eq!(
        common::failed_as(&outcome),
        (FailureClass::UnknownVariant, "argument_masked".to_owned())
    );
    // The save never reached the driver, nothing was written under the
    // mask, and nothing that was stored holds the code.
    assert_eq!(scene.hooks.count("add_item"), 0);
    assert!(scene.app.writes().await.unwrap().is_empty());
    assert!(!evidence_holds(&scene, "N-001"));
}

#[tokio::test]
async fn a_record_in_doubt_whose_stored_values_were_masked_is_not_looked_for_under_the_mask() {
    // The code the system hands out before the save is recorded for
    // reconcile. It holds a registered secret, so what is recorded is its
    // mask. The save itself does not take the code here, so it is sent,
    // and its answer is lost.
    let scene = Scene::new(common::SLOT_ADD, |compiled_target| {
        compiled_target.operations[7].bindings[0].act[1]
            .args
            .retain(|arg| arg.name != "code");
    })
    .await;
    scene.vault.register(&a2m_core::Secret::new("N-001"));
    let input = common::slot("A-102", 4, "12");
    approved(&scene, common::SLOT_ADD, &input).await;
    scene.hooks.lose_answer.store(true, Ordering::SeqCst);
    let outcome = scene
        .work(JobKind::Execute, common::SLOT_ADD, input.clone())
        .await;
    assert_eq!(
        common::failed_as(&outcome),
        (FailureClass::PossibleCommit, "no_confirmation".to_owned())
    );
    assert_eq!(scene.app.writes().await.unwrap().len(), 1);
    assert!(!evidence_holds(&scene, "N-001"));

    // On resume the record cannot be found by what was recorded. Reading
    // without it could find nothing, and nothing found would be taken for
    // a write that did not land: the run stops for a person, and nothing
    // is written a second time.
    scene.new_lease().await;
    let outcome = scene.work(JobKind::Execute, common::SLOT_ADD, input).await;
    assert_eq!(
        common::failed_as(&outcome),
        (FailureClass::PossibleCommit, "cannot_reconcile".to_owned())
    );
    assert_eq!(scene.hooks.count("add_item"), 1);
    assert_eq!(scene.app.writes().await.unwrap().len(), 1);
}

// ---- the binding a person approved is the binding that writes ----

/// The stored diff of the scene's run, read back.
async fn stored_diff(scene: &Scene) -> a2m_engine::Diff {
    let blob = scene.run_row().await.diff.expect("a diff was stored");
    let opened = scene.control.store.blobs().get(&blob).await.unwrap();
    a2m_engine::Diff::decode(opened.expose()).unwrap()
}

#[tokio::test]
async fn a_write_through_another_binding_than_the_approved_one_is_not_sent() {
    // A keyless create with two bindings: the first reads its record back,
    // the second offers only the system's acknowledgement.
    let scene = Scene::new(common::SLOT_ADD, |compiled_target| {
        common::keyless_slot_add(compiled_target);
        let operation = &mut compiled_target.operations[7];
        let mut second = operation.bindings[0].clone();
        second.mode = a2m_core::Mode::new(common::SECOND_MODE).unwrap();
        second.evidence = a2m_core::WriteEvidence::Acknowledged;
        operation.bindings.push(second);
    })
    .await;
    let input = common::slot("A-102", 4, "12");
    approved(&scene, common::SLOT_ADD, &input).await;
    // The person was shown the first binding and its evidence.
    let diff = stored_diff(&scene).await;
    let named = diff.records[0]
        .binding
        .as_ref()
        .expect("the binding is named");
    assert_eq!(
        (named.position, named.evidence),
        (0, a2m_core::WriteEvidence::ReadBack)
    );
    assert!(!diff.records[0].not_read_back());

    // At the write only the second binding may act: nothing is sent
    // through it, because its evidence is not what was approved.
    *scene.pin.lock().unwrap() = Some(a2m_core::Mode::new(common::SECOND_MODE).unwrap());
    let outcome = scene.work(JobKind::Execute, common::SLOT_ADD, input).await;
    assert_eq!(
        common::failed_as(&outcome),
        (FailureClass::Policy, "approval_stale".to_owned())
    );
    assert_eq!(scene.hooks.count("intend"), 0);
    assert_eq!(scene.hooks.count("reserve_code"), 0);
    assert_eq!(scene.hooks.count("add_item"), 0);
    assert!(scene.app.writes().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_record_whose_binding_is_chosen_later_is_shown_with_every_binding_it_could_take() {
    let scene = Scene::new(common::KIT_ENTER, |compiled_target| {
        // The lines' operation gets a second binding that is only
        // acknowledged.
        let operation = &mut compiled_target.operations[2];
        operation.effect = a2m_core::EffectClass::Irreversible;
        let mut second = operation.bindings[0].clone();
        second.mode = a2m_core::Mode::new(common::SECOND_MODE).unwrap();
        second.evidence = a2m_core::WriteEvidence::Acknowledged;
        second.read_back.clear();
        operation.bindings.push(second);
        for pipeline in &mut compiled_target.pipelines {
            pipeline.effect = a2m_core::EffectClass::Irreversible;
        }
        for tool in &mut compiled_target.tools {
            tool.annotations.read_only = false;
        }
    })
    .await;
    *scene.source.lock().unwrap() = Some(std::sync::Arc::new(common::sheet(&[(
        "A", "Left leg", "2", "30",
    )])));
    let kit = a2m_core::Values::new()
        .with("order_id", "A-102")
        .with("title", "Table kit");
    let outcome = scene.work(JobKind::Preview, common::KIT_ENTER, kit).await;
    assert!(waits(&outcome), "{outcome:?}");
    let diff = stored_diff(&scene).await;
    // The header's binding is known; the line's waits for the header's
    // key, so the diff names both bindings it could take, and warns that
    // its write may not be read back.
    assert!(diff.records[0].binding.is_some());
    let line = &diff.records[1];
    assert!(line.binding_unknown());
    assert_eq!(
        line.could_take
            .iter()
            .map(|binding| (binding.position, binding.evidence))
            .collect::<Vec<_>>(),
        [
            (0, a2m_core::WriteEvidence::ReadBack),
            (1, a2m_core::WriteEvidence::Acknowledged)
        ]
    );
    assert!(line.not_read_back());
}

// ---- decision 0315: every declared output is in a write's result ----

/// The line-item upsert with one more output, `batch`, which neither its
/// save nor its read-back observes.
fn with_batch(may_be_absent: bool) -> impl FnOnce(&mut a2m_core::CompiledTarget) {
    move |compiled_target| {
        let operation = compiled_target
            .operations
            .iter_mut()
            .find(|operation| operation.name.as_str() == ITEM_UPSERT)
            .unwrap();
        operation.outputs.push(a2m_core::CompiledOutput {
            name: "batch".to_owned(),
            ty: a2m_core::ValueType::Text,
            field_type: None,
            may_be_absent,
        });
    }
}

#[tokio::test]
async fn a_write_whose_read_back_does_not_observe_an_output_gives_null_or_stops() {
    // An output that may be absent and was not observed is in the result,
    // as `null`.
    let scene = Scene::new(ITEM_UPSERT, with_batch(true)).await;
    let input = item("A-102", "L-300", "New rail", 3, "150.5");
    approved(&scene, ITEM_UPSERT, &input).await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, input).await;
    assert!(completed(&outcome), "{outcome:?}");
    let result = scene.last_result();
    assert_eq!(result.get("batch"), Some(&Value::Null), "{result:?}");
    assert_eq!(result.get("version"), Some(&Value::from(1_i64)));
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);

    // One that may not be absent stops the run with the engine's own code:
    // the result would not hold what its schema requires.
    let scene = Scene::new(ITEM_UPSERT, with_batch(false)).await;
    let input = item("A-102", "L-300", "New rail", 3, "150.5");
    approved(&scene, ITEM_UPSERT, &input).await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, input).await;
    let failure = outcome.failure().expect("a missing output stops the run");
    assert_eq!(failure.code().as_str(), "output_unexpected", "{failure:?}");
    assert!(failure.message().contains("`batch`"), "{failure:?}");
    // The write itself landed once, and the ledger says so.
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);
    assert_eq!(scene.record_states().await, [RecordState::Verified]);
}

#[tokio::test]
async fn a_record_found_unchanged_gives_every_output_and_on_resume_is_read_again() {
    // What the test application is seeded with: the record is found
    // unchanged, nothing is written, and its result holds every output.
    let input = item("A-100", "L-101", "Left rail", 2, "150.5");
    let scene = Scene::new(ITEM_UPSERT, with_batch(true)).await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, input).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.record_states().await, [RecordState::Unchanged]);
    let result = scene.last_result();
    assert_eq!(result.get("batch"), Some(&Value::Null), "{result:?}");
    assert_eq!(result.get("version"), Some(&Value::from(1_i64)));
    assert!(scene.app.writes().await.unwrap().is_empty());

    // A sheet of one row the system holds already and one it does not.
    // The preview finds the first unchanged and stores no outputs for it;
    // the approved execute resumes from the snapshot and reads it once
    // more, and its outputs are worked out by the same rule.
    let rows = || {
        std::sync::Arc::new(common::sheet(&[
            ("L-101", "Left rail", "2", "150.5"),
            ("L-900", "Spare rail", "1", "60"),
        ]))
    };
    let order = a2m_core::Values::new().with("order_id", "A-100");
    let scene = Scene::new(common::SHEET_ENTER, with_batch(true)).await;
    *scene.source.lock().unwrap() = Some(rows());
    let outcome = scene
        .work(JobKind::Preview, common::SHEET_ENTER, order.clone())
        .await;
    assert!(waits(&outcome), "{outcome:?}");
    assert_eq!(
        scene.record_states().await,
        [RecordState::Unchanged, RecordState::Planned]
    );
    let diff = scene.run_row().await.diff_hash.expect("a stored diff");
    scene.control.approve(diff).await;
    let before = scene.hooks.count("get_item");
    let outcome = scene
        .work(JobKind::Execute, common::SHEET_ENTER, order.clone())
        .await;
    assert!(completed(&outcome), "{outcome:?}");
    assert!(scene.hooks.count("get_item") > before);
    assert_eq!(scene.app.write_count("A-100", "L-900").await.unwrap(), 1);
    assert_eq!(scene.app.write_count("A-100", "L-101").await.unwrap(), 0);

    // An output that may not be absent and that the record does not give
    // stops the run where it is first worked out, before anything is
    // written.
    let scene = Scene::new(common::SHEET_ENTER, with_batch(false)).await;
    *scene.source.lock().unwrap() = Some(rows());
    let outcome = scene
        .work(JobKind::Preview, common::SHEET_ENTER, order)
        .await;
    let failure = outcome.failure().expect("a missing output stops the run");
    assert_eq!(failure.code().as_str(), "output_unexpected", "{failure:?}");
    assert!(scene.app.writes().await.unwrap().is_empty());
}

// ---- decision 0444: a result's number is padded to its type's places ----

/// The line-item upsert with one more output, `length_in`, which its
/// read-back observes as the application stores it, typed by an exact
/// number of `places`.
fn with_length(places: u32) -> impl FnOnce(&mut a2m_core::CompiledTarget) {
    move |compiled_target| {
        let field_type = common::decimal_type(compiled_target, "length_shown", places);
        let operation = compiled_target
            .operations
            .iter_mut()
            .find(|operation| operation.name.as_str() == ITEM_UPSERT)
            .unwrap();
        operation.outputs.push(a2m_core::CompiledOutput {
            name: "length_in".to_owned(),
            ty: a2m_core::ValueType::Decimal,
            field_type: Some(a2m_core::FieldTypeIndex::new(field_type)),
            may_be_absent: false,
        });
    }
}

/// The result's `length_in` after a verified write of `length`, as the
/// engine hands it on and as its stored form reads back.
async fn length_shown(places: u32, code: &str, length: &str) -> (String, String) {
    let scene = Scene::new(ITEM_UPSERT, with_length(places)).await;
    let input = item("A-102", code, "New rail", 3, length);
    approved(&scene, ITEM_UPSERT, &input).await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, input).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.record_states().await, [RecordState::Verified]);
    let result = scene.last_result();
    let shown = match result.get("length_in") {
        Some(Value::Decimal(number)) => number.to_string(),
        other => panic!("the result's length is {other:?}"),
    };
    let stored = a2m_engine::encode_values(&result).unwrap();
    let read = a2m_engine::decode_values(&stored).unwrap();
    assert_eq!(read, result);
    let read = match read.get("length_in") {
        Some(Value::Decimal(number)) => number.to_string(),
        other => panic!("the stored length is {other:?}"),
    };
    (shown, read)
}

#[tokio::test]
async fn a_results_decimal_is_padded_to_the_places_its_field_type_states() {
    // The application stores 120 and 21.5, which it shows as `120` and
    // `21.5`; a type of one place and one of two give them with those.
    for (places, code, length, want) in
        [(1, "L-300", "120", "120.0"), (2, "L-301", "21.5", "21.50")]
    {
        let (shown, read) = length_shown(places, code, length).await;
        assert_eq!(shown, want, "{length} to {places}");
        assert_eq!(read, want, "{length} to {places}, stored");
    }
}

#[tokio::test]
async fn a_results_decimal_with_more_places_than_its_type_is_rounded_by_the_type_alone() {
    // The application stores 21 9/16 inches, `21.5625`. A type of two
    // places, rounded half to even, gives `21.56` by its own rule, as it
    // normalizes every value it compares; the padding adds and never takes
    // away a digit. The write itself is compared by the input's own type,
    // of four places, and is verified as before.
    let (shown, read) = length_shown(2, "L-302", "21.5625").await;
    assert_eq!(shown, "21.56");
    assert_eq!(read, "21.56");
}
