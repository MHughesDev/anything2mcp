//! Package 3.7: stop and resume. A write is killed after each call to
//! `Control` in turn, as a process killed at that point would leave it,
//! and resumed under a new lease. Whatever the point, the test application
//! holds the write at most once, the record ends verified or in doubt with
//! the run stopped as `possible_commit`, and a resume reads a record before
//! it writes it (ledger rule 5).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::sync::atomic::Ordering;

use a2m_core::{FailureClass, RecordState, RunState};
use a2m_engine::{JobKind, JobOutcome};
use common::{ITEM_UPSERT, Scene, approved, completed, item};

fn rail() -> a2m_core::Values {
    item("A-102", "L-300", "New rail", 3, "150.5")
}

/// How many calls to `Control` the execute of a clean, approved write
/// makes.
async fn calls_in_a_clean_write() -> u32 {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    approved(&scene, ITEM_UPSERT, &rail()).await;
    scene.control.faults.calls.store(0, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);
    scene.control.faults.calls.load(Ordering::SeqCst)
}

/// Stops the run as `possible_commit`, `runner_lost`, under the lease it
/// now holds, as the scheduler does when it declares a lease lost.
async fn declare_lost(scene: &Scene) {
    let fence = *scene.control.fence.lock().unwrap();
    let why = common::failure(
        FailureClass::PossibleCommit,
        "runner_lost",
        "The runner's lease ended without a result.",
    );
    let state = scene.run_row().await.state;
    // The states the scheduler stops a run from.
    if !matches!(
        state,
        RunState::Queued
            | RunState::Validating
            | RunState::Running
            | RunState::NeedsInput
            | RunState::NeedsLogin
    ) {
        return;
    }
    scene
        .control
        .store
        .write(a2m_ledger::Durability::Synced, move |tx| {
            tx.ledger().set_state(&fence, RunState::Stopped, Some(&why))
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn killed_after_every_control_call_and_resumed_nothing_is_duplicated_or_unverified() {
    let clean = calls_in_a_clean_write().await;
    assert!(clean > 5, "a write makes more calls than that: {clean}");
    let mut sent_before_the_kill = 0;
    for k in 0..clean {
        let why = format!("killed after {k} of {clean} calls");
        let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
        approved(&scene, ITEM_UPSERT, &rail()).await;

        // The execute runs until call `k + 1` to `Control`, which never
        // answers; then its future is dropped, as a killed process leaves
        // it, with nothing recorded after that call.
        let faults = &scene.control.faults;
        faults.calls.store(0, Ordering::SeqCst);
        faults.abort_after.store(k, Ordering::SeqCst);
        faults.abort_armed.store(true, Ordering::SeqCst);
        let job = scene.job(JobKind::Execute, ITEM_UPSERT, rail()).await;
        {
            let running = scene.engine.run_job(&job, scene.control.as_ref());
            tokio::pin!(running);
            tokio::select! {
                outcome = &mut running => panic!("{why}: the job ended: {outcome:?}"),
                () = faults.aborted.notified() => {}
            }
        }
        faults.abort_armed.store(false, Ordering::SeqCst);
        let before = scene.app.write_count("A-102", "L-300").await.unwrap();
        assert!(before <= 1, "{why}: {before} writes");
        sent_before_the_kill += before;

        // The lease of the killed job has ended; the next one resumes. Every
        // other time the run is first stopped as the scheduler stops it
        // when it declares a lease lost, and reconciled before the resume.
        let from = scene.hooks.seen().len();
        scene.new_lease().await;
        let declared_lost = k % 2 == 1;
        if declared_lost {
            declare_lost(&scene).await;
            let reconciled = scene.work(JobKind::Reconcile, ITEM_UPSERT, rail()).await;
            assert!(
                matches!(reconciled, JobOutcome::Settled { .. }),
                "{why}: {reconciled:?}"
            );
            scene.new_lease().await;
        }
        let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
        let log = scene.hooks.seen()[from..].to_vec();

        // The application holds the write at most once, whatever the point.
        let writes = scene.app.write_count("A-102", "L-300").await.unwrap();
        assert!(writes <= 1, "{why}: {writes} writes; {log:?}");
        // A write sent before the kill is never sent again.
        if before == 1 {
            assert!(
                !log.iter().any(|entry| entry == "put_item"),
                "{why}: {log:?}"
            );
        }
        // Rule 5: the resume reads the record before it writes it.
        if let Some(save) = log.iter().position(|entry| entry == "put_item") {
            assert!(
                log[..save].iter().any(|entry| entry == "get_item"),
                "{why}: written before it was read: {log:?}"
            );
        }
        // The record is verified and the run completed, or the record is
        // in doubt and the run stopped as a possible commit; never anything
        // that is neither.
        let states = scene.record_states().await;
        if completed(&outcome) {
            assert_eq!(states, [RecordState::Verified], "{why}");
            assert_eq!(writes, 1, "{why}: completed with nothing written");
        } else {
            let JobOutcome::Finished {
                state: RunState::Stopped,
                failure: Some(failure),
            } = &outcome
            else {
                panic!("{why}: {outcome:?}");
            };
            assert_eq!(failure.class(), FailureClass::PossibleCommit, "{why}");
            assert!(
                !states.contains(&RecordState::Verified),
                "{why}: {states:?}"
            );
        }
    }
    // Some kills came after the write was sent, and some before it: both
    // sides of the save were exercised.
    assert!(sent_before_the_kill > 0, "no kill came after the save");
    assert!(
        sent_before_the_kill < usize::try_from(clean).unwrap(),
        "no kill came before the save"
    );
}
