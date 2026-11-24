//! Package 9.2a (decision 0397): a person copies the data folder while the
//! engine is stopped, and later puts the copy back in place of the newer
//! folder. The engine starts on the copy and holds what the copy held: a
//! write's run that was stopped mid-write is stopped again, and resuming it
//! reconciles before anything is sent, so the test application holds the
//! write once although the newer folder had already made it.
//!
//! The run is stopped the hard way: the real program is killed with its
//! write held back unsent, after the intent was recorded. The copy is taken
//! then. The newer folder resumes the run and the write lands; the copy,
//! put back, knows nothing of that, and finds it by reading.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use a2m_actions::{Paths, PresenceProof};
use a2m_core::{RunId, Timestamp};
use a2m_policy::{Channel, PresenceCheck};
use a2m_telemetry::{Clock, ManualClock, SystemClock};
use common::{Folder, Machine, World, open_under};
use example_app::harness::TestApp;
use serde_json::Value as Json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

const PLACEHOLDER: &str = "http://127.0.0.1:4010";
const UPSERT: &str = "example_line_item_upsert";
const ORDER: &str = "A-102";
const ITEM: &str = "C-1";
/// How long a test waits for something that should come at once. Only a
/// failure waits this long.
const PATIENCE: Duration = Duration::from_secs(180);

/// A relay between the engine and the test application that can hold the
/// next write back unsent, for as long as its caller lives.
struct Relay {
    base: String,
    hold_write: Arc<AtomicBool>,
    reached: Arc<Notify>,
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
        let hold_write = Arc::new(AtomicBool::new(false));
        let reached = Arc::new(Notify::new());
        let (hold, told) = (Arc::clone(&hold_write), Arc::clone(&reached));
        let server = tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                tokio::spawn(relay(
                    client,
                    upstream.clone(),
                    Arc::clone(&hold),
                    Arc::clone(&told),
                ));
            }
        });
        Self {
            base: format!("http://{address}"),
            hold_write,
            reached,
            server,
        }
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Reads one request, and gives it back to send on one connection, with
/// whether it is a read.
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

async fn relay(mut client: TcpStream, upstream: String, hold: Arc<AtomicBool>, told: Arc<Notify>) {
    let Some((is_read, request)) = read_request(&mut client).await else {
        return;
    };
    if !is_read && hold.swap(false, Ordering::SeqCst) {
        // Held back unsent until the caller goes away.
        told.notify_one();
        let mut rest = [0u8; 64];
        while matches!(client.read(&mut rest).await, Ok(read) if read > 0) {}
        return;
    }
    let Ok(mut stand) = TcpStream::connect(&upstream).await else {
        return;
    };
    if stand.write_all(&request).await.is_err() {
        return;
    }
    let mut answer = Vec::new();
    if stand.read_to_end(&mut answer).await.is_err() {
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

/// Copies a folder and everything in it, as a person copies a folder with
/// the tools of their system.
fn copy_folder(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap().path();
        let target = to.join(entry.file_name().unwrap());
        if entry.is_dir() {
            copy_folder(&entry, &target);
        } else {
            std::fs::copy(&entry, &target).unwrap();
        }
    }
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

fn completed(trace: &Json) -> bool {
    events(trace, "state_changed")
        .iter()
        .any(|event| event["detail"]["to"] == "completed")
}

/// Opens the machine `minutes` from now, past any dead program's lease,
/// and lets the lost run's quarantine pass.
async fn opened_later(machine: &Machine, minutes: i64) -> World {
    let later = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(
        SystemClock::new().now().as_unix_millis() + minutes * 60_000,
    )));
    let world = machine
        .open_at(Some(Arc::clone(&later) as Arc<dyn Clock>))
        .await;
    world.engine.kernel().tend().await;
    later.advance(Duration::from_secs(120));
    world.engine.kernel().tend().await;
    world
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_data_folder_put_back_holds_the_copy_and_its_stopped_run_resumes_with_no_second_write() {
    let app = TestApp::start().await.unwrap();
    let relay = Relay::start(&app.url("")).await;
    let machine = Machine::new("orders", &[(PLACEHOLDER, &relay.base)]).with_passphrase();
    let data = Paths::under(machine.home.path()).data;

    // A person publishes, an agent asks for a write, and the person
    // approves it.
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    let input = format!(
        r#"{{"order_id": "{ORDER}", "item_code": "{ITEM}", "description": "Copied rail", "quantity": 2, "length_in": "90.5\""}}"#
    );
    let rested = world
        .agent(&["tool", "call", UPSERT, "--input", &input])
        .await;
    assert_eq!(
        rested.result()["status"],
        "needs_approval",
        "{}",
        rested.json
    );
    let run = rested.result()["run_id"].as_str().unwrap().to_owned();
    approve(&world, &run).await;
    world.close().await;

    // The real program resumes the run and is killed with its write held
    // back unsent: the intent is recorded and nothing has reached the
    // application.
    relay.hold_write.store(true, Ordering::SeqCst);
    let mut program = machine.program(&["run", "resume", &run]).spawn().unwrap();
    tokio::time::timeout(PATIENCE, relay.reached.notified())
        .await
        .expect("the program sent no write");
    program.kill().unwrap();
    program.wait().unwrap();
    assert_eq!(app.write_count(ORDER, ITEM).await.unwrap(), 0);

    // With nothing running, the person copies the data folder.
    let kept = Folder::new("copy");
    copy_folder(&data, kept.path());

    // The newer folder goes on: the run is resumed, its write lands, and
    // the run completes.
    let world = opened_later(&machine, 20).await;
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    assert_eq!(app.write_count(ORDER, ITEM).await.unwrap(), 1);
    let newer = world.ok(&["trace", "show", &run]).await;
    assert!(completed(&newer), "{newer}");
    world.close().await;
    // A closed engine has let go of its store: Windows refuses to remove a
    // file the store's database has open, which it shares for reading and
    // writing and not for removal. (The process's log file is opened as the
    // standard library opens every file, shared for removal too, and may
    // stay open: the last engine's log receives the process's log lines.)
    assert_eq!(
        open_under(&Paths::under(machine.home.path()).store),
        Vec::<std::path::PathBuf>::new()
    );

    // The person puts the copy back in place of the newer folder, with the
    // engine stopped, and starts it again.
    std::fs::remove_dir_all(&data).unwrap();
    copy_folder(kept.path(), &data);
    let world = opened_later(&machine, 40).await;

    // The engine holds the copy's state: the run is not completed, its one
    // intent is there, and nothing the newer folder did is.
    let copied = world.ok(&["trace", "show", &run]).await;
    assert!(!completed(&copied), "{copied}");
    assert_eq!(events(&copied, "intent").len(), 1, "{copied}");
    assert!(events(&copied, "verified").is_empty(), "{copied}");

    // Resuming reconciles first: the read finds the write the newer folder
    // made, the record is verified by it, and nothing is sent again.
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    assert_eq!(app.write_count(ORDER, ITEM).await.unwrap(), 1);
    let stored = app.stored_item(ORDER, ITEM).await.unwrap().unwrap();
    assert_eq!((stored.quantity, stored.length_sixteenths), (2, 1448));
    let trace = world.ok(&["trace", "show", &run]).await;
    assert_eq!(events(&trace, "intent").len(), 1, "{trace}");
    let read = events(&trace, "reconcile_read");
    assert_eq!(read.len(), 1, "{trace}");
    assert_eq!(read[0]["detail"]["state"], "verified", "{trace}");
    world.close().await;
}
