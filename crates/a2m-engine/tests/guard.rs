//! Guard's five checks, in their order (decision 0309): the plan's hash,
//! the kernel's health, the session's state, the policy decision, and the
//! edge block. Each test shows its check stopping the run with its own
//! failure, and holds something that only that check, made where guard
//! makes it, would give: were the check taken out, a later one, choose, or
//! a driver would be reached instead, and the test would fail.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::sync::atomic::Ordering;

use a2m_core::{Digest, FailureClass, FailureCode, RunState, SessionState, SurfaceName, Values};
use a2m_engine::{JobKind, KernelHealth};
use common::{ITEM_UPSERT, ORDER_GET, Scene, failed_as, item};

fn order() -> Values {
    Values::new().with("order_id", "A-100")
}

fn rail() -> Values {
    item("A-102", "L-300", "New rail", 3, "150.5")
}

fn app() -> SurfaceName {
    SurfaceName::new("app").unwrap()
}

/// Nothing reached a driver, and nothing was asked of the keeper for a
/// lease.
fn no_driver_reached(scene: &Scene) {
    assert!(scene.hooks.postures.lock().unwrap().is_empty());
    assert!(scene.control.wants.lock().unwrap().is_empty());
    assert!(
        scene
            .steps(&["get_order", "get_item", "put_item", "lock", "intend"])
            .is_empty()
    );
}

fn questions(scene: &Scene) -> usize {
    scene.control.questions.lock().unwrap().len()
}

fn moved_to_running(scene: &Scene) -> bool {
    scene
        .hooks
        .seen()
        .iter()
        .any(|entry| entry == "moved:running")
}

#[tokio::test]
async fn guard_one_a_plan_that_is_not_the_published_version_stops_the_run() {
    let mut scene = Scene::new(ORDER_GET, |_| {}).await;
    scene.plan_hash = Digest::of(b"another plan");
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Policy, "plan_unverified".to_owned())
    );
    assert_eq!(outcome.run_state(), Some(RunState::Failed));
    // Nothing after the first check was asked.
    assert_eq!(questions(&scene), 0);
    no_driver_reached(&scene);
}

#[tokio::test]
async fn guard_two_a_write_on_a_kernel_that_is_not_ok_stops_the_run() {
    let scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    scene.conditions.lock().unwrap().health =
        KernelHealth::Degraded(FailureCode::fixed("storage_full"));
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Busy, "engine_degraded".to_owned())
    );
    // Without the check the policy engine would be asked and the preview
    // would read the record.
    assert_eq!(questions(&scene), 0);
    no_driver_reached(&scene);
    assert!(scene.app.writes().await.unwrap().is_empty());
}

#[tokio::test]
async fn guard_three_a_session_that_has_ended_stops_the_run_before_policy_is_asked() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    scene
        .conditions
        .lock()
        .unwrap()
        .sessions
        .push((app(), SessionState::NeedsPerson));
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::SessionExpired, "needs_login".to_owned())
    );
    assert_eq!(outcome.run_state(), Some(RunState::NeedsLogin));
    // Choose would refuse the bindings too, but only after the policy
    // engine had been asked and the run had moved to `running`: guard
    // stops it first.
    assert_eq!(questions(&scene), 0);
    assert!(!moved_to_running(&scene));
    no_driver_reached(&scene);
}

#[tokio::test]
async fn guard_three_a_session_that_is_usable_lets_the_run_through() {
    // The check reads the keeper's answer: a session that is renewing is
    // usable, and the call goes on.
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    scene
        .conditions
        .lock()
        .unwrap()
        .sessions
        .push((app(), SessionState::Renewing));
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order()).await;
    assert_eq!(
        outcome.run_state(),
        Some(RunState::Completed),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn guard_four_a_policy_refusal_stops_the_run_and_the_question_was_asked() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    scene.control.faults.denied.store(true, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Policy, "not_granted".to_owned())
    );
    assert_eq!(outcome.run_state(), Some(RunState::Failed));
    // The decision was asked for once, for this tool, and nothing after
    // it ran.
    let asked = scene.control.questions.lock().unwrap().clone();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].tool.as_str(), ORDER_GET);
    assert!(!moved_to_running(&scene));
    no_driver_reached(&scene);
}

#[tokio::test]
async fn guard_five_an_edge_block_stops_the_run_after_policy_and_before_running() {
    let scene = Scene::new(ORDER_GET, |_| {}).await;
    scene.conditions.lock().unwrap().edge_blocked.push(app());
    let outcome = scene.work(JobKind::Execute, ORDER_GET, order()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::EdgeBlocked, "edge_blocked".to_owned())
    );
    // Policy was asked, and the run never moved to `running`: without the
    // check at guard, choose would find the block only after that move.
    assert_eq!(questions(&scene), 1);
    assert!(!moved_to_running(&scene));
    no_driver_reached(&scene);
}

#[tokio::test]
async fn guard_asks_its_five_checks_in_order_and_the_first_that_fails_names_the_stop() {
    // A write on which all five checks fail. Each run takes one fault
    // away, and the next check names the stop.
    let mut scene = Scene::new(ITEM_UPSERT, |_| {}).await;
    let published = scene.plan_hash;
    {
        let mut conditions = scene.conditions.lock().unwrap();
        conditions.health = KernelHealth::Degraded(FailureCode::fixed("storage_full"));
        conditions.sessions.push((app(), SessionState::NeedsPerson));
        conditions.edge_blocked.push(app());
    }
    scene.control.faults.denied.store(true, Ordering::SeqCst);
    scene.plan_hash = Digest::of(b"another plan");

    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(failed_as(&outcome).1, "plan_unverified");

    scene.plan_hash = published;
    scene.another_run(ITEM_UPSERT).await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(failed_as(&outcome).1, "engine_degraded");

    scene.conditions.lock().unwrap().health = KernelHealth::Ok;
    scene.another_run(ITEM_UPSERT).await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(failed_as(&outcome).1, "needs_login");
    assert_eq!(questions(&scene), 0, "the session is asked before policy");

    scene.conditions.lock().unwrap().sessions.clear();
    scene.another_run(ITEM_UPSERT).await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(failed_as(&outcome).1, "not_granted");
    assert_eq!(questions(&scene), 1);

    scene.control.faults.denied.store(false, Ordering::SeqCst);
    scene.another_run(ITEM_UPSERT).await;
    let outcome = scene.work(JobKind::Execute, ITEM_UPSERT, rail()).await;
    assert_eq!(failed_as(&outcome).1, "edge_blocked");
    assert_eq!(questions(&scene), 2);

    // Not one of the five runs reached a driver or the system.
    no_driver_reached(&scene);
    assert!(scene.app.writes().await.unwrap().is_empty());
}
