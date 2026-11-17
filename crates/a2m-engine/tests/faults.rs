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

use a2m_core::{FailureClass, FailureCode, Mode, OperationName, RecordState, RunState};
use a2m_engine::{Directive, JobKind, JobOutcome, KernelHealth, codes};
use common::{
    FIRST_MODE, ITEM_UPSERT, ORDER_FILL, ORDER_GET, SECOND_MODE, SLOT_ADD, Scene, approved, at,
    completed, failed_as, item, slot,
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
