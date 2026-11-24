//! Case 5 through the product (decision 0399): the note of an order is
//! written over direct requests with the anti-forgery token its page gives.
//! The page is fetched in the same pass as the write, the write rests for a
//! person's approval, is sent once and read back, an answer lost after the
//! write was stored puts the record in doubt and the resume reconciles it
//! by reading, and a limit in front of the page stops the write before it is
//! sent. The token is in nothing the product gives back or keeps. Every
//! claim is checked by asking the test application what it stored.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use a2m_actions::PresenceProof;
use a2m_core::RunId;
use a2m_policy::{Channel, PresenceCheck, PrincipalRef};
use common::{Machine, World};
use example_app::faults::{Fault, FaultKind};
use example_app::harness::TestApp;
use serde_json::Value as Json;

const PLACEHOLDER: &str = "http://127.0.0.1:4010";
const NOTE: &str = "example_note_put";

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
    let proof = PresenceProof::new(PresenceCheck::OsPrompt, person.id, world.clock.now());
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

/// Every file under `folder`, read as bytes.
fn files_under(folder: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(folder) else {
        return found;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files_under(&path));
        } else if let Ok(bytes) = std::fs::read(&path) {
            found.push((path, bytes));
        }
    }
    found
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_note_written_with_its_pages_token_is_approved_written_once_read_back_and_never_quotes_the_token()
 {
    let app = TestApp::start().await.unwrap();
    let before = app.stored_notes().await.unwrap();
    let machine = Machine::new("forgery", &[(PLACEHOLDER, &app.url(""))]).on_disk();
    let world = machine.open().await;
    let linted = world.ok(&["draft", "lint", "example"]).await;
    assert_eq!(linted["ok"], true, "{linted}");
    let tested = world.agent(&["test", "example"]).await;
    assert_eq!(tested.result()["passed"], true, "{}", tested.json);
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    let mut given = vec![tested.json.to_string(), published.json.to_string()];

    // Called by an agent, the write rests, and nothing was written: the
    // page was not fetched, for the run has not begun to act.
    let input = r#"{"order_id": "A-102", "note": "Deliver to the side door"}"#;
    let rested = world.agent(&["tool", "call", NOTE, "--input", input]).await;
    assert_eq!(
        rested.result()["status"],
        "needs_approval",
        "{:?}",
        rested.finished
    );
    let run = rested.result()["run_id"].as_str().unwrap().to_owned();
    assert!(app.stored_notes().await.unwrap().writes.is_empty());
    given.push(rested.json.to_string());

    // Approved by the person, it is written once and verified: one page
    // fetched, its token sent and taken back by the application.
    approve(&world, &run).await;
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.finished.code, 0, "{:?}", done.finished);
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    let stored = app.stored_notes().await.unwrap();
    assert_eq!(stored.notes["A-102"], "Deliver to the side door");
    assert_eq!(stored.writes["A-102"], 1);
    assert_eq!(stored.forgery_refused, 0);
    assert_eq!(
        stored.tokens_issued - before.tokens_issued,
        stored.tokens_used
    );
    assert_eq!(stored.tokens_used, 1);
    let trace = world.ok(&["trace", "show", &run]).await;
    assert!(kinds(&trace).contains(&"verified"), "{trace}");
    given.extend([done.json.to_string(), trace.to_string()]);

    // A limit in front of the page stops the write before it is sent, as the
    // edge's own answer, and the application still holds the first note.
    let input = r#"{"order_id": "A-102", "note": "Never mind"}"#;
    let rested = world.agent(&["tool", "call", NOTE, "--input", input]).await;
    let limited = rested.result()["run_id"].as_str().unwrap().to_owned();
    approve(&world, &limited).await;
    app.set_fault(&Fault::once(FaultKind::Status { status: 429 }).on_paths("/orders"))
        .await
        .unwrap();
    let stopped = world.agent(&["run", "resume", &limited]).await;
    assert_ne!(stopped.result()["status"], "completed", "{}", stopped.json);
    assert_eq!(
        stopped.error()["code"],
        "rate_limited_by_edge",
        "{}",
        stopped.json
    );
    let stored = app.stored_notes().await.unwrap();
    assert_eq!(stored.writes["A-102"], 1, "the stopped write was sent");
    assert_eq!(stored.notes["A-102"], "Deliver to the side door");
    given.extend([rested.json.to_string(), stopped.json.to_string()]);

    // An answer lost after the write was stored: the record is in doubt,
    // and the resume reconciles it by reading, and sends nothing again.
    let input = r#"{"order_id": "A-100", "note": "Leave with the neighbour"}"#;
    let rested = world.agent(&["tool", "call", NOTE, "--input", input]).await;
    let second = rested.result()["run_id"].as_str().unwrap().to_owned();
    approve(&world, &second).await;
    app.set_fault(&Fault::once(FaultKind::LostAfterCommit).on_paths("/api/orders/A-100/note"))
        .await
        .unwrap();
    let lost = world.agent(&["run", "resume", &second]).await;
    assert_eq!(lost.error()["class"], "possible_commit", "{}", lost.json);
    let written = app.stored_notes().await.unwrap().writes["A-100"];
    assert_eq!(
        written,
        before.writes.get("A-100").copied().unwrap_or(0) + 1
    );
    let reconciled = world.agent(&["run", "resume", &second]).await;
    assert_eq!(
        reconciled.result()["status"],
        "completed",
        "{}",
        reconciled.json
    );
    let stored = app.stored_notes().await.unwrap();
    assert_eq!(
        stored.writes["A-100"], written,
        "the reconcile sent the write again"
    );
    assert_eq!(stored.notes["A-100"], "Leave with the neighbour");
    let trace = world.ok(&["trace", "show", &second]).await;
    let kinds = kinds(&trace);
    assert!(kinds.contains(&"reconcile_read"), "{kinds:?}");
    assert_eq!(kinds.iter().filter(|kind| **kind == "intent").count(), 1);
    given.extend([
        rested.json.to_string(),
        lost.json.to_string(),
        reconciled.json.to_string(),
        trace.to_string(),
    ]);
    world.close().await;

    // The page's token is in no result, event, or trace given, and in no
    // file the product keeps: its store, its blobs, its journal, and its
    // logs.
    let token = example_app::forgery::TOKEN_PREFIX;
    for text in &given {
        assert!(!text.contains(token), "a token in {text}");
    }
    let paths = a2m_actions::Paths::under(machine.home.path());
    let mut scanned = 0;
    for folder in [machine.home.path(), machine.project.path()] {
        for (path, bytes) in files_under(folder) {
            scanned += 1;
            let found = bytes
                .windows(token.len())
                .any(|window| window == token.as_bytes());
            assert!(!found, "a token in {}", path.display());
        }
    }
    assert!(
        files_under(&paths.store).len() + files_under(&paths.logs).len() > 0,
        "the store and the logs were not where the test looked ({scanned} files)"
    );
}
