//! Temporary probe of the bundled engine, before the fix.

use std::path::Path;
use std::time::Duration;

use example_db::harness::TestDb;

use super::Folder;
use crate::model::{Access, Isolation};
use crate::sqlite::Sqlite;
use crate::store::{Bound, Opener, RunLimits};

#[test]
fn probe() {
    let db = TestDb::start().unwrap();
    let other = TestDb::start().unwrap();
    let folder = Folder::new("guards");
    let limits = RunLimits {
        rows: 10,
        bytes: 1_000_000,
        wait: Duration::from_secs(5),
    };
    let path = |p: &Path| Bound::Text(p.to_str().unwrap().to_owned());
    for in_tx in [false, true] {
        let mut conn = Sqlite.open(db.path(), Access::ReadOnly).unwrap();
        if in_tx {
            conn.begin(Isolation::Serializable, Duration::from_secs(1)).unwrap();
        }
        let copy = folder.path().join(format!("copy-{in_tx}.db"));
        println!("in_tx={in_tx}");
        println!(" describe attach: {:?}", conn.describe("ATTACH ? AS x"));
        println!(" run attach: {:?}", conn.run("ATTACH ? AS x", &[path(other.path())], &limits).map(|r| r.rows.len()));
        println!(" read attached: {:?}", conn.run("SELECT name FROM x.customers", &[], &limits).map(|r| r.rows.len()));
        println!(" vacuum into (query_only on): {:?} exists={}", conn.run("VACUUM INTO ?", &[path(&copy)], &limits).map(|r| r.rows.len()), copy.exists());
        let _ = std::fs::remove_file(&copy);
        println!(" pragma query_only=0: {:?}", conn.run("PRAGMA query_only = 0", &[], &limits).map(|r| r.rows.len()));
        println!(" vacuum into: {:?} exists={} len={:?}", conn.run("VACUUM INTO ?", &[path(&copy)], &limits).map(|r| r.rows.len()), copy.exists(), std::fs::metadata(&copy).map(|m| m.len()));
        println!(" load_extension: {:?}", conn.run("SELECT load_extension('x')", &[], &limits).map(|r| r.rows.len()));
        println!(" dq literal: {:?}", conn.run("SELECT \"no_such_column\" FROM customers", &[], &limits).map(|r| (r.columns, r.rows.len())));
        println!(" two statements: {:?}", conn.describe("SELECT 1; SELECT 2"));
        println!(" pragma fn: {:?}", conn.run("SELECT file FROM pragma_database_list", &[], &limits).map(|r| r.rows));
        println!(" update returning: {:?}", conn.run("UPDATE customers SET name = name RETURNING name", &[], &limits).map(|r| r.rows.len()));
        println!(" create temp: {:?}", conn.run("CREATE TEMP TABLE t(x)", &[], &limits).map(|r| r.rows.len()));
        println!(" commit: {:?}", conn.run("COMMIT", &[], &limits).map(|r| r.rows.len()));
    }
}
