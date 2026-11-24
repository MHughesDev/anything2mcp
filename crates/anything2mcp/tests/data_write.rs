//! Package G3, writes through the data pack, through the product: a write
//! of a line in a relational store is recorded first, made once inside one
//! transaction that must change exactly one row, and read back by its key;
//! when the program was ended after the store had the write and before
//! anyone saw its answer, the resume reconciles and never writes again. A
//! stale approval writes nothing, and bad input starts nothing. Every claim
//! about what the store holds is asked of the store itself, through the
//! stand's own client, never of the pack's read path alone.
//!
//! It runs against an embedded database file, and, when this machine has the
//! PostgreSQL programs and the build has the pack's feature `postgres`,
//! against a real server (with `A2M_REQUIRE_POSTGRES=1` a missing server
//! fails each test instead of skipping it).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::io::Read as _;
use std::sync::Arc;
use std::time::Duration;

use a2m_actions::PresenceProof;
use a2m_core::{Decimal, RunId, Timestamp};
use a2m_policy::{Channel, PresenceCheck};
use a2m_telemetry::{Clock, ManualClock, SystemClock};
use common::{Folder, Machine, World};
use example_db::harness::TestDb;
use example_db::schema::Table;
use example_db::server::{APP_PASSWORD, RelayFault, TestServer};
use serde_json::Value as Json;

const UPDATE: &str = "example_line_item_update";
const GET: &str = "example_line_item_get";
const LINE: [&str; 2] = ["A-100", "L-101"];
/// How long a test waits for something that should come at once. Only a
/// failure waits this long.
const PATIENCE: Duration = Duration::from_secs(180);

/// What the line holds before any test writes.
const SEED_LENGTH: &str = "150.5";
const SEED_PRICE: &str = "12.50";

/// One of the two stores, with the stand's own client.
enum Stand {
    File(Box<TestDb>),
    Server(Box<TestServer>),
}

/// What a line holds, as the store prints it.
struct Held {
    length: String,
    price: String,
    version: i64,
}

fn same_number(left: &str, right: &str) -> bool {
    Decimal::parse(left).unwrap() == Decimal::parse(right).unwrap()
}

impl Stand {
    fn name(&self) -> &'static str {
        match self {
            Self::File(_) => "file",
            Self::Server(_) => "server",
        }
    }

    /// The line, as the store's own client reads it.
    fn held(&self) -> Held {
        match self {
            Self::File(db) => {
                let row = db.stored_row(Table::OrderLines, &LINE).unwrap().unwrap();
                Held {
                    length: row.text("length_in").unwrap().to_owned(),
                    price: row.text("unit_price").unwrap().to_owned(),
                    version: row.integer("row_version").unwrap(),
                }
            }
            Self::Server(server) => {
                let row = server
                    .stored_row(
                        "order_lines",
                        &[("order_id", LINE[0]), ("item_code", LINE[1])],
                    )
                    .unwrap()
                    .unwrap();
                let text = |column: &str| row.get(column).unwrap().clone().unwrap();
                Held {
                    length: text("length_in"),
                    price: text("unit_price"),
                    version: text("row_version").parse().unwrap(),
                }
            }
        }
    }

    /// How many times the store's log saw the line written, by anyone.
    /// What the file's log counts of a change made "as someone else" is
    /// left out, so [`Stand::someone_else_changes_the_price`] is counted
    /// the same way on both.
    fn writes_of_the_line(&self) -> usize {
        match self {
            Self::File(db) => db.write_count(Table::OrderLines, &LINE).unwrap(),
            Self::Server(server) => {
                usize::try_from(server.write_count("order_lines", &LINE).unwrap()).unwrap()
            }
        }
    }

    /// Every write the store's log holds, of any table.
    fn all_writes(&self) -> usize {
        match self {
            Self::File(db) => db.writes().unwrap().len(),
            Self::Server(server) => server.writes().unwrap().len(),
        }
    }

    /// Somebody else changes the price of the line, after the person
    /// approved a diff against what it held.
    fn someone_else_changes_the_price(&self) {
        let sql = "UPDATE order_lines SET unit_price = 99.00 WHERE order_id = 'A-100' AND item_code = 'L-101'";
        match self {
            Self::File(db) => {
                assert_eq!(db.someone_else(sql, &[]).unwrap(), 1);
            }
            Self::Server(server) => server.execute(sql).unwrap(),
        }
    }

    /// For the server, the relay keeps the answer to the commit from the
    /// program, so that the program is ended while it waits. The file has
    /// no such relay: the program is ended as soon as the file has the
    /// write, which is any time between its commit and its end.
    fn hold_the_answer(&self) {
        if let Self::Server(server) = self {
            server.arm(RelayFault::HoldCommitAnswer);
        }
    }

    /// Puts the stand back to what it held before any write.
    fn start_again(&self) {
        if let Self::File(db) = self {
            db.reset().unwrap();
        }
    }

    /// Whether the answer the program waits for is the one held back.
    fn held_back(&self) -> bool {
        match self {
            Self::File(_) => true,
            Self::Server(server) => server.fault_met(),
        }
    }
}

/// The product's machine for a stand, and the stand. `None` when this
/// machine cannot run the stand, or the build has no store for it.
struct Rig {
    stand: Stand,
    machine: Machine,
}

impl Rig {
    fn file() -> Self {
        // The project folder is the folder the stand keeps its file in.
        let db = TestDb::start().unwrap();
        let folder = Folder::of_another(db.path().parent().unwrap());
        let file = db.path().file_name().unwrap().to_str().unwrap().to_owned();
        let machine = Machine::in_folder("storewrite", &[("example.db", &file)], folder);
        Self {
            stand: Stand::File(Box::new(db)),
            machine,
        }
    }

    fn server() -> Option<Self> {
        // The server store is a feature of the data pack; a build without it
        // has no such surface kind, and says so here, not in a lint.
        let built =
            a2m_provider::Pack::manifest(&a2m_pack_data::pack()).contains("database_server");
        if !built {
            assert!(
                std::env::var("A2M_REQUIRE_POSTGRES").map_or(true, |value| value != "1"),
                "A2M_REQUIRE_POSTGRES=1, and this build has no server store (feature `postgres` of the data pack)"
            );
            eprintln!("skipped: this build has no server store");
            return None;
        }
        let Some(server) = TestServer::start().unwrap() else {
            eprintln!("skipped: this machine has no PostgreSQL server for the stand");
            return None;
        };
        let address = server.address();
        let machine = Machine::new(
            "serverwrite",
            &[
                ("port: 5432", &format!("port: {}", address.port())),
                (
                    "database: example",
                    &format!("database: {}", server.database()),
                ),
            ],
        );
        Some(Self {
            stand: Stand::Server(Box::new(server)),
            machine,
        })
    }

    /// The same, for a real program that is killed: its store on disk, its
    /// vault's key from a passphrase.
    fn killable(self) -> Self {
        Self {
            machine: self.machine.with_passphrase(),
            ..self
        }
    }

    fn on_disk(self) -> Self {
        Self {
            machine: self.machine.on_disk(),
            ..self
        }
    }
}

/// Opens the machine's engine, publishes, and, for the server, stores the
/// role's password.
async fn published(rig: &Rig) -> World {
    let world = rig.machine.open().await;
    if matches!(rig.stand, Stand::Server(_)) {
        let stored = world
            .agent(&[
                "secret",
                "set",
                "example/service/password",
                "--value",
                APP_PASSWORD,
            ])
            .await;
        assert_eq!(stored.finished.code, 0, "{:?}", stored.finished);
    }
    let linted = world.ok(&["draft", "lint", "example"]).await;
    assert_eq!(linted["ok"], true, "{linted}");
    let tested = world.agent(&["test", "example"]).await;
    assert_eq!(tested.finished.code, 0, "{}", tested.json);
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(
        published.finished.code, 0,
        "{:?} {}",
        published.finished, published.json
    );
    world
}

/// The person approves, at the desk, the diff they were shown.
async fn approve(world: &World, run: &str) {
    let run_id: RunId = run.parse().unwrap();
    let asked = world.surface.last_ask();
    assert_eq!(asked.run, run_id);
    let person = world
        .engine
        .caller(a2m_actions::FrontDoor::Command, false)
        .principal
        .principal()
        .clone();
    let proof = PresenceProof::new(PresenceCheck::OsPrompt, person.id, world.clock.now());
    world
        .surface
        .desk()
        .approve(run_id, asked.diff_hash, person, Channel::LocalPrompt, proof)
        .await
        .unwrap();
}

/// A write that rested for approval, and the person approved it.
async fn approved(world: &World, input: &str) -> String {
    let rested = world
        .agent(&["tool", "call", UPDATE, "--input", input])
        .await;
    assert_eq!(
        rested.result()["status"],
        "needs_approval",
        "{}",
        rested.json
    );
    let run = rested.result()["run_id"].as_str().unwrap().to_owned();
    approve(world, &run).await;
    run
}

/// The input of the write: the line, and its new length and price.
fn input(length: &str, price: &str) -> String {
    format!(
        r#"{{"order_id": "{}", "item_code": "{}", "length": "{length}", "unit_price": "{price}"}}"#,
        LINE[0], LINE[1]
    )
}

fn kinds(trace: &Json) -> Vec<&str> {
    trace["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["kind"].as_str().unwrap())
        .collect()
}

fn assert_seed(stand: &Stand) {
    let held = stand.held();
    assert!(same_number(&held.length, SEED_LENGTH), "{}", held.length);
    assert!(same_number(&held.price, SEED_PRICE), "{}", held.price);
    assert_eq!(held.version, 1);
}

async fn an_approved_write_is_made_once_and_read_back(rig: Rig) {
    let world = published(&rig).await;
    let stand = &rig.stand;
    assert_seed(stand);

    let run = approved(&world, &input("151.25", "13.10")).await;
    // Nothing was written before the person approved, and nothing by
    // approving.
    assert_eq!(stand.writes_of_the_line(), 0, "{}", stand.name());
    assert_seed(stand);

    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    // The store's own client: one write, with the values, and the version
    // the store raised.
    assert_eq!(stand.writes_of_the_line(), 1, "{}", stand.name());
    let held = stand.held();
    assert!(same_number(&held.length, "151.25"), "{}", held.length);
    assert!(same_number(&held.price, "13.10"), "{}", held.price);
    assert_eq!(held.version, 2);
    // The answer says what was read back by the key.
    let result = &done.result()["result"];
    assert!(
        same_number(result["unit_price"].as_str().unwrap(), "13.10"),
        "{}",
        done.json
    );

    let trace = world.ok(&["trace", "show", &run]).await;
    let kinds = kinds(&trace);
    let at = |kind: &str| {
        kinds
            .iter()
            .position(|found| *found == kind)
            .unwrap_or_else(|| panic!("no {kind} in {kinds:?}"))
    };
    assert!(at("approval_bound") < at("intent"));
    assert!(at("intent") < at("acted"));
    assert!(at("acted") < at("verified"), "{kinds:?}");

    // The run again changes nothing.
    let again = world.agent(&["run", "resume", &run]).await;
    assert_eq!(again.result()["status"], "completed", "{}", again.json);
    assert_eq!(stand.writes_of_the_line(), 1, "{}", stand.name());
    world.close().await;
}

/// Waits until the store has the write and the answer the program waits
/// for is held back, or the program ended.
async fn stored_and_held(stand: &Stand, program: &mut std::process::Child) {
    let give_up = std::time::Instant::now() + PATIENCE;
    loop {
        if stand.writes_of_the_line() >= 1 && stand.held_back() {
            return;
        }
        if let Some(ended) = program.try_wait().unwrap() {
            let mut said = String::new();
            for mut pipe in [
                program
                    .stdout
                    .take()
                    .map(|p| Box::new(p) as Box<dyn std::io::Read>),
                program
                    .stderr
                    .take()
                    .map(|p| Box::new(p) as Box<dyn std::io::Read>),
            ]
            .into_iter()
            .flatten()
            {
                let _ = pipe.read_to_string(&mut said);
            }
            panic!("the program ended ({ended}) before the store had the write: {said}");
        }
        assert!(
            std::time::Instant::now() < give_up,
            "the store never had the write"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

async fn a_program_killed_after_the_write_was_sent_is_reconciled_and_writes_no_second_time(
    rig: Rig,
) {
    let world = published(&rig).await;
    let stand = &rig.stand;
    world.close().await;

    // The machine is started again twenty minutes after the program died:
    // the dead process's lease has ended.
    let later = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(
        SystemClock::new().now().as_unix_millis() + 1_200_000,
    )));
    let mut world = None;
    let mut run = String::new();
    // The server's relay holds the answer, so the first try is the one. The
    // file gives no such hold, and a program that finished before the end
    // came is tried again on a stand put back.
    for attempt in 0..40 {
        let asking = rig.machine.open().await;
        run = approved(&asking, &input("151.25", "13.10")).await;
        asking.close().await;

        // The real program resumes the run. The store has the write, the
        // program has not seen the answer, and the program is ended.
        stand.hold_the_answer();
        let mut program = rig
            .machine
            .program(&["run", "resume", &run])
            .spawn()
            .unwrap();
        stored_and_held(stand, &mut program).await;
        program.kill().unwrap();
        program.wait().unwrap();
        assert_eq!(stand.writes_of_the_line(), 1, "{}", stand.name());
        let held = stand.held();
        assert!(same_number(&held.price, "13.10"), "{}", held.price);
        assert_eq!(held.version, 2);

        // Did the program finish what it was doing before it was ended?
        let looking = rig.machine.open().await;
        let seen = looking.agent(&["run", "get", &run]).await;
        let finished = seen.result()["status"] == "completed";
        looking.close().await;
        if !finished {
            let opened = rig
                .machine
                .open_at(Some(Arc::clone(&later) as Arc<dyn Clock>))
                .await;
            opened.engine.kernel().tend().await;
            let left = opened.agent(&["run", "get", &run]).await;
            // The run says a write may have gone through.
            assert_eq!(left.result()["status"], "stopped", "{}", left.json);
            assert_eq!(left.error()["class"], "possible_commit", "{}", left.json);
            assert_eq!(left.error()["retry_safe"], false, "{}", left.json);
            eprintln!("the program was ended in doubt at try {}", attempt + 1);
            world = Some(opened);
            break;
        }
        assert!(
            matches!(stand, Stand::File(_)) && attempt < 39,
            "the program finished before it was ended: {}",
            seen.json
        );
        stand.start_again();
    }
    let world = world.expect("a program was ended after the store had the write");

    // Once the quarantine has passed, resuming reconciles the record by
    // its key before anything is sent, finds the write, and completes the
    // run without writing again.
    later.advance(Duration::from_secs(120));
    world.engine.kernel().tend().await;
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    assert_eq!(stand.writes_of_the_line(), 1, "{}", stand.name());
    assert_eq!(stand.held().version, 2);
    let trace = world.ok(&["trace", "show", &run]).await;
    let kinds = kinds(&trace);
    assert_eq!(
        kinds.iter().filter(|kind| **kind == "intent").count(),
        1,
        "{kinds:?}"
    );
    assert!(kinds.contains(&"reconcile_read"), "{kinds:?}");
    world.close().await;
}

async fn a_stale_approval_writes_nothing(rig: Rig) {
    let world = published(&rig).await;
    let stand = &rig.stand;
    let run = approved(&world, &input("151.25", "13.10")).await;

    // Somebody changes the line after the person approved the diff
    // against the line as it was.
    stand.someone_else_changes_the_price();
    let after_them = stand.writes_of_the_line();
    let held = stand.held();
    assert!(same_number(&held.price, "99.00"), "{}", held.price);
    assert_eq!(held.version, 2);

    let stale = world.agent(&["run", "resume", &run]).await;
    assert_ne!(stale.result()["status"], "completed", "{}", stale.json);
    assert_eq!(stale.error()["class"], "stale", "{}", stale.json);
    // The store holds what the other person wrote, and nothing else came.
    assert_eq!(stand.writes_of_the_line(), after_them, "{}", stand.name());
    let held = stand.held();
    assert!(same_number(&held.price, "99.00"), "{}", held.price);
    assert!(same_number(&held.length, SEED_LENGTH), "{}", held.length);
    assert_eq!(held.version, 2);
    world.close().await;
}

async fn bad_input_starts_nothing(rig: Rig) {
    let world = published(&rig).await;
    let stand = &rig.stand;
    let before = stand.all_writes();

    // A length that is no number, a price that is no money, and a key with a part missing: refused before anything rests or is
    // sent.
    for bad in [
        input("long", "13.10"),
        input("151.25", "cheap"),
        r#"{"order_id": "A-100", "length": "151.25", "unit_price": "13.10"}"#.to_owned(),
    ] {
        let ran = world
            .agent(&["tool", "call", UPDATE, "--input", &bad])
            .await;
        assert_ne!(
            ran.result()["status"],
            "needs_approval",
            "{bad}: {}",
            ran.json
        );
        assert_ne!(ran.finished.code, 0, "{bad}: {}", ran.json);
        assert_eq!(ran.error()["class"], "input", "{bad}: {}", ran.json);
    }
    assert_eq!(stand.all_writes(), before, "{}", stand.name());
    assert_seed(stand);

    // A key no row has, and a key that is text with quotes in it, which the
    // store takes as a value and never as a statement: the person is shown
    // the write, or it is refused, and the store holds what it held either
    // way, with its tables.
    for key in ["L-999", "L-101'; DROP TABLE order_lines; --"] {
        let missing = input("151.25", "13.10").replace("L-101", key);
        let ran = world
            .agent(&["tool", "call", UPDATE, "--input", &missing])
            .await;
        if ran.result()["status"] == "needs_approval" {
            let run = ran.result()["run_id"].as_str().unwrap().to_owned();
            approve(&world, &run).await;
            let resumed = world.agent(&["run", "resume", &run]).await;
            assert_ne!(resumed.result()["status"], "completed", "{}", resumed.json);
        } else {
            assert_ne!(ran.result()["status"], "completed", "{}", ran.json);
        }
        assert_eq!(stand.all_writes(), before, "{}", stand.name());
        assert_seed(stand);
    }

    // The read of the same line still answers, and the store's own client
    // agrees with it.
    let read = world
        .agent(&[
            "tool",
            "call",
            GET,
            "--input",
            &format!(
                r#"{{"order_id": "{}", "item_code": "{}"}}"#,
                LINE[0], LINE[1]
            ),
        ])
        .await;
    assert_eq!(read.result()["status"], "completed", "{}", read.json);
    assert!(
        same_number(
            read.result()["result"]["unit_price"].as_str().unwrap(),
            SEED_PRICE
        ),
        "{}",
        read.json
    );
    world.close().await;
}

// ---- on the embedded file ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_an_approved_write_is_made_once_and_read_back() {
    an_approved_write_is_made_once_and_read_back(Rig::file().on_disk()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_a_program_killed_after_the_write_was_sent_is_reconciled_and_writes_no_second_time() {
    a_program_killed_after_the_write_was_sent_is_reconciled_and_writes_no_second_time(
        Rig::file().killable(),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_a_stale_approval_writes_nothing() {
    a_stale_approval_writes_nothing(Rig::file().on_disk()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_bad_input_starts_nothing() {
    bad_input_starts_nothing(Rig::file().on_disk()).await;
}

// ---- on a PostgreSQL server ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_an_approved_write_is_made_once_and_read_back() {
    if let Some(rig) = Rig::server() {
        an_approved_write_is_made_once_and_read_back(rig.on_disk()).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_a_program_killed_after_the_write_was_sent_is_reconciled_and_writes_no_second_time()
{
    if let Some(rig) = Rig::server() {
        a_program_killed_after_the_write_was_sent_is_reconciled_and_writes_no_second_time(
            rig.killable(),
        )
        .await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_a_stale_approval_writes_nothing() {
    if let Some(rig) = Rig::server() {
        a_stale_approval_writes_nothing(rig.on_disk()).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_bad_input_starts_nothing() {
    if let Some(rig) = Rig::server() {
        bad_input_starts_nothing(rig.on_disk()).await;
    }
}
