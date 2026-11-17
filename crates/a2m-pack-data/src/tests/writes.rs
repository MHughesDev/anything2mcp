//! Writes against the stand. Each is proven by what the stand stored and by
//! how many times the row was written, never by the pack's own read alone.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use a2m_core::{Decimal, FailureClass, NoSecrets, Value, Values};
use a2m_expr::Normalizer;
use a2m_provider::{Posture, ReadOnlyPort, SandboxPort, Sent, StepOutcome, codes as contract};
use example_db::faults::Fault;
use example_db::schema::Table;

use super::{Scene, failed, inputs, values};
use crate::model::{Access, Isolation};
use crate::names::codes;
use crate::store::{
    Bound, Catalog, CommitFailed, Conn, Interrupter, Opener, Ran, Refusal, RunLimits, Shape,
};

const LINE: [&str; 2] = ["A-100", "L-101"];

fn decimal(text: &str) -> Value {
    Value::Decimal(Decimal::parse(text).unwrap())
}

/// The inputs of `example_line_item_update` for the seeded line.
fn update(length: &str, price: &str, version: i64) -> Values {
    inputs(&[("order_id", LINE[0]), ("item_code", LINE[1])])
        .with("length", decimal(length))
        .with("unit_price", decimal(price))
        .with("row_version", version)
}

/// Whether two raw values are the same under a field type's own rule: the
/// comparison the engine makes of the intended value and the read-back.
fn agree(scene: &Scene, field_type: &str, intended: &Value, read_back: &Value) -> bool {
    let field_type = scene
        .plan
        .field_types
        .iter()
        .find(|known| known.name == field_type)
        .unwrap();
    let normalizer = Normalizer::load(
        scene
            .plan
            .expression(field_type.normalizer.unwrap())
            .unwrap(),
    )
    .unwrap();
    normalizer
        .normalize(intended)
        .unwrap()
        .agrees_with(&normalizer.normalize(read_back).unwrap())
}

#[tokio::test]
async fn an_update_is_written_once_and_reads_back() {
    let scene = Scene::new();
    let binding = scene.binding("example_line_item_update");
    let (mut run, write) = scene.live(&LINE).await;
    let wanted = update("151.25", "13.10", 1);

    // The committing step without the ledger's token never reaches the
    // driver, and nothing is committed.
    for step in &binding.act[..3] {
        run.step(step, &wanted).await.unwrap();
    }
    let refused = run.step(&binding.act[3], &wanted).await;
    assert_eq!(
        failed(refused),
        (
            FailureClass::Policy,
            contract::INTENT_MISSING,
            Sent::NotSent
        )
    );
    assert_eq!(scene.db.write_count(Table::OrderLines, &LINE).unwrap(), 0);

    // With it, the open transaction commits: exactly one write.
    run.step_with(&binding.act[3], &Values::new(), Some(&write))
        .await
        .unwrap();
    assert_eq!(scene.db.write_count(Table::OrderLines, &LINE).unwrap(), 1);
    let stored = scene
        .db
        .stored_row(Table::OrderLines, &LINE)
        .unwrap()
        .unwrap();
    assert_eq!(stored.text("length_in"), Some("151.25"));
    // A decimal is bound as its exact text, which carries no trailing zero.
    assert_eq!(stored.text("unit_price"), Some("13.1"));
    assert_eq!(stored.integer("row_version"), Some(2));

    // Read-back, in a new read-only transaction after the commit.
    let read = values(run.steps(&binding.read_back, &wanted, None).await.unwrap());
    assert!(agree(
        &scene,
        "length_inches",
        &decimal("151.25"),
        read.get("length").unwrap()
    ));
    assert!(agree(
        &scene,
        "price",
        &decimal("13.10"),
        read.get("unit_price").unwrap()
    ));
    assert_eq!(read.get("row_version"), Some(&Value::Int(2)));
    run.release().await;
    assert_eq!(scene.db.write_count(Table::OrderLines, &LINE).unwrap(), 1);
}

#[tokio::test]
async fn a_value_the_store_reformats_verifies_and_a_different_value_does_not() {
    let scene = Scene::new();
    let binding = scene.binding("example_line_item_update");
    scene.db.set_fault(Fault::LengthRescaled).unwrap();
    scene.db.set_fault(Fault::PriceRoundedDown).unwrap();
    let (mut run, write) = scene.live(&LINE).await;
    let wanted = update("151.25", "12.49", 1);
    run.steps(&binding.act, &wanted, Some(&write))
        .await
        .unwrap();
    let read = values(run.steps(&binding.read_back, &wanted, None).await.unwrap());
    run.release().await;

    // The stand stored the length at another scale and dropped the cents.
    let stored = scene
        .db
        .stored_row(Table::OrderLines, &LINE)
        .unwrap()
        .unwrap();
    assert_eq!(stored.text("length_in"), Some("151.2500"));
    assert_eq!(stored.text("unit_price"), Some("12.00"));
    // The driver returns what is stored, as it is stored.
    assert_eq!(read.get("length"), Some(&Value::from("151.2500")));
    // The same value at another scale agrees; a different value does not.
    assert!(agree(
        &scene,
        "length_inches",
        &decimal("151.25"),
        read.get("length").unwrap()
    ));
    assert!(!agree(
        &scene,
        "price",
        &decimal("12.49"),
        read.get("unit_price").unwrap()
    ));
}

#[tokio::test]
async fn padded_text_verifies_through_the_field_types_rule() {
    let scene = Scene::new();
    let binding = scene.binding("example_line_item_describe");
    scene.db.set_fault(Fault::DescriptionPadded).unwrap();
    let (mut run, write) = scene.live(&LINE).await;
    let wanted = inputs(&[
        ("order_id", LINE[0]),
        ("item_code", LINE[1]),
        ("description", "Left rail, long"),
    ]);
    run.steps(&binding.act, &wanted, Some(&write))
        .await
        .unwrap();
    let read = values(run.steps(&binding.read_back, &wanted, None).await.unwrap());
    run.release().await;
    let stored = scene
        .db
        .stored_row(Table::OrderLines, &LINE)
        .unwrap()
        .unwrap();
    assert_eq!(stored.text("description").unwrap().len(), 24);
    assert_eq!(scene.db.write_count(Table::OrderLines, &LINE).unwrap(), 1);
    assert_ne!(
        read.get("description"),
        Some(&Value::from("Left rail, long"))
    );
    assert!(agree(
        &scene,
        "note",
        &Value::from("Left rail, long"),
        read.get("description").unwrap()
    ));
    assert!(!agree(
        &scene,
        "note",
        &Value::from("Left rail"),
        read.get("description").unwrap()
    ));
}

#[tokio::test]
async fn a_row_changed_since_the_preview_is_stale_and_nothing_is_written() {
    let scene = Scene::new();
    let (mut run, write) = scene.live(&LINE).await;

    // Someone else changes the row after the preview read version 1.
    scene
        .db
        .someone_else(
            "UPDATE order_lines SET unit_price = '99.00' WHERE order_id = 'A-100' AND item_code = 'L-101'",
            &[],
        )
        .unwrap();
    let guarded = run
        .steps(
            &scene.binding("example_line_item_update").act,
            &update("151.25", "13.10", 1),
            Some(&write),
        )
        .await;
    assert_eq!(
        failed(guarded),
        (FailureClass::Stale, codes::VERSION_CONFLICT, Sent::Answered)
    );
    let stored = scene
        .db
        .stored_row(Table::OrderLines, &LINE)
        .unwrap()
        .unwrap();
    assert_eq!(stored.text("unit_price"), Some("99.00"));
    assert_eq!(stored.text("length_in"), Some("150.5"));
    assert_eq!(scene.db.write_count(Table::OrderLines, &LINE).unwrap(), 0);

    // Without a guard, a record that is gone is the other stale code.
    scene
        .db
        .someone_else(
            "DELETE FROM order_lines WHERE order_id = 'A-100' AND item_code = 'L-101'",
            &[],
        )
        .unwrap();
    let gone = run
        .steps(
            &scene.binding("example_line_item_describe").act,
            &inputs(&[
                ("order_id", LINE[0]),
                ("item_code", LINE[1]),
                ("description", "x"),
            ]),
            Some(&write),
        )
        .await;
    assert_eq!(
        failed(gone),
        (
            FailureClass::Stale,
            codes::CHANGED_SINCE_PREVIEW,
            Sent::Answered
        )
    );
    run.release().await;
    assert_eq!(scene.db.write_count(Table::OrderLines, &LINE).unwrap(), 0);
}

#[tokio::test]
async fn a_constraint_refuses_a_write_and_nothing_lands() {
    let scene = Scene::new();
    let binding = scene.binding("example_line_item_create");
    let line = |item: &str, quantity: i64| {
        inputs(&[
            ("order_id", "A-102"),
            ("item_code", item),
            ("description", "Shelf"),
        ])
        .with("quantity", quantity)
        .with("length", decimal("12"))
        .with("unit_price", decimal("3.00"))
    };
    let rejected = (
        FailureClass::Rejected,
        codes::CONSTRAINT_VIOLATED,
        Sent::Answered,
    );
    let (mut run, write) = scene.live(&["A-102", "L-301"]).await;

    // A check constraint, a foreign key, and a rule of the store.
    assert_eq!(
        failed(
            run.steps(&binding.act, &line("L-301", 0), Some(&write))
                .await
        ),
        rejected
    );
    let orphan = line("L-301", 1).with("order_id", "A-999");
    assert_eq!(
        failed(run.steps(&binding.act, &orphan, Some(&write)).await),
        rejected
    );
    scene.db.set_fault(Fault::LineWritesRefused).unwrap();
    assert_eq!(
        failed(
            run.steps(&binding.act, &line("L-301", 1), Some(&write))
                .await
        ),
        rejected
    );
    scene.db.clear_fault(Fault::LineWritesRefused).unwrap();
    assert!(scene.db.writes().unwrap().is_empty());
    assert!(
        scene
            .db
            .stored_row(Table::OrderLines, &["A-102", "L-301"])
            .unwrap()
            .is_none()
    );

    // The same insert, allowed: one row, written once, and it reads back.
    run.steps(&binding.act, &line("L-301", 1), Some(&write))
        .await
        .unwrap();
    let stored = scene
        .db
        .stored_row(Table::OrderLines, &["A-102", "L-301"])
        .unwrap()
        .unwrap();
    assert_eq!(stored.text("description"), Some("Shelf"));
    assert_eq!(stored.integer("quantity"), Some(1));
    assert_eq!(
        scene
            .db
            .write_count(Table::OrderLines, &["A-102", "L-301"])
            .unwrap(),
        1
    );
    assert!(
        run.steps(&binding.read_back, &line("L-301", 1), None)
            .await
            .is_ok()
    );

    // A second insert under the same key is refused by the key itself.
    assert_eq!(
        failed(
            run.steps(&binding.act, &line("L-301", 1), Some(&write))
                .await
        ),
        rejected
    );
    run.release().await;
    assert_eq!(
        scene
            .db
            .write_count(Table::OrderLines, &["A-102", "L-301"])
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn a_delete_removes_one_row_and_its_read_back_finds_it_absent() {
    let scene = Scene::new();
    let binding = scene.binding("example_line_item_delete");
    let key = inputs(&[("order_id", LINE[0]), ("item_code", LINE[1])]);
    let (mut run, write) = scene.live(&LINE).await;
    run.steps(&binding.act, &key, Some(&write)).await.unwrap();
    let read = run.steps(&binding.read_back, &key, None).await.unwrap();
    assert!(matches!(read, StepOutcome::Absent(_)));
    run.release().await;
    assert!(
        scene
            .db
            .stored_row(Table::OrderLines, &LINE)
            .unwrap()
            .is_none()
    );
    assert_eq!(scene.db.write_count(Table::OrderLines, &LINE).unwrap(), 1);
    // The rows the write did not name are as they were.
    assert_eq!(scene.db.stored_rows(Table::OrderLines).unwrap().len(), 3);
    assert_eq!(scene.db.writes().unwrap().len(), 1);
}

#[tokio::test]
async fn a_write_that_matches_more_rows_than_one_is_rolled_back() {
    // The record declares a key that three rows of the stand share.
    let scene = Scene::new();
    let binding = scene.binding("example_order_lines_describe");
    let wanted = inputs(&[("order_id", "A-100"), ("description", "All the same")]);
    let (mut run, write) = scene.live(&["A-100"]).await;
    let several = run.steps(&binding.act, &wanted, Some(&write)).await;
    assert_eq!(
        failed(several),
        (FailureClass::Drift, codes::KEY_NOT_UNIQUE, Sent::Answered)
    );
    // Its read-back is a failure too, never a pick among the rows.
    let read = run.steps(&binding.read_back, &wanted, None).await;
    assert_eq!(
        failed(read),
        (FailureClass::Drift, codes::KEY_NOT_UNIQUE, Sent::Answered)
    );
    run.release().await;
    // The stand: no row was written, and each holds what it held.
    assert!(scene.db.writes().unwrap().is_empty());
    for row in scene.db.stored_rows(Table::OrderLines).unwrap() {
        assert_ne!(row.text("description"), Some("All the same"));
    }
}

#[tokio::test]
async fn nothing_is_committed_but_one_statement_whose_one_row_was_counted() {
    let scene = Scene::new();
    let binding = scene.binding("example_line_item_update");
    let wanted = update("151.25", "13.10", 1);
    let (mut run, write) = scene.live(&LINE).await;
    let out_of_order = (
        FailureClass::UnknownVariant,
        codes::UNEXPECTED_STATUS,
        Sent::NotSent,
    );

    // A commit with the ledger's token and no count checked.
    run.step(&binding.act[0], &wanted).await.unwrap();
    run.step(&binding.act[1], &wanted).await.unwrap();
    let uncounted = run
        .step_with(&binding.act[3], &Values::new(), Some(&write))
        .await;
    assert_eq!(failed(uncounted), out_of_order);
    assert_eq!(scene.db.write_count(Table::OrderLines, &LINE).unwrap(), 0);

    // A second statement in a transaction that takes one: the count checked
    // afterwards would be the count of the second alone.
    run.step(&binding.act[0], &wanted).await.unwrap();
    run.step(&binding.act[1], &wanted).await.unwrap();
    let second = run.step(&binding.act[1], &wanted).await;
    assert_eq!(failed(second), out_of_order);
    assert_eq!(scene.db.write_count(Table::OrderLines, &LINE).unwrap(), 0);

    // A plan that asks for a count other than one row.
    let mut many = binding.act[2].clone();
    many.payload = a2m_core::PackPayload::encode(&crate::model::StepDetail {
        rows: Some(0),
        ..crate::model::StepDetail::default()
    })
    .unwrap();
    run.step(&binding.act[0], &wanted).await.unwrap();
    run.step(&binding.act[1], &wanted.clone().with("row_version", 99_i64))
        .await
        .unwrap();
    assert_eq!(failed(run.step(&many, &wanted).await), out_of_order);
    let unchecked = run
        .step_with(&binding.act[3], &Values::new(), Some(&write))
        .await;
    assert_eq!(failed(unchecked), out_of_order);

    // Each refusal rolled back: the stand holds the seed, and the same
    // prepared driver then writes the row once.
    assert_eq!(scene.db.write_count(Table::OrderLines, &LINE).unwrap(), 0);
    run.steps(&binding.act, &wanted, Some(&write))
        .await
        .unwrap();
    run.release().await;
    assert_eq!(scene.db.write_count(Table::OrderLines, &LINE).unwrap(), 1);
}

#[tokio::test]
async fn a_key_the_store_assigns_is_known_before_the_commit() {
    let scene = Scene::new();
    let binding = scene.binding("example_shipment_create");
    let wanted = inputs(&[
        ("order_id", "A-100"),
        ("carrier", "Example Carrier"),
        ("external_ref", "SHIP-0002"),
    ]);
    let (mut run, write) = scene.live(&["SHIP-0002"]).await;
    for step in &binding.act[..3] {
        run.step(step, &wanted).await.unwrap();
    }
    // `fetch`, before the commit point: the key, for the result and for
    // reconcile.
    let StepOutcome::Done(fetched) = run.step(&binding.act[3], &wanted).await.unwrap() else {
        panic!("the insert returned its key");
    };
    assert_eq!(fetched.values.get("shipment_id"), Some(&Value::Int(2)));
    assert_eq!(
        fetched.for_reconcile.get("shipment_id"),
        Some(&Value::Int(2))
    );
    assert_eq!(fetched.assigned_key.unwrap().parts(), [Value::Int(2)]);
    // Nothing is committed yet.
    assert!(
        scene
            .db
            .stored_row(Table::Shipments, &["2"])
            .unwrap()
            .is_none()
    );

    run.step_with(&binding.act[4], &Values::new(), Some(&write))
        .await
        .unwrap();
    let stored = scene
        .db
        .stored_row(Table::Shipments, &["2"])
        .unwrap()
        .unwrap();
    assert_eq!(stored.text("external_ref"), Some("SHIP-0002"));
    assert_eq!(scene.db.write_count(Table::Shipments, &["2"]).unwrap(), 1);

    // Found again by the captured key.
    let read = values(
        run.steps(
            &scene.binding("example_shipment_get").act,
            &Values::new().with("shipment_id", 2_i64),
            None,
        )
        .await
        .unwrap(),
    );
    assert_eq!(read.get("external_ref"), Some(&Value::from("SHIP-0002")));
    run.release().await;
}

#[tokio::test]
async fn a_trigger_the_target_did_not_declare_stops_a_write_as_drift() {
    let scene = Scene::new();
    let binding = scene.binding("example_customer_credit_update");
    let wanted = inputs(&[("customer_id", "C-001")]).with("credit_limit_cents", 750_000_i64);

    // With the stand's own logging triggers out of the catalog, `customers`
    // has no trigger and no cascading key: the write has no hazard.
    scene.db.set_write_log(false).unwrap();
    let (mut run, write) = scene.live(&["C-001"]).await;
    run.steps(&binding.act, &wanted, Some(&write))
        .await
        .unwrap();
    let read = values(run.steps(&binding.read_back, &wanted, None).await.unwrap());
    assert_eq!(read.get("credit_limit_cents"), Some(&Value::Int(750_000)));
    run.release().await;
    let stored = scene
        .db
        .stored_row(Table::Customers, &["C-001"])
        .unwrap()
        .unwrap();
    assert_eq!(stored.integer("credit_limit_cents"), Some(750_000));

    // A trigger appears on the table after publishing. The statement
    // declares no hazard, so the write is stopped before it is sent.
    scene.db.set_write_log(true).unwrap();
    let (mut run, write) = scene.live(&["C-001"]).await;
    let stopped = run
        .steps(
            &binding.act,
            &wanted.clone().with("credit_limit_cents", 1_i64),
            Some(&write),
        )
        .await;
    assert_eq!(
        failed(stopped),
        (FailureClass::Drift, codes::SCHEMA_CHANGED, Sent::NotSent)
    );
    run.release().await;
    let stored = scene
        .db
        .stored_row(Table::Customers, &["C-001"])
        .unwrap()
        .unwrap();
    assert_eq!(stored.integer("credit_limit_cents"), Some(750_000));
    assert_eq!(
        scene.db.write_count(Table::Customers, &["C-001"]).unwrap(),
        0
    );
}

#[tokio::test]
async fn a_writer_that_cannot_take_the_lock_is_busy_and_writes_nothing() {
    let scene = Scene::new();
    let binding = scene.binding("example_line_item_update");
    let (mut run, write) = scene.live(&LINE).await;
    run.wait = Duration::from_millis(300);
    scene.db.set_fault(Fault::WritersLocked).unwrap();
    let locked = run
        .steps(&binding.act, &update("151.25", "13.10", 1), Some(&write))
        .await;
    assert_eq!(
        failed(locked),
        (FailureClass::Busy, codes::STORE_LOCKED, Sent::NotSent)
    );
    scene.db.clear_fault(Fault::WritersLocked).unwrap();
    assert_eq!(scene.db.write_count(Table::OrderLines, &LINE).unwrap(), 0);

    // The lock is gone: the same prepared driver writes, once.
    run.wait = Duration::from_secs(30);
    run.steps(&binding.act, &update("151.25", "13.10", 1), Some(&write))
        .await
        .unwrap();
    run.release().await;
    assert_eq!(scene.db.write_count(Table::OrderLines, &LINE).unwrap(), 1);
}

#[tokio::test]
async fn nothing_is_written_where_nothing_may_be() {
    let scene = Scene::new();
    let binding = scene.binding("example_line_item_update");
    let before = scene.db.data_version().unwrap();
    for posture in [
        Posture::Discovery,
        Posture::Sandbox {
            test_records: false,
        },
    ] {
        // The driver's own guard: a transaction that may write is refused,
        // whatever the handle would do with the commit.
        let mut run = scene.prepare(posture).await;
        let begun = run.step(&binding.act[0], &Values::new()).await;
        assert_eq!(
            failed(begun),
            (
                FailureClass::Policy,
                contract::READ_ONLY_POSTURE,
                Sent::NotSent
            )
        );
        // And a write statement inside a read-only transaction is refused
        // by the driver before it is sent.
        run.step(&binding.read_back[0], &Values::new())
            .await
            .unwrap();
        let written = run
            .step(&binding.act[1], &update("151.25", "13.10", 1))
            .await;
        assert_eq!(failed(written).2, Sent::NotSent);
        run.release().await;
    }
    // A committing step through the discoverer's port never reaches the
    // driver.
    let handle = scene.handle();
    let prepared = scene
        .try_prepare(
            &handle,
            Posture::Discovery,
            a2m_ledger::Fence {
                run: a2m_core::RunId::from_bytes([7; 16]),
                epoch: a2m_core::Epoch::new(1),
            },
        )
        .await
        .unwrap();
    let port = ReadOnlyPort::new(handle, prepared);
    let args = Values::new();
    let input = a2m_provider::StepInput {
        args: &args,
        record: None,
        key: None,
        stamp: None,
        deadline: a2m_provider::Deadline::after(Duration::from_secs(5)),
    };
    let refused = port.act(&binding.act[3], &input).await;
    assert_eq!(failed(refused).0, FailureClass::Policy);
    let (handle, prepared) = port.into_parts();
    handle.release(prepared, false).await;
    assert_eq!(scene.db.data_version().unwrap(), before);
    assert!(scene.db.writes().unwrap().is_empty());
}

// ---- a connection lost after the commit was sent ----

/// A connection that commits for real and then loses the store's answer:
/// what the relay of a server stand does to a network link.
struct LosesAnswer {
    inner: Box<dyn Conn>,
    armed: Arc<AtomicBool>,
}

impl Conn for LosesAnswer {
    fn access(&self) -> Access {
        self.inner.access()
    }
    fn begin(&mut self, isolation: Isolation, wait: Duration) -> Result<(), Refusal> {
        self.inner.begin(isolation, wait)
    }
    fn in_transaction(&self) -> bool {
        self.inner.in_transaction()
    }
    fn describe(&mut self, text: &str) -> Result<Shape, Refusal> {
        self.inner.describe(text)
    }
    fn run(&mut self, text: &str, params: &[Bound], limits: &RunLimits) -> Result<Ran, Refusal> {
        self.inner.run(text, params, limits)
    }
    fn commit(&mut self, wait: Duration) -> Result<(), CommitFailed> {
        let done = self.inner.commit(wait);
        if self.armed.load(Ordering::SeqCst) {
            return Err(CommitFailed::Unknown);
        }
        done
    }
    fn rollback(&mut self) {
        self.inner.rollback();
    }
    fn catalog(&mut self, tables: usize) -> Result<Catalog, Refusal> {
        self.inner.catalog(tables)
    }
    fn interrupter(&self) -> Interrupter {
        self.inner.interrupter()
    }
}

struct LossyStore {
    armed: Arc<AtomicBool>,
}

impl Opener for LossyStore {
    fn store(&self) -> &'static str {
        "sqlite"
    }
    fn quote(&self, identifier: &str) -> Option<String> {
        crate::sqlite::Sqlite.quote(identifier)
    }
    fn placeholder(&self) -> &'static str {
        crate::sqlite::Sqlite.placeholder()
    }
    fn returns_assigned_key(&self) -> bool {
        true
    }
    fn open(&self, file: &Path, access: Access) -> Result<Box<dyn Conn>, Refusal> {
        Ok(Box::new(LosesAnswer {
            inner: crate::sqlite::Sqlite.open(file, access)?,
            armed: Arc::clone(&self.armed),
        }))
    }
}

#[tokio::test]
async fn a_lost_answer_to_a_commit_is_a_possible_commit_and_never_a_second_write() {
    let armed = Arc::new(AtomicBool::new(true));
    let lossy = Arc::clone(&armed);
    let pack = crate::DataPack::with_openers(Arc::new(move |_store: &str| {
        Some(Arc::new(LossyStore {
            armed: Arc::clone(&lossy),
        }) as Arc<dyn Opener>)
    }));
    let scene = Scene::with(&pack, Arc::new(NoSecrets));
    let binding = scene.binding("example_line_item_update");
    let wanted = update("151.25", "13.10", 1);
    let (mut run, write) = scene.live(&LINE).await;

    let lost = run.steps(&binding.act, &wanted, Some(&write)).await;
    assert_eq!(
        failed(lost),
        (
            FailureClass::PossibleCommit,
            contract::NO_CONFIRMATION,
            Sent::Unknown
        )
    );
    // The write did land, once. The driver cannot know; the stand does.
    assert_eq!(scene.db.write_count(Table::OrderLines, &LINE).unwrap(), 1);

    // The connection is in doubt: the same prepared driver sends nothing
    // more, neither the write again nor a read.
    for step in binding.act.iter().chain(&binding.read_back) {
        let args = scene.args(step, &wanted);
        let again = run
            .step_with(step, &args, step.commits.then_some(&write))
            .await;
        assert_eq!(
            failed(again),
            (
                FailureClass::Transient,
                contract::CONNECTION_LOST,
                Sent::NotSent
            )
        );
    }
    run.release().await;
    assert_eq!(scene.db.write_count(Table::OrderLines, &LINE).unwrap(), 1);

    // Reconcile is a new connection and the read-back by key: it finds the
    // intended values, so the write is known to have landed.
    armed.store(false, Ordering::SeqCst);
    let mut run = scene.prepare(Posture::Live).await;
    let read = values(run.steps(&binding.read_back, &wanted, None).await.unwrap());
    assert!(agree(
        &scene,
        "length_inches",
        &decimal("151.25"),
        read.get("length").unwrap()
    ));
    run.release().await;
    assert_eq!(scene.db.write_count(Table::OrderLines, &LINE).unwrap(), 1);
}
