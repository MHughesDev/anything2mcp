//! What a statement may not do, whatever a target file says: the guards of
//! the connection, of the driver, and of the compiler, each on its own.
//!
//! Every test asks the stand, or the file system, what was left behind.

use std::fmt::Write as _;
use std::path::Path;
use std::time::Duration;

use a2m_core::{FailureClass, Value, Values};
use a2m_provider::{Posture, Sent};
use example_db::harness::TestDb;
use example_db::schema::Table;

use super::{Folder, Scene, failed, inputs};
use crate::model::{Access, Isolation};
use crate::names::codes;
use crate::sqlite::Sqlite;
use crate::store::{Bound, Conn, Opener, Refusal, RunLimits, StoreAt, Unseen};

fn limits() -> RunLimits {
    RunLimits {
        rows: 10,
        bytes: 1_000_000,
        wait: Duration::from_secs(5),
    }
}

fn as_text(path: &Path) -> Bound {
    Bound::Text(path.to_str().unwrap().to_owned())
}

/// Runs a statement the way a step would, and says whether the store ran it.
fn ran(conn: &mut dyn Conn, text: &str, params: &[Bound]) -> bool {
    conn.describe(text).is_ok() && conn.run(text, params, &limits()).is_ok()
}

// ---- the connection ----

#[test]
fn a_connection_opens_no_other_file_and_leaves_none_behind() {
    let db = TestDb::start().unwrap();
    let other = TestDb::start().unwrap();
    let folder = Folder::new("guards");
    for access in [Access::ReadOnly, Access::ReadWrite] {
        for in_transaction in [false, true] {
            let mut conn = Sqlite
                .open(&StoreAt::File(db.path().to_path_buf()), access)
                .unwrap();
            if in_transaction {
                conn.begin(Isolation::Serializable, Duration::from_secs(5))
                    .unwrap();
            }
            let conn = conn.as_mut();
            // A file an input names is not attached, by its path or as an
            // address that asks for the right to create it.
            let made = folder.path().join("made.db");
            let address = format!("file:{}?mode=rwc", made.to_str().unwrap());
            assert!(!ran(conn, "ATTACH ? AS x", &[as_text(other.path())]));
            assert!(!ran(conn, "ATTACH ? AS x", &[Bound::Text(address)]));
            assert!(!ran(conn, "SELECT name FROM x.customers", &[]));
            assert!(!made.exists());
            // The settings that hold the connection cannot be changed.
            assert!(!ran(conn, "PRAGMA query_only = 0", &[]));
            assert!(!ran(conn, "PRAGMA foreign_keys = 0", &[]));
            assert!(!ran(conn, "PRAGMA writable_schema = 1", &[]));
            // No copy of the store is written anywhere, and no empty file.
            let copy = folder.path().join("copy.db");
            assert!(!ran(conn, "VACUUM INTO ?", &[as_text(&copy)]));
            assert!(!copy.exists());
            // No schema change, in the store or in the connection's own
            // temporary schema; no extension; no transaction of its own.
            assert!(!ran(conn, "CREATE TABLE made_here (x)", &[]));
            assert!(!ran(conn, "CREATE TEMP TABLE made_here (x)", &[]));
            assert!(!ran(conn, "DROP TABLE customers", &[]));
            assert!(!ran(conn, "SELECT load_extension(?)", &[as_text(&copy)]));
            assert!(!ran(conn, "COMMIT", &[]));
            assert!(!ran(conn, "SAVEPOINT s", &[]));
            // Where the store's file is, is not a value a statement reads.
            assert!(!ran(conn, "SELECT file FROM pragma_database_list", &[]));
            // Several statements are refused as that, not as a changed shape.
            assert_eq!(
                conn.describe("SELECT 1; SELECT 2").unwrap_err(),
                Refusal::NotAllowed
            );
            // A quoted name that is no column is an error, never a text.
            assert_eq!(
                conn.describe("SELECT \"no_such_column\" FROM customers")
                    .unwrap_err(),
                Refusal::Shape
            );
            assert_eq!(conn.in_transaction(), in_transaction);
        }
    }
    // The stand: every table is where it was, and nothing was written.
    assert!(db.writes().unwrap().is_empty());
    assert_eq!(db.stored_rows(Table::Customers).unwrap().len(), 3);
}

#[test]
fn a_reading_connection_runs_only_what_reads_and_returns_columns() {
    let db = TestDb::start().unwrap();
    let before = db.data_version().unwrap();
    let mut conn = Sqlite
        .open(&StoreAt::File(db.path().to_path_buf()), Access::ReadOnly)
        .unwrap();
    conn.begin(Isolation::Serializable, Duration::from_secs(5))
        .unwrap();
    let conn = conn.as_mut();
    for text in [
        "UPDATE customers SET name = name RETURNING name",
        "DELETE FROM customers",
        "INSERT INTO customers (customer_id, name, credit_limit_cents) VALUES ('C-9', 'x', 0)",
        "WITH one AS (SELECT 1) DELETE FROM customers",
    ] {
        assert!(!ran(conn, text, &[]), "{text}");
    }
    // A plain read still runs.
    let read = conn
        .run("SELECT customer_id FROM customers", &[], &limits())
        .unwrap();
    assert_eq!(read.rows.len(), 3);
    assert_eq!(db.data_version().unwrap(), before);
    assert_eq!(db.stored_rows(Table::Customers).unwrap().len(), 3);
}

#[test]
fn only_an_absolute_path_is_opened() {
    // The bundled engine reads a name that begins `file:` as an address
    // with options, whatever flags it is opened with.
    for name in ["file:example.db?mode=rwc", "example.db"] {
        for access in [Access::ReadOnly, Access::ReadWrite] {
            assert_eq!(
                Sqlite
                    .open(&StoreAt::File(Path::new(name).to_path_buf()), access)
                    .err(),
                Some(Refusal::NotGuarded)
            );
        }
    }
}

// ---- the driver, given a compiled target the compiler would not make ----

#[tokio::test]
async fn a_read_that_attaches_a_file_an_input_names_is_refused_unsent() {
    let other = TestDb::start().unwrap();
    let mut scene = Scene::new();
    scene.alter("customer_by_key", |statement| {
        statement.text = "ATTACH ? AS x".to_owned();
    });
    scene.alter("shipment_by_key", |statement| {
        statement.text =
            "SELECT name AS carrier FROM x.customers WHERE customer_id <> ?".to_owned();
    });
    let mut run = scene.prepare(Posture::Live).await;
    let attached = run
        .steps(
            &scene.binding("example_customer_get").act,
            &inputs(&[("customer_id", other.path().to_str().unwrap())]),
            None,
        )
        .await;
    assert_eq!(
        failed(attached),
        (
            FailureClass::Policy,
            codes::STATEMENT_NOT_ALLOWED,
            Sent::NotSent
        )
    );
    // The other file was never opened, so nothing of it can be read.
    let read = run
        .steps(
            &scene.binding("example_shipment_get").act,
            &Values::new().with("shipment_id", 1_i64),
            None,
        )
        .await;
    assert_eq!(failed(read).0, FailureClass::Drift);
    run.release().await;
}

/// The embedded dialect holds the refusal of decision 0184 as the server's
/// does
/// (`a_comment_the_server_ends_at_a_carriage_return_is_refused_before_it_is_sent`):
/// the bundled store reads a form feed and a carriage return as space and
/// runs such a statement, and the driver refuses every control character but
/// a line feed and a tab in a compiled target's statement, before it is sent.
#[tokio::test]
async fn a_control_character_in_an_embedded_statement_is_refused_unsent() {
    let db = TestDb::start().unwrap();
    let mut conn = Sqlite
        .open(&StoreAt::File(db.path().to_path_buf()), Access::ReadOnly)
        .unwrap();
    let read = conn
        .as_mut()
        .run(
            "SELECT customer_id\u{c}FROM customers WHERE customer_id <> ?\r",
            &[Bound::Text("none".to_owned())],
            &limits(),
        )
        .unwrap();
    assert_eq!(read.rows.len(), 3);
    for text in [
        "SELECT customer_id\u{c}FROM customers WHERE customer_id = ?",
        "SELECT customer_id FROM customers --\r\nWHERE customer_id = ?",
        "SELECT customer_id\u{b}FROM customers WHERE customer_id = ?",
        "SELECT customer_id FROM customers WHERE customer_id = ?\0",
        "SELECT customer_id FROM customers WHERE customer_id = ? AND name <> 'a\r'",
        "SELECT customer_id FROM customers WHERE customer_id = ? \u{1b}",
    ] {
        let mut scene = Scene::new();
        scene.alter("customer_by_key", |statement| {
            statement.text = text.to_owned();
        });
        let mut run = scene.prepare(Posture::Live).await;
        let refused = run
            .steps(
                &scene.binding("example_customer_get").act,
                &inputs(&[("customer_id", "C-1")]),
                None,
            )
            .await;
        assert_eq!(
            failed(refused),
            (
                FailureClass::Policy,
                codes::STATEMENT_NOT_ALLOWED,
                Sent::NotSent
            ),
            "{text:?}"
        );
        run.release().await;
    }
}

#[tokio::test]
async fn a_read_that_returns_no_column_or_is_not_one_select_is_refused_unsent() {
    for text in [
        "PRAGMA query_only = 0",
        "COMMIT",
        "VACUUM INTO ?",
        "SELECT customer_id FROM customers WHERE customer_id = ?; SELECT 1",
        "SELECT customer_id FROM pragma_database_list WHERE ? IS NOT NULL",
        "SELECT load_extension(?) AS customer_id",
    ] {
        let mut scene = Scene::new();
        let folder = Folder::new("guards");
        let copy = folder.path().join("copy.db");
        scene.alter("customer_by_key", |statement| {
            statement.text = text.to_owned();
            if !text.contains('?') {
                statement.params.clear();
            }
        });
        let mut run = scene.prepare(Posture::Live).await;
        let refused = run
            .steps(
                &scene.binding("example_customer_get").act,
                &inputs(&[("customer_id", copy.to_str().unwrap())]),
                None,
            )
            .await;
        assert_eq!(
            failed(refused),
            (
                FailureClass::Policy,
                codes::STATEMENT_NOT_ALLOWED,
                Sent::NotSent
            ),
            "{text}"
        );
        run.release().await;
        assert!(!copy.exists(), "{text}");
    }
}

// ---- what a write reaches ----

#[tokio::test]
async fn a_trigger_named_in_another_case_than_its_table_stops_a_write() {
    let scene = Scene::new();
    // With the stand's own triggers out of the catalog, `customers` has
    // none. The store's owner then writes one, naming the table in capitals.
    scene.db.set_write_log(false).unwrap();
    scene
        .db
        .connect()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER credit_audit AFTER UPDATE ON CUSTOMERS BEGIN
                 UPDATE customers SET notes = 'changed by a trigger'
                 WHERE customer_id = 'C-002' AND NEW.customer_id <> 'C-002';
             END;",
        )
        .unwrap();
    let before = scene
        .db
        .stored_row(Table::Customers, &["C-002"])
        .unwrap()
        .unwrap();
    // Discovery's account of the table names the trigger too.
    let catalog = Sqlite
        .open(
            &StoreAt::File(scene.db.path().to_path_buf()),
            Access::ReadOnly,
        )
        .unwrap()
        .catalog(crate::driver::CATALOG_TABLES)
        .unwrap();
    let customers = catalog
        .tables
        .iter()
        .find(|table| table.name == "customers")
        .unwrap();
    assert_eq!(customers.triggers, ["credit_audit"]);
    let (mut run, write) = scene.live(&["C-001"]).await;
    let stopped = run
        .steps(
            &scene.binding("example_customer_credit_update").act,
            &inputs(&[("customer_id", "C-001")]).with("credit_limit_cents", 1_i64),
            Some(&write),
        )
        .await;
    assert_eq!(
        failed(stopped),
        (FailureClass::Drift, codes::SCHEMA_CHANGED, Sent::NotSent)
    );
    run.release().await;
    // The stand: neither the row named nor the row the trigger reaches.
    let stored = scene
        .db
        .stored_row(Table::Customers, &["C-001"])
        .unwrap()
        .unwrap();
    assert_ne!(stored.integer("credit_limit_cents"), Some(1));
    let after = scene
        .db
        .stored_row(Table::Customers, &["C-002"])
        .unwrap()
        .unwrap();
    assert_eq!(after.text("notes"), before.text("notes"));
}

#[tokio::test]
async fn a_catalog_that_does_not_fit_stops_a_write_that_declares_no_hazard() {
    let scene = Scene::new();
    scene.db.set_write_log(false).unwrap();
    // More tables than one catalog read takes, and after the last of them a
    // table whose key cascades from `customers`.
    let mut schema = String::from("BEGIN;");
    for number in 0..crate::driver::CATALOG_TABLES {
        write!(schema, "CREATE TABLE zz_filler_{number:04} (x);").unwrap();
    }
    schema.push_str(
        "CREATE TABLE zzz_dependents (customer_id TEXT REFERENCES customers (customer_id)
             ON UPDATE CASCADE ON DELETE CASCADE);
         COMMIT;",
    );
    scene.db.connect().unwrap().execute_batch(&schema).unwrap();
    let (mut run, write) = scene.live(&["C-001"]).await;
    let stopped = run
        .steps(
            &scene.binding("example_customer_credit_update").act,
            &inputs(&[("customer_id", "C-001")]).with("credit_limit_cents", 1_i64),
            Some(&write),
        )
        .await;
    assert_eq!(
        failed(stopped),
        (
            FailureClass::UnknownVariant,
            codes::CATALOG_TOO_LARGE,
            Sent::NotSent
        )
    );
    run.release().await;
    let stored = scene
        .db
        .stored_row(Table::Customers, &["C-001"])
        .unwrap()
        .unwrap();
    assert_ne!(stored.integer("credit_limit_cents"), Some(1));
}

#[tokio::test]
async fn a_table_named_in_another_case_than_the_catalogs_is_the_same_table() {
    // The target writes the table's name in capitals. The stand's own
    // triggers are on `customers`, and the statement declares no hazard.
    let mut scene = Scene::new();
    scene.alter("customer_credit_update", |statement| {
        statement.table = Some("CUSTOMERS".to_owned());
    });
    let (mut run, write) = scene.live(&["C-001"]).await;
    let stopped = run
        .steps(
            &scene.binding("example_customer_credit_update").act,
            &inputs(&[("customer_id", "C-001")]).with("credit_limit_cents", 1_i64),
            Some(&write),
        )
        .await;
    assert_eq!(
        failed(stopped),
        (FailureClass::Drift, codes::SCHEMA_CHANGED, Sent::NotSent)
    );
    run.release().await;
    assert_eq!(
        scene.db.write_count(Table::Customers, &["C-001"]).unwrap(),
        0
    );

    // A table the catalog does not hold at all is not "no trigger".
    let catalog = Sqlite
        .open(
            &StoreAt::File(scene.db.path().to_path_buf()),
            Access::ReadOnly,
        )
        .unwrap()
        .catalog(crate::driver::CATALOG_TABLES)
        .unwrap();
    assert_eq!(catalog.reach("no_such_table"), Err(Unseen::NoTable));
    assert!(matches!(catalog.reach("Customers"), Ok((triggers, _)) if triggers > 0));
    // A key that cascades is found whatever case its table was written in.
    assert!(matches!(catalog.reach("ORDERS"), Ok((_, cascades)) if cascades > 0));
}

// ---- the read of a record by its key ----

#[tokio::test]
async fn a_read_by_key_that_returns_another_record_is_stopped() {
    let key = inputs(&[("customer_id", "C-001")]);
    let quoted = "\"customer_id\", \"name\", \"credit_limit_cents\", \"notes\"";

    // A statement that finds some other record: it is one plain read, the
    // store runs it, and its answer is not taken for the record asked for.
    let mut scene = Scene::new();
    scene.alter("customer_by_key", |statement| {
        statement.text = format!(
            "SELECT {quoted} FROM \"customers\" WHERE \"customer_id\" <> ? ORDER BY \"customer_id\" LIMIT 1"
        );
    });
    let mut run = scene.prepare(Posture::Live).await;
    let other = run
        .steps(&scene.binding("example_customer_get").act, &key, None)
        .await;
    assert_eq!(
        failed(other),
        (FailureClass::Drift, codes::READ_NOT_BY_KEY, Sent::Answered)
    );
    run.release().await;

    // A statement that does not return the key cannot be held to it, and
    // is not run.
    let mut scene = Scene::new();
    scene.alter("customer_by_key", |statement| {
        statement.text = "SELECT \"name\", \"credit_limit_cents\", \"notes\" FROM \"customers\" WHERE \"customer_id\" = ?".to_owned();
    });
    let mut run = scene.prepare(Posture::Live).await;
    let keyless = run
        .steps(&scene.binding("example_customer_get").act, &key, None)
        .await;
    assert_eq!(
        failed(keyless),
        (FailureClass::Drift, codes::READ_NOT_BY_KEY, Sent::NotSent)
    );
    run.release().await;

    // A statement declared for another surface is not run on this one.
    let mut scene = Scene::new();
    scene.alter("customer_by_key", |statement| {
        statement.surface = "another_db".to_owned();
    });
    let mut run = scene.prepare(Posture::Live).await;
    let elsewhere = run
        .steps(&scene.binding("example_customer_get").act, &key, None)
        .await;
    assert_eq!(
        failed(elsewhere),
        (
            FailureClass::UnknownVariant,
            codes::UNEXPECTED_STATUS,
            Sent::NotSent
        )
    );
    run.release().await;

    // The pack's own read finds the record, and its key is the one asked.
    let scene = Scene::new();
    let mut run = scene.prepare(Posture::Live).await;
    let found = super::values(
        run.steps(&scene.binding("example_customer_get").act, &key, None)
            .await
            .unwrap(),
    );
    assert_eq!(found.get("customer_id"), Some(&Value::from("C-001")));
    run.release().await;
}

// ---- the file a run was prepared with ----

/// On Windows a file the reader holds open cannot be replaced at its name.
#[cfg(unix)]
#[tokio::test]
async fn a_store_file_replaced_after_prepare_is_never_written() {
    let scene = Scene::new();
    let other = TestDb::start().unwrap();
    let (mut run, write) = scene.live(&["C-001"]).await;
    // Another file is put at the store's name while the run holds its reader.
    std::fs::rename(other.path(), scene.db.path()).unwrap();
    let stopped = run
        .steps(
            &scene.binding("example_customer_credit_update").act,
            &inputs(&[("customer_id", "C-001")]).with("credit_limit_cents", 1_i64),
            Some(&write),
        )
        .await;
    assert_eq!(
        failed(stopped),
        (
            FailureClass::Drift,
            codes::STORE_FILE_REPLACED,
            Sent::NotSent
        )
    );
    run.release().await;
    // The file at the name now, asked through the stand's own connection.
    let stored = scene
        .db
        .stored_row(Table::Customers, &["C-001"])
        .unwrap()
        .unwrap();
    assert_ne!(stored.integer("credit_limit_cents"), Some(1));
    assert_eq!(
        scene.db.write_count(Table::Customers, &["C-001"]).unwrap(),
        0
    );
}
