//! Milestone 3, the verified write: a write tool rests at `needs_approval`,
//! nothing the command line offers can approve it, a person's approval at
//! the approval surface lets `run resume` complete it, and the test
//! application confirms exactly one write.

mod common;

use a2m_actions::PresenceProof;
use a2m_core::{RunId, Timestamp};
use a2m_policy::{Channel, PresenceCheck, PrincipalRef};
use common::proxy::{OnWrite, Proxy};
use common::{Machine, World};
use example_app::harness::TestApp;
use serde_json::Value as Json;

const PLACEHOLDER: &str = "http://127.0.0.1:4010";
const UPSERT: &str = "example_line_item_upsert";
/// The length is written as a person writes it, in feet and inches; the
/// system keeps inches, and the field's type converts.
const INPUT: &str = r#"{"order_id": "A-102", "item_code": "L-300", "description": "New rail", "quantity": 3, "length_in": "12' 6 1/2\""}"#;

async fn published(app: &TestApp, machine: &Machine) -> World {
    let _ = app;
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    world
}

/// The person approves, at the desk, the diff they were shown.
async fn approve(world: &World, run: &str) {
    let run_id: RunId = run.parse().unwrap();
    let desk = world.surface.desk();
    let asked = world.surface.last_ask();
    assert_eq!(asked.run, run_id);
    let person = world
        .engine
        .caller(a2m_actions::FrontDoor::Command, false)
        .principal
        .principal()
        .clone();
    let proof = PresenceProof::new(PresenceCheck::OsPrompt, person.id, Timestamp::from_unix_millis(0));
    // An agent cannot approve, whatever it holds.
    let by_agent = desk
        .approve(
            run_id,
            asked.diff_hash,
            PrincipalRef::agent(person.id),
            Channel::LocalPrompt,
            proof,
        )
        .await
        .unwrap_err();
    assert_eq!(by_agent.class(), a2m_core::FailureClass::Policy);
    desk.approve(run_id, asked.diff_hash, person, Channel::LocalPrompt, proof)
        .await
        .unwrap();
}

fn kinds(trace: &Json) -> Vec<&str> {
    trace["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["kind"].as_str().unwrap())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_write_rests_is_approved_by_a_person_and_is_written_and_verified_exactly_once() {
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("orders", &[(PLACEHOLDER, &app.url(""))]);
    let world = published(&app, &machine).await;

    // Called by an agent, the write rests, with the diff's reference.
    let rested = world.agent(&["tool", "call", UPSERT, "--input", INPUT]).await;
    assert_eq!(rested.finished.code, 3, "{:?}", rested.finished);
    assert_eq!(rested.json["ok"], false);
    let result = rested.result();
    assert_eq!(result["status"], "needs_approval", "{result}");
    assert!(result["diff"]["hash"].is_string(), "{result}");
    let run = result["run_id"].as_str().unwrap().to_owned();
    assert_eq!(rested.json["next"]["action"], "run.resume");
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 0);

    // Nothing the command line offers can approve it. No command has the
    // word; resuming, as an agent or as a person, leaves the run resting.
    let help = world.agent_text(&["--help"]).await;
    assert!(!help.to_lowercase().contains("approv"), "{help}");
    for line in anything2mcp::cli::usage_lines().unwrap() {
        assert!(!line.to_lowercase().contains("approv"), "{line}");
    }
    for still in [
        world.agent(&["run", "resume", &run]).await,
        world.person(&["run", "resume", &run]).await,
    ] {
        assert_eq!(still.result()["status"], "needs_approval");
        assert_eq!(still.finished.code, 3);
    }
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 0);

    // A person approves at the approval surface, and the run completes.
    approve(&world, &run).await;
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.finished.code, 0, "{:?}", done.finished);
    assert_eq!(done.result()["status"], "completed");

    // The test application confirms exactly one write, with the values.
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 1);
    let stored = app.stored_item("A-102", "L-300").await.unwrap().unwrap();
    assert_eq!(stored.description, "New rail");
    assert_eq!(stored.quantity, 3);
    assert_eq!(stored.length_sixteenths, 2408);

    // The ledger tells the write in order: the approval, the intent, the
    // act, and the read-back that verified it.
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
    assert_eq!(kinds.last(), Some(&"state_changed"));

    // The run again changes nothing: the approval was used once.
    let again = world.agent(&["run", "resume", &run]).await;
    assert_eq!(again.result()["status"], "completed");
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 1);

    world.close().await;
}

/// A write that rested and was approved; the run is ready to resume.
async fn approved_write(world: &World) -> String {
    let rested = world.agent(&["tool", "call", UPSERT, "--input", INPUT]).await;
    assert_eq!(rested.result()["status"], "needs_approval", "{:?}", rested.finished);
    let run = rested.result()["run_id"].as_str().unwrap().to_owned();
    approve(world, &run).await;
    run
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answer_lost_after_the_write_was_sent_is_reconciled_and_not_written_twice() {
    let app = TestApp::start().await.unwrap();
    let proxy = Proxy::start(&app.url("")).await;
    let machine = Machine::new("orders", &[(PLACEHOLDER, proxy.url())]);
    let world = published(&app, &machine).await;
    let run = approved_write(&world).await;

    // The system stores the write and its answer never arrives.
    proxy.on_write(OnWrite::LoseAnswer);
    let lost = world.agent(&["run", "resume", &run]).await;
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 1);
    assert_eq!(lost.finished.code, 1, "{:?}", lost.finished);
    assert_eq!(lost.result()["status"], "stopped", "{:?}", lost.finished);
    assert_eq!(lost.error()["class"], "possible_commit", "{:?}", lost.finished);
    // Calling again is not safe, and the answer says so.
    assert_eq!(lost.error()["retry_safe"], false);

    // Resuming reconciles first: it reads the record, finds the write
    // landed, and sends nothing.
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.finished.code, 0, "{:?}", done.finished);
    assert_eq!(done.result()["status"], "completed");
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 1);
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

/// What the real program printed, read as the one JSON object of the
/// output contract.
fn printed(output: &std::process::Output) -> Json {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "not one JSON object: {} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_killed_after_the_write_was_sent_leaves_no_duplicate_when_the_run_is_resumed() {
    let app = TestApp::start().await.unwrap();
    let proxy = Proxy::start(&app.url("")).await;
    let machine = Machine::new("orders", &[(PLACEHOLDER, proxy.url())]).with_passphrase();

    // A person publishes and approves on an engine that then closes.
    let world = published(&app, &machine).await;
    let run = approved_write(&world).await;
    world.close().await;

    // The real program resumes the run. The system stores the write, the
    // answer is held back, and the program is killed while it waits.
    proxy.on_write(OnWrite::HoldAnswer);
    let mut program = machine.program(&["run", "resume", &run]).spawn().unwrap();
    proxy.stored().await;
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 1);
    program.kill().unwrap();
    program.wait().unwrap();

    // A new engine finds the run its dead runner left, and says a write
    // may have gone through.
    let world = machine.open().await;
    let left = world.ok(&["run", "get", &run]).await;
    assert_eq!(left["status"], "stopped", "{left}");
    assert_eq!(left["failure"]["class"], "possible_commit", "{left}");

    // Resuming reconciles the record before anything is sent, finds the
    // write, and completes the run without writing again.
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.finished.code, 0, "{:?}", done.finished);
    assert_eq!(done.result()["status"], "completed");
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 1);
    let stored = app.stored_item("A-102", "L-300").await.unwrap().unwrap();
    assert_eq!(stored.quantity, 3);
    world.close().await;

    // The real program reads the finished run back, and prints one object.
    let output = machine.program(&["run", "get", &run]).output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(printed(&output)["result"]["status"], "completed");
    // The passphrase is in nothing the program printed.
    for text in [&output.stdout, &output.stderr] {
        assert!(!String::from_utf8_lossy(text).contains(common::PASSPHRASE));
    }
}
