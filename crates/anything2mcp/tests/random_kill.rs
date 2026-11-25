//! Package 3.7: the real program is killed at a step chosen by a seeded
//! random number while it carries out an approved write, and the run is
//! resumed on a machine started again later. Whatever the step, the test
//! application holds the write exactly once and the run ends completed.
//!
//! A seed chooses one of the program's requests to the application, whether
//! the kill comes while that request is held back unsent or a few
//! milliseconds after its answer was given, and how many milliseconds.
//! A failure prints its seed. `A2M_RANDOM_KILL_SEEDS` sets how many seeds
//! are tried, from 0; `A2M_RANDOM_KILL_SEED` tries one alone.

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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

const PLACEHOLDER: &str = "http://127.0.0.1:4010";
const UPSERT: &str = "example_line_item_upsert";
const ORDER: &str = "A-102";
/// The seeds tried when the environment names no other number.
const SEEDS: u64 = 16;
/// The requests a clean resume of the write makes, and one more: a seed
/// that names the last is a kill after the program has done everything.
const REQUESTS: u64 = 4;
/// How long a test waits for the program to reach its step. Only a
/// failure waits this long.
const PATIENCE: Duration = Duration::from_secs(180);

/// SplitMix64: the same numbers from the same seed on every machine.
struct Draw(u64);

impl Draw {
    fn below(&mut self, below: u64) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        (z ^ (z >> 31)) % below
    }
}

/// Where a seed kills the program.
#[derive(Debug, Clone, Copy)]
struct Kill {
    /// The program's request, from 0, at which the kill comes.
    request: u32,
    /// Whether the request is held back unsent until the kill, or passes
    /// and the kill comes after its answer.
    held: bool,
    /// For a kill after the answer: the milliseconds the program runs on.
    after_ms: u64,
}

impl Kill {
    fn of(seed: u64) -> Self {
        let mut draw = Draw(seed);
        Self {
            request: u32::try_from(draw.below(REQUESTS)).unwrap(),
            held: draw.below(2) == 0,
            after_ms: draw.below(40),
        }
    }
}

/// A relay between the program and the application that counts the
/// program's requests and stops at the one a kill names.
struct Relay {
    base: String,
    /// The request to stop at, and whether it is held back unsent.
    at: Arc<AtomicU32>,
    held: Arc<AtomicBool>,
    reached: Arc<Notify>,
    /// The program's requests counted since the relay was armed.
    count: Arc<AtomicU32>,
    server: JoinHandle<()>,
}

/// No request is stopped at.
const NONE: u32 = u32::MAX;

impl Relay {
    async fn start(upstream: &str) -> Self {
        let upstream = upstream
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_owned();
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let at = Arc::new(AtomicU32::new(NONE));
        let held = Arc::new(AtomicBool::new(false));
        let reached = Arc::new(Notify::new());
        let count = Arc::new(AtomicU32::new(0));
        let (stop, hold, told, counter) = (
            Arc::clone(&at),
            Arc::clone(&held),
            Arc::clone(&reached),
            Arc::clone(&count),
        );
        let server = tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                tokio::spawn(relay(
                    client,
                    upstream.clone(),
                    Arc::clone(&counter),
                    Arc::clone(&stop),
                    Arc::clone(&hold),
                    Arc::clone(&told),
                ));
            }
        });
        Self {
            base: format!("http://{address}"),
            at,
            held,
            reached,
            count,
            server,
        }
    }

    /// Stops at the kill's request; requests are counted from now.
    fn arm(&self, kill: Kill) {
        self.count.store(0, Ordering::SeqCst);
        self.held.store(kill.held, Ordering::SeqCst);
        self.at.store(kill.request, Ordering::SeqCst);
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

async fn read_request(client: &mut TcpStream) -> Option<Vec<u8>> {
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
    // One request for each connection, so the answer ends where the
    // application closes.
    let mut lines: Vec<&str> = head
        .trim_end()
        .lines()
        .filter(|line| !line.to_ascii_lowercase().starts_with("connection:"))
        .collect();
    lines.push("connection: close");
    let mut request = lines.join("\r\n").into_bytes();
    request.extend_from_slice(b"\r\n\r\n");
    request.extend_from_slice(&bytes[head_end..]);
    Some(request)
}

async fn relay(
    mut client: TcpStream,
    upstream: String,
    count: Arc<AtomicU32>,
    at: Arc<AtomicU32>,
    held: Arc<AtomicBool>,
    reached: Arc<Notify>,
) {
    let Some(request) = read_request(&mut client).await else {
        return;
    };
    let this = count.fetch_add(1, Ordering::SeqCst);
    let stops = this == at.load(Ordering::SeqCst);
    if stops {
        at.store(NONE, Ordering::SeqCst);
    }
    if stops && held.load(Ordering::SeqCst) {
        // Held back unsent until the caller goes away.
        reached.notify_one();
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
    if stops {
        reached.notify_one();
    }
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

/// The real program with `args`, as the machine runs it. A2M_RANDOM_KILL_PROGRAM
/// may name another build of the same program, for a run by hand on a
/// machine where several builds share one output folder.
fn program(machine: &Machine, args: &[&str]) -> std::process::Command {
    let made = machine.program(args);
    let Ok(other) = std::env::var("A2M_RANDOM_KILL_PROGRAM") else {
        return made;
    };
    let mut command = std::process::Command::new(other);
    command.args(made.get_args());
    if let Some(folder) = made.get_current_dir() {
        command.current_dir(folder);
    }
    for (name, value) in made.get_envs() {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    command
}

/// Waits until the relay reached the kill's request or the program ended
/// by itself. True when the relay reached it.
async fn reached_or_ended(relay: &Relay, program: &mut std::process::Child) -> bool {
    let give_up = tokio::time::Instant::now() + PATIENCE;
    loop {
        tokio::select! {
            () = relay.reached.notified() => return true,
            () = tokio::time::sleep(Duration::from_millis(20)) => {}
        }
        if program.try_wait().unwrap().is_some() {
            return false;
        }
        assert!(
            tokio::time::Instant::now() < give_up,
            "the program reached no step"
        );
    }
}

async fn one_seed(app: &TestApp, seed: u64) {
    let kill = Kill::of(seed);
    let item = format!("K-{seed}");
    let why = format!("seed {seed}, {kill:?}");
    let relay = Relay::start(&app.url("")).await;
    let machine = Machine::new("orders", &[(PLACEHOLDER, &relay.base)]).with_passphrase();

    // A person publishes, an agent asks for the write, and the person
    // approves it, on an engine that then closes.
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(
        published.finished.code, 0,
        "{why}: {:?}",
        published.finished
    );
    let input = format!(
        r#"{{"order_id": "{ORDER}", "item_code": "{item}", "description": "Killed rail", "quantity": 2, "length_in": "90.5\""}}"#
    );
    let rested = world
        .agent(&["tool", "call", UPSERT, "--input", &input])
        .await;
    assert_eq!(rested.result()["status"], "needs_approval", "{why}");
    let run = rested.result()["run_id"].as_str().unwrap().to_owned();
    approve(&world, &run).await;
    world.close().await;

    // The real program resumes the run and is killed at the seed's step.
    relay.arm(kill);
    let mut program = program(&machine, &["run", "resume", &run]).spawn().unwrap();
    if reached_or_ended(&relay, &mut program).await && !kill.held {
        tokio::time::sleep(Duration::from_millis(kill.after_ms)).await;
    }
    let _ = program.kill();
    program.wait().unwrap();
    let after_the_kill = app.write_count(ORDER, &item).await.unwrap();
    assert!(after_the_kill <= 1, "{why}: {after_the_kill} writes");

    // The machine is started again twenty minutes later, past the dead
    // program's lease; the run is declared lost, and once its quarantine
    // has passed, resuming reconciles before anything is sent.
    let later = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(
        SystemClock::new().now().as_unix_millis() + 1_200_000,
    )));
    let world = machine
        .open_at(Some(Arc::clone(&later) as Arc<dyn Clock>))
        .await;
    world.engine.kernel().tend().await;
    later.advance(Duration::from_secs(120));
    world.engine.kernel().tend().await;
    let done = world.agent(&["run", "resume", &run]).await;
    if done.result()["status"] != "completed" {
        let trace = world.ok(&["trace", "show", &run]).await;
        panic!(
            "{why}: the run did not complete: {}; its events: {trace}",
            done.json
        );
    }
    // The application holds the write once, with its values.
    assert_eq!(app.write_count(ORDER, &item).await.unwrap(), 1, "{why}");
    let stored = app.stored_item(ORDER, &item).await.unwrap().unwrap();
    assert_eq!(
        (stored.quantity, stored.length_sixteenths),
        (2, 1448),
        "{why}"
    );
    let trace = world.ok(&["trace", "show", &run]).await;
    // The record's last attempt ended verified: by its own read-back, or
    // by the reconcile's read that found the write had landed.
    let events = trace["events"].as_array().unwrap();
    let last_intent = events
        .iter()
        .rposition(|event| event["kind"] == "intent")
        .unwrap_or_else(|| panic!("{why}: no intent: {trace}"));
    let verified = events[last_intent..].iter().any(|event| {
        event["kind"] == "verified"
            || (event["kind"] == "reconcile_read" && event["detail"]["state"] == "verified")
    });
    assert!(
        verified,
        "{why}: the run completed with nothing verified: {trace}"
    );
    world.close().await;
}

fn seeds() -> Vec<u64> {
    let read = |name: &str| std::env::var(name).ok().map(|text| text.parse::<u64>());
    if let Some(one) = read("A2M_RANDOM_KILL_SEED") {
        return vec![one.expect("A2M_RANDOM_KILL_SEED is a whole number")];
    }
    let count = read("A2M_RANDOM_KILL_SEEDS").map_or(SEEDS, |count| {
        count.expect("A2M_RANDOM_KILL_SEEDS is a whole number")
    });
    (0..count).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn killed_at_a_random_step_and_resumed_the_write_is_made_once_and_verified() {
    let app = TestApp::start().await.unwrap();
    for seed in seeds() {
        eprintln!("random kill: seed {seed}, {:?}", Kill::of(seed));
        one_seed(&app, seed).await;
    }
}
