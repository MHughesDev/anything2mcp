//! Package 3.10, the faults of section 224 and 502 through the product:
//! each fault is made at the test stand, or on the way to it, while a
//! write runs through the engine loop, and the stand is asked afterwards
//! what it stored. No fault leaves a duplicate, and none leaves a record
//! called verified that the stand does not hold.
//!
//! The faults: a session that ends between the approval and the resume,
//! and one that ends between the write and its read-back; a connection
//! lost after the write was sent; input that is not what the tool
//! declares; an approval made stale by a change at the stand, and an
//! approval of a diff that was superseded; and a surface an edge blocks,
//! before the write and during its read-back. The connection lost after a
//! write through the `page_http` mode is proven in `verified_write.rs`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use a2m_actions::PresenceProof;
use a2m_core::{Digest, RunId};
use a2m_policy::{Channel, PresenceCheck};
use common::proxy::{Alteration, Proxy};
use common::{Machine, World};
use example_app::auth::Credential;
use example_app::faults::{Fault, FaultKind};
use example_app::harness::{BEARER_TOKEN, TestApp};
use serde_json::{Value as Json, json};

const PLACEHOLDER: &str = "http://127.0.0.1:4010";
const UPSERT: &str = "example_line_item_upsert";
const INPUT: &str = r#"{"order_id": "A-102", "item_code": "L-300", "description": "New rail", "quantity": 3, "length_in": "12' 6 1/2\""}"#;

/// The same target signed in: its endpoints are the stand's copy behind a
/// bearer token, and the token is the stored credential.
const AUTH_FILE: &str = r#"auth:
  - surfaces: [api]
    mode: custom
    provider: bearer_static
    probe: order_get
    credentials:
      token: "vault://example/service/token"
    attempt_limit: 3
"#;

/// The orders project, published, with its address changed to `base`.
async fn published(machine: &Machine) -> World {
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    world
}

/// The orders project behind the stand's bearer token, at `base`, with
/// the token stored and the session signed in.
async fn signed_in(base: &str) -> (Machine, World) {
    let machine = Machine::new(
        "orders",
        &[
            (PLACEHOLDER, base),
            ("path: \"/api/", "path: \"/secure/bearer/api/"),
            ("auth: none", "auth: token"),
        ],
    );
    std::fs::write(
        machine.project.path().join("targets/example/auth.yaml"),
        AUTH_FILE,
    )
    .unwrap();
    let world = published(&machine).await;
    let stored = world
        .agent(&[
            "secret",
            "set",
            "example/service/token",
            "--value",
            BEARER_TOKEN,
        ])
        .await;
    assert_eq!(stored.finished.code, 0, "{:?}", stored.finished);
    let login = world.agent(&["session", "login", "example"]).await;
    assert_eq!(login.result()["state"], "signed_in", "{}", login.json);
    (machine, world)
}

/// The person approves, at the desk, the diff they were shown.
async fn approve(world: &World, run: &str) {
    let asked = world.surface.last_ask();
    approve_hash(world, run, asked.diff_hash).await.unwrap();
}

/// The person approves `hash` for `run`, at the desk.
async fn approve_hash(world: &World, run: &str, hash: Digest) -> Result<(), a2m_core::Failure> {
    let run_id: RunId = run.parse().unwrap();
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
        .approve(run_id, hash, person, Channel::LocalPrompt, proof)
        .await
        .map(|_| ())
}

/// A write that rested and was approved; the run is ready to resume.
async fn approved_write(world: &World) -> String {
    let rested = world
        .agent(&["tool", "call", UPSERT, "--input", INPUT])
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

fn kinds(trace: &Json) -> Vec<String> {
    trace["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["kind"].as_str().unwrap().to_owned())
        .collect()
}

fn count(kinds: &[String], kind: &str) -> usize {
    kinds.iter().filter(|found| *found == kind).count()
}

/// What the stand stores for the item the write is about: how many writes
/// reached it.
async fn writes(app: &TestApp) -> usize {
    app.write_count("A-102", "L-300").await.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_that_ends_between_the_approval_and_the_resume_sends_nothing() {
    let app = TestApp::start().await.unwrap();
    let (_machine, world) = signed_in(&app.url("")).await;
    let run = approved_write(&world).await;

    // The stand ends the session: its token is no longer taken.
    app.revoke(Credential::BearerToken).await.unwrap();
    let refused_before = app.auth_report().await.unwrap().refused_tokens;
    let resumed = world.agent(&["run", "resume", &run]).await;
    assert_eq!(resumed.finished.code, 3, "{}", resumed.json);
    assert_eq!(
        resumed.result()["status"],
        "needs_login",
        "{}",
        resumed.json
    );
    // The stand refused what was sent under the ended session, and stored
    // nothing.
    assert!(app.auth_report().await.unwrap().refused_tokens > refused_before);
    assert_eq!(writes(&app).await, 0);
    let trace = world.ok(&["trace", "show", &run]).await;
    assert_eq!(count(&kinds(&trace), "acted"), 0, "{trace}");
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_that_ends_during_the_read_back_leaves_the_write_in_doubt_until_it_is_read() {
    let app = TestApp::start().await.unwrap();
    let proxy = Proxy::start(&app.url("")).await;
    let (_machine, world) = signed_in(proxy.url()).await;
    let run = approved_write(&world).await;

    // The write reaches the stand; every read after it is refused there,
    // as reads under an ended session are.
    proxy.alter_reads(Alteration::StripAuthorization, true);
    let ended = world.agent(&["run", "resume", &run]).await;
    assert_eq!(writes(&app).await, 1);
    // The write may have landed and was not read back: the run says so,
    // and says that sending it again is not safe (decision 0377).
    assert_eq!(ended.result()["status"], "stopped", "{}", ended.json);
    assert_eq!(ended.error()["class"], "possible_commit", "{}", ended.json);
    assert_eq!(ended.error()["code"], "no_confirmation", "{}", ended.json);
    assert_eq!(ended.error()["retry_safe"], false, "{}", ended.json);
    let kinds_now = kinds(&world.ok(&["trace", "show", &run]).await);
    assert_eq!(count(&kinds_now, "acted"), 1, "{kinds_now:?}");
    assert_eq!(count(&kinds_now, "verified"), 0, "{kinds_now:?}");

    // Resumed while the session is still refused, the run does not
    // complete, and nothing more reaches the stand.
    let waiting = world.agent(&["run", "resume", &run]).await;
    assert_ne!(waiting.result()["status"], "completed", "{}", waiting.json);
    assert_eq!(writes(&app).await, 1);
    let kinds_now = kinds(&world.ok(&["trace", "show", &run]).await);
    assert_eq!(count(&kinds_now, "intent"), 1, "{kinds_now:?}");

    // Signed in again, the resume reads the record first, finds the write,
    // and sends nothing more.
    proxy.pass_reads();
    let login = world.agent(&["session", "login", "example"]).await;
    assert_eq!(login.result()["state"], "signed_in", "{}", login.json);
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    assert_eq!(writes(&app).await, 1);
    let stored = app.stored_item("A-102", "L-300").await.unwrap().unwrap();
    assert_eq!(stored.quantity, 3);
    let kinds = kinds(&world.ok(&["trace", "show", &run]).await);
    assert_eq!(count(&kinds, "intent"), 1, "{kinds:?}");
    assert!(kinds.contains(&"reconcile_read".to_owned()), "{kinds:?}");
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connection_lost_after_the_write_was_sent_is_in_doubt_and_never_written_twice() {
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("orders", &[(PLACEHOLDER, &app.url(""))]);
    let world = published(&machine).await;
    let run = approved_write(&world).await;

    app.set_fault(&Fault::once(FaultKind::LostAfterCommit))
        .await
        .unwrap();
    let lost = world.agent(&["run", "resume", &run]).await;
    assert_eq!(writes(&app).await, 1);
    assert_eq!(lost.error()["class"], "possible_commit", "{}", lost.json);
    assert_eq!(lost.error()["code"], "no_confirmation", "{}", lost.json);
    assert_eq!(lost.error()["retry_safe"], false);

    // Resumed twice: the first reconciles and completes, the second has
    // nothing left to do. The stand holds one write.
    for _ in 0..2 {
        let done = world.agent(&["run", "resume", &run]).await;
        assert_eq!(done.result()["status"], "completed", "{}", done.json);
        assert_eq!(writes(&app).await, 1);
    }
    let kinds = kinds(&world.ok(&["trace", "show", &run]).await);
    assert_eq!(count(&kinds, "intent"), 1, "{kinds:?}");
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn input_the_tool_does_not_declare_reaches_nothing_at_the_stand() {
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("orders", &[(PLACEHOLDER, &app.url(""))]);
    let world = published(&machine).await;
    let base: Json = serde_json::from_str(INPUT).unwrap();
    let heard = app.seen().await.unwrap();
    let with = |field: &str, value: Json| {
        let mut changed = base.clone();
        changed[field] = value;
        changed
    };

    // Each is refused by name before anything is sent: a field the tool
    // does not declare, a number written as an expression, and values in a
    // path placeholder that read like a path, an address, or a header.
    let refused = [
        (
            with("deliver_to", json!("http://127.0.0.1:9/elsewhere")),
            "input_invalid",
        ),
        (with("quantity", json!("1 + 2")), "input_invalid"),
        (
            with("item_code", json!("../../admin")),
            "path_value_refused",
        ),
        (
            with("item_code", json!("A-102/../../__control/reset")),
            "path_value_refused",
        ),
        (
            with("item_code", json!("http://127.0.0.1:9/x")),
            "path_value_refused",
        ),
        (
            with("item_code", json!("A-102\r\nX-Injected: 1")),
            "path_value_refused",
        ),
    ];
    for (input, code) in refused {
        let text = input.to_string();
        let ran = world
            .agent(&["tool", "call", UPSERT, "--input", &text])
            .await;
        assert_eq!(ran.finished.code, 1, "{text}: {}", ran.json);
        assert_eq!(ran.error()["class"], "input", "{text}: {}", ran.json);
        assert_eq!(ran.error()["code"], code, "{text}: {}", ran.json);
        assert_eq!(ran.result()["status"], "rejected", "{text}: {}", ran.json);
    }
    // No request reached the stand, and no approval was asked.
    assert!(app.heard_since(&heard).await.unwrap().is_empty());
    assert!(world.surface.approvals.lock().unwrap().is_empty());

    // A value that reads like an expression is a value: nothing evaluates
    // it. The preview reads the record it names, the text itself in one
    // encoded path segment, and the write rests for the person.
    let input = with("item_code", json!("${input.description}")).to_string();
    let ran = world
        .agent(&["tool", "call", UPSERT, "--input", &input])
        .await;
    assert_eq!(ran.result()["status"], "needs_approval", "{}", ran.json);
    let asked = app.heard_since(&heard).await.unwrap();
    assert_eq!(
        asked,
        ["GET /api/orders/A-102/items/%24%7Binput.description%7D"],
        "{asked:?}"
    );
    assert!(app.writes().await.unwrap().is_empty());
    world.close().await;
}

/// The stand's record changed by somebody else, as a person at the
/// system would change it.
async fn changed_at_the_stand(app: &TestApp, description: &str) {
    let answer = app
        .http()
        .put(app.url("/api/orders/A-102/items/L-300"))
        .json(&json!({"description": description, "quantity": 7, "length_in": "10"}))
        .send()
        .await
        .unwrap();
    assert!(answer.status().is_success(), "{}", answer.status());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_approval_made_stale_at_the_stand_writes_nothing_and_a_superseded_diff_is_refused() {
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("orders", &[(PLACEHOLDER, &app.url(""))]);
    let world = published(&machine).await;
    let run = approved_write(&world).await;
    let first_hash = world.surface.last_ask().diff_hash;

    // Somebody changes the record at the stand after the person approved.
    changed_at_the_stand(&app, "Changed by hand").await;
    assert_eq!(writes(&app).await, 1);
    let stale = world.agent(&["run", "resume", &run]).await;
    assert_ne!(stale.result()["status"], "completed", "{}", stale.json);
    assert_eq!(stale.error()["class"], "stale", "{}", stale.json);
    assert_eq!(
        stale.error()["code"],
        "changed_since_preview",
        "{}",
        stale.json
    );
    // The stand holds what the person there wrote, and nothing else came.
    assert_eq!(writes(&app).await, 1);
    let held = app.stored_item("A-102", "L-300").await.unwrap().unwrap();
    assert_eq!(held.description, "Changed by hand");

    // The same change previewed again is another diff. The approval of
    // the first, superseded diff does not clear it.
    let again = world
        .agent(&["tool", "call", UPSERT, "--input", INPUT])
        .await;
    assert_eq!(again.result()["status"], "needs_approval", "{}", again.json);
    let second = again.result()["run_id"].as_str().unwrap().to_owned();
    assert_ne!(world.surface.last_ask().diff_hash, first_hash);
    let refused = approve_hash(&world, &second, first_hash).await.unwrap_err();
    assert_eq!(refused.code().as_str(), "approval_stale", "{refused:?}");
    let still = world.agent(&["run", "resume", &second]).await;
    assert_eq!(still.result()["status"], "needs_approval", "{}", still.json);
    assert_eq!(writes(&app).await, 1);

    // The new diff, approved, is written once.
    approve(&world, &second).await;
    let done = world.agent(&["run", "resume", &second]).await;
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    assert_eq!(writes(&app).await, 2);
    let held = app.stored_item("A-102", "L-300").await.unwrap().unwrap();
    assert_eq!(held.description, "New rail");
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_surface_an_edge_blocks_writes_nothing_and_a_block_during_the_read_back_leaves_doubt() {
    let app = TestApp::start().await.unwrap();
    let proxy = Proxy::start(&app.url("")).await;
    let machine = Machine::new("orders", &[(PLACEHOLDER, proxy.url())]);
    let world = published(&machine).await;
    let run = approved_write(&world).await;

    // Every read goes to the stand's edge, which answers 403 with its
    // page. The resume stops before the write, and the run can be resumed.
    // The HTTP pack names the page for what it is: `edge_blocked`,
    // `forbidden_by_edge` (decision 0357).
    proxy.alter_reads(Alteration::Divert("/edge/blocked".to_owned()), false);
    let blocked = world.agent(&["run", "resume", &run]).await;
    assert_eq!(blocked.result()["status"], "stopped", "{}", blocked.json);
    assert_eq!(blocked.finished.code, 1, "{}", blocked.json);
    assert_eq!(blocked.error()["class"], "edge_blocked", "{}", blocked.json);
    assert_eq!(
        blocked.error()["code"],
        "forbidden_by_edge",
        "{}",
        blocked.json
    );
    assert_eq!(writes(&app).await, 0);
    assert!(app.writes().await.unwrap().is_empty());
    proxy.pass_reads();
    world.close().await;

    // Another write: the edge blocks the reads after it, its read-back
    // among them, and the record is left in doubt.
    let app = TestApp::start().await.unwrap();
    let proxy = Proxy::start(&app.url("")).await;
    let machine = Machine::new("orders", &[(PLACEHOLDER, proxy.url())]);
    let world = published(&machine).await;
    let run = approved_write(&world).await;
    proxy.alter_reads(Alteration::Divert("/edge/blocked".to_owned()), true);
    let doubt = world.agent(&["run", "resume", &run]).await;
    assert_eq!(writes(&app).await, 1);
    assert_eq!(doubt.error()["class"], "possible_commit", "{}", doubt.json);
    assert_eq!(doubt.error()["code"], "no_confirmation", "{}", doubt.json);
    assert_eq!(doubt.error()["retry_safe"], false, "{}", doubt.json);
    // Once the edge lets reads through, the resume reads the record first
    // and sends nothing more.
    proxy.pass_reads();
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    assert_eq!(writes(&app).await, 1);
    world.close().await;
}
