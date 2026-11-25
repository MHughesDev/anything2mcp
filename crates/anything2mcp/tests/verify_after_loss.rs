//! Package 3.7, the check after a lost runner (decision 0347): the real
//! program is killed after the test application stored its write and
//! before the answer reached it. On a machine started again later, the
//! reconcile reads the record, finds the write landed, and, in the same
//! job, reads every verified record once more. A record that still reads
//! as it was written is noted as read; one that someone changed in
//! between is kept as late evidence of the run. Either way the check
//! changes no state, and the person's resume completes the run without
//! writing again.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use a2m_actions::PresenceProof;
use a2m_core::{RunId, Timestamp};
use a2m_logs::{Clock, ManualClock, SystemClock};
use a2m_policy::{Channel, PresenceCheck};
use common::{Machine, World};
use example_app::harness::TestApp;
use serde_json::Value as Json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

const PLACEHOLDER: &str = "http://127.0.0.1:4010";
const UPSERT: &str = "example_line_item_upsert";
const ORDER: &str = "A-102";
/// How long a test waits for something that should come at once. Only a
/// failure waits this long.
const PATIENCE: Duration = Duration::from_secs(180);

/// A relay between the engine and the test application that can hold back
/// the answer to the next write, after the application stored it, and
/// hold a chosen read unsent until the test lets it go.
struct Relay {
    base: String,
    /// The next write's answer is held back for ever.
    hold_write: Arc<AtomicBool>,
    /// The read, counted from 1 since the relay was armed, that is held
    /// unsent until `release`. 0: none.
    hold_read: Arc<AtomicU32>,
    reads: Arc<AtomicU32>,
    reached: Arc<Notify>,
    release: Arc<Notify>,
    server: JoinHandle<()>,
}

impl Relay {
    async fn start(upstream: &str) -> Self {
        let upstream = upstream
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_owned();
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let shared = Shared {
            upstream,
            hold_write: Arc::new(AtomicBool::new(false)),
            hold_read: Arc::new(AtomicU32::new(0)),
            reads: Arc::new(AtomicU32::new(0)),
            reached: Arc::new(Notify::new()),
            release: Arc::new(Notify::new()),
        };
        Self {
            base: format!("http://{address}"),
            hold_write: Arc::clone(&shared.hold_write),
            hold_read: Arc::clone(&shared.hold_read),
            reads: Arc::clone(&shared.reads),
            reached: Arc::clone(&shared.reached),
            release: Arc::clone(&shared.release),
            server: tokio::spawn(async move {
                while let Ok((client, _)) = listener.accept().await {
                    tokio::spawn(relay(client, shared.clone()));
                }
            }),
        }
    }

    /// Holds the read numbered `at`, counted from 1 from now.
    fn hold_read(&self, at: u32) {
        self.reads.store(0, Ordering::SeqCst);
        self.hold_read.store(at, Ordering::SeqCst);
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.server.abort();
    }
}

#[derive(Clone)]
struct Shared {
    upstream: String,
    hold_write: Arc<AtomicBool>,
    hold_read: Arc<AtomicU32>,
    reads: Arc<AtomicU32>,
    reached: Arc<Notify>,
    release: Arc<Notify>,
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Reads one request, and gives it back to send on one connection.
async fn read_request(client: &mut TcpStream) -> Option<(bool, Vec<u8>)> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(at) = find(&bytes, b"\r\n\r\n") {
            break at + 4;
        }
        let read = client.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8(bytes[..head_end].to_vec()).ok()?;
    let length: usize = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse().ok())
        .unwrap_or(0);
    while bytes.len() < head_end + length {
        let read = client.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    let is_read = head.starts_with("GET ") || head.starts_with("HEAD ");
    let mut lines: Vec<&str> = head
        .trim_end()
        .lines()
        .filter(|line| !line.to_ascii_lowercase().starts_with("connection:"))
        .collect();
    lines.push("connection: close");
    let mut request = lines.join("\r\n").into_bytes();
    request.extend_from_slice(b"\r\n\r\n");
    request.extend_from_slice(&bytes[head_end..]);
    Some((is_read, request))
}

async fn relay(mut client: TcpStream, shared: Shared) {
    let Some((is_read, request)) = read_request(&mut client).await else {
        return;
    };
    if is_read {
        let this = shared.reads.fetch_add(1, Ordering::SeqCst) + 1;
        if this == shared.hold_read.load(Ordering::SeqCst) {
            shared.hold_read.store(0, Ordering::SeqCst);
            shared.reached.notify_one();
            shared.release.notified().await;
        }
    }
    let Ok(mut stand) = TcpStream::connect(&shared.upstream).await else {
        return;
    };
    if stand.write_all(&request).await.is_err() {
        return;
    }
    let mut answer = Vec::new();
    if stand.read_to_end(&mut answer).await.is_err() {
        return;
    }
    if !is_read && shared.hold_write.swap(false, Ordering::SeqCst) {
        shared.reached.notify_one();
        // Holds the caller's connection open, silent, until it goes away.
        let mut rest = [0u8; 64];
        while matches!(client.read(&mut rest).await, Ok(read) if read > 0) {}
        return;
    }
    let _ = client.write_all(&answer).await;
    let _ = client.shutdown().await;
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

/// The run's events of one kind.
fn events<'a>(trace: &'a Json, kind: &str) -> Vec<&'a Json> {
    trace["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == kind)
        .collect()
}

/// A write is approved, and the real program is killed after the
/// application stored it and before its answer arrived. The machine is
/// opened again twenty minutes later, past the dead program's lease, and
/// the quarantine is let pass. Gives the machine, the open world, and
/// the run.
async fn lost_after_the_write(
    app: &TestApp,
    relay: &Relay,
    item: &str,
) -> (Machine, World, String) {
    let machine = Machine::new("orders", &[(PLACEHOLDER, &relay.base)]).with_passphrase();
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    let input = format!(
        r#"{{"order_id": "{ORDER}", "item_code": "{item}", "description": "Checked rail", "quantity": 2, "length_in": "90.5\""}}"#
    );
    let rested = world
        .agent(&["tool", "call", UPSERT, "--input", &input])
        .await;
    assert_eq!(rested.result()["status"], "needs_approval");
    let run = rested.result()["run_id"].as_str().unwrap().to_owned();
    approve(&world, &run).await;
    world.close().await;

    relay.hold_write.store(true, Ordering::SeqCst);
    let mut program = machine.program(&["run", "resume", &run]).spawn().unwrap();
    tokio::time::timeout(PATIENCE, relay.reached.notified())
        .await
        .expect("the program sent no write");
    program.kill().unwrap();
    program.wait().unwrap();
    assert_eq!(app.write_count(ORDER, item).await.unwrap(), 1);

    let later = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(
        SystemClock::new().now().as_unix_millis() + 1_200_000,
    )));
    let world = machine
        .open_at(Some(Arc::clone(&later) as Arc<dyn Clock>))
        .await;
    world.engine.kernel().tend().await;
    later.advance(Duration::from_secs(120));
    world.engine.kernel().tend().await;
    (machine, world, run)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn after_a_lost_runner_the_reconcile_reads_its_verified_records_again_and_the_resume_completes()
 {
    let app = TestApp::start().await.unwrap();
    let relay = Relay::start(&app.url("")).await;
    let item = "V-1";
    let (_machine, world, run) = lost_after_the_write(&app, &relay, item).await;

    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    assert_eq!(app.write_count(ORDER, item).await.unwrap(), 1);

    let trace = world.ok(&["trace", "show", &run]).await;
    // The reconcile found the write had landed, and then read the record
    // once more, in the same job: the note follows the reconcile's read,
    // under its lease, and precedes the resume's own lease.
    let read = events(&trace, "reconcile_read");
    assert_eq!(read.len(), 1, "{trace}");
    assert_eq!(read[0]["detail"]["state"], "verified", "{trace}");
    let checks: Vec<&Json> = events(&trace, "need")
        .into_iter()
        .filter(|event| event["detail"]["verify_after_loss"].is_string())
        .collect();
    assert_eq!(checks.len(), 1, "{trace}");
    assert_eq!(checks[0]["detail"]["verify_after_loss"], "read", "{trace}");
    assert_eq!(checks[0]["lease"], read[0]["lease"], "{trace}");
    assert!(checks[0]["seq"].as_u64() > read[0]["seq"].as_u64());
    let completed = events(&trace, "state_changed")
        .into_iter()
        .find(|event| event["detail"]["to"] == "completed")
        .unwrap();
    assert!(completed["lease"].as_u64() > checks[0]["lease"].as_u64());
    // Nothing differed, so nothing was kept as late evidence.
    assert!(events(&trace, "late_evidence").is_empty(), "{trace}");
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_verified_record_changed_after_a_lost_runner_is_kept_as_late_evidence_and_changes_no_state()
 {
    let app = TestApp::start().await.unwrap();
    let relay = Relay::start(&app.url("")).await;
    let item = "V-2";
    let (_machine, world, run) = lost_after_the_write(&app, &relay, item).await;

    // The reconcile's read passes; the check's read, the second, is held
    // until someone else has changed the record at the application.
    relay.hold_read(2);
    let changed_by_hand = async {
        tokio::time::timeout(PATIENCE, relay.reached.notified())
            .await
            .expect("the check after the loss read nothing");
        let version = app.stored_item(ORDER, item).await.unwrap().unwrap().version;
        let answer = app
            .http()
            .put(app.url(&format!("/api/orders/{ORDER}/items/{item}")))
            .header("content-type", "application/json")
            .body(format!(
                r#"{{"description": "Changed by hand", "quantity": 7, "length_in": "90.5", "version": {version}}}"#
            ))
            .send()
            .await
            .unwrap();
        assert!(answer.status().is_success(), "{answer:?}");
        relay.release.notify_one();
    };
    let resume = ["run", "resume", run.as_str()];
    let (done, ()) = tokio::join!(world.agent(&resume), changed_by_hand);

    // The check changed no state: the run's own write was verified when it
    // was made, the resume completes it, and nothing was written again.
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    assert_eq!(app.write_count(ORDER, item).await.unwrap(), 2);
    let stored = app.stored_item(ORDER, item).await.unwrap().unwrap();
    assert_eq!(stored.quantity, 7);
    let trace = world.ok(&["trace", "show", &run]).await;
    assert_eq!(events(&trace, "intent").len(), 1, "{trace}");
    // The difference is kept as late evidence, and the check is noted as
    // read.
    let late = events(&trace, "late_evidence");
    assert_eq!(late.len(), 1, "{trace}");
    assert!(late[0]["evidence"].is_string(), "{trace}");
    let checks: Vec<&Json> = events(&trace, "need")
        .into_iter()
        .filter(|event| event["detail"]["verify_after_loss"].is_string())
        .collect();
    assert_eq!(checks.len(), 1, "{trace}");
    assert_eq!(checks[0]["detail"]["verify_after_loss"], "read", "{trace}");
    assert!(late[0]["seq"].as_u64() < checks[0]["seq"].as_u64());
    world.close().await;
}
