//! Build-order step 8, first half: each identity strategy of spec section
//! 209 as the engine carries it out, and a header with its lines.
//!
//! The test application takes a line item's code from its caller. Where a
//! test needs a system that assigns the key itself, the test pack's driver
//! stands in for that part: it picks the next free code, as such a system
//! would. Every test asks the test application what it stored.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::sync::atomic::Ordering;

use a2m_core::{FailureClass, RecordState, Value, Values};
use a2m_engine::{Directive, JobKind, JobOutcome};
use common::{
    KIT_ENTER, NOTE_ADD, REF_ADD, SLOT_ADD, Scene, approved, at, completed, failed_as, sheet, slot,
    waits,
};

fn note(order: &str, description: &str) -> Values {
    slot(order, 2, "10").with("description", description)
}

/// The line items the test application holds for an order, by code.
async fn stored(scene: &Scene, order: &str) -> Vec<(String, String)> {
    scene
        .app
        .stored_order(order)
        .await
        .unwrap()
        .expect("the order exists")
        .items
        .values()
        .map(|item| (item.item_code.clone(), item.description.clone()))
        .collect()
}

fn text(values: &Values, name: &str) -> String {
    match values.get(name) {
        Some(Value::Text(text)) => text.clone(),
        other => panic!("`{name}` is {other:?}"),
    }
}

// ---- `find`: a lookup before a create ----

#[tokio::test]
async fn a_record_found_by_a_lookup_is_created_once_and_a_second_run_creates_nothing() {
    let scene = Scene::new(NOTE_ADD, |_| {}).await;
    let input = note("A-102", "Spare bracket");
    approved(&scene, NOTE_ADD, &input).await;
    // The preview looked the record up by the declared search and found
    // nothing: a create. Nothing was written.
    assert_eq!(scene.steps(&["find_item", "add_item"]), ["find_item"]);
    let outcome = scene.work(JobKind::Execute, NOTE_ADD, input.clone()).await;
    assert!(completed(&outcome), "{outcome:?}");

    // The system assigned the code; it came back with the write, was kept
    // with the record, and the read-back used it.
    assert_eq!(text(&scene.last_result(), "item_code"), "N-001");
    let read_back = scene.hooks.given("get_item");
    assert_eq!(text(&read_back.last().unwrap().args, "item_code"), "N-001");
    assert_eq!(
        scene.records().await[0].assigned_key.as_deref(),
        Some(r#"["A-102","N-001"]"#)
    );
    assert_eq!(
        stored(&scene, "A-102").await,
        [("N-001".to_owned(), "Spare bracket".to_owned())]
    );

    // The same call again, as a new run: the lookup finds the record, it
    // holds what is intended, and nothing is created.
    scene.another_run(NOTE_ADD).await;
    let outcome = scene.work(JobKind::Execute, NOTE_ADD, input).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.record_states().await, [RecordState::Unchanged]);
    assert_eq!(scene.hooks.count("add_item"), 1);
    assert_eq!(scene.app.writes().await.unwrap().len(), 1);
    assert_eq!(text(&scene.last_result(), "item_code"), "N-001");
}

#[tokio::test]
async fn a_lookup_that_finds_two_records_stops_the_run_and_picks_neither() {
    let scene = Scene::new(NOTE_ADD, |_| {}).await;
    for code in ["X-1", "X-2"] {
        let put = scene
            .app
            .http()
            .put(scene.app.url(&format!("/api/orders/A-102/items/{code}")))
            .json(&serde_json::json!({
                "description": "Spare bracket", "quantity": 2, "length_in": "10"
            }))
            .send()
            .await
            .unwrap();
        assert!(put.status().is_success());
    }
    let outcome = scene
        .work(JobKind::Preview, NOTE_ADD, note("A-102", "Spare bracket"))
        .await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::PossibleCommit, "ambiguous_match".to_owned())
    );
    assert_eq!(scene.hooks.count("add_item"), 0);
    assert_eq!(scene.app.writes().await.unwrap().len(), 2);
}

// ---- `stamped`: found again by the stamp ----

#[tokio::test]
async fn a_stamped_record_is_found_again_by_its_stamp_after_a_lost_answer() {
    let scene = Scene::new(REF_ADD, |_| {}).await;
    let input = slot("A-102", 4, "12");
    approved(&scene, REF_ADD, &input).await;
    // No record carries this run's stamp before the run has written one:
    // the preview classes the record as a create without a lookup.
    assert_eq!(scene.hooks.count("find_item"), 0);

    scene.hooks.lose_answer.store(true, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, REF_ADD, input.clone()).await;
    assert_eq!(failed_as(&outcome).0, FailureClass::PossibleCommit);
    let token = scene.records().await[0].idempotency_token.clone();
    // The write carried the record's stamp, and it landed.
    assert_eq!(
        scene.hooks.given("add_item")[0].stamp.as_deref(),
        Some(token.as_str())
    );
    assert_eq!(
        stored(&scene, "A-102").await,
        [("N-001".to_owned(), format!("ref:{token}"))]
    );

    // Before the intent, the freshness check looked for the stamp once,
    // and found no record carrying it.
    assert_eq!(
        scene.steps(&["find_item", "add_item"]),
        ["find_item", "add_item"]
    );

    // Reconcile looks the record up by its stamp, finds the write landed,
    // and writes nothing.
    scene.new_lease().await;
    let outcome = scene.work(JobKind::Reconcile, REF_ADD, input.clone()).await;
    assert_eq!(outcome, JobOutcome::Settled { remaining: 0 });
    assert_eq!(scene.record_states().await, [RecordState::Verified]);
    let lookups = scene.hooks.given("find_item");
    assert_eq!(lookups.len(), 2);
    assert_eq!(lookups[1].stamp.as_deref(), Some(token.as_str()));

    let outcome = scene.work(JobKind::Execute, REF_ADD, input).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.hooks.count("add_item"), 1);
    assert_eq!(stored(&scene, "A-102").await.len(), 1);
    assert_eq!(text(&scene.last_result(), "item_code"), "N-001");
}

#[tokio::test]
async fn a_stamped_record_whose_write_never_left_is_written_once_on_resume() {
    let scene = Scene::new(REF_ADD, |_| {}).await;
    let input = slot("A-102", 4, "12");
    approved(&scene, REF_ADD, &input).await;
    scene
        .control
        .faults
        .intend_answer_lost
        .store(true, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, REF_ADD, input.clone()).await;
    assert!(outcome.failure().is_some(), "{outcome:?}");
    assert_eq!(scene.record_states().await, [RecordState::Intent]);
    assert_eq!(scene.hooks.count("add_item"), 0);

    // The lookup by the stamp finds nothing: the record goes back to
    // `planned` and is written, once.
    scene.new_lease().await;
    let outcome = scene.work(JobKind::Execute, REF_ADD, input).await;
    assert!(completed(&outcome), "{outcome:?}");
    let log = scene.steps(&["find_item", "add_item"]);
    assert_eq!(log, ["find_item", "find_item", "add_item"]);
    assert_eq!(stored(&scene, "A-102").await.len(), 1);
    assert_eq!(scene.record_states().await, [RecordState::Verified]);
}

#[tokio::test]
async fn a_lookup_that_lags_behind_the_write_is_asked_again_after_its_settle_time() {
    let scene = Scene::new(REF_ADD, |plan| {
        plan.operations[6].bindings[0].settle_ms = 5;
    })
    .await;
    let input = slot("A-102", 4, "12");
    approved(&scene, REF_ADD, &input).await;
    scene.hooks.lose_answer.store(true, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, REF_ADD, input.clone()).await;
    assert_eq!(failed_as(&outcome).0, FailureClass::PossibleCommit);

    // The first lookup after the write finds nothing yet. Inside the
    // settle time that means "not known": the engine waits it out and
    // looks again, and does not write a second record.
    let before = scene.hooks.count("find_item");
    scene.hooks.lookup_lags.store(1, Ordering::SeqCst);
    scene.new_lease().await;
    let outcome = scene.work(JobKind::Execute, REF_ADD, input).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.hooks.count("find_item"), before + 2);
    assert_eq!(scene.hooks.count("add_item"), 1);
    assert_eq!(stored(&scene, "A-102").await.len(), 1);
}

// ---- a key the system assigns before the commit point ----

#[tokio::test]
async fn a_key_assigned_before_the_commit_point_is_recorded_first_and_read_back_by() {
    let scene = Scene::new(SLOT_ADD, |_| {}).await;
    let input = slot("A-102", 4, "12");
    approved(&scene, SLOT_ADD, &input).await;
    let outcome = scene.work(JobKind::Execute, SLOT_ADD, input).await;
    assert!(completed(&outcome), "{outcome:?}");

    // What the step before the save learned was stored, and acknowledged,
    // before the save was sent.
    let log = scene.hooks.seen();
    assert!(at(&log, "intend") < at(&log, "reserve_code"), "{log:?}");
    assert!(
        at(&log, "reserve_code") < at(&log, "before_commit"),
        "{log:?}"
    );
    assert!(at(&log, "before_commit") < at(&log, "add_item"), "{log:?}");
    // The save and the read-back used the assigned code. The one lookup
    // by the stamp is the freshness check, before the intent.
    assert_eq!(
        text(&scene.hooks.given("add_item")[0].args, "code"),
        "N-001"
    );
    let read_back = scene.hooks.given("get_item");
    assert_eq!(text(&read_back.last().unwrap().args, "item_code"), "N-001");
    assert_eq!(scene.hooks.count("find_item"), 1);
    assert!(at(&log, "find_item") < at(&log, "intend"), "{log:?}");
    let stored = scene
        .app
        .stored_item("A-102", "N-001")
        .await
        .unwrap()
        .expect("the item was stored");
    assert_eq!(stored.quantity, 4);
    assert_eq!(scene.app.write_count("A-102", "N-001").await.unwrap(), 1);
}

#[tokio::test]
async fn a_lost_answer_is_reconciled_by_what_was_recorded_before_the_commit_point() {
    let scene = Scene::new(SLOT_ADD, |_| {}).await;
    let input = slot("A-102", 4, "12");
    approved(&scene, SLOT_ADD, &input).await;
    scene.hooks.lose_answer.store(true, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, SLOT_ADD, input.clone()).await;
    assert_eq!(failed_as(&outcome).0, FailureClass::PossibleCommit);
    let reads_before = scene.hooks.count("get_item");
    let lookups_before = scene.hooks.count("find_item");

    scene.new_lease().await;
    let outcome = scene
        .work(JobKind::Reconcile, SLOT_ADD, input.clone())
        .await;
    assert_eq!(outcome, JobOutcome::Settled { remaining: 0 });
    assert_eq!(scene.record_states().await, [RecordState::Verified]);
    // The record was read by the code recorded before the save, not looked
    // up by its stamp.
    assert_eq!(scene.hooks.count("find_item"), lookups_before);
    let reads = scene.hooks.given("get_item");
    assert_eq!(reads.len(), reads_before + 1);
    assert_eq!(text(&reads.last().unwrap().args, "item_code"), "N-001");

    let outcome = scene.work(JobKind::Execute, SLOT_ADD, input).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.hooks.count("add_item"), 1);
    assert_eq!(scene.hooks.count("reserve_code"), 1);
    assert_eq!(scene.app.writes().await.unwrap().len(), 1);
}

// ---- a header with its lines ----

fn kit() -> Values {
    Values::new()
        .with("order_id", "A-102")
        .with("title", "Table kit")
}

fn legs() -> Vec<u8> {
    sheet(&[("A", "Left leg", "2", "30"), ("B", "Right leg", "2", "30")])
}

fn give(scene: &Scene, bytes: Vec<u8>) {
    *scene.source.lock().unwrap() = Some(std::sync::Arc::new(bytes));
}

#[tokio::test]
async fn a_header_is_created_once_and_its_lines_wait_for_its_assigned_key() {
    let scene = Scene::new(KIT_ENTER, |_| {}).await;
    give(&scene, legs());

    // The preview shows the whole structure: the header and both lines,
    // each a create. The lines cannot exist before their header does, so
    // none of them is looked up, and each is planned under its place in
    // the run: no code that has not been assigned is in the diff.
    let outcome = scene.work(JobKind::Preview, KIT_ENTER, kit()).await;
    assert!(waits(&outcome), "{outcome:?}");
    assert_eq!(scene.record_states().await, [RecordState::Planned; 3]);
    assert_eq!(scene.steps(&["find_item", "get_item"]), ["find_item"]);
    assert!(scene.app.writes().await.unwrap().is_empty());
    let diff = scene.run_row().await.diff_hash.unwrap();
    scene.control.approve(diff).await;

    // The runner is told to pause while the header is being saved: the
    // save is finished and recorded, and no line is started.
    let lease = scene.lease.clone();
    scene.hooks.on_step(move |kind| {
        if kind == "add_item" {
            lease.direct(Directive::Pause);
        }
    });
    let outcome = scene.work(JobKind::Execute, KIT_ENTER, kit()).await;
    assert_eq!(outcome, JobOutcome::LeaseEnding);
    assert_eq!(
        scene.record_states().await,
        [
            RecordState::Verified,
            RecordState::Planned,
            RecordState::Planned
        ]
    );
    assert_eq!(
        stored(&scene, "A-102").await,
        [("N-001".to_owned(), "Table kit".to_owned())]
    );

    // Resumed: the lines take the header's code from the ledger. The
    // header is not created again.
    scene.lease.direct(Directive::Continue);
    scene.new_lease().await;
    let outcome = scene.work(JobKind::Execute, KIT_ENTER, kit()).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.hooks.count("add_item"), 1);
    assert_eq!(scene.record_states().await, [RecordState::Verified; 3]);
    assert_eq!(
        stored(&scene, "A-102").await,
        [
            ("N-001".to_owned(), "Table kit".to_owned()),
            ("N-001-A".to_owned(), "Left leg".to_owned()),
            ("N-001-B".to_owned(), "Right leg".to_owned()),
        ]
    );
    for code in ["N-001", "N-001-A", "N-001-B"] {
        assert_eq!(scene.app.write_count("A-102", code).await.unwrap(), 1);
    }

    // The same kit again, as a new run: the header is found, its code is
    // read from the system, every line is found under it, and nothing is
    // written or asked to be approved.
    scene.another_run(KIT_ENTER).await;
    let outcome = scene.work(JobKind::Execute, KIT_ENTER, kit()).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.record_states().await, [RecordState::Unchanged; 3]);
    assert_eq!(scene.app.writes().await.unwrap().len(), 3);
}

#[tokio::test]
async fn lines_are_marked_blocked_when_their_header_fails_and_written_once_it_is_settled() {
    let scene = Scene::new(KIT_ENTER, |_| {}).await;
    give(&scene, legs());
    approved(&scene, KIT_ENTER, &kit()).await;
    scene.hooks.lose_answer.store(true, Ordering::SeqCst);
    let outcome = scene.work(JobKind::Execute, KIT_ENTER, kit()).await;
    assert_eq!(failed_as(&outcome).0, FailureClass::PossibleCommit);
    let records = scene.records().await;
    assert_eq!(
        records
            .iter()
            .map(|record| record
                .failure
                .as_ref()
                .map(|(_, code)| code.to_string())
                .unwrap_or_default())
            .collect::<Vec<_>>(),
        ["no_confirmation", "blocked_by_parent", "blocked_by_parent"]
    );
    assert_eq!(scene.hooks.count("put_item"), 0);

    // Resumed: the header is found by its lookup, so it is not created
    // again; the lines go back to `planned` and are written under it.
    scene.new_lease().await;
    let outcome = scene.work(JobKind::Execute, KIT_ENTER, kit()).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.hooks.count("add_item"), 1);
    assert_eq!(
        stored(&scene, "A-102")
            .await
            .iter()
            .map(|(code, _)| code.as_str())
            .collect::<Vec<_>>(),
        ["N-001", "N-001-A", "N-001-B"]
    );
    assert_eq!(scene.app.writes().await.unwrap().len(), 3);
}

#[tokio::test]
async fn a_resumed_run_reads_an_unchanged_header_again_for_the_lines_under_it() {
    let scene = Scene::new(KIT_ENTER, |_| {}).await;
    give(&scene, sheet(&[("A", "Left leg", "2", "30")]));
    approved(&scene, KIT_ENTER, &kit()).await;
    let outcome = scene.work(JobKind::Execute, KIT_ENTER, kit()).await;
    assert!(completed(&outcome), "{outcome:?}");

    // A second run adds a line under the header that exists. It is paused
    // before the line is written, and resumed.
    scene.another_run(KIT_ENTER).await;
    give(&scene, legs());
    approved(&scene, KIT_ENTER, &kit()).await;
    assert_eq!(
        scene.record_states().await,
        [
            RecordState::Unchanged,
            RecordState::Unchanged,
            RecordState::Planned
        ]
    );
    scene.lease.direct(Directive::Pause);
    let outcome = scene.work(JobKind::Execute, KIT_ENTER, kit()).await;
    assert_eq!(outcome, JobOutcome::LeaseEnding);
    scene.lease.direct(Directive::Continue);
    scene.new_lease().await;
    let outcome = scene.work(JobKind::Execute, KIT_ENTER, kit()).await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(scene.hooks.count("add_item"), 1);
    assert_eq!(scene.app.write_count("A-102", "N-001-B").await.unwrap(), 1);
    assert_eq!(scene.app.writes().await.unwrap().len(), 3);
}

#[tokio::test]
async fn a_value_that_is_written_may_not_wait_for_a_record_that_is_not_written_yet() {
    // The line's description, a value a person approves, is made to come
    // from the header's result: it cannot be shown before the header
    // exists, so the run is refused at the preview.
    let scene = Scene::new(KIT_ENTER, |plan| {
        let from_header = plan.pipelines[2].steps[1].with[1].value;
        plan.pipelines[2].steps[1].with[2].value = from_header;
    })
    .await;
    give(&scene, legs());
    let outcome = scene.work(JobKind::Preview, KIT_ENTER, kit()).await;
    assert_eq!(
        failed_as(&outcome),
        (FailureClass::Input, "input_invalid".to_owned())
    );
    assert!(scene.app.writes().await.unwrap().is_empty());
    assert_eq!(scene.hooks.count("add_item"), 0);
}
