//! A binding in shadow beside a live read, and what it earns (decision
//! 0207): the call is the verified binding's, whatever the shadow does; the
//! comparison is kept by the shadow binding's digest; a binding is
//! promotable only after the set number of exact matches in a row, and a
//! mismatch starts the count again. The shadow of a write is not built.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use a2m_core::{BindingStatus, Digest, FailureClass, Mode, OperationName, Plan, Value, Values};
use a2m_engine::{JobKind, binding_digest};
use a2m_ledger::{
    Difference, DifferingOutput, NotPromotable, Promotable, ShadowOutcome, ShadowStanding,
    ShadowSubject, ShadowThreshold,
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
            )
        })
        .await
        .unwrap()
}

async fn promotable(scene: &Scene) -> Promotable {
    let binding = shadow_digest(scene);
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
                },
                ShadowThreshold::default(),
            )
        })
        .await
        .unwrap()
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

/// One live call of the read, in a run of its own.
async fn call(scene: &Scene, first: bool) {
    if !first {
        scene.another_run(ORDER_GET).await;
    }
    let outcome = scene
        .work(JobKind::Execute, ORDER_GET, order("A-100"))
        .await;
    assert!(completed(&outcome), "{outcome:?}");
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
async fn a_shadow_binding_is_promotable_after_five_matches_in_a_row_and_a_mismatch_starts_again() {
    let scene = shadow_scene().await;
    assert_eq!(
        promotable(&scene).await,
        Promotable::No(NotPromotable::NothingCompared)
    );
    for at in 0..4 {
        call(&scene, at == 0).await;
    }
    *scene.hooks.mode_alters.lock().unwrap() = Some((
        SECOND_MODE.to_owned(),
        "lines".to_owned(),
        Value::from(99_i64),
    ));
    call(&scene, false).await;
    *scene.hooks.mode_alters.lock().unwrap() = None;
    for _ in 0..4 {
        call(&scene, false).await;
    }
    assert!(matches!(
        promotable(&scene).await,
        Promotable::No(NotPromotable::MismatchSince {
            in_a_row: 4,
            threshold: 5,
            ..
        })
    ));
    call(&scene, false).await;
    assert!(matches!(
        promotable(&scene).await,
        Promotable::Yes {
            in_a_row: 5,
            threshold: 5,
            ..
        }
    ));
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
                },
                ShadowThreshold::default(),
            )
        })
        .await
        .unwrap();
    assert_eq!(answer, Promotable::No(NotPromotable::WritesNotBuilt));
}
