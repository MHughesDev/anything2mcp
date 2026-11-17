//! The stand's own tests: the schema and seed, the control interface, and every
//! fault. Each test changes the store through an ordinary connection, as the data
//! pack will, and proves the result by asking the control interface what is stored.

use std::error::Error;
use std::time::Duration;

use example_db::faults::{Fault, LINES_FROZEN_MESSAGE};
use example_db::harness::{StandError, TestDb};
use example_db::rows::{Value, WriteAction, Writer};
use example_db::schema::{READ_THAT_WRITES, STAND_PREFIX, Table, VIEW};
use rusqlite::{Connection, ErrorCode, OpenFlags};

type Outcome = Result<(), Box<dyn Error>>;

const LINE: [&str; 2] = ["A-100", "L-101"];

/// The engine's code for a refused statement, or a test failure if it ran.
fn refusal<T>(result: rusqlite::Result<T>) -> Result<(ErrorCode, String), Box<dyn Error>> {
    match result {
        Ok(_) => Err("the store should have refused the statement".into()),
        Err(e) => {
            let code = e.sqlite_error_code().ok_or("not an engine error")?;
            Ok((code, e.to_string()))
        }
    }
}

/// An ordinary connection that does not wait for a lock.
fn impatient(db: &TestDb) -> Result<Connection, Box<dyn Error>> {
    let conn = db.connect()?;
    conn.busy_timeout(Duration::ZERO)?;
    Ok(conn)
}

fn line(db: &TestDb) -> Result<example_db::rows::Row, Box<dyn Error>> {
    Ok(db
        .stored_row(Table::OrderLines, &LINE)?
        .ok_or("the seed line is missing")?)
}

/// The slice's proof: change a row through an ordinary connection opened by path,
/// then ask the stand what is stored and how many times it was written.
#[test]
fn a_write_is_proven_by_asking_the_stand_what_it_stored() -> Outcome {
    fn shareable<T: Send + Sync>(_: &T) {}
    let db = TestDb::start()?;
    shareable(&db);
    let before = line(&db)?;
    assert_eq!(before.text("length_in"), Some("150.5"));
    assert_eq!(before.text("unit_price"), Some("12.50"));
    assert_eq!(before.integer("row_version"), Some(1));
    assert_eq!(db.write_count(Table::OrderLines, &LINE)?, 0);

    let conn = Connection::open_with_flags(db.path(), OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let changed = conn.execute(
        "UPDATE order_lines SET length_in = ?1, unit_price = ?2 \
         WHERE order_id = ?3 AND item_code = ?4 AND row_version = ?5",
        ("151.25", "13.10", "A-100", "L-101", 1),
    )?;
    assert_eq!(changed, 1, "the store's own bookkeeping is not counted");

    let after = line(&db)?;
    assert_eq!(after.text("length_in"), Some("151.25"));
    assert_eq!(after.text("unit_price"), Some("13.10"));
    assert_eq!(
        after.integer("row_version"),
        Some(2),
        "the store moved the version"
    );
    assert_eq!(after.text("description"), Some("Left rail"));
    assert_eq!(db.write_count(Table::OrderLines, &LINE)?, 1);
    assert_eq!(db.write_count(Table::OrderLines, &["A-100", "L-102"])?, 0);
    let writes = db.writes()?;
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].seq, 1);
    assert_eq!(writes[0].table, Table::OrderLines);
    assert_eq!(writes[0].key, LINE);
    assert_eq!(writes[0].action, WriteAction::Update);
    assert_eq!(writes[0].writer, Writer::Other);
    Ok(())
}

/// Case 1: orders and line items, read by key and by filter, with typed columns,
/// nulls, and a view. Nothing is stored as a float.
#[test]
fn case_1_the_seed_is_read_by_key_and_by_filter() -> Outcome {
    let db = TestDb::start()?;
    let conn = Connection::open_with_flags(
        db.uri(),
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI,
    )?;

    let by_key: (String, i64) = conn.query_row(
        "SELECT length_in, row_version FROM order_lines WHERE order_id = ? AND item_code = ?",
        LINE,
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(by_key, ("150.5".to_owned(), 1));
    let mut by_filter =
        conn.prepare("SELECT item_code FROM order_lines WHERE order_id = ? ORDER BY item_code")?;
    let items: Vec<String> = by_filter
        .query_map(["A-100"], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    assert_eq!(items, ["L-101", "L-102", "L-103"]);
    let absent: Option<String> = conn
        .query_row(
            "SELECT item_code FROM order_lines WHERE order_id = ? AND item_code = ?",
            ["A-100", "L-999"],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    assert_eq!(absent, None);

    let summary: (String, i64, i64) = conn.query_row(
        &format!("SELECT customer_name, line_count, total_quantity FROM {VIEW} WHERE order_id = ?"),
        ["A-100"],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(summary, ("Example Customer One".to_owned(), 3, 8));

    // The control interface reports the same rows, with their storage classes.
    let order = db
        .stored_row(Table::Orders, &["A-100"])?
        .ok_or("the seed order is missing")?;
    assert_eq!(order.text("ordered_on"), Some("2026-03-02"));
    assert!(order.is_null("submitted_at"));
    assert_eq!(order.text("customer_ref"), Some("PO-7781"));
    let customer = db
        .stored_row(Table::Customers, &["C-003"])?
        .ok_or("the seed customer is missing")?;
    assert!(customer.is_null("email"));
    assert_eq!(customer.integer("credit_limit_cents"), Some(0));
    assert_eq!(db.stored_row(Table::Orders, &["A-999"])?, None);
    let history = db
        .stored_row(Table::OrderStatusHistory, &["1"])?
        .ok_or("the seed history row is missing")?;
    assert_eq!(history.text("new_status"), Some("submitted"));
    assert!(matches!(
        db.stored_row(Table::OrderLines, &["A-100"]),
        Err(StandError::WrongKey {
            expected: 2,
            given: 1,
            ..
        })
    ));

    let counts: Vec<usize> = Table::ALL
        .iter()
        .map(|table| db.stored_rows(*table).map(|rows| rows.len()))
        .collect::<Result<_, _>>()?;
    assert_eq!(counts, [3, 3, 4, 1, 2, 1]);
    for table in Table::ALL {
        for row in db.stored_rows(table)? {
            for (column, value) in row.values() {
                assert!(
                    !matches!(value, Value::Real(_)),
                    "{}.{column} holds a float",
                    table.name()
                );
            }
        }
    }

    let columns = db.columns(Table::OrderLines)?;
    let described: Vec<(&str, &str, u32)> = columns
        .iter()
        .map(|c| (c.name.as_str(), c.declared_type.as_str(), c.key_position))
        .collect();
    assert_eq!(
        described,
        [
            ("order_id", "TEXT", 1),
            ("item_code", "TEXT", 2),
            ("description", "TEXT", 0),
            ("quantity", "INTEGER", 0),
            ("length_in", "TEXT", 0),
            ("unit_price", "TEXT", 0),
            ("row_version", "INTEGER", 0),
        ]
    );
    assert!(columns.iter().all(|c| c.not_null));
    let orders = db.columns(Table::Orders)?;
    let ordered_on = orders
        .iter()
        .find(|c| c.name == "ordered_on")
        .ok_or("no such column")?;
    assert_eq!(ordered_on.declared_type, "DATE");
    assert!(
        db.columns(Table::OrderStatusHistory)?
            .iter()
            .all(|c| c.key_position == 0)
    );
    Ok(())
}

/// Case 2: a statement that looks like a read and writes is refused by the store on
/// a read-only open, and the stand confirms nothing changed.
#[test]
fn case_2_a_read_only_open_refuses_a_statement_that_writes() -> Outcome {
    let db = TestDb::start()?;
    let version = db.data_version()?;

    let reader = db.connect_read_only()?;
    assert!(reader.is_readonly("main")?);
    let quantity: i64 = reader.query_row(
        "SELECT quantity FROM order_lines WHERE order_id = ? AND item_code = ?",
        LINE,
        |row| row.get(0),
    )?;
    assert_eq!(quantity, 2);
    let (code, _) = refusal(reader.query_row(READ_THAT_WRITES, LINE, |row| row.get::<_, i64>(2)))?;
    assert_eq!(code, ErrorCode::ReadOnly);
    assert_eq!(line(&db)?.integer("quantity"), Some(2));
    assert_eq!(db.write_count(Table::OrderLines, &LINE)?, 0);
    assert_eq!(db.data_version()?, version, "nothing was committed");

    // The same statement on an ordinary connection does write, and the stand sees it.
    let writer = db.connect()?;
    let returned: i64 = writer.query_row(READ_THAT_WRITES, LINE, |row| row.get(2))?;
    assert_eq!(returned, 3);
    assert_eq!(line(&db)?.integer("quantity"), Some(3));
    assert_eq!(db.write_count(Table::OrderLines, &LINE)?, 1);
    assert_ne!(db.data_version()?, version);
    Ok(())
}

/// Case 3: a constraint refuses a write, names itself, and nothing is stored.
#[test]
fn case_3_constraints_refuse_writes_and_nothing_is_stored() -> Outcome {
    let db = TestDb::start()?;
    let conn = db.connect()?;
    let refused = [
        (
            "UPDATE order_lines SET quantity = 0 WHERE order_id = 'A-100' AND item_code = 'L-101'",
            "quantity_in_range",
        ),
        (
            "UPDATE order_lines SET description = '  ' WHERE order_id = 'A-100'",
            "description_not_empty",
        ),
        (
            "UPDATE orders SET status = 'lost' WHERE order_id = 'A-100'",
            "order_status_known",
        ),
        (
            "UPDATE orders SET customer_ref = 'PO-7790' WHERE order_id = 'A-100'",
            "orders.customer_ref",
        ),
        (
            "UPDATE customers SET credit_limit_cents = -1 WHERE customer_id = 'C-001'",
            "credit_limit_not_negative",
        ),
        (
            "UPDATE customers SET email = 'two@example.test' WHERE customer_id = 'C-001'",
            "customers.email",
        ),
        (
            "UPDATE order_lines SET length_in = NULL WHERE order_id = 'A-100'",
            "order_lines.length_in",
        ),
        (
            "INSERT INTO order_lines (order_id, item_code, description, quantity, length_in, unit_price) \
             VALUES ('A-100', 'L-101', 'Again', 1, '1', '1.00')",
            "order_lines.order_id, order_lines.item_code",
        ),
        (
            "INSERT INTO orders (order_id, customer_id, status, ordered_on) \
             VALUES ('A-200', 'C-999', 'draft', '2026-03-09')",
            "FOREIGN KEY",
        ),
        ("DELETE FROM orders WHERE order_id = 'A-101'", "FOREIGN KEY"),
    ];
    for (statement, named) in refused {
        let (code, message) = refusal(conn.execute(statement, []))?;
        assert_eq!(code, ErrorCode::ConstraintViolation, "{statement}");
        assert!(message.contains(named), "{message} does not name {named}");
    }
    assert!(db.writes()?.is_empty(), "a refused write stores nothing");
    assert_eq!(line(&db)?.integer("quantity"), Some(2));
    assert!(db.stored_row(Table::Orders, &["A-101"])?.is_some());
    Ok(())
}

/// Case 4: the system's own trigger writes another table, and a delete cascades.
/// The stand reports every row that changed, in order.
#[test]
fn case_4_a_trigger_writes_another_table_and_a_delete_cascades() -> Outcome {
    let db = TestDb::start()?;
    let conn = db.connect()?;
    assert_eq!(
        conn.execute(
            "UPDATE orders SET status = 'submitted' WHERE order_id = 'A-102'",
            []
        )?,
        1
    );
    let history = db.stored_rows(Table::OrderStatusHistory)?;
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].text("order_id"), Some("A-102"));
    assert_eq!(history[1].text("old_status"), Some("draft"));

    assert_eq!(
        conn.execute("DELETE FROM orders WHERE order_id = 'A-100'", [])?,
        1
    );
    assert!(db.stored_row(Table::OrderLines, &LINE)?.is_none());
    assert_eq!(db.stored_rows(Table::OrderLines)?.len(), 1);
    assert!(db.stored_row(Table::OrderNotes, &["1"])?.is_none());

    let seen: Vec<(Table, WriteAction, String)> = db
        .writes()?
        .into_iter()
        .map(|w| (w.table, w.action, w.key.join("/")))
        .collect();
    let update = WriteAction::Update;
    let insert = WriteAction::Insert;
    let delete = WriteAction::Delete;
    assert_eq!(seen.len(), 7);
    assert!(seen.contains(&(Table::Orders, update, "A-102".to_owned())));
    assert!(seen.contains(&(Table::OrderStatusHistory, insert, "2".to_owned())));
    assert!(seen.contains(&(Table::Orders, delete, "A-100".to_owned())));
    for item in ["L-101", "L-102", "L-103"] {
        assert!(seen.contains(&(Table::OrderLines, delete, format!("A-100/{item}"))));
    }
    assert!(seen.contains(&(Table::OrderNotes, delete, "1".to_owned())));

    // The catalog shows the system's two triggers apart from the stand's own.
    let own: Vec<String> = db
        .triggers()?
        .into_iter()
        .filter(|t| !t.name.starts_with(STAND_PREFIX))
        .map(|t| format!("{} on {}", t.name, t.table))
        .collect();
    assert_eq!(
        own,
        [
            "order_lines_bump_version on order_lines",
            "orders_record_status_change on orders"
        ]
    );
    Ok(())
}

/// Case 5: the store assigns a key on insert. One table reuses the number of a
/// deleted last row and has a spare reference column; the other never reuses one.
#[test]
fn case_5_the_store_assigns_keys_on_insert() -> Outcome {
    let db = TestDb::start()?;
    let conn = db.connect()?;
    let ship = "INSERT INTO shipments (order_id, carrier, external_ref) VALUES ('A-101', 'Example Carrier', ?)";
    conn.execute(ship, ["SHIP-0002"])?;
    assert_eq!(
        conn.last_insert_rowid(),
        2,
        "the stand's log does not disturb the assigned key"
    );
    let stored = db
        .stored_row(Table::Shipments, &["2"])?
        .ok_or("the shipment is missing")?;
    assert_eq!(stored.text("external_ref"), Some("SHIP-0002"));
    assert!(stored.is_null("shipped_on"));
    assert_eq!(db.write_count(Table::Shipments, &["2"])?, 1);
    conn.execute("DELETE FROM shipments WHERE shipment_id = 2", [])?;
    conn.execute(ship, ["SHIP-0003"])?;
    assert_eq!(
        conn.last_insert_rowid(),
        2,
        "the number of a deleted last row is reused"
    );
    let (code, _) = refusal(conn.execute(ship, ["SHIP-0003"]))?;
    assert_eq!(code, ErrorCode::ConstraintViolation);

    let note = "INSERT INTO order_notes (order_id, author, body) VALUES ('A-102', 'Example Clerk', 'Call first.')";
    conn.execute(note, [])?;
    assert_eq!(conn.last_insert_rowid(), 3);
    conn.execute("DELETE FROM order_notes WHERE note_id = 3", [])?;
    conn.execute(note, [])?;
    assert_eq!(
        conn.last_insert_rowid(),
        4,
        "a note's number is never reused"
    );
    assert!(db.stored_row(Table::OrderNotes, &["3"])?.is_none());
    assert_eq!(db.write_count(Table::OrderNotes, &["4"])?, 1);
    Ok(())
}

/// Case 6: someone else changes a row between a read and a write. The guarded
/// write then changes no row, and the stand shows who wrote.
#[test]
fn case_6_someone_else_changes_a_row_between_read_and_write() -> Outcome {
    let db = TestDb::start()?;
    let conn = db.connect()?;
    let seen: i64 = conn.query_row(
        "SELECT row_version FROM order_lines WHERE order_id = ? AND item_code = ?",
        LINE,
        |row| row.get(0),
    )?;

    let changed = db.someone_else(
        "UPDATE order_lines SET quantity = ?1 WHERE order_id = ?2 AND item_code = ?3",
        &[Value::from(7), Value::from("A-100"), Value::from("L-101")],
    )?;
    assert_eq!(changed, 1);

    let landed = conn.execute(
        "UPDATE order_lines SET quantity = 5 \
         WHERE order_id = 'A-100' AND item_code = 'L-101' AND row_version = ?",
        [seen],
    )?;
    assert_eq!(landed, 0, "the guard did not hold");
    let stored = line(&db)?;
    assert_eq!(stored.integer("quantity"), Some(7));
    assert_eq!(stored.integer("row_version"), Some(2));
    assert_eq!(db.write_count(Table::OrderLines, &LINE)?, 0);
    let writes = db.writes()?;
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].writer, Writer::Stand);

    // A statement the store refuses changes nothing and leaves no trace.
    assert!(matches!(
        db.someone_else("UPDATE order_lines SET quantity = 0", &[]),
        Err(StandError::Sql(_))
    ));
    assert_eq!(db.writes()?.len(), 1);
    conn.execute(
        "UPDATE order_lines SET quantity = 5 WHERE order_id = 'A-100' AND item_code = 'L-101'",
        [],
    )?;
    assert_eq!(db.writes()?[1].writer, Writer::Other);
    Ok(())
}

/// Case 7: the store keeps a value differently from what was sent. Each way is a
/// fault, each counts as one write, and each moves the version once, whether or not
/// the writing connection lets a trigger fire itself.
#[test]
fn case_7_the_store_keeps_a_value_differently_from_what_was_sent() -> Outcome {
    let db = TestDb::start()?;
    let write = "UPDATE order_lines SET description = ?1, length_in = ?2, unit_price = ?3 \
                 WHERE order_id = 'A-100' AND item_code = 'L-101'";
    for fault in [
        Fault::LengthRescaled,
        Fault::PriceRoundedDown,
        Fault::DescriptionPadded,
    ] {
        db.set_fault(fault)?;
    }
    for (round, recursive) in [(1, "OFF"), (2, "ON")] {
        let conn = db.connect()?;
        conn.execute_batch(&format!("PRAGMA recursive_triggers = {recursive}"))?;
        assert_eq!(conn.execute(write, ("Left rail", "151.25", "12.49"))?, 1);
        let stored = line(&db)?;
        assert_eq!(
            stored.text("length_in"),
            Some("151.2500"),
            "same value, another scale"
        );
        assert_eq!(
            stored.text("unit_price"),
            Some("12.00"),
            "a different value"
        );
        assert_eq!(stored.text("description"), Some("Left rail               "));
        assert_eq!(stored.integer("row_version"), Some(1 + round));
        assert_eq!(
            db.write_count(Table::OrderLines, &LINE)?,
            usize::try_from(round)?
        );
    }
    let conn = db.connect()?;
    conn.execute(
        "INSERT INTO order_lines (order_id, item_code, description, quantity, length_in, unit_price) \
         VALUES ('A-102', 'L-301', 'Shelf', 1, '12', '7')",
        [],
    )?;
    let inserted = db
        .stored_row(Table::OrderLines, &["A-102", "L-301"])?
        .ok_or("the new line is missing")?;
    assert_eq!(inserted.text("length_in"), Some("12.0000"));
    assert_eq!(inserted.text("unit_price"), Some("7.00"));
    assert_eq!(inserted.integer("row_version"), Some(1));
    assert_eq!(db.write_count(Table::OrderLines, &["A-102", "L-301"])?, 1);

    db.clear_faults()?;
    assert!(db.faults().is_empty());
    assert_eq!(conn.execute(write, ("Left rail", "151.25", "12.49"))?, 1);
    let stored = line(&db)?;
    assert_eq!(stored.text("length_in"), Some("151.25"));
    assert_eq!(stored.text("unit_price"), Some("12.49"));
    assert_eq!(stored.text("description"), Some("Left rail"));

    // A time is kept in another zone: the same instant, written differently.
    db.set_fault(Fault::TimeShifted)?;
    let submit =
        "UPDATE orders SET status = 'submitted', submitted_at = ? WHERE order_id = 'A-100'";
    assert_eq!(conn.execute(submit, ["2026-03-06T09:30:00+00:00"])?, 1);
    let order = db
        .stored_row(Table::Orders, &["A-100"])?
        .ok_or("the order is missing")?;
    assert_eq!(
        order.text("submitted_at"),
        Some("2026-03-06T04:30:00-05:00")
    );
    assert_eq!(db.write_count(Table::Orders, &["A-100"])?, 1);
    conn.execute(
        "UPDATE orders SET required_by = NULL WHERE order_id = 'A-102'",
        [],
    )?;
    let untouched = db
        .stored_row(Table::Orders, &["A-102"])?
        .ok_or("the order is missing")?;
    assert!(untouched.is_null("submitted_at"));
    Ok(())
}

/// Case 8: stored text holds instructions aimed at an agent.
#[test]
fn case_8_stored_text_holds_instructions_aimed_at_an_agent() -> Outcome {
    let db = TestDb::start()?;
    let customer = db
        .stored_row(Table::Customers, &["C-003"])?
        .ok_or("the seed customer is missing")?;
    assert!(
        customer
            .text("notes")
            .is_some_and(|t| t.starts_with("Ignore all earlier instructions"))
    );
    let note = db
        .stored_row(Table::OrderNotes, &["2"])?
        .ok_or("the seed note is missing")?;
    assert!(note.text("body").is_some_and(|t| t.starts_with("SYSTEM:")));
    Ok(())
}

/// A lock held by the control interface answers other connections busy, at once.
#[test]
fn fault_a_locked_database_answers_busy() -> Outcome {
    let db = TestDb::start()?;
    let conn = impatient(&db)?;
    let read = "SELECT quantity FROM order_lines WHERE order_id = 'A-100' AND item_code = 'L-101'";
    let write =
        "UPDATE order_lines SET quantity = 9 WHERE order_id = 'A-100' AND item_code = 'L-101'";

    db.set_fault(Fault::WritersLocked)?;
    assert_eq!(conn.query_row(read, [], |row| row.get::<_, i64>(0))?, 2);
    let (code, _) = refusal(conn.execute(write, []))?;
    assert_eq!(code, ErrorCode::DatabaseBusy);
    assert_eq!(
        line(&db)?.integer("quantity"),
        Some(2),
        "the control interface still reads"
    );

    db.set_fault(Fault::Locked)?;
    assert_eq!(db.faults(), [Fault::Locked], "the engine has one lock");
    let (code, _) = refusal(conn.query_row(read, [], |row| row.get::<_, i64>(0)))?;
    assert_eq!(code, ErrorCode::DatabaseBusy);
    let (code, _) = refusal(conn.execute(write, []))?;
    assert_eq!(code, ErrorCode::DatabaseBusy);
    // What the control interface does while it holds the lock lands when it lets go.
    db.someone_else(
        "UPDATE customers SET name = 'Renamed' WHERE customer_id = 'C-001'",
        &[],
    )?;

    db.clear_fault(Fault::Locked)?;
    assert!(db.faults().is_empty());
    assert_eq!(conn.execute(write, [])?, 1);
    assert_eq!(line(&db)?.integer("quantity"), Some(9));
    assert_eq!(db.write_count(Table::OrderLines, &LINE)?, 1);
    let name: String = conn.query_row(
        "SELECT name FROM customers WHERE customer_id = 'C-001'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(name, "Renamed");
    Ok(())
}

/// A rule in the store refuses a write that the constraints would allow.
#[test]
fn fault_a_rule_in_the_store_refuses_a_valid_write() -> Outcome {
    let db = TestDb::start()?;
    let conn = db.connect()?;
    let write =
        "UPDATE order_lines SET quantity = 3 WHERE order_id = 'A-100' AND item_code = 'L-101'";
    db.set_fault(Fault::LineWritesRefused)?;
    for statement in [
        write,
        "DELETE FROM order_lines WHERE order_id = 'A-100' AND item_code = 'L-101'",
        "INSERT INTO order_lines (order_id, item_code, description, quantity, length_in, unit_price) \
         VALUES ('A-102', 'L-301', 'Shelf', 1, '12', '7.00')",
    ] {
        let (code, message) = refusal(conn.execute(statement, []))?;
        assert_eq!(code, ErrorCode::ConstraintViolation);
        assert!(message.contains(LINES_FROZEN_MESSAGE), "{message}");
    }
    assert_eq!(line(&db)?.integer("quantity"), Some(2));
    assert!(db.writes()?.is_empty());
    assert_eq!(
        conn.execute(
            "UPDATE orders SET required_by = '2026-04-01' WHERE order_id = 'A-100'",
            []
        )?,
        1
    );

    db.clear_fault(Fault::LineWritesRefused)?;
    assert_eq!(conn.execute(write, [])?, 1);
    assert_eq!(line(&db)?.integer("quantity"), Some(3));
    Ok(())
}

/// Case 10: the schema changes under a connection that is already open. A renamed
/// column and a retyped one are both seen, and both can be put back.
#[test]
fn case_10_fault_the_schema_changes_under_an_open_connection() -> Outcome {
    let db = TestDb::start()?;
    let conn = db.connect()?;
    let by_old_name = "SELECT length_in FROM order_lines WHERE order_id = ? AND item_code = ?";
    let price = "SELECT unit_price, typeof(unit_price) FROM order_lines WHERE order_id = ? AND item_code = ?";
    assert_eq!(
        conn.query_row(by_old_name, LINE, |row| row.get::<_, String>(0))?,
        "150.5"
    );

    db.set_fault(Fault::ColumnRenamed)?;
    let (code, message) =
        refusal(conn.query_row(by_old_name, LINE, |row| row.get::<_, String>(0)))?;
    assert_eq!(code, ErrorCode::Unknown);
    assert!(message.contains("no such column"), "{message}");
    let names: Vec<String> = db
        .columns(Table::OrderLines)?
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert!(
        names.contains(&"length_inches".to_owned()) && !names.contains(&"length_in".to_owned())
    );
    assert_eq!(line(&db)?.text("length_inches"), Some("150.5"));
    // A fault that names the column follows the new name.
    db.set_fault(Fault::LengthRescaled)?;
    conn.execute(
        "UPDATE order_lines SET quantity = 3 WHERE order_id = 'A-100' AND item_code = 'L-101'",
        [],
    )?;
    assert_eq!(line(&db)?.text("length_inches"), Some("150.5000"));
    db.clear_fault(Fault::LengthRescaled)?;
    db.clear_fault(Fault::ColumnRenamed)?;
    assert_eq!(
        conn.query_row(by_old_name, LINE, |row| row.get::<_, String>(0))?,
        "150.5000"
    );

    db.set_fault(Fault::ColumnRetyped)?;
    let retyped = db.columns(Table::OrderLines)?;
    let last = retyped.last().ok_or("no columns")?;
    assert_eq!(
        (last.name.as_str(), last.declared_type.as_str()),
        ("unit_price", "INTEGER")
    );
    assert_eq!(line(&db)?.integer("unit_price"), Some(1250));
    let stored: (i64, String) =
        conn.query_row(price, LINE, |row| Ok((row.get(0)?, row.get(1)?)))?;
    assert_eq!(stored, (1250, "integer".to_owned()));
    assert_eq!(
        line(&db)?.integer("row_version"),
        Some(2),
        "a change of schema is not a write"
    );
    assert_eq!(db.write_count(Table::OrderLines, &LINE)?, 1);
    // The fault that rounds a price works on the new type too.
    db.set_fault(Fault::PriceRoundedDown)?;
    conn.execute(
        "UPDATE order_lines SET unit_price = 1299 WHERE order_id = 'A-100' AND item_code = 'L-101'",
        [],
    )?;
    assert_eq!(line(&db)?.integer("unit_price"), Some(1200));

    db.clear_faults()?;
    let stored: (String, String) =
        conn.query_row(price, LINE, |row| Ok((row.get(0)?, row.get(1)?)))?;
    assert_eq!(stored, ("12.00".to_owned(), "text".to_owned()));
    let other = db
        .stored_row(Table::OrderLines, &["A-100", "L-103"])?
        .ok_or("the seed line is missing")?;
    assert_eq!(other.text("unit_price"), Some("4.75"));
    assert_eq!(other.integer("row_version"), Some(1));
    Ok(())
}

/// A file marked read-only lets a new connection read and refuses its writes. A
/// privileged user is not bound by the mark; the stand then says the fault has no
/// effect, and leaves the file writable.
#[test]
fn fault_a_read_only_file_refuses_writes_on_a_new_connection() -> Outcome {
    let db = TestDb::start()?;
    let read = "SELECT quantity FROM order_lines WHERE order_id = 'A-100' AND item_code = 'L-101'";
    let write =
        "UPDATE order_lines SET quantity = 9 WHERE order_id = 'A-100' AND item_code = 'L-101'";
    match db.set_fault(Fault::ReadOnlyFile) {
        Ok(()) => {
            assert_eq!(db.faults(), [Fault::ReadOnlyFile]);
            let conn = Connection::open_with_flags(db.path(), OpenFlags::SQLITE_OPEN_READ_WRITE)?;
            assert_eq!(conn.query_row(read, [], |row| row.get::<_, i64>(0))?, 2);
            let (code, _) = refusal(conn.execute(write, []))?;
            assert_eq!(code, ErrorCode::ReadOnly);
            assert_eq!(line(&db)?.integer("quantity"), Some(2));
            assert_eq!(db.write_count(Table::OrderLines, &LINE)?, 0);
            drop(conn);
            db.clear_fault(Fault::ReadOnlyFile)?;
        }
        Err(StandError::FaultNotEffective(Fault::ReadOnlyFile)) => {
            assert!(db.faults().is_empty());
        }
        Err(other) => return Err(other.into()),
    }
    let conn = db.connect()?;
    assert_eq!(conn.execute(write, [])?, 1);
    assert_eq!(line(&db)?.integer("quantity"), Some(9));
    Ok(())
}

/// With the write log off, a catalog read finds only the system's own triggers,
/// and the stand refuses to give a count it does not have.
#[test]
fn the_write_log_can_be_taken_out_of_the_catalog() -> Outcome {
    let db = TestDb::start()?;
    assert_eq!(db.triggers()?.len(), 2 + 3 * Table::ALL.len());
    db.set_write_log(false)?;
    let names: Vec<String> = db.triggers()?.into_iter().map(|t| t.name).collect();
    assert_eq!(
        names,
        ["order_lines_bump_version", "orders_record_status_change"]
    );
    assert!(matches!(db.writes(), Err(StandError::WriteLogOff)));
    assert!(matches!(
        db.write_count(Table::Customers, &["C-001"]),
        Err(StandError::WriteLogOff)
    ));

    let conn = db.connect()?;
    conn.execute(
        "UPDATE customers SET name = 'Unlogged' WHERE customer_id = 'C-001'",
        [],
    )?;
    let customer = db
        .stored_row(Table::Customers, &["C-001"])?
        .ok_or("the seed customer is missing")?;
    assert_eq!(customer.text("name"), Some("Unlogged"));

    db.set_write_log(true)?;
    assert!(db.writes()?.is_empty());
    conn.execute(
        "UPDATE customers SET name = 'Logged' WHERE customer_id = 'C-001'",
        [],
    )?;
    assert_eq!(db.write_count(Table::Customers, &["C-001"])?, 1);
    Ok(())
}

/// Reset restores the seed in the same file and clears every fault and the log.
/// Two stands share nothing, and a dropped stand leaves no folder behind.
#[test]
fn reset_restores_the_seed_and_two_stands_share_nothing() -> Outcome {
    let db = TestDb::start()?;
    let other = TestDb::start()?;
    assert_ne!(db.path(), other.path());
    assert!(db.path().starts_with(std::env::temp_dir()));

    let conn = db.connect()?;
    conn.execute("DELETE FROM orders WHERE order_id = 'A-100'", [])?;
    conn.execute(
        "INSERT INTO order_notes (order_id, author, body) VALUES ('A-102', 'Example Clerk', 'x')",
        [],
    )?;
    for fault in [
        Fault::ColumnRenamed,
        Fault::ColumnRetyped,
        Fault::PriceRoundedDown,
        Fault::LineWritesRefused,
        Fault::Locked,
    ] {
        db.set_fault(fault)?;
    }
    db.set_write_log(false)?;
    assert_eq!(line(&other)?.text("length_in"), Some("150.5"));
    assert!(other.writes()?.is_empty());

    db.reset()?;
    assert!(db.faults().is_empty());
    assert!(db.writes()?.is_empty());
    assert_eq!(db.triggers()?.len(), 2 + 3 * Table::ALL.len());
    let restored = line(&db)?;
    assert_eq!(restored.text("length_in"), Some("150.5"));
    assert_eq!(restored.text("unit_price"), Some("12.50"));
    assert_eq!(restored.integer("row_version"), Some(1));
    assert_eq!(db.stored_rows(Table::OrderNotes)?.len(), 2);
    // The connection opened before the reset goes on working, and keys start again.
    conn.execute(
        "INSERT INTO order_notes (order_id, author, body) VALUES ('A-102', 'Example Clerk', 'y')",
        [],
    )?;
    assert_eq!(conn.last_insert_rowid(), 3);
    assert_eq!(db.writes()?[0].seq, 1);

    let folder = db.path().parent().ok_or("no folder")?.to_path_buf();
    drop(conn);
    drop(db);
    assert!(!folder.exists(), "the folder is removed with the stand");
    Ok(())
}
