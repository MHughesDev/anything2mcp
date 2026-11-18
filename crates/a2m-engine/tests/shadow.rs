//! A binding in shadow beside a live read, and what it earns (decisions
//! 0207 and 0251): the call is the verified binding's, whatever the shadow
//! does; every shadow of a job shares one budget of time, and its
//! comparisons are kept only after the result was sent; the comparison is
//! kept by the shadow binding's digest with the keyed digest of the input;
//! a binding is promotable only after the set number of distinct inputs
//! matched, and a mismatch starts the count again. The shadow of a write is
//! not built.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use a2m_core::{
    BindingStatus, Digest, Environment, FailureClass, Mode, OperationName, Plan, RatePlan, Value,
    Values,
};
use a2m_engine::{JobKind, binding_digest};
use a2m_ledger::{
    Difference, DifferingOutput, EnvironmentEvidence, NotPromotable, Promotable, ShadowOutcome,
    ShadowStanding, ShadowSubject, ShadowThreshold,
};
use common::{
    FIRST_MODE, ITEM_UPSERT, ORDER_GET, SECOND_MODE, Scene, approved, completed, item, workspace,
};

fn order(id: &str) -> Values {
    Values::new().with("order_id", id)
}

/// The read of an order with its second binding in shadow.
fn in_shadow(plan: &mut Plan) {
    plan.operations[0].bindings[1].status = BindingStatus::Shadow;
}

async fn shadow_scene() -> Scene {
    let scene = Scene::new(ORDER_GET, in_shadow).await;
    scene.control.keeps_shadow.store(true, Ordering::SeqCst);
    scene
}

fn shadow_digest(scene: &Scene) -> Digest {
    let plan = &scene.control.plan;
    binding_digest(plan, &plan.operations[0].bindings[1]).unwrap()
}

async fn standing(scene: &Scene) -> Option<ShadowStanding> {
    let binding = shadow_digest(scene);
    scene
        .control
        .store
        .read(move |tx| {
            tx.shadow().standing(
                &workspace(),
                &a2m_core::TargetName::new("example").unwrap(),
                &OperationName::new(ORDER_GET).unwrap(),
                &binding,
                None,
            )
        })
        .await
        .unwrap()
}

/// Three distinct inputs make a binding promotable here: the test
/// application holds three orders.
const THRESHOLD: ShadowThreshold = ShadowThreshold { read: 3, write: 20 };

const ORDERS: [&str; 3] = ["A-100", "A-101", "A-102"];

async fn promotable(scene: &Scene) -> Promotable {
    let plan = std::sync::Arc::clone(&scene.control.plan);
    let binding = shadow_digest(scene);
    let against = a2m_engine::verified_binding(&plan, &plan.operations[0])
        .unwrap()
        .digest;
    let environments = a2m_engine::shadow_evidence_environments(&plan);
    scene
        .control
        .store
        .read(move |tx| {
            tx.shadow().promotable(
                &ShadowSubject {
                    workspace: workspace(),
                    target: a2m_core::TargetName::new("example").unwrap(),
                    operation: OperationName::new(ORDER_GET).unwrap(),
                    binding,
                    writes: false,
                    against,
                    environments,
                },
                THRESHOLD,
            )
        })
        .await
        .unwrap()
}

fn held(distinct_inputs: u32, matches: u32) -> EnvironmentEvidence {
    EnvironmentEvidence {
        environment: Environment::new("test").unwrap(),
        distinct_inputs,
        matches,
    }
}

fn customer(scene: &Scene) -> Value {
    scene.last_result().get("customer").cloned().unwrap()
}

fn last_outcome(scene: &Scene) -> ShadowOutcome {
    scene
        .control
        .shadows
        .lock()
        .unwrap()
        .last()
        .expect("a shadow was compared")
        .outcome
        .clone()
}

/// One live call of the read of the order `id`, in a run of its own.
async fn call_of(scene: &Scene, first: bool, id: &str) {
    if !first {
        scene.another_run(ORDER_GET).await;
    }
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order(id)).await;
    assert!(completed(&outcome), "{outcome:?}");
}

/// One live call of the read of `A-100`.
async fn call(scene: &Scene, first: bool) {
    call_of(scene, first, "A-100").await;
}

#[tokio::test]
async fn a_shadow_read_runs_beside_the_live_one_under_its_guard_and_leaves_the_trace_alone() {
    let scene = shadow_scene().await;
    // Without a control plane that keeps comparisons, no shadow runs.
    scene.control.keeps_shadow.store(false, Ordering::SeqCst);
    call(&scene, true).await;
    assert_eq!(scene.hooks.steps_of(SECOND_MODE), 0);
    let alone = scene.control.traces.lock().unwrap()[0].signature;

    scene.control.keeps_shadow.store(true, Ordering::SeqCst);
    call(&scene, false).await;
    // The caller was given the verified binding's result.
    assert_eq!(customer(&scene), Value::from("Example Customer One"));
    // The shadow binding read once, in a context whose posture keeps its
    // driver's read-only guard on, never the live call's.
    assert_eq!(scene.hooks.steps_of(FIRST_MODE), 2);
    assert_eq!(scene.hooks.steps_of(SECOND_MODE), 1);
    let postures = scene.hooks.postures.lock().unwrap().clone();
    assert_eq!(
        postures,
        ["Live", "Live", "Sandbox { test_records: false }"]
    );
    // It matched, and the ledger counts it by the shadow binding's digest.
    assert_eq!(last_outcome(&scene), ShadowOutcome::Matched);
    let compared = scene.control.shadows.lock().unwrap()[0].clone();
    assert_eq!(compared.shadow.digest, shadow_digest(&scene));
    assert_eq!(compared.shadow.position, 1);
    assert_eq!(compared.verified.position, 0);
    let standing = standing(&scene).await.unwrap();
    assert_eq!((standing.matches, standing.in_a_row), (1, 1));
    // It says what it cost: one step, and the run's events say a shadow
    // read was made beside the call, with no value.
    assert_eq!(compared.requests, 1);
    assert_eq!(standing.requests, 1);
    let run = scene.run();
    let events = scene
        .control
        .store
        .read(move |tx| tx.ledger().events(run, 0, 100))
        .await
        .unwrap();
    let said: Vec<&String> = events
        .iter()
        .map(|event| &event.payload)
        .filter(|payload| payload.contains("shadow_read"))
        .collect();
    assert_eq!(said.len(), 1, "{events:?}");
    assert!(said[0].contains("\"requests\":1") && said[0].contains("\"outcome\":\"match\""));
    assert!(!said[0].contains("A-100") && !said[0].contains("Example Customer"));
    // What the shadow did is in no step of the run's trace.
    let traces = scene.control.traces.lock().unwrap().clone();
    assert_eq!(traces[1].records, 1);
    assert_eq!(traces[1].signature, alone);
}

#[tokio::test]
async fn a_shadow_that_differs_fails_or_hangs_changes_nothing_the_caller_sees() {
    let scene = Scene::with_settings(ORDER_GET, in_shadow, |settings| {
        settings.shadow_limit = Duration::from_millis(300);
    })
    .await;
    scene.control.keeps_shadow.store(true, Ordering::SeqCst);

    // It differs: a mismatch, named by output, with no value kept.
    *scene.hooks.mode_alters.lock().unwrap() = Some((
        SECOND_MODE.to_owned(),
        "customer".to_owned(),
        Value::from("Someone Else"),
    ));
    call(&scene, true).await;
    assert_eq!(customer(&scene), Value::from("Example Customer One"));
    assert_eq!(
        last_outcome(&scene),
        ShadowOutcome::Mismatched(vec![DifferingOutput {
            name: "customer".to_owned(),
            how: Difference::Differs,
        }])
    );
    *scene.hooks.mode_alters.lock().unwrap() = None;

    // It fails: not compared, with what stopped it.
    *scene.hooks.mode_fails.lock().unwrap() = Some(SECOND_MODE.to_owned());
    call(&scene, false).await;
    assert_eq!(customer(&scene), Value::from("Example Customer One"));
    assert!(matches!(
        last_outcome(&scene),
        ShadowOutcome::NotCompared {
            class: FailureClass::Transient,
            ..
        }
    ));
    *scene.hooks.mode_fails.lock().unwrap() = None;

    // It never ends: cancelled at its bound, and not compared.
    *scene.hooks.mode_hangs.lock().unwrap() = Some(SECOND_MODE.to_owned());
    call(&scene, false).await;
    assert_eq!(customer(&scene), Value::from("Example Customer One"));
    match last_outcome(&scene) {
        ShadowOutcome::NotCompared { class, code } => {
            assert_eq!(class, FailureClass::Busy);
            assert_eq!(code.as_str(), "shadow_time_exceeded");
        }
        other => panic!("a shadow past its bound was {other:?}"),
    }

    let standing = standing(&scene).await.unwrap();
    assert_eq!(
        (
            standing.matches,
            standing.mismatches,
            standing.not_compared,
            standing.in_a_row
        ),
        (0, 1, 2, 0)
    );
    let last = standing.last_mismatch.unwrap();
    assert_eq!(last.differing[0].name, "customer");
}

#[tokio::test]
async fn one_input_called_again_and_again_is_one_input_and_a_mismatch_starts_again() {
    let scene = shadow_scene().await;
    assert_eq!(
        promotable(&scene).await,
        Promotable::No(NotPromotable::NothingCompared)
    );
    // Five calls with one input: one distinct input, not promotable.
    for at in 0..5 {
        call(&scene, at == 0).await;
    }
    assert_eq!(
        promotable(&scene).await,
        Promotable::No(NotPromotable::TooFew {
            evidence: vec![held(1, 5)],
            threshold: 3
        })
    );
    // A mismatch starts the count again.
    *scene.hooks.mode_alters.lock().unwrap() = Some((
        SECOND_MODE.to_owned(),
        "lines".to_owned(),
        Value::from(99_i64),
    ));
    call_of(&scene, false, "A-101").await;
    *scene.hooks.mode_alters.lock().unwrap() = None;
    for id in &ORDERS[..2] {
        call_of(&scene, false, id).await;
    }
    assert!(matches!(
        promotable(&scene).await,
        Promotable::No(NotPromotable::MismatchSince { ref evidence, threshold: 3, .. })
            if *evidence == vec![held(2, 2)]
    ));
    // The third distinct input that matches makes it promotable.
    call_of(&scene, false, "A-102").await;
    assert!(matches!(
        promotable(&scene).await,
        Promotable::Yes { ref evidence, threshold: 3, .. } if *evidence == vec![held(3, 3)]
    ));
}

#[tokio::test]
async fn every_shadow_of_a_call_shares_one_budget_and_its_keeping_never_holds_the_answer() {
    // A shadow that never ends, beside one whose comparison is never kept:
    // the call is held up by at most the one budget.
    let limit = Duration::from_secs(1);
    let scene = Scene::with_settings(ORDER_GET, in_shadow, |settings| {
        settings.shadow_limit = limit;
    })
    .await;
    scene.control.keeps_shadow.store(true, Ordering::SeqCst);
    *scene.hooks.mode_hangs.lock().unwrap() = Some(SECOND_MODE.to_owned());
    scene
        .control
        .shadow_write_hangs
        .store(true, Ordering::SeqCst);
    let started = Instant::now();
    call(&scene, true).await;
    let took = started.elapsed();
    assert!(took < limit + Duration::from_secs(1), "{took:?}");
    assert_eq!(customer(&scene), Value::from("Example Customer One"));
    // The comparison was dropped and counted; nothing was kept.
    assert_eq!(scene.engine.shadow_comparisons_lost(), 1);
    assert!(standing(&scene).await.is_none());

    // A keeping that fails changes nothing the caller is given.
    *scene.hooks.mode_hangs.lock().unwrap() = None;
    scene
        .control
        .shadow_write_hangs
        .store(false, Ordering::SeqCst);
    scene
        .control
        .shadow_write_fails
        .store(true, Ordering::SeqCst);
    call(&scene, false).await;
    assert_eq!(customer(&scene), Value::from("Example Customer One"));
    assert_eq!(scene.engine.shadow_comparisons_lost(), 2);
    assert!(standing(&scene).await.is_none());
}

#[tokio::test]
async fn a_shadow_never_takes_what_a_callers_read_needs_of_the_rate_budget() {
    let scene = Scene::new(ORDER_GET, |plan| {
        in_shadow(plan);
        for surface in &mut plan.surfaces {
            surface.rate = Some(RatePlan {
                requests: 10,
                per_ms: 1000,
            });
        }
    })
    .await;
    scene.control.keeps_shadow.store(true, Ordering::SeqCst);
    // Granted as a shadow's: counted, and compared.
    call(&scene, true).await;
    assert_eq!(scene.control.shadow_requests.load(Ordering::SeqCst), 1);
    assert_eq!(last_outcome(&scene), ShadowOutcome::Matched);
    // Refused as a shadow's: nothing sent, not compared, and the caller
    // given the verified binding's result.
    scene
        .control
        .shadow_rate_refused
        .store(true, Ordering::SeqCst);
    let before = scene.hooks.steps_of(SECOND_MODE);
    call(&scene, false).await;
    assert_eq!(scene.hooks.steps_of(SECOND_MODE), before);
    assert_eq!(customer(&scene), Value::from("Example Customer One"));
    match last_outcome(&scene) {
        ShadowOutcome::NotCompared { class, code } => {
            assert_eq!(class, FailureClass::Busy);
            assert_eq!(code.as_str(), "no_rate_budget");
        }
        other => panic!("a shadow with no budget was {other:?}"),
    }
    assert_eq!(scene.control.shadows.lock().unwrap()[1].requests, 0);
}

#[tokio::test]
async fn nothing_runs_in_shadow_in_a_try_an_offline_test_a_pinned_call_or_a_named_binding() {
    let scene = shadow_scene().await;
    let outcome = scene.work(JobKind::Try, ORDER_GET, order("A-100")).await;
    assert!(completed(&outcome), "{outcome:?}");
    scene.another_run(ORDER_GET).await;
    *scene.pin.lock().unwrap() = Some(Mode::new(FIRST_MODE).unwrap());
    let outcome = scene
        .work(JobKind::Execute, ORDER_GET, order("A-100"))
        .await;
    assert!(completed(&outcome), "{outcome:?}");
    *scene.pin.lock().unwrap() = None;
    scene.another_run(ORDER_GET).await;
    let plan = std::sync::Arc::clone(&scene.control.plan);
    *scene.binding.lock().unwrap() = a2m_engine::BindingPin::of(&plan, &plan.operations[0], 0);
    let outcome = scene
        .work(JobKind::Execute, ORDER_GET, order("A-100"))
        .await;
    assert!(completed(&outcome), "{outcome:?}");
    // An offline test, against a replay of recorded answers.
    scene.another_run(ORDER_GET).await;
    *scene.binding.lock().unwrap() = None;
    let recorded = Values::new()
        .with("order_id", "A-100")
        .with("customer", "Replayed Customer")
        .with("status", "draft")
        .with("lines", Value::Int(2));
    let replay = a2m_provider::Replay::new(vec![a2m_provider::RecordedAnswer {
        name: "get_order".to_owned(),
        args: None,
        answer: a2m_provider::Recorded::Done(recorded),
        repeats: true,
    }]);
    let job = scene.job(JobKind::Execute, ORDER_GET, order("A-100")).await;
    let outcome = scene
        .engine
        .run_offline(&job, scene.control.as_ref(), replay)
        .await;
    assert!(completed(&outcome), "{outcome:?}");

    assert_eq!(scene.hooks.steps_of(SECOND_MODE), 0);
    assert!(scene.control.shadows.lock().unwrap().is_empty());
    assert!(standing(&scene).await.is_none());
}

#[tokio::test]
async fn a_write_never_runs_its_shadow_binding_and_goes_through_the_approved_one() {
    // The write has a second binding, in shadow, beside its verified one.
    let scene = Scene::new(ITEM_UPSERT, |plan| {
        let operation = &mut plan.operations[2];
        let mut spare = operation.bindings[0].clone();
        spare.mode = Mode::new(SECOND_MODE).unwrap();
        spare.status = BindingStatus::Shadow;
        operation.bindings.push(spare);
    })
    .await;
    scene.control.keeps_shadow.store(true, Ordering::SeqCst);
    let input = item("A-102", "L-300", "New rail", 3, "150.5");
    approved(&scene, ITEM_UPSERT, &input).await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, input).await;
    assert!(completed(&outcome), "{outcome:?}");
    // The stand holds one write, sent through the approved binding; the
    // binding in shadow saw no step, not even a read, and nothing was
    // compared or counted.
    assert_eq!(scene.app.write_count("A-102", "L-300").await.unwrap(), 1);
    assert_eq!(scene.hooks.steps_of(SECOND_MODE), 0);
    assert!(scene.control.shadows.lock().unwrap().is_empty());
    // Such a binding is never promotable through shadow.
    let plan = std::sync::Arc::clone(&scene.control.plan);
    let binding = binding_digest(&plan, &plan.operations[2].bindings[1]).unwrap();
    let answer = scene
        .control
        .store
        .read(move |tx| {
            tx.shadow().promotable(
                &ShadowSubject {
                    workspace: workspace(),
                    target: a2m_core::TargetName::new("example").unwrap(),
                    operation: OperationName::new(ITEM_UPSERT).unwrap(),
                    binding,
                    writes: true,
                    against: Digest::of(b"the verified binding"),
                    environments: vec![Environment::new("test").unwrap()],
                },
                ShadowThreshold::default(),
            )
        })
        .await
        .unwrap();
    assert_eq!(answer, Promotable::No(NotPromotable::WritesNotBuilt));
}

#[tokio::test]
async fn a_shadow_never_asks_for_a_session_the_call_does_not_hold() {
    // The binding in shadow reads through the surface that needs a sign-in;
    // the verified one through the surface that needs none.
    let scene = Scene::new(ORDER_GET, |plan| {
        in_shadow(plan);
        plan.operations[0].bindings[1].surface = a2m_core::SurfaceIndex::new(1);
    })
    .await;
    scene.control.keeps_shadow.store(true, Ordering::SeqCst);
    call(&scene, true).await;
    assert_eq!(customer(&scene), Value::from("Example Customer One"));
    // Nobody was asked for a session, and nothing was sent through it.
    assert!(scene.control.wants.lock().unwrap().is_empty());
    assert_eq!(scene.hooks.steps_of(SECOND_MODE), 0);
    match last_outcome(&scene) {
        ShadowOutcome::NotCompared { class, code } => {
            assert_eq!(class, FailureClass::Busy);
            assert_eq!(code.as_str(), "no_binding");
        }
        other => panic!("a shadow without the call's session was {other:?}"),
    }
}
