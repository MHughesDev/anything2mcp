//! Package 3.4: a value is compared after its field type's normalization,
//! proven against what the test application stored. The application keeps
//! a length as whole sixteenths of an inch and shows it as decimal inches
//! with no zeros after the last digit. A length typed `length_ft_in` is
//! rounded to the sixteenth before it is sent, and what is read back
//! compares equal; a length typed as plain text is sent as it was given,
//! the application shows it in its own form, and the read-back differs:
//! the write is made once and never called verified.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use a2m_actions::PresenceProof;
use a2m_core::RunId;
use a2m_policy::{Channel, PresenceCheck};
use common::{Machine, World};
use example_app::harness::TestApp;
use serde_json::Value as Json;

const PLACEHOLDER: &str = "http://127.0.0.1:4010";
const UPSERT: &str = "example_line_item_upsert";
const ORDER: &str = "A-102";
const ITEM: &str = "L-400";

/// The input of the operation's length as the test project declares it.
const TYPED: &str = r#"length_in: {type: length_ft_in, description: "Length, in inches"}"#;

fn input(length: &str) -> String {
    format!(
        r#"{{"order_id": "{ORDER}", "item_code": "{ITEM}", "description": "Short rail", "quantity": 1, "length_in": "{length}"}}"#
    )
}

async fn published(machine: &Machine) -> World {
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
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

/// Calls the write as an agent, has the person approve it, and resumes it.
/// Gives the run and the answer of the resume.
async fn written(world: &World, length: &str) -> (String, common::Ran) {
    let given = input(length);
    let rested = world
        .agent(&["tool", "call", UPSERT, "--input", &given])
        .await;
    assert_eq!(
        rested.result()["status"],
        "needs_approval",
        "{:?}",
        rested.finished
    );
    let run = rested.result()["run_id"].as_str().unwrap().to_owned();
    approve(world, &run).await;
    let done = world.agent(&["run", "resume", &run]).await;
    (run, done)
}

fn kinds(trace: &Json) -> Vec<String> {
    trace["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["kind"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_length_the_type_rounds_to_the_sixteenth_is_stored_so_and_verified() {
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("orders", &[(PLACEHOLDER, &app.url(""))]);
    let world = published(&machine).await;

    // 2.53 inches is 40.48 sixteenths; the type rounds it half up to 40,
    // which the application can hold.
    let (run, done) = written(&world, r#"2.53\""#).await;
    assert_eq!(done.finished.code, 0, "{:?}", done.finished);
    assert_eq!(done.result()["status"], "completed", "{}", done.json);

    // The application stored one write, of forty sixteenths.
    assert_eq!(app.write_count(ORDER, ITEM).await.unwrap(), 1);
    let stored = app.stored_item(ORDER, ITEM).await.unwrap().unwrap();
    assert_eq!(stored.length_sixteenths, 40);

    // The run verified it: what was read back equals what was intended,
    // after the same normalization on both sides.
    let trace = world.ok(&["trace", "show", &run]).await;
    let kinds = kinds(&trace);
    assert_eq!(kinds.iter().filter(|kind| *kind == "intent").count(), 1);
    assert!(kinds.iter().any(|kind| kind == "verified"), "{kinds:?}");
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_length_typed_without_its_rounding_reads_back_different_and_is_never_verified() {
    let app = TestApp::start().await.unwrap();
    // The same operation, with the length typed as plain text, as an input
    // and as an output: nothing brings what is sent and what the
    // application shows to one form.
    let untyped = r#"length_in: {type: text, description: "Length, in inches"}"#;
    let output = (
        "  length_in: {type: length_ft_in}\nbindings:\n  - mode: http\n    verb: upsert",
        "  length_in: {type: text}\nbindings:\n  - mode: http\n    verb: upsert",
    );
    // Its offline test sends the length as its fixture shows it, so that
    // the version can be published: as text, nothing else compares equal.
    let test = (
        r#"length_in: "90.5"}
expect: {description: "Test rail", quantity: 2, length_in: "90.5"}"#,
        r#"length_in: "7' 6 1/2\""}
expect: {description: "Test rail", quantity: 2, length_in: "7' 6 1/2\""}"#,
    );
    let test_request = (
        r#""body.length_in": "90.5""#,
        r#""body.length_in": "7' 6 1/2\"""#,
    );
    let machine = Machine::new(
        "orders",
        &[
            (PLACEHOLDER, &app.url("")),
            (TYPED, untyped),
            output,
            test,
            test_request,
        ],
    );
    let world = published(&machine).await;

    // The application takes `2.5000` and shows it as `2.5`.
    let (run, done) = written(&world, "2.5000").await;
    assert_ne!(done.finished.code, 0, "{:?}", done.finished);
    assert_eq!(done.result()["status"], "stopped", "{}", done.json);
    assert_eq!(done.error()["class"], "mismatch", "{}", done.json);
    assert_eq!(done.error()["code"], "read_back_differs", "{}", done.json);

    // The application holds the one write; nothing was sent twice.
    assert_eq!(app.write_count(ORDER, ITEM).await.unwrap(), 1);
    let stored = app.stored_item(ORDER, ITEM).await.unwrap().unwrap();
    assert_eq!(stored.length_sixteenths, 40);
    let trace = world.ok(&["trace", "show", &run]).await;
    let seen = kinds(&trace);
    assert_eq!(seen.iter().filter(|kind| *kind == "intent").count(), 1);
    assert!(!seen.iter().any(|kind| kind == "verified"), "{seen:?}");

    // Resuming sends nothing again, and still calls nothing verified.
    let again = world.agent(&["run", "resume", &run]).await;
    assert_ne!(again.result()["status"], "completed", "{}", again.json);
    assert_eq!(app.write_count(ORDER, ITEM).await.unwrap(), 1);
    let trace = world.ok(&["trace", "show", &run]).await;
    assert!(
        !kinds(&trace).iter().any(|kind| kind == "verified"),
        "{trace}"
    );
    world.close().await;
}
