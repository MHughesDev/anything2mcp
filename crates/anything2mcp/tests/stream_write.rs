//! Package J3, publishing through the stream pack, through the product,
//! against a real NATS server with JetStream (`A2M_TEST_NATS`, such as
//! `127.0.0.1:4222`, started with `nats-server -js`; the test says so and
//! ends when none is named, and CI's `nats` job requires it to run). A
//! publish is recorded first, approved, sent with the record's token, and
//! read back by a scan of the log for that token. When the connection was
//! cut after the server had stored the message and before its answer came
//! back, the resume reconciles and the stream holds the message once. A
//! publish to a topic that nothing keeps is declared irreversible and is
//! never called verified. Every claim about what the server holds is asked
//! of the server directly, through a few lines of the protocol of this
//! test's own, never through the pack's client.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use a2m_actions::PresenceProof;
use a2m_core::RunId;
use a2m_policy::{Channel, PresenceCheck};
use common::{Machine, World};
use serde_json::Value as Json;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

const RECORD: &str = "example_order_event_record";
const NOTICE: &str = "example_order_notice_send";
const STREAM: &str = "J3_EVENTS";
const SUBJECT: &str = "example.j3.events";
/// Messages the log holds before any test publishes.
const SEEDED: i64 = 1;
const NOTICES: &str = "example.orders.notices";

fn kinds(trace: &Json) -> Vec<&str> {
    trace["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["kind"].as_str().unwrap())
        .collect()
}

// ---- the server's own word, by a few lines of the protocol ----

struct Raw {
    stream: BufReader<TcpStream>,
    next: u64,
    /// The inbox of this connection alone, so that no other one hears its
    /// answers.
    inbox: String,
}

impl Raw {
    async fn connect(address: &str) -> Self {
        let stream = TcpStream::connect(address).await.unwrap();
        static CONNECTIONS: AtomicU32 = AtomicU32::new(0);
        let mut raw = Self {
            stream: BufReader::new(stream),
            next: 0,
            inbox: format!(
                "_INBOX.j3raw{}x{}",
                std::process::id(),
                CONNECTIONS.fetch_add(1, Ordering::SeqCst)
            ),
        };
        let mut info = String::new();
        raw.stream.read_line(&mut info).await.unwrap();
        assert!(info.starts_with("INFO "), "{info}");
        let hello = format!(
            "CONNECT {{\"verbose\":false,\"headers\":true,\"no_responders\":true,\"protocol\":1}}\r\nSUB {}.* 1\r\nPING\r\n",
            raw.inbox
        );
        raw.stream
            .get_mut()
            .write_all(hello.as_bytes())
            .await
            .unwrap();
        loop {
            let mut line = String::new();
            raw.stream.read_line(&mut line).await.unwrap();
            if line.starts_with("PONG") {
                break;
            }
        }
        raw
    }

    /// One request, and the payload of the answer.
    async fn request(&mut self, subject: &str, body: &str) -> String {
        self.next += 1;
        let reply = format!("{}.{}", self.inbox, self.next);
        let line = format!("PUB {subject} {reply} {}\r\n{body}\r\n", body.len());
        self.stream
            .get_mut()
            .write_all(line.as_bytes())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let (subject, payload) = self.message().await;
                if subject == reply {
                    return payload;
                }
            }
        })
        .await
        .unwrap()
    }

    async fn api(&mut self, subject: &str, body: &str) -> Json {
        let text = self.request(subject, body).await;
        serde_json::from_str(&text).unwrap_or(Json::Null)
    }

    /// The next message this connection is subscribed to, as its subject
    /// and its payload with any header text left off.
    async fn message(&mut self) -> (String, String) {
        loop {
            let mut head = String::new();
            self.stream.read_line(&mut head).await.unwrap();
            let parts: Vec<&str> = head.split_whitespace().collect();
            match parts.first().copied() {
                Some("MSG") | Some("HMSG") => {
                    let size: usize = parts.last().unwrap().parse().unwrap();
                    let mut payload = vec![0_u8; size + 2];
                    self.stream.read_exact(&mut payload).await.unwrap();
                    payload.truncate(size);
                    let text = String::from_utf8_lossy(&payload).into_owned();
                    let body = text
                        .rfind("\r\n\r\n")
                        .map_or(text.as_str(), |at| &text[at + 4..])
                        .to_owned();
                    return (parts[1].to_owned(), body);
                }
                Some("PING") => self.stream.get_mut().write_all(b"PONG\r\n").await.unwrap(),
                _ => {}
            }
        }
    }

    async fn subscribe(&mut self, subject: &str, sid: u32) {
        let line = format!("SUB {subject} {sid}\r\nPING\r\n");
        self.stream
            .get_mut()
            .write_all(line.as_bytes())
            .await
            .unwrap();
        loop {
            let mut line = String::new();
            self.stream.read_line(&mut line).await.unwrap();
            if line.starts_with("PONG") {
                return;
            }
        }
    }

    async fn stored(&mut self) -> i64 {
        let info = self.api(&format!("$JS.API.STREAM.INFO.{STREAM}"), "").await;
        info["state"]["messages"].as_i64().unwrap()
    }

    async fn remove_stream(&mut self) {
        self.api(&format!("$JS.API.STREAM.DELETE.{STREAM}"), "")
            .await;
    }
}

/// One test at a time makes and empties the stream.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The server, or `None` when the test names none.
async fn server() -> Option<(String, Raw, tokio::sync::MutexGuard<'static, ()>)> {
    let Ok(address) = std::env::var("A2M_TEST_NATS") else {
        eprintln!("skipped: A2M_TEST_NATS names no real server");
        return None;
    };
    let turn = ONE_AT_A_TIME.lock().await;
    let mut raw = Raw::connect(&address).await;
    raw.remove_stream().await;
    let config = format!(
        r#"{{"name":"{STREAM}","subjects":["{SUBJECT}.*"],"retention":"limits","storage":"memory","duplicate_window":120000000000}}"#
    );
    let made = raw
        .api(&format!("$JS.API.STREAM.CREATE.{STREAM}"), &config)
        .await;
    assert!(made.get("error").is_none(), "{made}");
    // The log already holds an event of another order, as a log in use does.
    let seed = raw
        .api(
            &format!("{SUBJECT}.B-200"),
            r#"{"order_id":"B-200","event":"created"}"#,
        )
        .await;
    assert!(seed.get("seq").is_some(), "{seed}");
    Some((address, raw, turn))
}

// ---- a connection that is cut after the server answered ----

/// A relay between the engine and the server. Armed, it lets the next
/// publish with a token through, waits for the server's acknowledgement of
/// it, and cuts both ends without handing that acknowledgement on: the
/// server has stored the message, and the engine never hears so.
struct Cutter {
    address: String,
    armed: Arc<AtomicBool>,
    cuts: Arc<AtomicU32>,
}

impl Cutter {
    async fn start(server: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let armed = Arc::new(AtomicBool::new(false));
        let cuts = Arc::new(AtomicU32::new(0));
        let (flag, count, server) = (armed.clone(), cuts.clone(), server.to_owned());
        tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                let upstream = TcpStream::connect(&server).await.unwrap();
                tokio::spawn(relay(client, upstream, flag.clone(), count.clone()));
            }
        });
        Self {
            address,
            armed,
            cuts,
        }
    }

    fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }

    fn cuts(&self) -> u32 {
        self.cuts.load(Ordering::SeqCst)
    }
}

async fn relay(
    client: TcpStream,
    upstream: TcpStream,
    armed: Arc<AtomicBool>,
    cuts: Arc<AtomicU32>,
) {
    let (mut from_client, mut to_client) = client.into_split();
    let (mut from_server, mut to_server) = upstream.into_split();
    // Set once the one publish to cut is on its way.
    let sent = Arc::new(AtomicBool::new(false));
    let up_sent = sent.clone();
    let up = tokio::spawn(async move {
        let mut buffer = vec![0_u8; 65536];
        while let Ok(read) = from_client.read(&mut buffer).await {
            if read == 0 {
                break;
            }
            let chunk = &buffer[..read];
            if find(chunk, b"Nats-Msg-Id") && armed.swap(false, Ordering::SeqCst) {
                up_sent.store(true, Ordering::SeqCst);
            }
            if to_server.write_all(chunk).await.is_err() {
                break;
            }
        }
    });
    let mut buffer = vec![0_u8; 65536];
    while let Ok(read) = from_server.read(&mut buffer).await {
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        if sent.load(Ordering::SeqCst) && find(chunk, b"\"seq\"") {
            cuts.fetch_add(1, Ordering::SeqCst);
            break;
        }
        if to_client.write_all(chunk).await.is_err() {
            break;
        }
    }
    up.abort();
}

fn find(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

// ---- the product ----

fn machine(address: &str) -> Machine {
    Machine::new(
        "streampub",
        &[
            ("127.0.0.1:4222", address),
            (
                "ORDERS_EVENTS/example.orders.events",
                &format!("{STREAM}/{SUBJECT}"),
            ),
        ],
    )
    .on_disk()
}

async fn published(machine: &Machine) -> World {
    let world = machine.open().await;
    let linted = world.ok(&["draft", "lint", "example"]).await;
    assert_eq!(linted["ok"], true, "{linted}");
    let tested = world.ok(&["test", "example"]).await;
    let replay = tested["gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["gate"] == "offline_replay")
        .unwrap();
    assert_eq!(replay["state"], "passed", "{tested}");
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

const EVENT: &str = r#"{"order_id": "A-100", "event": "shipped"}"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_publish_is_approved_sent_once_and_read_back_by_its_token() {
    let Some((address, mut raw, _turn)) = server().await else {
        return;
    };
    let machine = machine(&address);
    let world = published(&machine).await;
    assert_eq!(raw.stored().await, SEEDED);

    let run = approved(&world, RECORD, EVENT).await;
    // Nothing was published before the person approved.
    assert_eq!(raw.stored().await, SEEDED);
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    assert_eq!(raw.stored().await, SEEDED + 1);

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

    // The run again sends nothing.
    let again = world.agent(&["run", "resume", &run]).await;
    assert_eq!(again.result()["status"], "completed", "{}", again.json);
    assert_eq!(raw.stored().await, SEEDED + 1);
    raw.remove_stream().await;
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_publish_cut_after_the_server_stored_it_is_resumed_and_the_stream_holds_it_once() {
    let Some((address, mut raw, _turn)) = server().await else {
        return;
    };
    let cutter = Cutter::start(&address).await;
    let machine = machine(&cutter.address);
    let world = published(&machine).await;
    let run = approved(&world, RECORD, EVENT).await;

    // The server stores the message and its answer never arrives.
    cutter.arm();
    let lost = world.agent(&["run", "resume", &run]).await;
    assert_eq!(cutter.cuts(), 1, "{}", lost.json);
    assert_eq!(lost.result()["status"], "stopped", "{}", lost.json);
    assert_eq!(lost.error()["class"], "possible_commit", "{}", lost.json);
    assert_eq!(lost.error()["retry_safe"], false, "{}", lost.json);
    assert_eq!(raw.stored().await, SEEDED + 1);

    // The resume reads the record first, looks for the token in the log,
    // and sends no second message, however often it is run.
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    assert_eq!(raw.stored().await, SEEDED + 1);
    let again = world.agent(&["run", "resume", &run]).await;
    assert_eq!(again.result()["status"], "completed", "{}", again.json);
    assert_eq!(raw.stored().await, SEEDED + 1);
    let trace = world.ok(&["trace", "show", &run]).await;
    let kinds = kinds(&trace);
    assert_eq!(kinds.iter().filter(|kind| **kind == "intent").count(), 1);
    assert!(kinds.contains(&"reconcile_read"), "{kinds:?}");
    // The message was found, not sent again: no second send was recorded.
    assert!(
        kinds.iter().filter(|kind| **kind == "acted").count() <= 1,
        "{kinds:?}"
    );
    raw.remove_stream().await;
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_publish_to_a_topic_nothing_keeps_is_approved_as_irreversible_and_never_called_verified()
{
    let Some((address, mut raw, _turn)) = server().await else {
        return;
    };
    // A listener of the test's own is the only thing that can say it came.
    let mut listener = Raw::connect(&address).await;
    listener.subscribe(NOTICES, 7).await;
    let machine = machine(&address);
    let world = published(&machine).await;

    let input = r#"{"order_id": "A-100", "text": "packed"}"#;
    let run = approved(&world, NOTICE, input).await;
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    let (subject, body) = tokio::time::timeout(Duration::from_secs(5), listener.message())
        .await
        .unwrap();
    assert_eq!(subject, NOTICES);
    assert!(body.contains("packed"), "{body}");

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

    // A resume sends nothing more; a second call asks for its own approval.
    let again = world.agent(&["run", "resume", &run]).await;
    assert_eq!(again.result()["status"], "completed", "{}", again.json);
    let next = world
        .agent(&["tool", "call", NOTICE, "--input", input])
        .await;
    assert_eq!(next.result()["status"], "needs_approval", "{}", next.json);
    assert!(
        tokio::time::timeout(Duration::from_millis(500), listener.message())
            .await
            .is_err(),
        "a second notice came"
    );
    raw.remove_stream().await;
    world.close().await;
}
