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

use a2m_core::{BindingStatus, FailureClass, RecordState, RunState, ValueType, Values};
use a2m_engine::{EngineEvent, JobKind, JobOutcome};
use a2m_provider::{Sent, StepOutcome};
use common::{
    ITEM_UPSERT, ORDER_GET, Scene, approved, completed, expr, failed_as, item, item_key, step,
    waits,
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

/// A target whose operations have no binding yet, as a target that
/// declares only its surface has none. Discovery is about the surface, and is made through the
/// surface's own mode, the one its kind's provider ranks first; before, the
/// port had no driver and every read answered `no_binding`.
#[tokio::test]
async fn a_surface_with_no_binding_is_observed_through_its_own_mode() {
    let scene = Scene::new(ORDER_GET, |compiled_target| {
        for operation in &mut compiled_target.operations {
            operation.bindings.clear();
        }
    })
    .await;
    let outcome = scene.discover("app").await;
    assert!(completed(&outcome), "{outcome:?}");
    let results = scene.discoverer.results.lock().unwrap().clone();
    // The read went through the driver of `app_json`, ranked before
    // `app_json_spare`, prepared for discovery.
    assert!(
        matches!(results[2], Ok(StepOutcome::Absent(_))),
        "{results:?}"
    );
    assert_eq!(*scene.hooks.postures.lock().unwrap(), ["Discovery"]);
    // The step that commits was refused all the same, and nothing was
    // written.
    assert_eq!(
        results[0].as_ref().unwrap_err().failure.code().as_str(),
        "sandbox_read_only"
    );
    assert!(scene.app.writes().await.unwrap().is_empty());
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
    let began = tokio::time::Instant::now();
    let outcome = scene.discover("app").await;
    assert!(completed(&outcome), "{outcome:?}");
    // Ended by the time its manifest declares, and not sooner.
    assert!(began.elapsed() >= std::time::Duration::from_secs(60));
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
async fn a_discovery_job_for_a_surface_the_compiled_target_does_not_hold_leaves_the_run_as_it_was()
{
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    let outcome = scene.discover("elsewhere").await;
    let JobOutcome::Left(failure) = &outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(failure.code().as_str(), "job_invalid");
    assert!(discovered(&scene).is_empty());
    assert!(scene.hooks.postures.lock().unwrap().is_empty());
}

// ---- files of the project's `inputs` folder (decision 0263) ----

/// A project with a `inputs` folder holding one file, set as the folder of
/// the request, and the files the call hands the discovery.
fn handing(scene: &Scene, files: &[(&str, &str)]) -> common::Folder {
    let project = common::Folder::new("handed");
    std::fs::create_dir_all(project.path().join("inputs")).unwrap();
    std::fs::write(
        project.path().join("inputs").join("example-openapi.json"),
        b"{\"openapi\": \"3.0.0\"}",
    )
    .unwrap();
    *scene.control.project.lock().unwrap() = Some(project.path().to_path_buf());
    *scene.control.discovery_files.lock().unwrap() = files
        .iter()
        .map(|(name, path)| ((*name).to_owned(), (*path).to_owned()))
        .collect();
    project
}

#[tokio::test]
async fn a_named_file_of_the_sources_folder_reaches_the_discoverer_under_its_name() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    let _project = handing(&scene, &[("description", "inputs/example-openapi.json")]);
    let outcome = scene.discover("app").await;
    assert!(completed(&outcome), "{outcome:?}");
    let handed = scene.discoverer.handed.lock().unwrap().clone();
    assert_eq!(
        handed,
        [vec![(
            "description".to_owned(),
            b"{\"openapi\": \"3.0.0\"}".to_vec()
        )]]
    );
}

/// Makes a link to a file. `None` where this machine does not let the test
/// make one, as Windows does without the privilege.
fn link_file(target: &std::path::Path, link: &std::path::Path) -> Option<()> {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link).ok();
    #[cfg(windows)]
    return std::os::windows::fs::symlink_file(target, link).ok();
}

#[tokio::test]
async fn a_link_out_of_the_sources_folder_is_refused_and_nothing_is_read() {
    let outside = common::Folder::new("outside");
    std::fs::write(outside.path().join("secret.txt"), b"not for discovery").unwrap();
    // The same refusal for a path that leaves the folder by its text, a
    // hidden file, a path outside `inputs/`, and a name no discoverer of
    // the surface reads.
    let mut cases = vec![
        ("description", "inputs/../outside.txt", "path_not_allowed"),
        ("description", "inputs/.hidden.json", "path_not_allowed"),
        ("description", "target.yaml", "path_not_allowed"),
        ("description", "inputs/missing.json", "path_not_allowed"),
        ("register_map", "inputs/example-openapi.json", "job_invalid"),
    ];
    let link = "inputs/out.json";
    cases.push(("description", link, "path_not_allowed"));
    for (name, path, code) in cases {
        let scene = Scene::new(ORDER_GET, |_| {}).await;
        let project = handing(&scene, &[(name, path)]);
        std::fs::write(project.path().join("outside.txt"), b"not in sources").unwrap();
        std::fs::write(project.path().join("inputs").join(".hidden.json"), b"{}").unwrap();
        if path == link
            && link_file(
                &outside.path().join("secret.txt"),
                &project.path().join("inputs").join("out.json"),
            )
            .is_none()
        {
            // Without the right to make a link there is nothing to plant.
            continue;
        }
        let outcome = scene.discover("app").await;
        let JobOutcome::Left(failure) = &outcome else {
            panic!("{path}: {outcome:?}");
        };
        assert_eq!(failure.code().as_str(), code, "{path}: {failure:?}");
        // No discoverer ran, no driver was prepared, and nothing was read.
        assert!(discovered(&scene).is_empty(), "{path}");
        assert!(scene.discoverer.handed.lock().unwrap().is_empty(), "{path}");
        assert!(scene.hooks.postures.lock().unwrap().is_empty(), "{path}");
    }
}

// ---- a try: a draft in the sandbox ----

#[tokio::test]
async fn a_try_runs_a_draft_read_that_may_not_act_live() {
    let scene = Scene::new(ORDER_GET, |compiled_target| {
        for binding in &mut compiled_target.operations[0].bindings {
            binding.status = BindingStatus::Draft;
        }
    })
    .await;
    // Live, a draft binding is never chosen, and the run is held to a
    // compiled target in which nothing else may act: it fails for good and
    // offers no resume (copy test 7, finding 47).
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Policy, "no_binding_may_act".to_owned())
    );
    assert_eq!(outcome.run_state(), Some(a2m_core::RunState::Failed));
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
    let scene = Scene::new(ITEM_UPSERT, |compiled_target| {
        compiled_target.environments[0].test_records = vec![item_key("A-102", "T-1")];
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
    // The record was looked up at the preview with the driver's own guard
    // on. One context alone was prepared in the posture that may write:
    // the one the test record was written in.
    let postures = scene.hooks.postures.lock().unwrap().clone();
    assert_eq!(
        postures,
        [
            "Sandbox { test_records: false }",
            "Sandbox { test_records: true }"
        ]
    );
}

/// An environment that declares one test record, and a drafted step that
/// changes a record that is not one while it says it commits nothing.
fn dressed_write(compiled_target: &mut a2m_core::CompiledTarget) -> a2m_core::Step {
    compiled_target.environments[0].test_records = vec![item_key("A-102", "T-1")];
    let order = expr(compiled_target, "\"A-102\"", ValueType::Text);
    let other = expr(compiled_target, "\"L-900\"", ValueType::Text);
    let text = expr(compiled_target, "\"Not a test record\"", ValueType::Text);
    let one = expr(compiled_target, "1", ValueType::Int);
    let length = expr(compiled_target, "\"1\"", ValueType::Text);
    step(
        "put_item",
        &[
            ("order_id", order),
            ("item_code", other),
            ("description", text),
            ("quantity", one),
            ("length_in", length),
        ],
        false,
        false,
    )
}

#[tokio::test]
async fn a_try_of_a_read_keeps_the_drivers_guard_on_where_test_records_are_declared() {
    // A draft read whose lowering is wrong: its first step changes a
    // record. The environment declares a test record, which says nothing
    // about this job: it writes no record at all.
    let scene = Scene::new(ORDER_GET, |compiled_target| {
        let dressed = dressed_write(compiled_target);
        for binding in &mut compiled_target.operations[0].bindings {
            binding.status = BindingStatus::Draft;
            binding.act.insert(0, dressed.clone());
        }
    })
    .await;
    let outcome = scene
        .work(
            JobKind::Try,
            ORDER_GET,
            Values::new().with("order_id", "A-100"),
        )
        .await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Policy, "read_only_posture".to_owned())
    );
    // Every driver of the job was prepared with its guard on, and the
    // test application holds nothing new.
    let postures = scene.hooks.postures.lock().unwrap().clone();
    assert!(!postures.is_empty());
    assert!(
        postures
            .iter()
            .all(|posture| posture == "Sandbox { test_records: false }"),
        "{postures:?}"
    );
    assert!(scene.app.writes().await.unwrap().is_empty());
    assert!(
        scene
            .app
            .stored_item("A-102", "L-900")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_try_of_a_write_runs_its_reads_under_the_guard_and_changes_its_test_record_alone() {
    // A drafted write to a declared test record. Its read-back, which
    // also looks the record up at the preview, holds a step that changes
    // a record that is not a test record.
    let scene = Scene::new(ITEM_UPSERT, |compiled_target| {
        let dressed = dressed_write(compiled_target);
        let binding = &mut compiled_target.operations[2].bindings[0];
        binding.status = BindingStatus::Draft;
        binding.read_back.insert(0, dressed);
    })
    .await;
    let test_record = item("A-102", "T-1", "Trial", 1, "1");
    let outcome = scene.work(JobKind::Try, ITEM_UPSERT, test_record).await;
    // The lookup ran where nothing may be written: the pack's guard
    // refused the step, unsent.
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Policy, "read_only_posture".to_owned())
    );
    assert_eq!(
        *scene.hooks.postures.lock().unwrap(),
        ["Sandbox { test_records: false }"]
    );
    assert!(scene.app.writes().await.unwrap().is_empty());

    // The same step among the act steps, before the save. It runs in the
    // context the test record is written in, where the kernel has only
    // the token to go by; the pack's guard there lets through the step
    // that commits and nothing else that changes the system.
    let scene = Scene::new(ITEM_UPSERT, |compiled_target| {
        let dressed = dressed_write(compiled_target);
        let binding = &mut compiled_target.operations[2].bindings[0];
        binding.status = BindingStatus::Draft;
        binding.act.insert(0, dressed);
    })
    .await;
    let test_record = item("A-102", "T-1", "Trial", 1, "1");
    let outcome = scene
        .work(JobKind::Try, ITEM_UPSERT, test_record.clone())
        .await;
    assert!(waits(&outcome), "{outcome:?}");
    let diff = scene.run_row().await.diff_hash.unwrap();
    scene.control.approve(diff).await;
    let outcome = scene.work(JobKind::Try, ITEM_UPSERT, test_record).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Policy, "read_only_posture".to_owned())
    );
    assert!(scene.app.writes().await.unwrap().is_empty());
    assert!(
        scene
            .app
            .stored_item("A-102", "L-900")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_record_is_read_again_in_the_context_that_writes_it() {
    // A driver may keep what it read of a record, such as its version,
    // with the context it read it in. In a try, the record is looked up
    // in one context and written in another, within one job and well
    // inside the freshness window.
    let scene = Scene::new(ITEM_UPSERT, |compiled_target| {
        compiled_target.environments[0].test_records = vec![item_key("A-102", "T-1")];
    })
    .await;
    let test_record = item("A-102", "T-1", "Trial", 1, "1");
    let outcome = scene
        .work(JobKind::Try, ITEM_UPSERT, test_record.clone())
        .await;
    assert!(waits(&outcome), "{outcome:?}");
    let diff = scene.run_row().await.diff_hash.unwrap();
    // A second run of the same call, whose diff is the same and is
    // approved before the job starts: it previews and writes in one job.
    scene.another_run(ITEM_UPSERT).await;
    scene.control.approve(diff).await;
    let before = scene.hooks.seen().len();
    let outcome = scene.work(JobKind::Try, ITEM_UPSERT, test_record).await;
    assert!(completed(&outcome), "{outcome:?}");
    let log: Vec<String> = scene.hooks.seen()[before..]
        .iter()
        .filter(|entry| ["get_item", "lock", "intend", "put_item"].contains(&entry.as_str()))
        .cloned()
        .collect();
    // Looked up for the preview; then, under the lock and before the
    // intent, read once more in the context that writes; then written and
    // read back.
    assert_eq!(
        log,
        [
            "get_item", "lock", "get_item", "intend", "put_item", "get_item"
        ]
    );
    assert_eq!(scene.app.write_count("A-102", "T-1").await.unwrap(), 1);
}

#[tokio::test]
async fn a_discoverer_that_floods_is_held_to_its_declared_size_and_the_bound_is_reported() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    // The discoverer hands over far more than the bytes its manifest
    // declares.
    let flood = 20_000_i64;
    let outcome = scene
        .discover_with("app", Values::new().with("flood", flood))
        .await;
    assert!(completed(&outcome), "{outcome:?}");
    let events = scene.control.events.lock().unwrap();
    let (findings, report) = events
        .iter()
        .find_map(|event| match event {
            EngineEvent::Discovered {
                findings, report, ..
            } => Some((findings, report)),
            _ => None,
        })
        .unwrap();
    let report = report.as_ref().unwrap();
    let kept = u64::try_from(findings.len()).unwrap();
    let made = u64::try_from(flood).unwrap() + 1;
    assert!(kept < made, "every finding was kept");
    assert!(report.bounds_hit.iter().any(|bound| bound == "bytes"));
    assert_eq!(kept + report.dropped, made);
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

/// A record with no key is keyed by its place in the run, which names
/// nothing in the system: a test record written in that form never lets
/// the sandbox write it (audit finding E-6).
#[tokio::test]
async fn a_record_keyed_by_its_place_in_the_run_is_never_a_test_record() {
    let scene = Scene::new(common::SLOT_ADD, |compiled_target| {
        common::keyless_slot_add(compiled_target);
        compiled_target.environments[0].test_records = vec![
            a2m_core::RecordKey::new(vec![
                a2m_core::Value::from(common::SLOT_ADD),
                a2m_core::Value::Int(0),
                a2m_core::Value::Int(0),
            ])
            .unwrap(),
        ];
    })
    .await;
    let input = common::slot("A-102", 4, "12");
    let outcome = scene.work(JobKind::Try, common::SLOT_ADD, input).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Policy, "sandbox_read_only".to_owned())
    );
    assert!(scene.app.writes().await.unwrap().is_empty());
}

/// The modes the engine chose, in order.
fn chosen_modes(scene: &Scene) -> Vec<String> {
    scene
        .control
        .events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            EngineEvent::BindingChosen { mode, .. } => Some(mode.to_string()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_trial_runs_the_binding_it_names_and_never_another_in_its_name() {
    // Two draft bindings: unnamed, a trial runs the first. Named, the
    // second runs, and nothing falls back to the first (audit finding
    // AC-6).
    let scene = Scene::new(ORDER_GET, |compiled_target| {
        for binding in &mut compiled_target.operations[0].bindings {
            binding.status = BindingStatus::Draft;
        }
    })
    .await;
    let operation = scene.control.compiled_target.operations[0].clone();
    assert_eq!(a2m_engine::default_binding(&operation), Some(0));
    let second = a2m_engine::BindingPin::of(&scene.control.compiled_target, &operation, 1).unwrap();
    assert_eq!(second.mode.as_str(), common::SECOND_MODE);
    *scene.binding.lock().unwrap() = Some(second.clone());
    let outcome = scene.work(JobKind::Try, ORDER_GET, order()).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(chosen_modes(&scene), vec![common::SECOND_MODE.to_owned()]);

    // A pin whose digest is not the binding's now runs nothing.
    let mut stale = second;
    stale.digest = a2m_core::Digest::of(b"another binding");
    *scene.binding.lock().unwrap() = Some(stale);
    scene.another_run(ORDER_GET).await;
    let calls = scene.hooks.count("get_order");
    let outcome = scene.work(JobKind::Try, ORDER_GET, order()).await;
    assert_eq!(
        outcome.failure().map(|failure| failure.code().to_string()),
        Some("binding_changed".to_owned()),
        "{outcome:?}"
    );
    assert_eq!(scene.hooks.count("get_order"), calls);
}

#[test]
fn a_tested_binding_is_the_one_that_would_act_live_and_its_digest_ignores_its_status() {
    let mut compiled_target = common::compiled_target("http://127.0.0.1:1");
    let operation = &mut compiled_target.operations[0];
    operation.bindings[0].status = BindingStatus::Draft;
    operation.bindings[1].status = BindingStatus::Verified;
    assert_eq!(a2m_engine::default_binding(operation), Some(1));
    operation.bindings[1].status = BindingStatus::Retired;
    assert_eq!(a2m_engine::default_binding(operation), Some(0));
    let before =
        a2m_engine::binding_digest(&compiled_target, &compiled_target.operations[0].bindings[0])
            .unwrap();
    compiled_target.operations[0].bindings[0].status = BindingStatus::Verified;
    let promoted =
        a2m_engine::binding_digest(&compiled_target, &compiled_target.operations[0].bindings[0])
            .unwrap();
    assert_eq!(before, promoted);
    compiled_target.operations[0].bindings[0].settle_ms += 1;
    let changed =
        a2m_engine::binding_digest(&compiled_target, &compiled_target.operations[0].bindings[0])
            .unwrap();
    assert_ne!(before, changed);
}
