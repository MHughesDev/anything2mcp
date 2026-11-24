//! Build-order step 8, second half: a pipeline of several operations over
//! the rows of a source, with its limits. The roadmap's proof for package
//! 3.9: a sheet is entered once, and entering it again changes nothing.
//!
//! The sheet's bytes are built in each test and read by the built-in
//! reader of delimited text. Every test asks the test application what it
//! stored.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{FailureClass, RecordState, RunState, Value, Values};
use a2m_engine::{JobKind, JobOutcome};
use common::{SHEET_ENTER, Scene, approved, completed, failed_as, sheet, waits};

const DRIVER_STEPS: [&str; 3] = ["get_order", "get_item", "put_item"];

fn order() -> Values {
    Values::new().with("order_id", "A-102")
}

fn three_rows() -> Vec<u8> {
    sheet(&[
        ("L-1", "Top rail", "1", "60"),
        ("L-2", "Bottom rail", "1", "60.5"),
        ("L-3", "  Stile ", "2", "84.25"),
    ])
}

fn give(scene: &Scene, bytes: Vec<u8>) {
    *scene.source.lock().unwrap() = Some(Arc::new(bytes));
}

#[tokio::test]
async fn a_sheet_is_entered_once_and_entering_it_again_changes_nothing() {
    let scene = Scene::new(SHEET_ENTER, |_| {}).await;
    give(&scene, three_rows());

    // The preview reads the whole sheet, runs the leading read with its
    // `expect`, and looks every row's record up: three creates.
    let outcome = scene.work(JobKind::Preview, SHEET_ENTER, order()).await;
    assert!(waits(&outcome), "{outcome:?}");
    assert_eq!(scene.record_states().await, [RecordState::Planned; 3]);
    assert!(scene.app.writes().await.unwrap().is_empty());
    // Policy was asked about a run of three records.
    let asked = scene.control.questions.lock().unwrap().last().cloned();
    assert_eq!(asked.unwrap().records, Some(3));
    let diff = scene.run_row().await.diff_hash.unwrap();
    scene.control.approve(diff).await;

    let outcome = scene.work(JobKind::Execute, SHEET_ENTER, order()).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.record_states().await, [RecordState::Verified; 3]);
    // The test application holds every row, typed by its field type: text
    // trimmed, lengths as exact numbers.
    let stored = scene.app.stored_order("A-102").await.unwrap().unwrap();
    let held: Vec<(&str, &str, u32, u32)> = stored
        .items
        .values()
        .map(|item| {
            (
                item.item_code.as_str(),
                item.description.as_str(),
                item.quantity,
                item.length_sixteenths,
            )
        })
        .collect();
    assert_eq!(
        held,
        [
            ("L-1", "Top rail", 1, 960),
            ("L-2", "Bottom rail", 1, 968),
            ("L-3", "Stile", 2, 1348),
        ]
    );
    assert_eq!(scene.app.writes().await.unwrap().len(), 3);
    // The step after the writes ran last, and its result is the tool's.
    assert_eq!(scene.last_result().get("lines"), Some(&Value::from(3_i64)));

    // The same sheet again, as a new run: every record is found holding
    // what is intended, nothing is written, and nobody is asked to approve.
    scene.another_run(SHEET_ENTER).await;
    let writes_before = scene.hooks.count("put_item");
    let outcome = scene.work(JobKind::Execute, SHEET_ENTER, order()).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.record_states().await, [RecordState::Unchanged; 3]);
    assert_eq!(scene.hooks.count("put_item"), writes_before);
    assert_eq!(scene.app.writes().await.unwrap().len(), 3);
    assert_eq!(scene.run_row().await.state, RunState::Completed);
}

#[tokio::test]
async fn a_row_the_source_rejects_ends_the_run_before_the_system_is_touched() {
    let scene = Scene::new(SHEET_ENTER, |_| {}).await;
    give(
        &scene,
        sheet(&[
            ("L-1", "Top rail", "1", "60"),
            ("L-2", "Bottom rail", "many", "60.5"),
            ("L-3", "Stile", "2", "long"),
        ]),
    );
    let outcome = scene.work(JobKind::Execute, SHEET_ENTER, order()).await;
    // The source is read and validated in full first: the run is rejected
    // with nothing done, and no row is entered, the good one included.
    let JobOutcome::Finished {
        state: RunState::Rejected,
        failure: Some(failure),
    } = &outcome
    else {
        panic!("{outcome:?}");
    };
    assert_eq!(failure.class(), FailureClass::Input);
    assert!(scene.steps(&DRIVER_STEPS).is_empty());
    assert!(scene.records().await.is_empty());
    assert!(scene.app.writes().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_sheet_with_more_rows_than_the_pipeline_allows_is_refused_whole() {
    let scene = Scene::new(SHEET_ENTER, |compiled_target| {
        compiled_target.pipelines[1].limits.max_records = Some(2);
    })
    .await;
    give(&scene, three_rows());
    let outcome = scene.work(JobKind::Execute, SHEET_ENTER, order()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Policy, "record_limit_exceeded".to_owned())
    );
    assert!(scene.steps(&DRIVER_STEPS).is_empty());
    assert!(scene.app.writes().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_run_past_its_time_limit_stops_before_the_next_record_and_resumes_there() {
    let scene = Scene::new(SHEET_ENTER, |compiled_target| {
        compiled_target.pipelines[1].limits.max_duration_ms = Some(15_000);
    })
    .await;
    give(&scene, three_rows());
    approved(&scene, SHEET_ENTER, &order()).await;
    // Every save takes ten seconds on the engine's clock.
    let clock = Arc::clone(&scene.clock);
    scene.hooks.on_step(move |kind| {
        if kind == "put_item" {
            clock.advance(Duration::from_secs(10));
        }
    });
    let outcome = scene.work(JobKind::Execute, SHEET_ENTER, order()).await;
    // Two records fit in fifteen seconds. The third is not started: what
    // was verified stays verified, and the run can be resumed.
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Busy, "run_time_exceeded".to_owned())
    );
    assert_eq!(
        scene.record_states().await,
        [
            RecordState::Verified,
            RecordState::Verified,
            RecordState::Planned
        ]
    );
    assert_eq!(scene.app.writes().await.unwrap().len(), 2);
    assert_eq!(scene.run_row().await.state, RunState::Stopped);

    scene.new_lease().await;
    let outcome = scene.work(JobKind::Execute, SHEET_ENTER, order()).await;
    assert!(completed(&outcome), "{outcome:?}");
    for code in ["L-1", "L-2", "L-3"] {
        assert_eq!(scene.app.write_count("A-102", code).await.unwrap(), 1);
    }
}

#[tokio::test]
async fn a_row_the_system_refuses_stops_the_run_at_that_record() {
    let scene = Scene::new(SHEET_ENTER, |_| {}).await;
    // The second row's length is not a whole number of sixteenths: the
    // sheet is valid, and the application refuses the line item.
    give(
        &scene,
        sheet(&[
            ("L-1", "Top rail", "1", "60"),
            ("L-2", "Bottom rail", "1", "60.03"),
            ("L-3", "Stile", "2", "84.25"),
        ]),
    );
    approved(&scene, SHEET_ENTER, &order()).await;
    let outcome = scene.work(JobKind::Execute, SHEET_ENTER, order()).await;
    assert_eq!(failed_as(&outcome).0, FailureClass::Rejected);
    assert_eq!(
        scene.record_states().await,
        [
            RecordState::Verified,
            RecordState::Failed,
            RecordState::Planned
        ]
    );
    let stored = scene.app.stored_order("A-102").await.unwrap().unwrap();
    assert_eq!(stored.items.keys().collect::<Vec<_>>(), ["L-1"]);
}

#[tokio::test]
async fn a_pipeline_that_reads_a_source_is_refused_when_the_job_carries_none() {
    let scene = Scene::new(SHEET_ENTER, |_| {}).await;
    let outcome = scene.work(JobKind::Execute, SHEET_ENTER, order()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Input, "input_invalid".to_owned())
    );
    assert!(scene.steps(&DRIVER_STEPS).is_empty());
}
