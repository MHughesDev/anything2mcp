//! Build-order steps 1 to 4: a read through the eight parts, the step
//! trace, the plan fetched by its hash, and a surface that needs a sign-in.
//!
//! Every test runs against the built-in test application, through a driver
//! registered by a test pack as a real pack registers one.

mod common;

use std::sync::atomic::Ordering;

use a2m_core::{Digest, FailureClass, RunState, SessionState, SurfaceName, Value, Values};
use a2m_engine::{EngineEvent, JobKind, JobOutcome};
use a2m_telemetry::ChoiceReason;
use common::{FIRST_MODE, ORDER_GET, SECOND_MODE, SECURE_GET, STOCK_GET, Scene};

fn order(id: &str) -> Values {
    Values::new().with("order_id", id)
}

fn completed(outcome: &JobOutcome) -> bool {
    matches!(
        outcome,
        JobOutcome::Finished {
            state: RunState::Completed,
            failure: None
        }
    )
}

fn chosen(scene: &Scene) -> Vec<(String, ChoiceReason)> {
    scene
        .control
        .events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            EngineEvent::BindingChosen { mode, reason, .. } => Some((mode.to_string(), *reason)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_read_is_guarded_chosen_prepared_acted_and_recorded() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    // The input is normalized by its field type before anything uses it.
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order("  A-100 ")).await;
    assert!(completed(&outcome), "{outcome:?}");

    // Guard asked the one decider, about a read that writes nothing.
    let questions = scene.control.questions.lock().unwrap().clone();
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].tool.as_str(), ORDER_GET);
    assert!(!questions[0].writes);

    // The first binding in the plan's order was chosen, and its driver was
    // prepared for a live run and asked for one step.
    assert_eq!(
        chosen(&scene),
        vec![(FIRST_MODE.to_owned(), ChoiceReason::FirstInOrder)]
    );
    assert_eq!(scene.hooks.postures.lock().unwrap().as_slice(), ["Live"]);
    assert_eq!(scene.hooks.count("get_order"), 1);

    // The result is what the test application holds.
    let result = scene.last_result();
    assert_eq!(result.get("order_id"), Some(&Value::from("A-100")));
    assert_eq!(
        result.get("customer"),
        Some(&Value::from("Example Customer One"))
    );
    assert_eq!(result.get("status"), Some(&Value::from("draft")));
    assert_eq!(result.get("lines"), Some(&Value::from(3_i64)));

    // The run moved through the ledger, and a read planned no record.
    assert_eq!(
        scene
            .hooks
            .seen()
            .iter()
            .filter(|entry| entry.starts_with("moved:"))
            .cloned()
            .collect::<Vec<_>>(),
        ["moved:validating", "moved:running"]
    );
    assert_eq!(scene.run_row().await.state, RunState::Completed);
    assert!(scene.records().await.is_empty());
    assert!(scene.app.writes().await.unwrap().is_empty());
}

#[tokio::test]
async fn the_step_trace_is_sent_with_its_signature() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order("A-100")).await;
    assert!(completed(&outcome), "{outcome:?}");
    let traces = scene.control.traces.lock().unwrap().clone();
    assert_eq!(traces.len(), 1);
    let trace = &traces[0];
    let text = String::from_utf8(trace.bytes().to_vec()).unwrap();
    // One line per part of the call that happened, in order.
    for part in ["guard", "choose", "prepare", "act", "record"] {
        assert!(text.contains(part), "the trace has no `{part}`: {text}");
    }
    assert!(text.contains(ORDER_GET));
    // The signature is over the bytes sent.
    assert_eq!(trace.signature(), &Digest::of(trace.bytes()));
}

#[tokio::test]
async fn a_plan_that_is_not_the_one_its_hash_names_is_refused_at_guard() {
    let mut scene = Scene::new(ORDER_GET, |_| {}).await;
    scene.plan_hash = Digest::of(b"another plan");
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order("A-100")).await;
    let JobOutcome::Finished {
        state: RunState::Rejected,
        failure: Some(failure),
    } = &outcome
    else {
        panic!("{outcome:?}");
    };
    assert_eq!(failure.class(), FailureClass::Input);
    assert_eq!(failure.code().as_str(), "plan_hash_mismatch");
    // Nothing was asked of the decider or of a driver.
    assert!(scene.control.questions.lock().unwrap().is_empty());
    assert!(scene.hooks.seen().is_empty());
}

#[tokio::test]
async fn a_refusal_at_guard_ends_the_run_before_a_driver_is_reached() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    scene.control.faults.denied.store(true, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order("A-100")).await;
    let failure = outcome.failure().expect("a refusal carries a failure");
    assert_eq!(failure.class(), FailureClass::Policy);
    assert_eq!(outcome.run_state(), Some(RunState::Failed));
    assert_eq!(scene.hooks.count("get_order"), 0);
}

#[tokio::test]
async fn an_input_of_the_wrong_type_is_rejected() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    let outcome = scene
        .work(
            JobKind::Execute,
            ORDER_GET,
            Values::new().with("order_id", 7_i64),
        )
        .await;
    assert_eq!(outcome.run_state(), Some(RunState::Rejected));
    assert_eq!(
        outcome.failure().map(a2m_core::Failure::class),
        Some(FailureClass::Input)
    );
    assert_eq!(scene.hooks.count("get_order"), 0);
}

#[tokio::test]
async fn a_read_falls_to_the_next_binding_when_the_first_drifts() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    scene.hooks.first_mode_drifts.store(true, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order("A-100")).await;
    assert!(completed(&outcome), "{outcome:?}");
    let chosen = chosen(&scene);
    assert_eq!(chosen.len(), 2, "{chosen:?}");
    assert_eq!(chosen[0].0, FIRST_MODE);
    assert_eq!(chosen[1].0, SECOND_MODE);
    assert_eq!(scene.last_result().get("lines"), Some(&Value::from(3_i64)));
    // The failure of the first binding was reported as its health.
    let health = scene
        .control
        .events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            EngineEvent::BindingHealth { mode, failure, .. } => Some((
                mode.to_string(),
                failure.as_ref().map(|(class, _)| *class),
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        health.contains(&(FIRST_MODE.to_owned(), Some(FailureClass::Drift))),
        "{health:?}"
    );
}

#[tokio::test]
async fn a_binding_known_to_have_drifted_is_passed_over() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    scene.conditions.lock().unwrap().drifted.push((
        a2m_core::OperationName::new(ORDER_GET).unwrap(),
        a2m_core::Mode::new(FIRST_MODE).unwrap(),
    ));
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order("A-100")).await;
    assert!(completed(&outcome), "{outcome:?}");
    let chosen = chosen(&scene);
    assert_eq!(chosen.len(), 1, "{chosen:?}");
    assert_eq!(chosen[0].0, SECOND_MODE);
}

#[tokio::test]
async fn a_transient_failure_of_a_read_is_tried_again_and_no_more_than_the_limit() {
    // The stock endpoint of the test application fails every third call.
    let scene = Scene::new(STOCK_GET, |_| {}).await;
    for _ in 0..2 {
        let app = scene.app.http();
        let _ = app.get(scene.app.url("/api/stock/L-101")).send().await;
    }
    let outcome = scene
        .work(
            JobKind::Execute,
            STOCK_GET,
            Values::new().with("item_code", "L-101"),
        )
        .await;
    assert!(completed(&outcome), "{outcome:?}");
    // The third call failed, and the step was sent once more.
    assert_eq!(scene.hooks.count("get_stock"), 2);
}

#[tokio::test]
async fn a_surface_that_needs_a_sign_in_rests_the_run_in_needs_login() {
    let scene = Scene::new(SECURE_GET, |_| {}).await;
    let outcome = scene.work(JobKind::Execute, SECURE_GET, order("A-100")).await;
    let JobOutcome::Rests {
        state: RunState::NeedsLogin,
        failure: Some(failure),
    } = &outcome
    else {
        panic!("{outcome:?}");
    };
    assert_eq!(failure.class(), FailureClass::SessionExpired);
    // The keeper was asked for a lease on the surface, and no driver acted.
    let wants = scene.control.wants.lock().unwrap().clone();
    assert_eq!(wants.len(), 1);
    assert_eq!(wants[0].surface.as_str(), "secure");
    assert_eq!(scene.hooks.count("get_order"), 0);
    assert_eq!(scene.run_row().await.state, RunState::NeedsLogin);
}

#[tokio::test]
async fn a_session_known_to_need_a_person_is_not_asked_for() {
    let scene = Scene::new(SECURE_GET, |_| {}).await;
    scene.conditions.lock().unwrap().sessions.push((
        SurfaceName::new("secure").unwrap(),
        SessionState::NeedsLogin,
    ));
    let outcome = scene.work(JobKind::Execute, SECURE_GET, order("A-100")).await;
    assert_eq!(outcome.run_state(), Some(RunState::NeedsLogin));
    assert!(scene.control.wants.lock().unwrap().is_empty());
    assert_eq!(scene.hooks.count("get_order"), 0);
}
