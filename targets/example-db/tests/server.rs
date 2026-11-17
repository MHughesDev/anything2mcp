//! The server side of the stand: the schema and seed on a real server, the
//! relay, and its two faults. Each case is proven with the server's own
//! client, `psql`, through the relay, and by the control queries.
//!
//! A machine with no server says each case was skipped; with
//! `A2M_REQUIRE_POSTGRES=1` a missing server fails instead.

use std::error::Error;
use std::process::{Command, Output};

use example_db::server::{APP_PASSWORD, APP_ROLE, OWNER_ROLE, RelayFault, TestServer};

type Outcome = Result<(), Box<dyn Error>>;

fn server() -> Result<Option<TestServer>, Box<dyn Error>> {
    let server = TestServer::start()?;
    if server.is_none() {
        eprintln!("skipped: this machine has no PostgreSQL server for the stand");
    }
    Ok(server)
}

/// The server's own client, as the app role, through the relay.
fn as_app(server: &TestServer, sql: &str) -> std::io::Result<Output> {
    let address = server.address();
    Command::new("psql")
        .arg("-X")
        .arg("-At")
        .arg("-v")
        .arg("ON_ERROR_STOP=1")
        .arg("-d")
        .arg(format!(
            "host={} port={} user={APP_ROLE} password={APP_PASSWORD} dbname={} sslmode=disable",
            address.ip(),
            address.port(),
            server.database()
        ))
        .arg("-c")
        .arg(sql)
        .output()
}

#[test]
fn the_schema_and_seed_are_there_and_the_tables_belong_to_the_owner() -> Outcome {
    let Some(server) = server()? else {
        return Ok(());
    };
    let row = server
        .stored_row(
            "order_lines",
            &[("order_id", "A-100"), ("item_code", "L-101")],
        )?
        .ok_or("no seeded line")?;
    assert_eq!(row["length_in"].as_deref(), Some("150.5000"));
    assert_eq!(row["unit_price"].as_deref(), Some("12.50"));
    assert_eq!(row["row_version"].as_deref(), Some("1"));
    let owners =
        server.query("SELECT DISTINCT tableowner FROM pg_tables WHERE schemaname = 'public'")?;
    assert_eq!(owners, vec![vec![Some(OWNER_ROLE.to_owned())]]);
    assert_eq!(server.write_count("order_lines", &["A-100", "L-101"])?, 0);
    Ok(())
}

#[test]
fn the_app_role_writes_through_the_relay_and_the_log_counts_it() -> Outcome {
    let Some(server) = server()? else {
        return Ok(());
    };
    let out = as_app(
        &server,
        "UPDATE order_lines SET unit_price = 13.10 WHERE order_id = 'A-100' AND item_code = 'L-101'",
    )?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let row = server
        .stored_row(
            "order_lines",
            &[("order_id", "A-100"), ("item_code", "L-101")],
        )?
        .ok_or("no line")?;
    assert_eq!(row["unit_price"].as_deref(), Some("13.10"));
    assert_eq!(row["row_version"].as_deref(), Some("2"));
    assert_eq!(server.write_count("order_lines", &["A-100", "L-101"])?, 1);
    // The app role cannot change the schema.
    let out = as_app(&server, "DROP TABLE order_lines")?;
    assert!(!out.status.success());
    Ok(())
}

#[test]
fn a_lost_answer_to_a_commit_leaves_the_write_landed() -> Outcome {
    let Some(server) = server()? else {
        return Ok(());
    };
    server.arm(RelayFault::LoseCommitAnswer);
    let out = as_app(
        &server,
        "BEGIN; UPDATE order_lines SET quantity = 3 WHERE order_id = 'A-100' AND item_code = 'L-101'; COMMIT",
    )?;
    assert!(
        !out.status.success(),
        "the client should have lost the answer"
    );
    assert!(server.fault_met());
    assert_eq!(server.write_count("order_lines", &["A-100", "L-101"])?, 1);
    let row = server
        .stored_row(
            "order_lines",
            &[("order_id", "A-100"), ("item_code", "L-101")],
        )?
        .ok_or("no line")?;
    assert_eq!(row["quantity"].as_deref(), Some("3"));
    Ok(())
}

#[test]
fn a_dropped_commit_leaves_nothing_written() -> Outcome {
    let Some(server) = server()? else {
        return Ok(());
    };
    server.arm(RelayFault::DropCommit);
    let out = as_app(
        &server,
        "BEGIN; UPDATE order_lines SET quantity = 3 WHERE order_id = 'A-100' AND item_code = 'L-101'; COMMIT",
    )?;
    assert!(!out.status.success());
    assert!(server.fault_met());
    assert_eq!(server.write_count("order_lines", &["A-100", "L-101"])?, 0);
    Ok(())
}

#[test]
fn a_held_lock_is_held_until_its_holder_is_dropped() -> Outcome {
    let Some(server) = server()? else {
        return Ok(());
    };
    let holder = server.hold_lock("order_lines", "order_id = 'A-100'")?;
    let out = as_app(
        &server,
        "SET lock_timeout = '200ms'; UPDATE order_lines SET quantity = 3 WHERE order_id = 'A-100' AND item_code = 'L-101'",
    )?;
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("lock"));
    drop(holder);
    assert_eq!(server.write_count("order_lines", &["A-100", "L-101"])?, 0);
    assert_eq!(server.terminate(APP_ROLE)?, 0);
    Ok(())
}
