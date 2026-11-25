//! Milestone 3, the verified write: a write tool rests at `needs_approval`,
//! nothing the command line offers can approve it, a person's approval at
//! the approval surface lets `run resume` complete it, and the test
//! application confirms exactly one write.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use a2m_actions::PresenceProof;
use a2m_core::{RunId, Timestamp};
use a2m_policy::{Channel, PresenceCheck, PrincipalRef};
use a2m_telemetry::{Clock, ManualClock, SystemClock};
use common::proxy::Proxy;
use common::{Machine, World};
use example_app::faults::{Fault, FaultKind};
use example_app::harness::TestApp;
use serde_json::Value as Json;
use std::sync::Arc;
use std::time::Duration;

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
    // The proof is taken now, on the engine's own clock: the desk refuses
    // one older than a proof of presence may be.
    let proof = PresenceProof::new(PresenceCheck::OsPrompt, person.id, world.clock.now());
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
    let rested = world
        .agent(&["tool", "call", UPSERT, "--input", INPUT])
        .await;
    assert_eq!(rested.finished.code, 3, "{:?}", rested.finished);
    // A write that waits for its approval has written nothing, so it is
    // never `ok` (decision 0458).
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
    let rested = world
        .agent(&["tool", "call", UPSERT, "--input", INPUT])
        .await;
    assert_eq!(
        rested.result()["status"],
        "needs_approval",
        "{:?}",
        rested.finished
    );
    let run = rested.result()["run_id"].as_str().unwrap().to_owned();
    approve(world, &run).await;
    run
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answer_lost_after_the_write_was_sent_is_reconciled_and_not_written_twice() {
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("orders", &[(PLACEHOLDER, &app.url(""))]);
    let world = published(&app, &machine).await;
    let run = approved_write(&world).await;

    // The application stores the write and cuts the connection with no
    // answer.
    app.set_fault(&Fault::once(FaultKind::LostAfterCommit))
        .await
        .unwrap();
    let lost = world.agent(&["run", "resume", &run]).await;
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 1);
    assert_eq!(lost.finished.code, 1, "{:?}", lost.finished);
    assert_eq!(lost.result()["status"], "stopped", "{:?}", lost.finished);
    assert_eq!(
        lost.error()["class"],
        "possible_commit",
        "{:?}",
        lost.finished
    );
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
    proxy.hold_next_write();
    let mut program = machine.program(&["run", "resume", &run]).spawn().unwrap();
    proxy.stored().await;
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 1);
    program.kill().unwrap();
    program.wait().unwrap();

    // The machine is started again twenty minutes later. The lease of the
    // dead process, which in one process is at least ten minutes long, has
    // ended; the engine's tending declares it lost, and the run says a
    // write may have gone through.
    let later = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(
        SystemClock::new().now().as_unix_millis() + 1_200_000,
    )));
    let world = machine
        .open_at(Some(Arc::clone(&later) as Arc<dyn Clock>))
        .await;
    world.engine.kernel().tend().await;
    let left = world.agent(&["run", "get", &run]).await;
    assert_eq!(left.result()["status"], "stopped", "{:?}", left.finished);
    assert_eq!(left.error()["class"], "possible_commit");
    assert_eq!(left.error()["code"], "runner_lost");
    assert_eq!(left.error()["retry_safe"], false);

    // The scheduler queued a reconcile that no runner is given before the
    // quarantine has passed. Once it has, resuming reconciles the record
    // before anything is sent, finds the write, and completes the run
    // without writing again.
    later.advance(Duration::from_secs(120));
    world.engine.kernel().tend().await;
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

/// How long a test waits for a child process to do what it should do at
/// once. Only a failure waits this long.
const PATIENCE: Duration = Duration::from_secs(180);

/// Waits for a child to end, within a bound, and gives what it printed. A
/// child that does not end is killed and the test fails by name.
async fn ended_within(mut child: std::process::Child) -> std::process::Output {
    tokio::task::spawn_blocking(move || {
        let give_up = std::time::Instant::now() + PATIENCE;
        loop {
            if child.try_wait().unwrap().is_some() {
                return child.wait_with_output().unwrap();
            }
            if std::time::Instant::now() >= give_up {
                let _ = child.kill();
                let _ = child.wait();
                panic!("the process did not end");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    })
    .await
    .unwrap()
}

/// The real program's `serve`, once it says it is ready.
fn serving(machine: &Machine) -> std::process::Child {
    use std::io::BufRead;
    let mut child = machine.program(&["serve"]).spawn().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (said, heard) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if line == anything2mcp::cli::READY_LINE {
                let _ = said.send(());
            }
        }
    });
    if heard.recv_timeout(PATIENCE).is_err() {
        let _ = child.kill();
        panic!("`serve` did not become ready");
    }
    child
}

/// A machine whose store on disk holds an approved write, through a relay
/// that can hold back the answer to it.
async fn approved_on_disk(app: &TestApp) -> (Proxy, Machine, String) {
    let proxy = Proxy::start(&app.url("")).await;
    let machine = Machine::new("orders", &[(PLACEHOLDER, proxy.url())]).with_passphrase();
    let world = published(app, &machine).await;
    let run = approved_write(&world).await;
    world.close().await;
    (proxy, machine, run)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answer_lost_through_the_served_engine_is_a_write_in_doubt_and_never_safe_to_retry() {
    let app = TestApp::start().await.unwrap();
    let (proxy, machine, run) = approved_on_disk(&app).await;
    let mut engine = serving(&machine);

    // A command resumes the write through the engine. The system stores
    // it, its answer is held back, and the engine's process dies while the
    // command waits for its answer.
    proxy.hold_next_write();
    let mut resume = machine.program(&["run", "resume", &run]);
    resume.env_remove(anything2mcp::PASSPHRASE_VARIABLE);
    let resume = resume.spawn().unwrap();
    proxy.stored().await;
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 1);
    engine.kill().unwrap();
    engine.wait().unwrap();

    // The command does not say that running it again is safe: the write
    // may have been made, and it was.
    let output = ended_within(resume).await;
    assert_eq!(output.status.code(), Some(4), "{output:?}");
    let error = &printed(&output)["error"];
    assert_eq!(error["class"], "possible_commit", "{error}");
    assert_eq!(error["code"], "answer_lost", "{error}");
    assert_eq!(error["retry_safe"], false, "{error}");
    assert!(
        error["fix"].as_str().unwrap().contains("run list"),
        "{error}"
    );
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_command_that_runs_the_engine_itself_holds_its_lock_and_no_second_engine_opens_the_store()
{
    let app = TestApp::start().await.unwrap();
    let (proxy, machine, run) = approved_on_disk(&app).await;

    // With no engine running, a command runs the engine inside itself. Its
    // write is stored and its answer held back, so it stays, holding the
    // store.
    proxy.hold_next_write();
    let mut first = machine.program(&["run", "resume", &run]).spawn().unwrap();
    proxy.stored().await;
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 1);

    // A second command that would run the engine itself waits for the
    // engine lock, and when it stays held is refused by name.
    let second = ended_within(machine.program(&["run", "get", &run]).spawn().unwrap()).await;
    assert_eq!(second.status.code(), Some(4), "{second:?}");
    assert_eq!(printed(&second)["error"]["code"], "engine_unreachable");

    // `serve` is refused while the command holds the store.
    let served = ended_within(machine.program(&["serve"]).spawn().unwrap()).await;
    assert_eq!(served.status.code(), Some(1), "{served:?}");
    assert_eq!(printed(&served)["error"]["code"], "engine_running");

    // A third command waits; once the first has gone, it runs the engine
    // itself and reads the run. The new engine declares the killed one's
    // lease lost at its start (decision 0416): the run is stopped with its
    // cause named, a write that may have been sent, and nothing is sent
    // again until it is reconciled.
    let third = machine.program(&["run", "get", &run]).spawn().unwrap();
    first.kill().unwrap();
    first.wait().unwrap();
    let third = ended_within(third).await;
    assert_eq!(third.status.code(), Some(1), "{third:?}");
    let error = &printed(&third)["error"];
    assert_eq!(error["class"], "possible_commit", "{error}");
    assert_eq!(error["code"], "runner_lost", "{error}");
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 1);
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

/// A write from inside a page (decision 0206), through the engine loop of
/// the product: the note of an order rests for approval, is written once
/// through the `page_http` mode with the page's own token, and is read
/// back; an answer lost after the write was stored puts the record in
/// doubt, and the resume's reconcile reads it and sends nothing again. The
/// page's token is in nothing the product gives back or keeps. It needs a
/// browser, as the web pack's tests find one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_http_update_through_the_engine_loop_is_approved_written_once_and_read_back() {
    const NOTE: &str = "example_order_note_update";
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("notes", &[(PLACEHOLDER, &app.url(""))]).on_disk();
    let world = machine.open().await;
    let linted = world.ok(&["draft", "lint", "example"]).await;
    assert_eq!(linted["ok"], true, "{linted}");
    let tested = world.agent(&["test", "example"]).await;
    assert_eq!(tested.result()["passed"], true, "{}", tested.json);
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    let mut given = vec![tested.json.to_string(), published.json.to_string()];

    // Called by an agent, the write rests, and nothing was written.
    let input = r#"{"order_id": "A-100", "note": "Deliver to the side door"}"#;
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

    // Approved by the person, it is written once and verified.
    approve(&world, &run).await;
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.finished.code, 0, "{:?}", done.finished);
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    let stored = app.stored_notes().await.unwrap();
    assert_eq!(stored.notes["A-100"], "Deliver to the side door");
    assert_eq!(stored.writes["A-100"], 1);
    assert_eq!(stored.tokens_used, 1);
    let trace = world.ok(&["trace", "show", &run]).await;
    assert!(kinds(&trace).contains(&"verified"), "{trace}");
    given.extend([done.json.to_string(), trace.to_string()]);

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
    assert_eq!(app.stored_notes().await.unwrap().writes["A-100"], 2);
    let reconciled = world.agent(&["run", "resume", &second]).await;
    assert_eq!(
        reconciled.result()["status"],
        "completed",
        "{}",
        reconciled.json
    );
    let stored = app.stored_notes().await.unwrap();
    assert_eq!(
        stored.writes["A-100"], 2,
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
    // logs. The browser's own data directory, which the engine owns and
    // which caches the page as a browser does, is the one place left out.
    let token = example_app::forgery::TOKEN_PREFIX;
    for text in &given {
        assert!(!text.contains(token), "a token in {text}");
    }
    let paths = a2m_actions::Paths::under(machine.home.path());
    let mut scanned = 0;
    for folder in [machine.home.path(), machine.project.path()] {
        for (path, bytes) in files_under(folder) {
            if path.starts_with(&paths.profiles) {
                continue;
            }
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

/// The line item's input for the form's write, with its length as the
/// application shows one.
fn line_item(item: &str, description: &str, quantity: u32, length: &str) -> String {
    format!(
        r#"{{"order_id": "A-100", "item_code": "{item}", "description": "{description}", "quantity": {quantity}, "length_in": "{length}"}}"#
    )
}

/// One write to one field set through a form, through the engine loop of
/// the product (roadmap package 3.3): called by an agent, the write of a
/// line item through the `ui` mode rests for approval and nothing is
/// written; approved by a person, it is filled, saved once, and read back
/// afresh, and the application holds what was asked. A browser lost to the
/// answer after the save was stored puts the record in doubt, and the
/// resume's reconcile reads it and saves nothing again. It needs a
/// browser, as the web pack's tests find one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_ui_update_through_the_engine_loop_rests_is_approved_written_once_and_read_back() {
    const UPDATE: &str = "example_line_item_update";
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("pages", &[(PLACEHOLDER, &app.url(""))]).on_disk();
    let world = machine.open().await;
    let linted = world.ok(&["draft", "lint", "example"]).await;
    assert_eq!(linted["ok"], true, "{linted}");
    let tested = world.agent(&["test", "example"]).await;
    assert_eq!(tested.result()["passed"], true, "{}", tested.json);
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);

    // Called by an agent, the write rests, and nothing was written.
    let input = line_item("L-101", "Oak rail, sanded", 7, "12.5");
    let rested = world
        .agent(&["tool", "call", UPDATE, "--input", &input])
        .await;
    assert_eq!(
        rested.result()["status"],
        "needs_approval",
        "{:?}",
        rested.finished
    );
    let run = rested.result()["run_id"].as_str().unwrap().to_owned();
    assert_eq!(app.write_count("A-100", "L-101").await.unwrap(), 0);

    // Approved by the person, it is written once and read back.
    approve(&world, &run).await;
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.finished.code, 0, "{:?}", done.finished);
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    assert_eq!(
        done.result()["result"]["description"],
        "Oak rail, sanded",
        "{}",
        done.json
    );
    assert_eq!(app.write_count("A-100", "L-101").await.unwrap(), 1);
    let stored = app.stored_item("A-100", "L-101").await.unwrap().unwrap();
    assert_eq!(stored.description, "Oak rail, sanded");
    assert_eq!(stored.quantity, 7);
    assert_eq!(stored.length_sixteenths, 200);
    let trace = world.ok(&["trace", "show", &run]).await;
    let written = kinds(&trace);
    assert_eq!(
        written.iter().filter(|kind| **kind == "intent").count(),
        1,
        "{written:?}"
    );
    assert_eq!(
        written.iter().filter(|kind| **kind == "verified").count(),
        1,
        "{written:?}"
    );

    // The save is stored and its answer lost: the record is in doubt, and
    // the resume reconciles it by reading, and saves nothing again.
    let input = line_item("L-102", "Pine rail", 4, "30");
    let rested = world
        .agent(&["tool", "call", UPDATE, "--input", &input])
        .await;
    let second = rested.result()["run_id"].as_str().unwrap().to_owned();
    approve(&world, &second).await;
    app.set_fault(
        &Fault::once(FaultKind::LostAfterCommit).on_paths("/api/orders/A-100/items/L-102"),
    )
    .await
    .unwrap();
    let lost = world.agent(&["run", "resume", &second]).await;
    assert_eq!(lost.error()["class"], "possible_commit", "{}", lost.json);
    assert_eq!(lost.error()["retry_safe"], false, "{}", lost.json);
    assert_eq!(app.write_count("A-100", "L-102").await.unwrap(), 1);
    let reconciled = world.agent(&["run", "resume", &second]).await;
    assert_eq!(
        reconciled.result()["status"],
        "completed",
        "{}",
        reconciled.json
    );
    assert_eq!(
        app.write_count("A-100", "L-102").await.unwrap(),
        1,
        "the reconcile saved the line item again"
    );
    let stored = app.stored_item("A-100", "L-102").await.unwrap().unwrap();
    assert_eq!(stored.description, "Pine rail");
    assert_eq!(stored.quantity, 4);
    assert_eq!(stored.length_sixteenths, 480);
    let trace = world.ok(&["trace", "show", &second]).await;
    let kinds = kinds(&trace);
    assert!(kinds.contains(&"reconcile_read"), "{kinds:?}");
    assert_eq!(kinds.iter().filter(|kind| **kind == "intent").count(), 1);
    world.close().await;
}
