//! Package D5, writes through the command pack, through the product: a
//! write of a command-line program is recorded first, read back through a
//! second command, and, when the program was ended before anyone saw its
//! answer, reconciled on resume and never started twice. A command with
//! nothing to read back is declared irreversible, approved as that, and
//! never called verified. Every claim about what the program stored is
//! asked of the stand's own files, never of the pack's read path alone.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::path::Path;

use a2m_actions::PresenceProof;
use a2m_core::RunId;
use a2m_policy::{Channel, PresenceCheck};
use common::{Machine, World};
use example_cli::faults::{Fault, FaultKind, StopHow, StopPoint};
use example_cli::harness::Stand;
use serde_json::Value as Json;

const SET: &str = "example_item_set_quantity";
const PUSH: &str = "example_catalog_push";
const SKU: &str = "SK-1001";
/// What the stand holds of that item before any test writes.
const SEED: i64 = 120;

fn quoted(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "''"))
}

/// The project, with the stand's program at the path the target names and
/// a time limit short enough for a program that hangs to be ended at it.
fn machine(stand: &Stand) -> Machine {
    let digest = a2m_core::Digest::of(&std::fs::read(stand.program()).unwrap());
    let platform = a2m_provider::current_platform();
    Machine::new(
        "stockcmd",
        &[
            ("'/opt/example/bin/example-cli'", &quoted(stand.program())),
            ("'/var/lib/example/home'", &quoted(stand.home())),
            (
                "linux-x86_64: [\"sha256:1111111111111111111111111111111111111111111111111111111111111111\"]",
                &format!("{platform}: [\"sha256:{digest}\"]"),
            ),
            ("timeout: 20s", "timeout: 4s"),
        ],
    )
    .on_disk()
}

async fn published(machine: &Machine) -> World {
    let world = machine.open().await;
    let linted = world.ok(&["draft", "lint", "example"]).await;
    assert_eq!(linted["ok"], true, "{linted}");
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
async fn approved(world: &World, tool: &str, input: &str) -> String {
    let rested = world.agent(&["tool", "call", tool, "--input", input]).await;
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

fn set_input(quantity: i64) -> String {
    format!(r#"{{"sku": "{SKU}", "quantity": {quantity}}}"#)
}

fn kinds(trace: &Json) -> Vec<&str> {
    trace["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["kind"].as_str().unwrap())
        .collect()
}

fn stored(stand: &Stand) -> i64 {
    stand
        .control()
        .stored_item(SKU)
        .unwrap()
        .unwrap()
        .quantity
        .into()
}

/// A fault on the one start of the write command.
fn on_the_write(at: StopPoint, how: StopHow) -> Fault {
    Fault::once(FaultKind::stop(at, how)).on("item set")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_write_is_approved_made_once_and_read_back_by_the_program_stand() {
    let stand = Stand::start().unwrap();
    let machine = machine(&stand);
    let world = published(&machine).await;
    assert_eq!(stored(&stand), SEED);

    let run = approved(&world, SET, &set_input(12)).await;
    // Nothing was written before the person approved.
    assert_eq!(stand.control().write_count(SKU).unwrap(), 0);
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.result()["status"], "completed", "{}", done.json);

    // The stand's own files: one write, with the value.
    assert_eq!(stand.control().write_count(SKU).unwrap(), 1);
    assert_eq!(stored(&stand), 12);
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
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_program_ended_at_its_limit_after_it_stored_the_write_is_reconciled_and_not_started_twice()
 {
    let stand = Stand::start().unwrap();
    let machine = machine(&stand);
    let world = published(&machine).await;
    let run = approved(&world, SET, &set_input(12)).await;

    // The program stores the write and then hangs; the supervisor ends it
    // at the time limit, and nothing confirms what it did.
    stand
        .control()
        .set_fault(&on_the_write(StopPoint::AfterCommit, StopHow::Hang))
        .unwrap();
    let lost = world.agent(&["run", "resume", &run]).await;
    assert_eq!(lost.result()["status"], "stopped", "{}", lost.json);
    assert_eq!(lost.error()["class"], "possible_commit", "{}", lost.json);
    assert_eq!(lost.error()["retry_safe"], false, "{}", lost.json);
    assert_eq!(stand.control().write_count(SKU).unwrap(), 1);
    assert_eq!(stored(&stand), 12);

    // The resume reads the record first, finds the write, and starts the
    // write command no more.
    let starts = |stand: &Stand| {
        stand
            .control()
            .invocations()
            .unwrap()
            .iter()
            .filter(|start| start.args.iter().any(|arg| arg == "set"))
            .count()
    };
    assert_eq!(starts(&stand), 1);
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    assert_eq!(stand.control().write_count(SKU).unwrap(), 1);
    assert_eq!(starts(&stand), 1);
    let trace = world.ok(&["trace", "show", &run]).await;
    let kinds = kinds(&trace);
    assert_eq!(kinds.iter().filter(|kind| **kind == "intent").count(), 1);
    assert!(kinds.contains(&"reconcile_read"), "{kinds:?}");
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_program_ended_before_it_stored_the_write_is_reconciled_and_the_write_is_made_once() {
    let stand = Stand::start().unwrap();
    let machine = machine(&stand);
    let world = published(&machine).await;
    let run = approved(&world, SET, &set_input(12)).await;

    stand
        .control()
        .set_fault(&on_the_write(StopPoint::BeforeCommit, StopHow::Abort))
        .unwrap();
    let lost = world.agent(&["run", "resume", &run]).await;
    assert_eq!(lost.error()["class"], "possible_commit", "{}", lost.json);
    // The same answer as after the commit; only the stand tells them apart.
    assert_eq!(stand.control().write_count(SKU).unwrap(), 0);
    assert_eq!(stored(&stand), SEED);

    // The reconcile reads the record, finds it unwritten, and the resume
    // writes it, once.
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    assert_eq!(stand.control().write_count(SKU).unwrap(), 1);
    assert_eq!(stored(&stand), 12);
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bad_input_starts_no_program_and_a_program_that_refuses_the_value_writes_nothing() {
    let stand = Stand::start().unwrap();
    let machine = machine(&stand);
    let world = published(&machine).await;

    // A value that is not a whole number, and a code that does not match
    // the pattern: refused before anything rests or starts.
    for input in [
        r#"{"sku": "SK-1001", "quantity": "twelve"}"#,
        r#"{"sku": "sk 1001; rm", "quantity": 12}"#,
        r#"{"quantity": 12}"#,
    ] {
        let bad = world.agent(&["tool", "call", SET, "--input", input]).await;
        assert_ne!(bad.result()["status"], "needs_approval", "{}", bad.json);
        assert_ne!(bad.finished.code, 0, "{}", bad.json);
        assert_eq!(bad.error()["class"], "input", "{}", bad.json);
    }
    // No start carried a word of the refused input.
    for start in stand.control().invocations().unwrap() {
        let said = start.args.join(" ");
        assert!(!said.contains("twelve") && !said.contains("rm"), "{said}");
        assert!(!start.args.iter().any(|arg| arg == "set"), "{said}");
    }
    assert_eq!(stand.control().write_count(SKU).unwrap(), 0);

    // A value the program's own rule refuses: the person approved it, the
    // program said no, and the stand holds what it held.
    let run = approved(&world, SET, &set_input(99_999)).await;
    let refused = world.agent(&["run", "resume", &run]).await;
    assert_ne!(refused.result()["status"], "completed", "{}", refused.json);
    assert_eq!(refused.error()["class"], "rejected", "{}", refused.json);
    assert_eq!(stand.control().write_count(SKU).unwrap(), 0);
    assert_eq!(stored(&stand), SEED);
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_approval_made_stale_at_the_stand_writes_nothing() {
    let stand = Stand::start().unwrap();
    let machine = machine(&stand);
    let world = published(&machine).await;
    let run = approved(&world, SET, &set_input(12)).await;

    // Somebody changes the item with the program itself, after the person
    // approved the diff against the quantity of 4.
    let changed = stand
        .run(&["item", "set", SKU, "--quantity=7", "--yes"])
        .unwrap();
    assert_eq!(changed.code, Some(0), "{changed:?}");
    assert_eq!(stand.control().write_count(SKU).unwrap(), 1);

    let stale = world.agent(&["run", "resume", &run]).await;
    assert_ne!(stale.result()["status"], "completed", "{}", stale.json);
    assert_eq!(stale.error()["class"], "stale", "{}", stale.json);
    // The stand holds what the person there wrote, and nothing else came.
    assert_eq!(stand.control().write_count(SKU).unwrap(), 1);
    assert_eq!(stored(&stand), 7);
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_command_with_nothing_to_read_back_is_approved_as_irreversible_and_never_called_verified()
{
    let stand = Stand::start().unwrap();
    let machine = machine(&stand);
    let world = published(&machine).await;
    assert!(stand.control().pushes().unwrap().is_empty());

    let run = approved(&world, PUSH, "{}").await;
    assert!(stand.control().pushes().unwrap().is_empty());
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    assert_eq!(stand.control().pushes().unwrap().len(), 1);

    // Acknowledged, not verified: the ledger has no verified record.
    let trace = world.ok(&["trace", "show", &run]).await;
    let kinds = kinds(&trace);
    assert!(kinds.contains(&"acted"), "{kinds:?}");
    assert!(!kinds.contains(&"verified"), "{kinds:?}");
    assert!(
        !done.json.to_string().contains("\"verified\":true"),
        "{}",
        done.json
    );

    // The run again changes nothing, and a second call asks for a second
    // approval of its own.
    let again = world.agent(&["run", "resume", &run]).await;
    assert_eq!(again.result()["status"], "completed", "{}", again.json);
    assert_eq!(stand.control().pushes().unwrap().len(), 1);
    let next = world.agent(&["tool", "call", PUSH, "--input", "{}"]).await;
    assert_eq!(next.result()["status"], "needs_approval", "{}", next.json);
    assert_eq!(stand.control().pushes().unwrap().len(), 1);
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_irreversible_command_ended_after_it_pushed_is_never_started_again() {
    let stand = Stand::start().unwrap();
    let machine = machine(&stand);
    let world = published(&machine).await;
    let run = approved(&world, PUSH, "{}").await;

    stand
        .control()
        .set_fault(
            &Fault::once(FaultKind::stop(StopPoint::AfterCommit, StopHow::Hang)).on("catalog push"),
        )
        .unwrap();
    let lost = world.agent(&["run", "resume", &run]).await;
    assert_eq!(lost.error()["class"], "possible_commit", "{}", lost.json);
    assert_eq!(stand.control().pushes().unwrap().len(), 1);

    // Nothing can say whether it landed, so nothing is sent again, however
    // often the run is resumed.
    for _ in 0..2 {
        let again = world.agent(&["run", "resume", &run]).await;
        assert_ne!(again.result()["status"], "verified", "{}", again.json);
        assert_eq!(stand.control().pushes().unwrap().len(), 1);
    }
    let trace = world.ok(&["trace", "show", &run]).await;
    assert!(!kinds(&trace).contains(&"verified"));
    world.close().await;
}
