//! Package 4.2, the faults of a write from inside a page through the
//! product (decision 0206): each fault is made at the test application, or
//! on the way to it, while the `page_http` write of an order's note runs
//! through the engine loop, and the application is asked afterwards what
//! it stored.
//!
//! The faults: input that would send the request elsewhere; a surface an
//! edge blocks, before the write and during its read-back; and a session
//! that ends between the approval and the resume. The connection lost after
//! the write was sent is proven in `verified_write.rs`. Each needs a
//! browser, as the web pack's tests find one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use a2m_actions::PresenceProof;
use a2m_core::RunId;
use a2m_policy::{Channel, PresenceCheck};
use common::proxy::{Alteration, Proxy};
use common::{Folder, Machine, World, copy_changed, project_source};
use example_app::harness::{PASSWORD, TestApp, USERNAME};
use serde_json::{Value as Json, json};

const PLACEHOLDER: &str = "http://127.0.0.1:4010";
const NOTE: &str = "example_order_note_update";
const INPUT: &str = r#"{"order_id": "A-100", "note": "Deliver to the side door"}"#;

/// The notes project, published, at `base`.
async fn published(base: &str) -> (Machine, World) {
    // On disk: the browser's own data directory is a root of the engine's
    // home.
    let machine = Machine::new("notes", &[(PLACEHOLDER, base)]).on_disk();
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    (machine, world)
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

/// A write that rested and was approved; the run is ready to resume.
async fn approved_write(world: &World) -> String {
    let rested = world.agent(&["tool", "call", NOTE, "--input", INPUT]).await;
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
    kinds.iter().filter(|seen| *seen == kind).count()
}

/// How many writes changed the note of `A-100` at the application.
async fn writes(app: &TestApp) -> u64 {
    app.stored_notes()
        .await
        .unwrap()
        .writes
        .get("A-100")
        .copied()
        .unwrap_or(0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn input_that_would_send_the_request_elsewhere_reaches_nothing_at_the_stand() {
    let app = TestApp::start().await.unwrap();
    let (_machine, world) = published(&app.url("")).await;
    let heard = app.seen().await.unwrap();

    // Each is refused by name before a tab is sent anywhere: a field the
    // tool does not declare, and keys that read like a path, an address,
    // or a header. The page and the request are made from the entry's own
    // patterns on the surface's own origin, never from where an input
    // points (spec section 502).
    let refused = [
        json!({"order_id": "A-100", "note": "x", "deliver_to": "http://127.0.0.1:9/elsewhere"}),
        json!({"order_id": "../../__control/reset", "note": "x"}),
        json!({"order_id": "A-100/../A-101", "note": "x"}),
        json!({"order_id": "http://127.0.0.1:9/x", "note": "x"}),
        json!({"order_id": "//127.0.0.1:9/x", "note": "x"}),
        json!({"order_id": "A-100\r\nX-Injected: 1", "note": "x"}),
        json!({"order_id": "A-100", "note": 7}),
    ];
    for input in refused {
        let text = input.to_string();
        let ran = world.agent(&["tool", "call", NOTE, "--input", &text]).await;
        assert_eq!(ran.finished.code, 1, "{text}: {}", ran.json);
        assert_eq!(ran.error()["class"], "input", "{text}: {}", ran.json);
        assert_eq!(ran.result()["status"], "rejected", "{text}: {}", ran.json);
    }
    // No request reached the stand, no approval was asked, and nothing
    // was stored.
    assert_eq!(app.heard_since(&heard).await.unwrap(), Vec::<String>::new());
    assert!(world.surface.approvals.lock().unwrap().is_empty());
    assert_eq!(writes(&app).await, 0);
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_surface_an_edge_blocks_writes_nothing_and_a_block_during_the_read_back_leaves_doubt() {
    let app = TestApp::start().await.unwrap();
    let proxy = Proxy::start(&app.url("")).await;
    let (_machine, world) = published(proxy.url()).await;
    let run = approved_write(&world).await;

    // Every read goes to the stand's edge, which answers 403 with its
    // page: the page of the write, loaded before anything is sent from it,
    // among them. The resume stops before the write, by the edge's name.
    proxy.alter_reads(Alteration::Divert("/edge/blocked".to_owned()), false);
    let blocked = world.agent(&["run", "resume", &run]).await;
    assert_eq!(blocked.result()["status"], "stopped", "{}", blocked.json);
    assert_eq!(blocked.error()["class"], "edge_blocked", "{}", blocked.json);
    assert_eq!(
        blocked.error()["code"],
        "forbidden_by_edge",
        "{}",
        blocked.json
    );
    assert_eq!(writes(&app).await, 0);
    let stored = app.stored_notes().await.unwrap();
    assert_eq!(stored.tokens_used, 0, "{stored:?}");
    let trace = kinds(&world.ok(&["trace", "show", &run]).await);
    assert_eq!(count(&trace, "acted"), 0, "{trace:?}");
    proxy.pass_reads();
    world.close().await;

    // Another write: the edge blocks the reads after it, the read-back's
    // page among them. The write landed, and the record is in doubt.
    let app = TestApp::start().await.unwrap();
    let proxy = Proxy::start(&app.url("")).await;
    let (_machine, world) = published(proxy.url()).await;
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
    let stored = app.stored_notes().await.unwrap();
    assert_eq!(stored.notes["A-100"], "Deliver to the side door");
    let trace = kinds(&world.ok(&["trace", "show", &run]).await);
    assert_eq!(count(&trace, "intent"), 1, "{trace:?}");
    assert!(trace.contains(&"reconcile_read".to_owned()), "{trace:?}");
    world.close().await;
}

/// The notes behind the application's own sign-in page: the target of
/// `signed_in_pages`, with its sign-in by `form_login`, and the note's page
/// and requests of `notes` under `/secure`, at `base`; published, with the
/// person's username and password stored and the session signed in.
async fn signed_in(base: &str) -> (Machine, World) {
    let folder = Folder::new("project");
    copy_changed(
        &project_source("signed_in_pages"),
        folder.path(),
        &[(PLACEHOLDER, base)],
    );
    let machine = Machine::in_folder(
        "notes",
        &[
            (PLACEHOLDER, base),
            ("\"/api/orders/", "\"/secure/api/orders/"),
            (
                "\"/orders/{order_id}/note\"",
                "\"/secure/orders/{order_id}/note\"",
            ),
        ],
        folder,
    )
    .on_disk();
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{}", published.json);
    for (name, value) in [
        ("example/app/user", USERNAME),
        ("example/app/password", PASSWORD),
    ] {
        let stored = world
            .agent(&["secret", "set", name, "--value", value])
            .await;
        assert_eq!(stored.finished.code, 0, "{}", stored.json);
    }
    let login = world.agent(&["session", "login", "example"]).await;
    assert_eq!(login.result()["state"], "signed_in", "{}", login.json);
    (machine, world)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_that_ends_between_the_approval_and_the_resume_sends_nothing_from_the_sign_in_page()
 {
    let app = TestApp::start().await.unwrap();
    let (_machine, world) = signed_in(&app.url("")).await;
    let run = approved_write(&world).await;
    assert_eq!(writes(&app).await, 0);

    // The application ends every session, as an idle timeout does. The
    // write's page is answered by the sign-in page, which the target
    // declares as a sign of expiry: nothing is sent from it.
    app.expire_sessions().await.unwrap();
    let before = app.auth_report().await.unwrap();
    let resumed = world.agent(&["run", "resume", &run]).await;
    assert_eq!(resumed.finished.code, 3, "{}", resumed.json);
    assert_eq!(
        resumed.result()["status"],
        "needs_login",
        "{}",
        resumed.json
    );
    assert_eq!(
        resumed.result()["failure"]["class"],
        "session_expired",
        "{}",
        resumed.json
    );
    // The application refused the page under the ended session, sent no
    // sign-in of its own accord, and stored nothing; no token was used.
    let report = app.auth_report().await.unwrap();
    assert!(report.refused_sessions > before.refused_sessions);
    assert_eq!(report.sign_in_attempts, before.sign_in_attempts);
    assert_eq!(writes(&app).await, 0);
    assert_eq!(app.stored_notes().await.unwrap().tokens_used, 0);
    let trace = kinds(&world.ok(&["trace", "show", &run]).await);
    assert_eq!(count(&trace, "intent"), 0, "{trace:?}");
    assert_eq!(count(&trace, "acted"), 0, "{trace:?}");

    // Signed in again, the resume writes once and reads it back.
    let login = world.agent(&["session", "login", "example"]).await;
    assert_eq!(login.result()["state"], "signed_in", "{}", login.json);
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    assert_eq!(writes(&app).await, 1);
    let stored = app.stored_notes().await.unwrap();
    assert_eq!(stored.notes["A-100"], "Deliver to the side door");
    assert_eq!(stored.tokens_used, 1);
    let trace = kinds(&world.ok(&["trace", "show", &run]).await);
    assert_eq!(count(&trace, "intent"), 1, "{trace:?}");
    assert!(trace.contains(&"verified".to_owned()), "{trace:?}");
    world.close().await;
}
