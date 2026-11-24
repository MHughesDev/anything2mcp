//! Package 8.2a: a tool a page registers for agents is called through the
//! engine loop. The test application's page `/cases/site-tools` registers
//! a tool that saves a note; the project publishes an operation whose
//! binding is of the mode `site_tools`; an agent calls the tool, the person
//! approves the call as one that cannot be taken back, and the run ends
//! with the page's own answer as an untrusted output. The application is
//! asked what its page was told, and it was told once.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use a2m_actions::PresenceProof;
use a2m_core::RunId;
use a2m_policy::{Channel, PresenceCheck};
use common::{Machine, World};
use example_app::harness::TestApp;

const PLACEHOLDER: &str = "http://127.0.0.1:4010";
const TOOL: &str = "example_page_note_save";

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tool_a_page_registers_is_called_through_the_engine_loop_once_a_person_approves_it() {
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("site_tools", &[(PLACEHOLDER, &app.url(""))]).on_disk();
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);

    // What cannot be taken back is not a read, whatever the page says.
    let found = world.ok(&["tool", "search", "note"]).await;
    assert!(
        found["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == TOOL && tool["effect"] == "irreversible"),
        "{found}"
    );

    let rested = world
        .agent(&[
            "tool",
            "call",
            TOOL,
            "--input",
            r#"{"text": "Dock 4 at nine"}"#,
        ])
        .await;
    assert_eq!(
        rested.result()["status"],
        "needs_approval",
        "{:?}",
        rested.finished
    );
    assert!(app.case_events().await.unwrap().is_empty());
    let run = rested.result()["run_id"].as_str().unwrap().to_owned();
    approve(&world, &run).await;
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.finished.code, 0, "{:?}", done.finished);
    assert_eq!(done.result()["status"], "completed", "{}", done.json);

    // The page was told once, by the page's own tool.
    assert_eq!(
        app.case_events().await.unwrap(),
        ["note-saved: Dock 4 at nine"]
    );
    // The page's answer is the output, and nothing in the run calls the
    // write verified: nothing reads the note back.
    assert!(
        done.json.to_string().contains("Saved note: Dock 4 at nine"),
        "{}",
        done.json
    );
    let trace = world.ok(&["trace", "show", &run]).await;
    let kinds: Vec<String> = trace["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["kind"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(kinds.iter().filter(|kind| *kind == "intent").count(), 1);
    assert!(!kinds.iter().any(|kind| kind == "verified"), "{kinds:?}");
    world.close().await;
}
