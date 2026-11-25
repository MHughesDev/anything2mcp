//! Start on demand, through the real program (decision 0165): a command or
//! a stdio proxy that finds no engine starts one, detached, and is answered by
//! it; two at once start one; an idle engine stops by itself and lets go
//! of its lock; and with `engine.in_process` on, nothing is started.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

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

/// Stops the machine's engine with `engine stop`. The engine's last object
/// names what asked it, so that a signal from anywhere else on the machine
/// is caught and named, in every test of this file (decision 0266).
async fn stop(machine: &Machine) {
    let stopped = output(machine.program(&["engine", "stop"])).await;
    assert_eq!(stopped.status.code(), Some(0), "{stopped:?}");
    let last = gone(machine).await;
    assert_eq!(last["result"]["stopped"], "asked", "{last}");
    assert_eq!(last["result"]["by"], "engine.stop", "{last}");
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

/// The real program's `stdio`, with the passphrase, so that it can start
/// an engine. Gives the answer to one request and what it said on
/// standard error.
fn through_the_stdio_proxy(mut command: Command, message: &Json) -> (Json, String) {
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
        let line = lines
            .recv_timeout(PATIENCE)
            .expect("the stdio proxy answered");
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
async fn a_stdio_proxy_starts_an_engine_and_the_engine_stops_by_itself_when_idle() {
    let app = TestApp::start().await.unwrap();
    let machine = published(&app).await;
    let mut stdio = starting(&machine, &["stdio"]);
    stdio.env("A2M_ENGINE__IDLE_STOP_MINUTES", "1");
    let (answer, said) =
        tokio::task::spawn_blocking(move || through_the_stdio_proxy(stdio, &call(1)))
            .await
            .unwrap();
    let run = &answer["result"]["structuredContent"];
    assert_eq!(run["status"], "completed", "{answer} {said}");
    assert_eq!(run["result"]["customer"], "Example Customer One");
    assert_eq!(started(&machine).len(), 1, "{}", log(&machine));

    // The stdio proxy has gone and nothing is in hand: after a minute the
    // engine stops by itself, and its lock is free. Nothing asked it, so
    // its last object names nothing.
    let last = gone(&machine).await;
    assert_eq!(last["result"]["stopped"], "idle", "{}", log(&machine));
    assert!(last["result"].get("by").is_none(), "{last}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_engine_in_process_on_nothing_is_started_unless_asked_by_name() {
    let app = TestApp::start().await.unwrap();
    let machine = published(&app).await;

    // A stdio proxy cannot run the engine inside itself, so it says what to do.
    let stdio = machine.program(&["stdio"]);
    let (answer, said) =
        tokio::task::spawn_blocking(move || through_the_stdio_proxy(stdio, &call(1)))
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

/// The read of the stock files' target, by the key both projects hold.
const STOCK_READ: [&str; 5] = [
    "tool",
    "call",
    "example_stock_get",
    "--input",
    r#"{"item_code": "I-1"}"#,
];

/// The stock files' surface, and the same surface in the root `project`.
const IN_FILES: &str = "    folder: prices\n";
const IN_PROJECT: &str = "    folder: prices\n    root: project\n";

/// Writes the stock file a project's surface names, with a description
/// that says which project holds it.
fn stock_file(project: &std::path::Path, which: &str) {
    std::fs::create_dir_all(project.join("prices")).unwrap();
    std::fs::write(
        project.join("prices/stock.csv"),
        format!("item_code,description,on_hand,note\nI-1,held in project {which},5,\n"),
    )
    .unwrap();
}

/// Project A, the machine's own, and project B, a second folder of the
/// same target files, each with its own stock file; the target is
/// published from A. Both are served by one engine, which A starts.
async fn two_projects(change: (&str, &str)) -> (Machine, common::Folder) {
    let a = Machine::new("stockfiles", &[change]).with_passphrase();
    let b = common::Folder::new("project-b");
    common::copy_changed(&common::project_source("stockfiles"), b.path(), &[change]);
    stock_file(a.project.path(), "A");
    stock_file(b.path(), "B");
    let world = a.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    world.close().await;
    (a, b)
}

/// The read, made in a project folder, through the engine of `machine`.
async fn stock_read_in(machine: &Machine, folder: &std::path::Path) -> Json {
    let mut command = starting(machine, &STOCK_READ);
    command.current_dir(folder);
    object(&output(command).await)
}

/// What a read returned, or the code it stopped with.
fn read_or_code(response: &Json) -> String {
    response["result"]["result"]["description"]
        .as_str()
        .or_else(|| response["error"]["code"].as_str())
        .or_else(|| response["result"]["failure"]["code"].as_str())
        .unwrap_or_else(|| panic!("neither a read nor a failure: {response}"))
        .to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_engine_serves_two_projects_and_each_read_resolves_its_files_in_its_own_project() {
    // The surface in the root `project`: the kernel hands each run the
    // project folder of the request that started it.
    let (a, b) = two_projects((IN_FILES, IN_PROJECT)).await;
    // A starts the engine; B is served by it, and reads B's file, never A's.
    let first = stock_read_in(&a, a.project.path()).await;
    assert_eq!(read_or_code(&first), "held in project A", "{first}");
    let second = stock_read_in(&a, b.path()).await;
    assert_eq!(read_or_code(&second), "held in project B", "{second}");
    assert_eq!(started(&a).len(), 1, "{}", log(&a));
    stop(&a).await;

    // Started from B this time, the same: neither project's reads depend
    // on which one started the engine.
    let first = stock_read_in(&a, b.path()).await;
    assert_eq!(read_or_code(&first), "held in project B", "{first}");
    let second = stock_read_in(&a, a.project.path()).await;
    assert_eq!(read_or_code(&second), "held in project A", "{second}");
    stop(&a).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_served_engine_never_resolves_a_root_against_the_project_that_started_it() {
    // The surface in the root `files`, which stands for the project folder
    // of each request (decision 0193): one engine serves both projects, and
    // each read resolves in its own project, whichever started the engine.
    // B is never given A's file, nor A B's (decisions 0192 and 0194).
    let (a, b) = two_projects((IN_FILES, IN_FILES)).await;
    // A starts the engine.
    let first = stock_read_in(&a, a.project.path()).await;
    assert_eq!(read_or_code(&first), "held in project A", "{first}");
    let second = stock_read_in(&a, b.path()).await;
    assert_eq!(read_or_code(&second), "held in project B", "{second}");
    assert_eq!(started(&a).len(), 1, "{}", log(&a));
    stop(&a).await;
    // B starts it this time; A's read is still A's.
    let first = stock_read_in(&a, b.path()).await;
    assert_eq!(read_or_code(&first), "held in project B", "{first}");
    let second = stock_read_in(&a, a.project.path()).await;
    assert_eq!(read_or_code(&second), "held in project A", "{second}");
    for (response, other) in [(&first, "A"), (&second, "B")] {
        assert!(
            !response
                .to_string()
                .contains(&format!("held in project {other}")),
            "{response}"
        );
    }
    stop(&a).await;
}

/// A settings variable, as a client's environment holds it.
const CAP: &str = "A2M_TOOLS__RESULT_CAP_BYTES";

fn warnings_of(response: &Json) -> Vec<String> {
    response["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|warning| warning.as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_whose_settings_variables_differ_from_the_engines_is_told_by_name() {
    let app = TestApp::start().await.unwrap();
    let machine = published(&app).await;
    let with_cap = |cap: Option<&str>| {
        let mut command = starting(&machine, &READ);
        command.env_remove(CAP);
        if let Some(cap) = cap {
            command.env(CAP, cap);
        }
        command
    };
    // The first starts the engine with its variable; it differs in nothing.
    let first = output(with_cap(Some("2000000"))).await;
    assert_read(&first, "Example Customer One");
    assert!(warnings_of(&object(&first)).is_empty(), "{first:?}");
    // A later client with another value, and one with none, are served by
    // the same engine, with its settings, and told so by the variable's
    // name, never its value. Nothing is started again.
    for cap in [Some("3000000"), None] {
        let later = output(with_cap(cap)).await;
        assert_read(&later, "Example Customer One");
        let warnings = warnings_of(&object(&later));
        let told: Vec<&String> = warnings
            .iter()
            .filter(|warning| warning.starts_with("settings_differ: "))
            .collect();
        assert_eq!(told.len(), 1, "{warnings:?}");
        assert!(told[0].contains(&format!("`{CAP}`")), "{told:?}");
        assert!(!told[0].contains("000000"), "{told:?}");
        assert!(
            String::from_utf8_lossy(&later.stderr).contains("warning: settings_differ: "),
            "{later:?}"
        );
    }
    // The same value again: nothing differs.
    let same = output(with_cap(Some("2000000"))).await;
    assert!(warnings_of(&object(&same)).is_empty(), "{same:?}");
    assert_eq!(started(&machine).len(), 1, "{}", log(&machine));
    stop(&machine).await;
}

const UPSERT: &str = "example_line_item_upsert";
const WRITE: &str = r#"{"order_id": "A-102", "item_code": "L-300", "description": "New rail", "quantity": 3, "length_in": "12' 6 1/2\""}"#;

/// The person approves, at the desk, the diff they were shown.
async fn approve(world: &common::World, run: &str) {
    let run_id: a2m_core::RunId = run.parse().unwrap();
    let asked = world.surface.last_ask();
    assert_eq!(asked.run, run_id);
    let person = world
        .engine
        .caller(a2m_actions::FrontDoor::Command, false)
        .principal
        .principal()
        .clone();
    let proof = a2m_actions::PresenceProof::new(
        a2m_policy::PresenceCheck::OsPrompt,
        person.id,
        world.clock.now(),
    );
    world
        .surface
        .desk()
        .approve(
            run_id,
            asked.diff_hash,
            person,
            a2m_policy::Channel::LocalPrompt,
            proof,
        )
        .await
        .unwrap();
}

/// Kills a process with no warning: it is given no chance to close.
fn kill_hard(process: u32) {
    let id = process.to_string();
    #[cfg(unix)]
    let status = Command::new("kill").args(["-KILL", &id]).status().unwrap();
    #[cfg(windows)]
    let status = Command::new("taskkill")
        .args(["/F", "/PID", &id])
        .status()
        .unwrap();
    assert!(status.success(), "{process} was not killed");
}

/// A machine whose engine, started on demand by a command that resumes an
/// approved write, was killed with no warning while the write was in
/// flight: the system stored it, and its answer was held back. Gives the
/// relay, the machine, the run, and the killed engine's process.
async fn killed_with_a_write_in_flight(
    app: &TestApp,
) -> (common::proxy::Proxy, Machine, String, u32) {
    let proxy = common::proxy::Proxy::start(&app.url("")).await;
    let machine = Machine::new("orders", &[(PLACEHOLDER, proxy.url())]).with_passphrase();
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    let rested = world
        .agent(&["tool", "call", UPSERT, "--input", WRITE])
        .await;
    assert_eq!(
        rested.result()["status"],
        "needs_approval",
        "{:?}",
        rested.finished
    );
    let run = rested.result()["run_id"].as_str().unwrap().to_owned();
    approve(&world, &run).await;
    world.close().await;

    // No engine runs. A command starts one and resumes the write through
    // it; the system stores the write, and its answer is held back.
    proxy.hold_next_write();
    let resume = starting(&machine, &["run", "resume", &run])
        .spawn()
        .unwrap();
    proxy.stored().await;
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 1);
    let first = started(&machine);
    assert_eq!(first.len(), 1, "{}", log(&machine));

    // The engine is killed with no warning. The command that waited on it
    // ends without saying the write is safe to send again.
    kill_hard(first[0]);
    let ended = tokio::task::spawn_blocking(move || resume.wait_with_output().unwrap())
        .await
        .unwrap();
    assert_ne!(ended.status.code(), Some(0), "{ended:?}");

    // The killed engine's lock is not held: the operating system let go of
    // it with the process.
    let paths = paths(&machine);
    let give_up = tokio::time::Instant::now() + PATIENCE;
    while EngineLock::held(&paths).unwrap() {
        assert!(
            tokio::time::Instant::now() < give_up,
            "the killed engine's lock is still held"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    (proxy, machine, run, first[0])
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_started_engine_killed_with_no_warning_is_replaced_by_the_next_command_and_its_write_is_made_once()
 {
    use a2m_logs::{Clock, ManualClock, SystemClock};
    let app = TestApp::start().await.unwrap();
    let (_proxy, machine, run, killed) = killed_with_a_write_in_flight(&app).await;

    // The next command on the same data folder starts a new engine, and
    // that engine answers it.
    let next = output(starting(&machine, &READ)).await;
    assert_read(&next, "Example Customer One");
    let processes = started(&machine);
    assert_eq!(processes.len(), 2, "{}", log(&machine));
    assert_ne!(processes[1], killed);
    assert!(EngineLock::held(&paths(&machine)).unwrap());
    // The new engine declares the killed one's lease lost at its start
    // (decision 0416): the run it left is stopped, its cause named, and
    // nothing is sent until it is reconciled.
    let left = output(starting(&machine, &["run", "get", &run])).await;
    assert_eq!(left.status.code(), Some(1), "{left:?}");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&left.stdout),
        String::from_utf8_lossy(&left.stderr)
    );
    assert!(said.contains("stopped"), "{said}");
    assert!(said.contains("runner_lost"), "{said}");
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 1);
    stop(&machine).await;

    // Once the killed engine's lease has run out, which in one process
    // takes at least ten minutes, and its quarantine has passed, resuming
    // reconciles the record first, finds the write, and completes the run
    // without writing again. The clock is moved, not waited for.
    let later = std::sync::Arc::new(ManualClock::starting_at(
        a2m_core::Timestamp::from_unix_millis(
            SystemClock::new().now().as_unix_millis() + 1_200_000,
        ),
    ));
    let world = machine
        .open_at(Some(
            std::sync::Arc::clone(&later) as std::sync::Arc<dyn Clock>
        ))
        .await;
    world.engine.kernel().tend().await;
    later.advance(Duration::from_secs(120));
    world.engine.kernel().tend().await;
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.result()["status"], "completed", "{:?}", done.finished);
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 1);
    let trace = world.ok(&["trace", "show", &run]).await;
    let intents = trace["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "intent")
        .count();
    assert_eq!(intents, 1, "{trace}");
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_engine_after_a_killed_one_resumes_its_run_at_once() {
    let app = TestApp::start().await.unwrap();
    let (_proxy, machine, run, _) = killed_with_a_write_in_flight(&app).await;
    let done = output(starting(&machine, &["run", "resume", &run])).await;
    assert_eq!(done.status.code(), Some(0), "{done:?}");
    assert_eq!(object(&done)["result"]["status"], "completed");
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 1);
    stop(&machine).await;
}
