//! Package 9.1, the engine's half (decision 0380): a drift a canary sees
//! at a step names the pack entry the step refers to; that entry stops
//! every binding of the plan that refers to it, a write among them, before
//! anything is sent; a canary is never stopped by a drift, since it is the
//! check of whether the binding fits again, and its pass vouches for what
//! it relied on; and a mark held to the entry's knowledge does not stop a
//! version that changed that knowledge, a repair, while it still stops one
//! that left it as it was.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::sync::atomic::Ordering;

use a2m_core::{FailureClass, Mode, OperationName, PackRef, SectionIndex, SurfaceName, Values};
use a2m_engine::{DriftKnowledge, DriftMark, EngineEvent, JobKind, drift_covers};
use common::{
    FIRST_MODE, ITEM_UPSERT, ITEM_UPSERT_EXACT, ORDER_GET, SCREEN, SECOND_MODE, STOCK_GET, Scene,
    completed, failed_as, item, plan, screens,
};

fn name(text: &str) -> OperationName {
    OperationName::new(text).unwrap()
}

fn mode(text: &str) -> Mode {
    Mode::new(text).unwrap()
}

/// Every binding health the scene's control was sent: the mode, whether
/// it failed, and the knowledge it named.
fn health(scene: &Scene) -> Vec<(String, Option<FailureClass>, Vec<DriftKnowledge>)> {
    scene
        .control
        .events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            EngineEvent::BindingHealth {
                mode,
                failure,
                knowledge,
                ..
            } => Some((
                mode.to_string(),
                failure.as_ref().map(|(class, _)| *class),
                knowledge.clone(),
            )),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_canary_that_drifts_names_the_screen_and_the_screen_stops_the_write_before_anything_is_sent()
 {
    // The system changed under the first mode: every step through it now
    // finds a shape the plan does not describe.
    let canary = Scene::new(ORDER_GET, |plan| screens(plan, "v1")).await;
    canary.hooks.first_mode_drifts.store(true, Ordering::SeqCst);
    let outcome = canary
        .work(
            JobKind::Canary,
            ORDER_GET,
            Values::new().with("order_id", "A-100"),
        )
        .await;
    // The canary's read fell to the spare mode, which still fits.
    assert!(completed(&outcome), "{outcome:?}");
    let told = health(&canary);
    let drifted: Vec<_> = told
        .iter()
        .filter(|(_, failure, _)| *failure == Some(FailureClass::Drift))
        .collect();
    assert_eq!(drifted.len(), 1, "{told:?}");
    assert_eq!(drifted[0].0, FIRST_MODE);
    let [DriftKnowledge::Entry { section, entry }] = drifted[0].2.as_slice() else {
        panic!("the drift names the screen: {told:?}");
    };
    assert_eq!(entry, SCREEN);
    // The spare mode's pass vouches again for the screen, through its own
    // mode only.
    assert!(
        told.iter()
            .any(|(mode, failure, knowledge)| mode == SECOND_MODE
                && failure.is_none()
                && knowledge.contains(&drifted[0].2[0])),
        "{told:?}"
    );

    // The mark stops every binding through that mode that goes through the
    // screen: the read and the write, and not the write that does not, nor
    // the read's binding through the spare mode.
    let mark = DriftMark {
        operation: name(ORDER_GET),
        mode: mode(FIRST_MODE),
        surface: SurfaceName::new("app").unwrap(),
        knowledge: DriftKnowledge::Entry {
            section: *section,
            entry: entry.clone(),
        },
    };
    let write = Scene::new(ITEM_UPSERT, |plan| screens(plan, "v1")).await;
    let mut written = plan(&write.app.url(""));
    screens(&mut written, "v1");
    let covered = drift_covers(&written, std::slice::from_ref(&mark));
    assert_eq!(
        covered,
        vec![
            (name(ORDER_GET), mode(FIRST_MODE)),
            (name(ITEM_UPSERT), mode(FIRST_MODE)),
        ]
    );

    // A write the mark stops is refused with `drift`, naming its binding,
    // and nothing reaches the system: no read for the preview, no save.
    write.conditions.lock().unwrap().drifted = covered;
    let rail = item("A-102", "L-300", "New rail", 3, "150.5");
    let outcome = write.work(JobKind::Preview, ITEM_UPSERT, rail).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Drift, "binding_drifted".to_owned())
    );
    let message = outcome.failure().unwrap().message().to_owned();
    assert!(message.contains(ITEM_UPSERT), "{message}");
    assert_eq!(write.hooks.steps_of(FIRST_MODE), 0);
    assert_eq!(write.app.write_count("A-102", "L-300").await.unwrap(), 0);
}

#[tokio::test]
async fn a_canary_is_never_stopped_by_a_drift_and_a_live_read_is() {
    let scene = Scene::new(ORDER_GET, |plan| screens(plan, "v1")).await;
    scene.conditions.lock().unwrap().drifted = vec![
        (name(ORDER_GET), mode(FIRST_MODE)),
        (name(ORDER_GET), mode(SECOND_MODE)),
    ];
    let order = Values::new().with("order_id", "A-100");
    let live = scene.work(JobKind::Execute, ORDER_GET, order.clone()).await;
    assert_eq!(
        failed_as(&live),
        (FailureClass::Drift, "binding_drifted".to_owned())
    );
    assert_eq!(scene.hooks.count("get_order"), 0);

    // The canary reads through the first binding, as a live read would if
    // nothing had drifted, and says that it fits.
    scene.new_lease().await;
    let canary = scene.work(JobKind::Canary, ORDER_GET, order).await;
    assert!(completed(&canary), "{canary:?}");
    assert_eq!(scene.hooks.steps_of(FIRST_MODE), 1);
    let told = health(&scene);
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].0, FIRST_MODE);
    assert_eq!(told[0].1, None);
    // What it vouches for: the screen and the binding itself.
    assert!(
        told[0]
            .2
            .iter()
            .any(|known| matches!(known, DriftKnowledge::Entry { entry, .. } if entry == SCREEN)),
        "{told:?}"
    );
    assert!(
        told[0]
            .2
            .iter()
            .any(|known| matches!(known, DriftKnowledge::Binding(_))),
        "{told:?}"
    );
}

#[test]
fn a_mark_does_not_stop_a_version_that_changed_its_screen_and_still_stops_one_that_did_not() {
    let address = "http://127.0.0.1:1";
    let mut before = plan(address);
    screens(&mut before, "v1");
    let section = a2m_engine::section_digest(
        &before,
        &PackRef {
            section: SectionIndex::new(0),
            entry: SCREEN.to_owned(),
        },
    )
    .unwrap();
    let mark = DriftMark {
        operation: name(ORDER_GET),
        mode: mode(FIRST_MODE),
        surface: SurfaceName::new("app").unwrap(),
        knowledge: DriftKnowledge::Entry {
            section,
            entry: SCREEN.to_owned(),
        },
    };

    // A version that changed something else: still stopped.
    let mut unrelated = before.clone();
    unrelated.tools[0].description = "Reads one order.".to_owned();
    assert_eq!(
        drift_covers(&unrelated, std::slice::from_ref(&mark)).len(),
        2
    );
    // The repair: the screen as its model now says it is.
    let mut repaired = plan(address);
    screens(&mut repaired, "v2");
    assert!(drift_covers(&repaired, std::slice::from_ref(&mark)).is_empty());
}

#[test]
fn a_drift_no_step_named_stops_every_binding_of_its_mode_on_its_surface_until_its_binding_changes()
{
    let address = "http://127.0.0.1:1";
    let before = plan(address);
    // The read of stock refers to no entry: its binding is the knowledge.
    let stock = before
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == STOCK_GET)
        .unwrap();
    let knowledge = a2m_engine::drifted_knowledge(&before, &stock.bindings[0], None).unwrap();
    assert!(matches!(knowledge, DriftKnowledge::Binding(_)));
    let mark = DriftMark {
        operation: name(STOCK_GET),
        mode: mode(FIRST_MODE),
        surface: SurfaceName::new("app").unwrap(),
        knowledge,
    };
    let covered = drift_covers(&before, std::slice::from_ref(&mark));
    for stopped in [STOCK_GET, ORDER_GET, ITEM_UPSERT, ITEM_UPSERT_EXACT] {
        assert!(
            covered.contains(&(name(stopped), mode(FIRST_MODE))),
            "{stopped}: {covered:?}"
        );
    }
    assert!(
        !covered.iter().any(|(_, mode)| mode.as_str() == SECOND_MODE),
        "{covered:?}"
    );
    // A version whose read of stock is another binding is not stopped.
    let mut repaired = before.clone();
    let at = repaired
        .operations
        .iter()
        .position(|operation| operation.name.as_str() == STOCK_GET)
        .unwrap();
    repaired.operations[at].bindings[0].act[0].timeout_ms += 1;
    assert!(drift_covers(&repaired, std::slice::from_ref(&mark)).is_empty());
}
