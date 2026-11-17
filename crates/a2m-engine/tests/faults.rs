//! Build-order step 9: the rows of spec section 224 that are the engine's,
//! each with its fault injected through the test `Control` or the test
//! pack's driver, and the invariants of the design that had no test.
//!
//! Three rules hold throughout: no ledger, no write; nothing unverified
//! runs; reads degrade before writes do, and both fail closed. Every test
//! of a write asks the test application what it stored.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use a2m_core::{
    BindingStatus, FailureClass, FailureCode, Mode, OperationName, Plan, RecordState, RunState,
    Value,
};
use a2m_engine::{Directive, JobKind, JobOutcome, KernelHealth, codes};
use common::{
    FIRST_MODE, ITEM_UPSERT, KIT_ENTER, ORDER_FILL, ORDER_GET, SECOND_MODE, SLOT_ADD, Scene,
    approved, at, completed, failed_as, item, sheet, slot, step,
};

fn rail() -> a2m_core::Values {
    item("A-102", "L-300", "New rail", 3, "150.5")
}

fn order() -> a2m_core::Values {
    a2m_core::Values::new().with("order_id", "A-100")
}

/// A scene whose engine gives a call to `Control` up after a short time.
async fn impatient(tool: &str) -> Scene {
    Scene::with_settings(
        tool,
        |_| {},
        |settings| settings.control_timeout = Duration::from_millis(40),
    )
    .await
}

// ---- a write that was sent is in doubt until it is verified ----

/// Gives the binding that writes a line item a step that confirms the
/// save, or a step after the save: a read of the record that was written.
fn checks_after_the_save(plan: &mut Plan, as_confirm: bool) {
    let binding = &mut plan.operations[2].bindings[0];
    let mut check = step("check_item", &[], false, false);
    check.args = binding.read_back[0].args.clone();
    if as_confirm {
        binding.confirm.push(check);
    } else {
        binding.act.push(check);
    }
}

/// The run stopped with its write in doubt: the test application holds the
/// write, the ledger does not call it verified, and the record's lock is
/// kept for reconcile.
async fn in_doubt_with_its_lock_kept(scene: &Scene, outcome: &JobOutcome, why: &str) {
    assert_eq!(
        failed_as(outcome),
        (FailureClass::PossibleCommit, "no_confirmation".to_owned()),
        "{why}"
    );
    assert_eq!(
        scene.app.write_count("A-102", "L-300").await.unwrap(),
        1,
        "{why}"
    );
    assert_eq!(scene.run_row().await.state, RunState::Stopped, "{why}");
    let records = scene.records().await;
    assert_eq!(records.len(), 1, "{why}");
    assert_ne!(records[0].state, RecordState::Verified, "{why}");
    if let Some((class, _)) = &records[0].failure {
        assert_eq!(*class, FailureClass::PossibleCommit, "{why}");
    }
    let locks = scene.locks().await;
    assert_eq!(locks.len(), 1, "the lock was released: {why}");
    assert!(locks[0].quarantined, "{why}");
}

/// A resume reads the record first, finds the write landed, and writes
/// nothing.
async fn resumed_as_landed(scene: &Scene, why: &str) {
    scene.new_lease().await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert!(completed(&outcome), "{why}: {outcome:?}");
    assert_eq!(scene.hooks.count("settle:Verified"), 1, "{why}");
    assert_eq!(scene.hooks.count("put_item"), 1, "{why}");
    assert_eq!(
        scene.app.write_count("A-102", "L-300").await.unwrap(),
        1,
        "{why}"
    );
    assert_eq!(scene.record_states().await, [RecordState::Verified]);
}

/// The three classes that would settle a record as not written, each with
/// a code the test pack declares, as the packs declare theirs for steps
/// that read.
const SETTLING: [(FailureClass, &str); 3] = [
    (FailureClass::Policy, "example_too_large"),
    (FailureClass::Input, "example_bad_shape"),
    (FailureClass::Stale, "example_gone"),
];

#[tokio::test]
async fn a_confirm_that_fails_after_the_save_was_sent_leaves_the_write_in_doubt() {
    for (class, code) in SETTLING {
        let why = format!("a confirm step that fails as {class}/{code}");
        let scene = Scene::new(ITEM_UPSERT, |plan| checks_after_the_save(plan, true)).await;
        approved(&scene, ITEM_UPSERT, &rail()).await;
        *scene.hooks.fails_after.lock().unwrap() = Some(("check_item".to_owned(), class, code));
        let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
        in_doubt_with_its_lock_kept(&scene, &outcome, &why).await;
        // The failure names what it stands in for.
        let message = outcome.failure().unwrap().message().to_owned();
        assert!(message.contains(&format!("{class}/{code}")), "{message}");
        resumed_as_landed(&scene, &why).await;
    }
}

#[tokio::test]
async fn a_read_back_that_fails_after_the_save_was_sent_leaves_the_write_in_doubt() {
    for (class, code) in SETTLING {
        let why = format!("a read-back step that fails as {class}/{code}");
        let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
        approved(&scene, ITEM_UPSERT, &rail()).await;
        // The read that follows the save fails; the reads before it do not.
        let hooks = scene.hooks.clone();
        scene.hooks.on_step(move |kind| {
            if kind == "put_item" {
                *hooks.fails_after.lock().unwrap() = Some(("get_item".to_owned(), class, code));
            }
        });
        let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
        in_doubt_with_its_lock_kept(&scene, &outcome, &why).await;
        resumed_as_landed(&scene, &why).await;
    }
}

#[tokio::test]
async fn a_step_after_the_save_that_fails_leaves_the_write_in_doubt() {
    for (class, code) in SETTLING {
        let why = format!("an act step after the save that fails as {class}/{code}");
        let scene = Scene::new(ITEM_UPSERT, |plan| checks_after_the_save(plan, false)).await;
        approved(&scene, ITEM_UPSERT, &rail()).await;
        *scene.hooks.fails_after.lock().unwrap() = Some(("check_item".to_owned(), class, code));
        let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
        in_doubt_with_its_lock_kept(&scene, &outcome, &why).await;
        resumed_as_landed(&scene, &why).await;
    }
}

#[tokio::test]
async fn a_save_whose_answer_the_driver_does_not_accept_is_in_doubt_and_not_refused() {
    // The save reached the application, which took it; the driver then
    // fails the step as answered, with a class that says "not allowed" or
    // "bad input": a redirect it may not follow, an answer too large or of
    // the wrong shape. That is no refusal by the application.
    for (class, code) in [SETTLING[0], SETTLING[1]] {
        let why = format!("a save answered and failed as {class}/{code}");
        let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
        approved(&scene, ITEM_UPSERT, &rail()).await;
        *scene.hooks.fails_after.lock().unwrap() = Some(("put_item".to_owned(), class, code));
        let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
        in_doubt_with_its_lock_kept(&scene, &outcome, &why).await;
        resumed_as_landed(&scene, &why).await;
    }
}

#[tokio::test]
async fn the_applications_own_refusal_in_a_confirm_step_stands_and_the_record_is_still_read() {
    // The application answers the confirmation with a refusal. The class
    // stands, so the caller is told what the system said; the record is
    // not settled by it: its lock is kept and a resume reads it first.
    let scene = Scene::new(ITEM_UPSERT, |plan| checks_after_the_save(plan, true)).await;
    approved(&scene, ITEM_UPSERT, &rail()).await;
    *scene.hooks.fails_after.lock().unwrap() = Some((
        "check_item".to_owned(),
        FailureClass::Rejected,
        "example_refused",
    ));
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Rejected, "example_refused".to_owned())
    );
    assert_eq!(scene.run_row().await.state, RunState::Stopped);
    let locks = scene.locks().await;
    assert_eq!(locks.len(), 1);
    assert!(locks[0].quarantined);
    resumed_as_landed(&scene, "a confirm step the application refused").await;
}

#[tokio::test]
async fn a_record_in_doubt_that_cannot_be_read_stays_in_doubt_and_the_run_can_be_resumed() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    approved(&scene, ITEM_UPSERT, &rail()).await;
    scene.hooks.lose_answer.store(true, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(failed_as(&outcome).0, FailureClass::PossibleCommit);

    // On resume the read of the record in doubt fails with a class that
    // would end a run for good. The record was not read, so nothing is
    // settled: the run stops, and its lock is kept.
    for (class, code) in SETTLING {
        *scene.hooks.fails_after.lock().unwrap() = Some(("get_item".to_owned(), class, code));
        scene.new_lease().await;
        let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
        assert_eq!(
            failed_as(&outcome),
            (FailureClass::PossibleCommit, "cannot_reconcile".to_owned()),
            "{class}/{code}"
        );
        assert_eq!(scene.run_row().await.state, RunState::Stopped);
        assert_eq!(scene.locks().await.len(), 1);
    }
    resumed_as_landed(&scene, "a record in doubt that could not be read").await;
}

#[tokio::test]
async fn a_binding_that_saves_twice_is_refused_before_anything_is_sent() {
    // The plan check in the core crate holds every binding of an operation
    // to the operation's number of commit points, so a binding that saves
    // twice cannot stand beside one that saves once. An operation all of
    // whose bindings save twice is refused when its records are worked
    // out, and again for the binding that is chosen to write: nothing is
    // locked, intended, or sent, so no second save can meet the handle
    // without a token.
    let scene = Scene::new(KIT_ENTER, |plan| {
        let operation = &mut plan.operations[2];
        operation.commit_points = 2;
        let save = operation.bindings[0].act[0].clone();
        operation.bindings[0].act.push(save);
        let mut spare = operation.bindings[0].clone();
        spare.mode = Mode::new(SECOND_MODE).unwrap();
        operation.bindings[0].status = BindingStatus::Retired;
        operation.bindings.push(spare);
    })
    .await;
    *scene.source.lock().unwrap() = Some(std::sync::Arc::new(sheet(&[("LEG", "Leg", "4", "28")])));
    let input = a2m_core::Values::new()
        .with("order_id", "A-102")
        .with("title", "Table kit");
    let outcome = scene.work(JobKind::Execute, KIT_ENTER, input).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Busy, "job_kind_not_built".to_owned())
    );
    assert!(
        scene
            .steps(&["lock", "intend", "put_item", "add_item"])
            .is_empty()
    );
    assert!(scene.app.writes().await.unwrap().is_empty());
}

#[tokio::test]
async fn outputs_that_cannot_be_stored_with_the_save_stop_the_job_and_are_read_again() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    approved(&scene, ITEM_UPSERT, &rail()).await;
    // The evidence store refuses the first piece it is given once the save
    // has reached the application: the record's outputs.
    scene
        .control
        .faults
        .outputs_refused
        .store(true, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Busy, "evidence_not_stored".to_owned())
    );
    // The ledger says the write was sent, and holds no outputs for it.
    let records = scene.records().await;
    assert_eq!(records[0].state, RecordState::Acted);
    assert!(records[0].outputs.is_none());

    // Resumed: the record is read once, found as intended, and not written
    // again. What the read gave is kept with the record as it is settled,
    // so the tool's result has its outputs without a second read, and so
    // does any later resume (decision 0145).
    let reads = scene.hooks.count("get_item");
    scene.new_lease().await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.hooks.count("put_item"), 1);
    assert_eq!(scene.hooks.count("get_item"), reads + 1);
    assert!(matches!(
        scene.last_result().get("version"),
        Some(Value::Int(_))
    ));
    let records = scene.records().await;
    assert_eq!(records[0].state, RecordState::Verified);
    assert!(records[0].outputs.is_some());
}

#[tokio::test]
async fn a_record_that_failed_with_the_code_of_a_blocked_child_is_read_when_it_had_an_intent() {
    // The save reached the application, and the ledger could not be told:
    // the record is left in `intent`.
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    approved(&scene, ITEM_UPSERT, &rail()).await;
    scene
        .control
        .faults
        .acted_refused
        .store(true, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(failed_as(&outcome).0, FailureClass::PossibleCommit);
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);
    scene
        .control
        .faults
        .acted_refused
        .store(false, Ordering::SeqCst);

    // The record is then marked failed under the name the engine gives a
    // record that waited for its parent, as a row written by something
    // other than this engine may be. The name alone is not trusted.
    let record = scene.records().await[0].record;
    let named = common::failure(
        FailureClass::Busy,
        "blocked_by_parent",
        "The record waits for a record that failed earlier in the run.",
    );
    a2m_engine::Control::failed(scene.control.as_ref(), record, &named, None)
        .await
        .unwrap();
    assert_eq!(scene.record_states().await, [RecordState::Failed]);

    // On resume the record is read like every record in doubt, because it
    // had an intent; it is not sent back to `planned` unread and written
    // a second time.
    resumed_as_landed(&scene, "a failure named `blocked_by_parent`").await;
}

// ---- the database cannot commit ----

#[tokio::test]
async fn a_write_that_was_sent_and_cannot_be_recorded_is_in_doubt_and_never_a_success() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    approved(&scene, ITEM_UPSERT, &rail()).await;
    scene
        .control
        .faults
        .acted_refused
        .store(true, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::PossibleCommit, "no_confirmation".to_owned())
    );
    // The write landed, and the ledger does not say so: the record is not
    // verified, the run is not completed, and nothing was sent twice.
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);
    assert_eq!(scene.record_states().await, [RecordState::Intent]);
    assert_eq!(scene.run_row().await.state, RunState::Stopped);

    // The store is back: the record is read before anything is written,
    // found as intended, and not written again.
    scene
        .control
        .faults
        .acted_refused
        .store(false, Ordering::SeqCst);
    scene.new_lease().await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.hooks.count("put_item"), 1);
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);
    assert_eq!(scene.record_states().await, [RecordState::Verified]);
}

#[tokio::test]
async fn a_record_that_cannot_be_marked_verified_does_not_complete_the_run() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    approved(&scene, ITEM_UPSERT, &rail()).await;
    scene
        .control
        .faults
        .verified_refused
        .store(true, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(failed_as(&outcome).0, FailureClass::Busy);
    assert_eq!(scene.record_states().await, [RecordState::Acted]);
    assert_eq!(scene.run_row().await.state, RunState::Stopped);

    scene
        .control
        .faults
        .verified_refused
        .store(false, Ordering::SeqCst);
    scene.new_lease().await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.hooks.count("put_item"), 1);
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);
}

// ---- the evidence store, and the vault behind a session ----

#[tokio::test]
async fn a_record_whose_evidence_cannot_be_stored_stays_unverified() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    approved(&scene, ITEM_UPSERT, &rail()).await;
    // The evidence store fills up once the write has been sent.
    scene
        .control
        .faults
        .evidence_full
        .store(u32::MAX, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Busy, "evidence_not_stored".to_owned())
    );
    assert_ne!(scene.record_states().await, [RecordState::Verified]);
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);

    // Space is freed: nothing is lost, and nothing is written twice.
    scene
        .control
        .faults
        .evidence_full
        .store(0, Ordering::SeqCst);
    scene.new_lease().await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.hooks.count("put_item"), 1);
    assert_eq!(scene.record_states().await, [RecordState::Verified]);
}

#[tokio::test]
async fn with_no_evidence_space_and_no_session_a_plain_read_runs_and_nothing_writes() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    scene
        .control
        .faults
        .evidence_down
        .store(true, Ordering::SeqCst);
    // A read on a surface that needs no sign-in asks for no session and
    // keeps no evidence: it runs.
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order()).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert!(scene.control.wants.lock().unwrap().is_empty());

    // A write cannot store what its preview found, so it does not start.
    scene.another_run(ITEM_UPSERT).await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Busy, "evidence_not_stored".to_owned())
    );
    assert_eq!(scene.hooks.count("put_item"), 0);
    assert!(scene.app.writes().await.unwrap().is_empty());
}

// ---- the kernel's health ----

#[tokio::test]
async fn a_degraded_kernel_takes_reads_and_refuses_writes_before_anything_runs() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    scene.conditions.lock().unwrap().health =
        KernelHealth::Degraded(FailureCode::fixed("storage_full"));
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Busy, "engine_degraded".to_owned())
    );
    assert!(
        scene
            .steps(&["get_item", "put_item", "lock", "intend"])
            .is_empty()
    );
    assert!(scene.hooks.postures.lock().unwrap().is_empty());

    scene.another_run(ORDER_GET).await;
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order()).await;
    assert!(completed(&outcome), "{outcome:?}");
}

// ---- a call to the control plane that never answers ----

#[tokio::test]
async fn an_intent_that_is_never_acknowledged_is_given_up_and_nothing_is_sent() {
    let scene = impatient(ITEM_UPSERT).await;
    approved(&scene, ITEM_UPSERT, &rail()).await;
    *scene.control.faults.hangs_on.lock().unwrap() = Some("intend");
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Busy, "control_unanswered".to_owned())
    );
    assert_eq!(scene.hooks.count("put_item"), 0);
    assert!(scene.app.writes().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_sent_write_whose_record_is_never_acknowledged_is_in_doubt() {
    let scene = impatient(ITEM_UPSERT).await;
    approved(&scene, ITEM_UPSERT, &rail()).await;
    *scene.control.faults.hangs_on.lock().unwrap() = Some("acted");
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(failed_as(&outcome).0, FailureClass::PossibleCommit);
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);
    assert_ne!(scene.record_states().await, [RecordState::Verified]);

    *scene.control.faults.hangs_on.lock().unwrap() = None;
    scene.new_lease().await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.hooks.count("put_item"), 1);
}

#[tokio::test]
async fn a_decision_that_never_comes_ends_the_job_before_a_driver_is_reached() {
    let scene = impatient(ORDER_GET).await;
    *scene.control.faults.hangs_on.lock().unwrap() = Some("decide");
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Busy, "control_unanswered".to_owned())
    );
    assert!(scene.hooks.seen().iter().all(|entry| entry != "get_order"));
}

// ---- a driver that panics, and one that hangs ----

#[tokio::test]
async fn a_driver_that_panics_in_a_read_is_named_and_the_engine_goes_on() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    *scene.hooks.panics_on.lock().unwrap() = Some("get_order".to_owned());
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::UnknownVariant, "driver_panicked".to_owned())
    );
    assert!(
        outcome.failure().unwrap().message().contains(FIRST_MODE),
        "{outcome:?}"
    );

    // The same engine carries out the next job.
    *scene.hooks.panics_on.lock().unwrap() = None;
    scene.another_run(ORDER_GET).await;
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order()).await;
    assert!(completed(&outcome), "{outcome:?}");
}

#[tokio::test]
async fn a_driver_that_panics_in_a_save_leaves_the_record_in_doubt() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    approved(&scene, ITEM_UPSERT, &rail()).await;
    *scene.hooks.panics_on.lock().unwrap() = Some("put_item".to_owned());
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    // Nothing says whether the write left before the panic.
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::PossibleCommit, "no_confirmation".to_owned())
    );
    assert_eq!(scene.record_states().await, [RecordState::Failed]);

    // Resumed: the record is read, found untouched, and written once.
    *scene.hooks.panics_on.lock().unwrap() = None;
    scene.new_lease().await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);
}

#[tokio::test]
async fn a_step_that_never_ends_is_ended_by_its_time_limit_with_its_declared_class() {
    let scene = Scene::new(ORDER_GET, |plan| {
        for binding in &mut plan.operations[0].bindings {
            binding.act[0].timeout_ms = 30;
        }
    })
    .await;
    *scene.pin.lock().unwrap() = Some(Mode::new(FIRST_MODE).unwrap());
    *scene.hooks.hangs_on.lock().unwrap() = Some("get_order".to_owned());
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Transient, "example_timed_out".to_owned())
    );
    // A `transient` failure of a read is tried again, twice, and no more.
    assert_eq!(scene.hooks.count("get_order"), 3);
}

#[tokio::test]
async fn a_driver_that_never_gets_ready_is_given_up() {
    let scene = Scene::with_settings(
        ORDER_GET,
        |_| {},
        |settings| settings.prepare_timeout = Duration::from_millis(30),
    )
    .await;
    scene.hooks.prepare_hangs.store(true, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Transient, "prepare_timed_out".to_owned())
    );
    assert_eq!(scene.hooks.count("get_order"), 0);
}

// ---- the lease and cancellation ----

#[tokio::test]
async fn a_cancel_during_a_save_takes_effect_after_the_save_is_recorded() {
    let scene = Scene::new(ORDER_FILL, |_| {}).await;
    let input = a2m_core::Values::new()
        .with("order_id", "A-102")
        .with("first_code", "L-301")
        .with("second_code", "L-302")
        .with("description", "Rail")
        .with("quantity", 1_i64)
        .with("length_in", "20");
    approved(&scene, ORDER_FILL, &input).await;
    let lease = scene.lease.clone();
    scene.hooks.on_step(move |kind| {
        if kind == "put_item" {
            lease.direct(Directive::Cancel);
        }
    });
    let outcome = scene.work(JobKind::Execute, ORDER_FILL, input).await;
    assert_eq!(
        outcome,
        JobOutcome::Finished {
            state: RunState::Cancelled,
            failure: None
        }
    );
    // The save that was in hand was finished, read back, and recorded; the
    // next record was not started.
    assert_eq!(
        scene.record_states().await,
        [RecordState::Verified, RecordState::Planned]
    );
    let log = scene.hooks.seen();
    assert!(at(&log, "put_item") < at(&log, "verified"), "{log:?}");
    assert_eq!(scene.hooks.count("put_item"), 1);
    assert_eq!(scene.app.write_count("A-102", "L-301").await.unwrap(), 1);
    assert!(
        scene
            .app
            .stored_item("A-102", "L-302")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_lease_that_is_ending_stops_the_job_before_its_commit_point() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    approved(&scene, ITEM_UPSERT, &rail()).await;
    // The lease has no time left for the record's steps.
    scene.lease.renew(tokio::time::Instant::now());
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(outcome, JobOutcome::LeaseEnding);
    assert_eq!(scene.hooks.count("put_item"), 0);
    assert_eq!(scene.record_states().await, [RecordState::Planned]);
    assert!(scene.app.writes().await.unwrap().is_empty());

    scene
        .lease
        .renew(tokio::time::Instant::now() + Duration::from_secs(3600));
    scene.new_lease().await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);
}

#[tokio::test]
async fn a_lease_that_runs_out_while_the_record_waits_sends_no_save() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    approved(&scene, ITEM_UPSERT, &rail()).await;
    // Policy asks for the freshness read, which comes after the lock. The
    // heartbeats have stopped, and the lease ends while the job waits and
    // reads: it had time enough when the record was begun.
    scene.conditions.lock().unwrap().strict_freshness = true;
    scene
        .lease
        .renew(tokio::time::Instant::now() + Duration::from_secs(3600));
    let lease = scene.lease.clone();
    scene.hooks.on_step(move |kind| {
        if kind == "get_item" {
            lease.renew(tokio::time::Instant::now());
        }
    });
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(outcome, JobOutcome::LeaseEnding);
    // No intent was stored and nothing was sent.
    assert_eq!(scene.hooks.count("intend"), 0);
    assert_eq!(scene.hooks.count("put_item"), 0);
    assert_eq!(scene.record_states().await, [RecordState::Planned]);
    assert!(scene.app.writes().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_lease_that_runs_out_after_the_intent_stops_the_job_at_the_commit_point() {
    let scene = Scene::new(SLOT_ADD, |_| {}).await;
    let input = slot("A-102", 4, "12");
    approved(&scene, SLOT_ADD, &input).await;
    scene
        .lease
        .renew(tokio::time::Instant::now() + Duration::from_secs(3600));
    // The lease ends during the step before the save, after the intent
    // was stored. It does so once.
    let lease = scene.lease.clone();
    let once = std::sync::atomic::AtomicBool::new(true);
    scene.hooks.on_step(move |kind| {
        if kind == "reserve_code" && once.swap(false, Ordering::SeqCst) {
            lease.renew(tokio::time::Instant::now());
        }
    });
    let outcome = scene.work(JobKind::Execute, SLOT_ADD, input.clone()).await;
    assert_eq!(outcome, JobOutcome::LeaseEnding);
    assert_eq!(scene.hooks.count("add_item"), 0);
    assert!(scene.app.writes().await.unwrap().is_empty());
    // The intent stands, so the next lease reads the record before it
    // writes: nothing landed, and the record is written once.
    assert_eq!(scene.record_states().await, [RecordState::Intent]);
    scene
        .lease
        .renew(tokio::time::Instant::now() + Duration::from_secs(3600));
    scene.new_lease().await;
    let outcome = scene.work(JobKind::Execute, SLOT_ADD, input).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.hooks.count("settle:Planned"), 1);
    assert_eq!(scene.hooks.count("add_item"), 1);
    assert_eq!(scene.app.writes().await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_step_with_no_time_limit_of_its_own_is_counted_at_the_default_limit() {
    // Every step of the binding takes the default limit, thirty seconds.
    let scene = Scene::new(ITEM_UPSERT, |plan| {
        let binding = &mut plan.operations[2].bindings[0];
        for step in binding.act.iter_mut().chain(&mut binding.read_back) {
            step.timeout_ms = 0;
        }
    })
    .await;
    approved(&scene, ITEM_UPSERT, &rail()).await;
    // The lease has less left than one such step may take.
    scene
        .lease
        .renew(tokio::time::Instant::now() + Duration::from_secs(10));
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(outcome, JobOutcome::LeaseEnding);
    assert_eq!(scene.hooks.count("put_item"), 0);
    assert!(scene.app.writes().await.unwrap().is_empty());
}

// ---- a save is never abandoned ----

#[tokio::test]
async fn a_save_that_outlasts_its_time_limit_is_waited_for_and_a_cancel_takes_effect_after_it() {
    // The save's time limit is a fraction of the time the driver takes,
    // and a cancel arrives while the save is under way.
    let scene = Scene::new(ITEM_UPSERT, |plan| {
        plan.operations[2].bindings[0].act[0].timeout_ms = 20;
    })
    .await;
    approved(&scene, ITEM_UPSERT, &rail()).await;
    scene.hooks.save_takes_ms.store(200, Ordering::SeqCst);
    let lease = scene.lease.clone();
    scene.hooks.on_step(move |kind| {
        if kind == "put_item" {
            lease.direct(Directive::Cancel);
        }
    });
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    // The engine did not stop waiting at the limit: the save's answer was
    // taken, the record read back and recorded, and the job ended.
    assert!(completed(&outcome), "{outcome:?}");
    let log = scene.hooks.seen();
    assert!(at(&log, "put_item") < at(&log, "acted"), "{log:?}");
    assert!(at(&log, "acted") < at(&log, "verified"), "{log:?}");
    assert_eq!(scene.record_states().await, [RecordState::Verified]);
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);
}

#[tokio::test]
async fn a_job_dropped_during_a_save_leaves_the_record_in_doubt_and_the_late_write_is_found() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    approved(&scene, ITEM_UPSERT, &rail()).await;
    // The driver commits away from the step's future, so the commit goes
    // on when the future is dropped.
    scene.hooks.detached_save.store(true, Ordering::SeqCst);
    let job = scene.job(JobKind::Execute, ITEM_UPSERT, rail()).await;
    {
        let running = scene.engine.run_job(&job, scene.control.as_ref());
        tokio::pin!(running);
        tokio::select! {
            outcome = &mut running => panic!("the job ended while its save was under way: {outcome:?}"),
            () = scene.hooks.in_flight.notified() => {}
        }
        // The engine's caller stops waiting here: the job's future, and
        // with it the future of the committing step, is dropped.
    }
    // Nothing recorded the record as failed or as not sent. Its intent
    // was stored before the save, so the ledger holds it in doubt.
    assert_eq!(scene.record_states().await, [RecordState::Intent]);
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 0);

    // The commit completes after the engine stopped waiting.
    scene.hooks.late_gate.notify_one();
    let late = scene.hooks.late.lock().unwrap().take().unwrap();
    late.await.unwrap();
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);

    // The next lease reads the record before it writes: the late write is
    // found as intended, and nothing is sent a second time.
    resumed_as_landed(&scene, "a save that landed after its job was dropped").await;
}

// ---- invariants of the design ----

#[tokio::test]
async fn a_record_in_doubt_is_read_through_the_mode_that_wrote_it_and_never_by_another() {
    // The operation has a second binding, through the spare mode.
    let scene = Scene::new(ITEM_UPSERT, |plan| {
        let mut spare = plan.operations[2].bindings[0].clone();
        spare.mode = Mode::new(SECOND_MODE).unwrap();
        plan.operations[2].bindings.push(spare);
    })
    .await;
    approved(&scene, ITEM_UPSERT, &rail()).await;
    scene.hooks.lose_answer.store(true, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    // The first binding failed after sending: the write does not fall to
    // the second.
    assert_eq!(failed_as(&outcome).0, FailureClass::PossibleCommit);
    assert_eq!(scene.hooks.steps_of(SECOND_MODE), 0);

    // While the mode that wrote the record may not act, the record is not
    // touched through the other one, not even to read it.
    let upsert = OperationName::new(ITEM_UPSERT).unwrap();
    scene.conditions.lock().unwrap().drifted = vec![(upsert, Mode::new(FIRST_MODE).unwrap())];
    scene.new_lease().await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Busy, "no_binding".to_owned())
    );
    assert_eq!(scene.hooks.steps_of(SECOND_MODE), 0);

    scene.conditions.lock().unwrap().drifted.clear();
    scene.new_lease().await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.hooks.steps_of(SECOND_MODE), 0);
    assert_eq!(scene.hooks.count("put_item"), 1);
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);
}

#[tokio::test]
async fn a_save_is_not_sent_when_what_was_learned_before_it_cannot_be_stored() {
    let scene = Scene::new(SLOT_ADD, |_| {}).await;
    let input = slot("A-102", 4, "12");
    approved(&scene, SLOT_ADD, &input).await;
    scene
        .control
        .faults
        .before_commit_refused
        .store(true, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, SLOT_ADD, input).await;
    assert_eq!(failed_as(&outcome).0, FailureClass::Busy);
    // The step before the save ran; the save did not.
    assert_eq!(scene.hooks.count("reserve_code"), 1);
    assert_eq!(scene.hooks.count("add_item"), 0);
    assert!(scene.app.writes().await.unwrap().is_empty());
}

fn repository() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

#[test]
fn the_crates_source_holds_no_registered_mode_name() {
    // The first-party modes, as the packs' manifests register them.
    let crates = repository().join("crates");
    let mut modes: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&crates).unwrap() {
        let folder = entry.unwrap().path();
        let is_pack = folder
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("a2m-pack-"));
        if !is_pack {
            continue;
        }
        for manifest in [
            folder.join("anything2mcp-provider.toml"),
            folder.join("src").join("anything2mcp-provider.toml"),
            folder.join("src").join("manifest.toml"),
        ] {
            let Ok(text) = std::fs::read_to_string(&manifest) else {
                continue;
            };
            let mut in_mode = false;
            for line in text.lines().map(str::trim) {
                if line.starts_with('[') {
                    in_mode = line == "[[modes]]";
                } else if in_mode && line.starts_with("name") {
                    let name = line.split('"').nth(1).unwrap();
                    modes.push(name.to_owned());
                }
            }
        }
    }
    modes.sort();
    modes.dedup();
    assert!(
        modes.len() >= 5,
        "the packs' manifests were not found: {modes:?}"
    );

    // No source file of the engine names one of them as text.
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in std::fs::read_dir(source).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        for mode in &modes {
            assert!(
                !text.contains(&format!("\"{mode}\"")),
                "{} names the mode `{mode}`",
                path.display()
            );
        }
    }
}

#[test]
fn every_failure_code_is_in_the_design_and_the_technical_reference_under_its_class() {
    let docs = repository().join("docs");
    let design = std::fs::read_to_string(
        docs.join("architecture")
            .join("crates")
            .join("a2m-engine.md"),
    )
    .unwrap();
    let reference =
        std::fs::read_to_string(docs.join("reference").join("technical-reference.md")).unwrap();
    let mut seen = Vec::new();
    for (class, code) in codes::ALL {
        assert!(!seen.contains(code), "`{code}` is listed twice");
        seen.push(*code);
        let class = format!("| `{class}` |");
        let code = format!("`{code}`");
        assert!(
            design
                .lines()
                .any(|line| line.starts_with(&class) && line.contains(&code)),
            "the design does not list {code} under {class}"
        );
        assert!(
            reference
                .lines()
                .any(|line| line.starts_with(&class) && line.contains(&code)),
            "the technical reference does not list {code} under {class}"
        );
    }
}
