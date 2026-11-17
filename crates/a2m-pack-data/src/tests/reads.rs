//! Reads against the stand: by key, as a list, and what a read may not do.

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{FailureClass, Secret, Value};
use a2m_provider::{Posture, Sent, StepOutcome};
use a2m_vault::Redactor;
use example_db::faults::Fault;
use example_db::schema::{READ_THAT_WRITES, Table};

use super::{Scene, failed, inputs, values};
use crate::names::codes;

fn text<'a>(values: &'a a2m_core::Values, name: &str) -> &'a str {
    values.get(name).and_then(Value::as_text).unwrap()
}

#[tokio::test]
async fn a_record_is_read_by_its_key_and_an_unknown_key_is_absent() {
    let scene = Scene::new();
    let before = scene.db.data_version().unwrap();
    let mut run = scene.prepare(Posture::Live).await;
    let act = &scene.binding("example_line_item_get").act;

    let found = values(
        run.steps(
            act,
            &inputs(&[("order_id", "A-100"), ("item_code", "L-103")]),
            None,
        )
        .await
        .unwrap(),
    );
    // What the driver returns is what the stand stores, value for value.
    let stored = scene
        .db
        .stored_row(Table::OrderLines, &["A-100", "L-103"])
        .unwrap()
        .unwrap();
    assert_eq!(text(&found, "length"), stored.text("length_in").unwrap());
    assert_eq!(text(&found, "length"), "36.0625");
    assert_eq!(text(&found, "unit_price"), "4.75");
    assert_eq!(text(&found, "description"), "Cross brace");
    assert_eq!(found.get("quantity"), Some(&Value::Int(4)));
    assert_eq!(found.get("row_version"), Some(&Value::Int(1)));
    assert_eq!(text(&found, "order_id"), "A-100");

    let absent = run
        .steps(
            act,
            &inputs(&[("order_id", "A-100"), ("item_code", "L-999")]),
            None,
        )
        .await
        .unwrap();
    assert!(matches!(absent, StepOutcome::Absent(_)));

    // An input is a bound value and never part of the statement: text that
    // would widen the predicate, were it pasted in, matches no row.
    let absent = run
        .steps(
            act,
            &inputs(&[
                ("order_id", "A-100' OR '1'='1"),
                ("item_code", "L-101' OR '1'='1"),
            ]),
            None,
        )
        .await
        .unwrap();
    assert!(matches!(absent, StepOutcome::Absent(_)));
    let absent = run
        .steps(
            act,
            &inputs(&[
                ("order_id", "A-100\"; DROP TABLE order_lines; --"),
                ("item_code", "x"),
            ]),
            None,
        )
        .await
        .unwrap();
    assert!(matches!(absent, StepOutcome::Absent(_)));

    run.release().await;
    assert_eq!(scene.db.data_version().unwrap(), before);
    assert!(scene.db.writes().unwrap().is_empty());
    assert_eq!(scene.db.stored_rows(Table::OrderLines).unwrap().len(), 4);
}

#[tokio::test]
async fn a_list_stops_at_its_limit_and_says_that_more_exist() {
    let scene = Scene::new();
    let mut run = scene.prepare(Posture::Live).await;
    let order = inputs(&[("order_id", "A-100")]);

    let all = values(
        run.steps(&scene.binding("example_order_lines_list").act, &order, None)
            .await
            .unwrap(),
    );
    let rows = all.get("rows").and_then(Value::as_list).unwrap();
    let codes_of = |rows: &[Value]| -> Vec<String> {
        rows.iter()
            .map(|row| text(row.as_record().unwrap(), "item_code").to_owned())
            .collect()
    };
    assert_eq!(codes_of(rows), ["L-101", "L-102", "L-103"]);
    assert_eq!(all.get("more"), Some(&Value::Bool(false)));

    let first = values(
        run.steps(
            &scene.binding("example_order_lines_first").act,
            &order,
            None,
        )
        .await
        .unwrap(),
    );
    assert_eq!(
        codes_of(first.get("rows").and_then(Value::as_list).unwrap()),
        ["L-101", "L-102"]
    );
    assert_eq!(first.get("more"), Some(&Value::Bool(true)));

    // A read of one record that matches several is a failure, never a pick.
    let several = run
        .steps(&scene.binding("example_order_line_get").act, &order, None)
        .await;
    assert_eq!(
        failed(several),
        (FailureClass::Drift, codes::KEY_NOT_UNIQUE, Sent::Answered)
    );
    // The same read of an order with one line finds it.
    let one = values(
        run.steps(
            &scene.binding("example_order_line_get").act,
            &inputs(&[("order_id", "A-101")]),
            None,
        )
        .await
        .unwrap(),
    );
    assert_eq!(text(&one, "item_code"), "L-201");
    run.release().await;
}

#[tokio::test]
async fn five_runs_of_one_read_do_the_same_thing() {
    let scene = Scene::new();
    let act = &scene.binding("example_line_item_get").act;
    let key = inputs(&[("order_id", "A-100"), ("item_code", "L-101")]);
    let mut seen = Vec::new();
    for _ in 0..5 {
        let mut run = scene.prepare(Posture::Live).await;
        let found = values(run.steps(act, &key, None).await.unwrap());
        let acting = run.acting();
        // What must be the same in two runs that did the same thing.
        let means: Vec<_> = acting.iter().map(|step| step.means.clone()).collect();
        seen.push((found, means));
        run.release().await;
    }
    assert!(seen.windows(2).all(|pair| pair[0] == pair[1]));
    let (_, means) = &seen[0];
    // The trace's own fields: a detail does not print what it holds.
    let shown: String = means
        .iter()
        .flatten()
        .map(|detail| format!("{}={};", detail.name, detail.value))
        .collect();
    assert_eq!(means.len(), 3);
    assert!(
        shown.contains("line_item_by_key") && shown.contains("read_only"),
        "{shown}"
    );
    // A value of a row is never a detail of how the step was done.
    assert!(
        !shown.contains("Left rail") && !shown.contains("150.5"),
        "{shown}"
    );
}

#[tokio::test]
async fn a_statement_declared_a_read_that_writes_is_refused_and_nothing_is_written() {
    // The compiler refuses it: stand case 2, a statement that returns rows
    // as a read does and changes one.
    let problems = super::compile::errors_after(
        &["statements", "orders.yaml"],
        "      SELECT order_id, item_code, description, quantity, length_in, unit_price, row_version\n      FROM order_lines WHERE order_id = ?\n    params: [{name: order_id, type: identifier}]\n    returns: one_or_none\n",
        &format!(
            "      {READ_THAT_WRITES}\n    params: [{{name: order_id, type: identifier}}, {{name: item_code, type: identifier}}]\n    returns: one_or_none\n"
        ),
    );
    let problem = problems
        .find("data.statement_invalid")
        .expect("a read that writes");
    assert_eq!(problem.at.file.as_str(), "statements/orders.yaml");
    assert!(problem.message.contains("changes data"), "{problem}");

    // The driver refuses it again, in a plan the compiler did not make,
    // before anything is sent.
    let mut scene = Scene::new();
    scene.alter("line_of_order", |statement| {
        statement.text = READ_THAT_WRITES.to_owned();
        statement.params.push(crate::model::Param {
            name: "item_code".to_owned(),
            field_type: None,
            key: true,
        });
    });
    let before = scene.db.data_version().unwrap();
    let mut run = scene.prepare(Posture::Live).await;
    let act = &scene.binding("example_order_line_get").act;
    let key = a2m_core::Values::new()
        .with("order_id", "A-100")
        .with("item_code", "L-101");
    run.step(&act[0], &a2m_core::Values::new()).await.unwrap();
    let touched = run.step_with(&act[1], &key, None).await;
    assert_eq!(
        failed(touched),
        (
            FailureClass::Policy,
            codes::READ_STATEMENT_WROTE,
            Sent::NotSent
        )
    );
    // The connection is still good for a real read.
    let found = values(
        run.steps(
            &scene.binding("example_line_item_get").act,
            &inputs(&[("order_id", "A-100"), ("item_code", "L-101")]),
            None,
        )
        .await
        .unwrap(),
    );
    assert_eq!(found.get("quantity"), Some(&Value::Int(2)));
    run.release().await;

    // The stand: nothing was written, and nothing was committed.
    assert_eq!(
        scene
            .db
            .write_count(Table::OrderLines, &["A-100", "L-101"])
            .unwrap(),
        0
    );
    assert_eq!(scene.db.data_version().unwrap(), before);
    let stored = scene
        .db
        .stored_row(Table::OrderLines, &["A-100", "L-101"])
        .unwrap()
        .unwrap();
    assert_eq!(stored.integer("quantity"), Some(2));
}

#[tokio::test]
async fn text_the_store_holds_comes_back_as_data_of_an_untrusted_type() {
    let scene = Scene::new();
    let mut run = scene.prepare(Posture::Live).await;
    let found = values(
        run.steps(
            &scene.binding("example_customer_get").act,
            &inputs(&[("customer_id", "C-003")]),
            None,
        )
        .await
        .unwrap(),
    );
    run.release().await;
    let stored = scene
        .db
        .stored_row(Table::Customers, &["C-003"])
        .unwrap()
        .unwrap();
    // The instructions aimed at an agent are a value, unchanged.
    assert_eq!(text(&found, "notes"), stored.text("notes").unwrap());
    assert!(text(&found, "notes").starts_with("Ignore all earlier instructions"));
    // And the plan marks the field's type as untrusted free text.
    let operation = scene
        .plan
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == "example_customer_get")
        .unwrap();
    let notes = operation
        .outputs
        .iter()
        .find(|output| output.name == "notes")
        .unwrap();
    assert!(
        scene
            .plan
            .field_type(notes.field_type.unwrap())
            .unwrap()
            .untrusted
    );
}

#[tokio::test]
async fn a_secret_in_the_stores_answer_reaches_no_result_and_no_trace() {
    // A secret the vault knows, planted in a row of the store.
    let redactor = Arc::new(Redactor::new());
    redactor.register(&Secret::new("hunter2-planted-secret"));
    let scene = Scene::with(&crate::pack(), redactor);
    scene
        .db
        .someone_else(
            "UPDATE customers SET notes = 'the password is hunter2-planted-secret' WHERE customer_id = 'C-001'",
            &[],
        )
        .unwrap();
    let mut run = scene.prepare(Posture::Live).await;
    let found = values(
        run.steps(
            &scene.binding("example_customer_get").act,
            &inputs(&[("customer_id", "C-001")]),
            None,
        )
        .await
        .unwrap(),
    );
    let acting = run.acting();
    run.release().await;
    let shown = format!("{found:?} {acting:?}");
    assert!(!shown.contains("hunter2"), "{shown}");
    assert!(text(&found, "notes").starts_with("the password is "));
}

#[tokio::test]
async fn a_lock_held_by_another_connection_is_named_busy() {
    let scene = Scene::new();
    let mut run = scene.prepare(Posture::Live).await;
    run.wait = Duration::from_millis(300);
    let key = inputs(&[("order_id", "A-100"), ("item_code", "L-101")]);
    let act = &scene.binding("example_line_item_get").act;

    // Writers locked: a read still works.
    scene.db.set_fault(Fault::WritersLocked).unwrap();
    assert!(run.steps(act, &key, None).await.is_ok());
    scene.db.clear_fault(Fault::WritersLocked).unwrap();

    // Everything locked: the read waits its time limit and is named busy.
    scene.db.set_fault(Fault::Locked).unwrap();
    let locked = run.steps(act, &key, None).await;
    assert_eq!(
        failed(locked),
        (FailureClass::Busy, codes::STORE_LOCKED, Sent::Answered)
    );
    scene.db.clear_fault(Fault::Locked).unwrap();

    // Nothing is left open: the same prepared driver reads again.
    assert!(run.steps(act, &key, None).await.is_ok());
    run.release().await;
}

#[tokio::test]
async fn a_changed_column_is_drift() {
    let scene = Scene::new();
    let key = inputs(&[("order_id", "A-100"), ("item_code", "L-101")]);
    let act = &scene.binding("example_line_item_get").act;

    // A column the statement names is gone: the statement no longer
    // prepares, and nothing runs.
    scene.db.set_fault(Fault::ColumnRenamed).unwrap();
    let mut run = scene.prepare(Posture::Live).await;
    let renamed = run.steps(act, &key, None).await;
    assert_eq!(
        failed(renamed),
        (FailureClass::Drift, codes::SCHEMA_CHANGED, Sent::Answered)
    );
    run.release().await;
    scene.db.clear_fault(Fault::ColumnRenamed).unwrap();

    // A column is stored another way than its field type declares: the
    // value is refused, never read as if it were the old one.
    scene.db.set_fault(Fault::ColumnRetyped).unwrap();
    let mut run = scene.prepare(Posture::Live).await;
    let retyped = run.steps(act, &key, None).await;
    assert_eq!(
        failed(retyped),
        (FailureClass::Drift, codes::SCHEMA_CHANGED, Sent::Answered)
    );
    run.release().await;
}

#[tokio::test]
async fn a_value_that_cannot_be_bound_stops_the_step_unsent() {
    let scene = Scene::new();
    let mut run = scene.prepare(Posture::Live).await;
    let act = &scene.binding("example_line_item_get").act;
    run.step(&act[0], &a2m_core::Values::new()).await.unwrap();
    // A list where one value belongs, and a missing argument.
    let args = a2m_core::Values::new()
        .with(
            "order_id",
            Value::List(vec![Value::from("A-100"), Value::from("A-101")]),
        )
        .with("item_code", "L-101");
    let listed = run.step_with(&act[1], &args, None).await;
    assert_eq!(
        failed(listed),
        (
            FailureClass::Input,
            codes::VALUE_NOT_BINDABLE,
            Sent::NotSent
        )
    );
    run.step(&act[0], &a2m_core::Values::new()).await.unwrap();
    let missing = run
        .step_with(
            &act[1],
            &a2m_core::Values::new().with("order_id", "A-100"),
            None,
        )
        .await;
    assert_eq!(
        failed(missing),
        (
            FailureClass::Input,
            codes::VALUE_NOT_BINDABLE,
            Sent::NotSent
        )
    );
    run.release().await;
}
