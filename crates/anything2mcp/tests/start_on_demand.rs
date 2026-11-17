//! Start on demand, through the real program (decision 0165): a command or
//! a bridge that finds no engine starts one, detached, and is answered by
//! it; two at once start one; an idle engine stops by itself and lets go
//! of its lock; and with `engine.in_process` on, nothing is started.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use a2m_actions::Paths;
use a2m_local::{EngineLock, Launch, STARTED_LINE};
use anything2mcp::PASSPHRASE_VARIABLE;
use anything2mcp::cli::READY_LINE;
use common::{IN_PROCESS_VARIABLE, Machine};
use example_app::harness::TestApp;
use serde_json::{Value as Json, json};

const PLACEHOLDER: &str = "http://127.0.0.1:4010";
const ORDER_GET: &str = "example_order_get";
const READ: [&str; 5] = [
    "tool",
    "call",
    ORDER_GET,
    "--input",
    r#"{"order_id": "A-100"}"#,
];
const OTHER: [&str; 5] = [
    "tool",
    "call",
    ORDER_GET,
    "--input",
    r#"{"order_id": "A-101"}"#,
];

/// How long a test waits for what should happen at once, or for an idle
/// engine of one minute to stop. Only a failure waits this long.
const PATIENCE: Duration = Duration::from_secs(240);

/// A machine whose store is on disk, with the vault's key from a
/// passphrase, and the published `example` target.
async fn published(app: &TestApp) -> Machine {
    let machine = Machine::new("orders", &[(PLACEHOLDER, &app.url(""))]).with_passphrase();
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    world.close().await;
    machine
}

/// The real program, which starts an engine when it finds none.
fn starting(machine: &Machine, args: &[&str]) -> Command {
    let mut command = machine.program(args);
    command.env_remove(IN_PROCESS_VARIABLE);
    command
}

/// Runs a command to its end, off the test's own threads. That it ends
/// at all while the engine it started runs on shows that the engine holds
/// none of the command's standard streams.
async fn output(mut command: Command) -> Output {
    tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap()
}

fn object(output: &Output) -> Json {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "not one JSON object: {} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn assert_read(output: &Output, customer: &str) {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let response = object(output);
    assert_eq!(response["result"]["status"], "completed", "{response}");
    assert_eq!(response["result"]["result"]["customer"], customer);
}

fn paths(machine: &Machine) -> Paths {
    Paths::under(machine.home.path())
}

/// The engine's log, as it is now.
fn log(machine: &Machine) -> String {
    std::fs::read_to_string(Launch::log_of(&paths(machine))).unwrap_or_default()
}

/// The processes started on demand, by the lines their clients wrote.
fn started(machine: &Machine) -> Vec<u32> {
    log(machine)
        .lines()
        .filter_map(|line| line.strip_prefix(STARTED_LINE))
        .map(|rest| rest.trim().trim_end_matches('.').parse().unwrap())
        .collect()
}

/// Waits until no engine holds the machine's lock, and gives the last
/// object the engine wrote to its log: what `serve` prints when it stops.
async fn gone(machine: &Machine) -> Json {
    let paths = paths(machine);
    let give_up = tokio::time::Instant::now() + PATIENCE;
    while EngineLock::held(&paths).unwrap() {
        assert!(
            tokio::time::Instant::now() < give_up,
            "the engine did not stop: {}",
            log(machine)
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // The engine lets go of its lock once its kernel has closed, and then
    // prints the object it ends with: that is waited for too.
    loop {
        let text = log(machine);
        if let Some(last) = text.lines().rev().find(|line| line.starts_with('{')) {
            return serde_json::from_str(last).unwrap();
        }
        assert!(
            tokio::time::Instant::now() < give_up,
            "the engine wrote no last object: {text}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn stop(machine: &Machine) {
    let stopped = output(machine.program(&["engine", "stop"])).await;
    assert_eq!(stopped.status.code(), Some(0), "{stopped:?}");
    assert_eq!(gone(machine).await["result"]["stopped"], "asked");
}

/// What the operating system says of a process, on Linux: its session,
/// its environment, its arguments, and its working folder.
#[cfg(target_os = "linux")]
struct Seen {
    session: u32,
    environment: Vec<u8>,
    arguments: Vec<u8>,
    folder: std::path::PathBuf,
}

#[cfg(target_os = "linux")]
fn seen(process: &str) -> Seen {
    let proc = std::path::Path::new("/proc").join(process);
    let stat = std::fs::read_to_string(proc.join("stat")).unwrap();
    // The fields after the program's name, which is in parentheses.
    let after = &stat[stat.rfind(')').unwrap() + 2..];
    let session = after.split(' ').nth(3).unwrap().parse().unwrap();
    Seen {
        session,
        environment: std::fs::read(proc.join("environ")).unwrap(),
        arguments: std::fs::read(proc.join("cmdline")).unwrap(),
        folder: std::fs::read_link(proc.join("cwd")).unwrap(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_command_with_no_engine_starts_one_detached_and_is_answered() {
    let app = TestApp::start().await.unwrap();
    let machine = published(&app).await;

    // No engine runs. The command starts one, waits for it, and is
    // answered by it; then it ends, and the engine runs on.
    let first = output(starting(&machine, &READ)).await;
    assert_read(&first, "Example Customer One");
    let processes = started(&machine);
    assert_eq!(processes.len(), 1, "{}", log(&machine));
    assert_eq!(log(&machine).matches(READY_LINE).count(), 1);

    // A command that holds no passphrase cannot run the engine itself, so
    // what it reads, the engine read.
    let mut keyless = machine.program(&OTHER);
    keyless.env_remove(PASSPHRASE_VARIABLE);
    assert_read(&output(keyless).await, "Example Customer Two");

    #[cfg(target_os = "linux")]
    {
        // The engine runs in a session of its own, in its data directory,
        // and the passphrase it was given is in neither its environment nor
        // its arguments: it came on its standard input.
        let engine = seen(&processes[0].to_string());
        let this = seen("self");
        assert_ne!(engine.session, this.session);
        assert_eq!(engine.session, processes[0]);
        assert_eq!(
            engine.folder,
            std::fs::canonicalize(paths(&machine).data).unwrap()
        );
        let secret = common::PASSPHRASE.as_bytes();
        for (what, bytes) in [
            ("environment", &engine.environment),
            ("arguments", &engine.arguments),
        ] {
            assert!(
                !bytes.windows(secret.len()).any(|window| window == secret),
                "the passphrase is in the engine's {what}"
            );
            assert!(
                !bytes
                    .windows(PASSPHRASE_VARIABLE.len())
                    .any(|window| window == PASSPHRASE_VARIABLE.as_bytes()),
                "the passphrase's variable is in the engine's {what}"
            );
        }
    }

    // Asked to stop, it stops, and lets go of its lock.
    stop(&machine).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_commands_at_once_start_one_engine() {
    let app = TestApp::start().await.unwrap();
    let machine = published(&app).await;
    let (first, second) = tokio::join!(
        output(starting(&machine, &READ)),
        output(starting(&machine, &OTHER))
    );
    assert_read(&first, "Example Customer One");
    assert_read(&second, "Example Customer Two");
    // One was started, and one became ready: the other command waited on
    // the start lock and connected to it.
    assert_eq!(started(&machine).len(), 1, "{}", log(&machine));
    assert_eq!(log(&machine).matches(READY_LINE).count(), 1);
    stop(&machine).await;
}

/// The real program's `bridge`, with the passphrase, so that it can start
/// an engine. Gives the answer to one request and what it said on
/// standard error.
fn bridged(mut command: Command, message: &Json) -> (Json, String) {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{message}").unwrap();
    stdin.flush().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (send, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if send.send(line).is_err() {
                break;
            }
        }
    });
    let answer = loop {
        let line = lines.recv_timeout(PATIENCE).expect("the bridge answered");
        let message: Json = serde_json::from_str(&line).unwrap();
        if message.get("id").is_some() {
            break message;
        }
    };
    drop(stdin);
    let mut said = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut said).unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(0), "{said}");
    (answer, said)
}

fn call(id: u64) -> Json {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": ORDER_GET,
            "arguments": {"order_id": "A-100"},
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
                "io.modelcontextprotocol/clientInfo": {"name": "raw", "version": "0"},
            },
        },
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bridge_starts_an_engine_and_the_engine_stops_by_itself_when_idle() {
    let app = TestApp::start().await.unwrap();
    let machine = published(&app).await;
    let mut bridge = starting(&machine, &["bridge"]);
    bridge.env("A2M_ENGINE__IDLE_STOP_MINUTES", "1");
    let (answer, said) = tokio::task::spawn_blocking(move || bridged(bridge, &call(1)))
        .await
        .unwrap();
    let run = &answer["result"]["structuredContent"];
    assert_eq!(run["status"], "completed", "{answer} {said}");
    assert_eq!(run["result"]["customer"], "Example Customer One");
    assert_eq!(started(&machine).len(), 1, "{}", log(&machine));

    // The bridge has gone and nothing is in hand: after a minute the
    // engine stops by itself, and its lock is free.
    assert_eq!(gone(&machine).await["result"]["stopped"], "idle");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_engine_in_process_on_nothing_is_started_unless_asked_by_name() {
    let app = TestApp::start().await.unwrap();
    let machine = published(&app).await;

    // A bridge cannot run the engine inside itself, so it says what to do.
    let bridge = machine.program(&["bridge"]);
    let (answer, said) = tokio::task::spawn_blocking(move || bridged(bridge, &call(1)))
        .await
        .unwrap();
    assert_eq!(answer["error"]["code"], -32000, "{answer}");
    let data = answer["error"]["data"].to_string();
    assert!(data.contains("engine_unreachable"), "{answer}");
    assert!(data.contains("engine.in_process"), "{answer}");
    assert!(data.contains("anything2mcp serve"), "{answer}");
    assert!(said.contains("anything2mcp serve"), "{said}");

    // A command runs the engine inside itself, and starts nothing.
    assert_read(
        &output(machine.program(&READ)).await,
        "Example Customer One",
    );
    assert!(started(&machine).is_empty(), "{}", log(&machine));

    // `engine start` starts one by name, and the second finds it running.
    let first = output(machine.program(&["engine", "start"])).await;
    assert_eq!(first.status.code(), Some(0), "{first:?}");
    assert_eq!(object(&first)["result"]["started"], true, "{first:?}");
    let again = output(machine.program(&["engine", "start"])).await;
    assert_eq!(object(&again)["result"]["started"], false, "{again:?}");
    assert_eq!(started(&machine).len(), 1, "{}", log(&machine));
    stop(&machine).await;
}
