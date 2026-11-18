//! A first run where the build has no key store of its own, through the
//! real program (decision 0220): on Linux the vault takes its root key
//! from a passphrase by default, so with an empty home, no settings file,
//! and the passphrase in the environment, `init`, `target add`, and a read
//! work; and without the passphrase a command stops by the vault's own
//! name for it, leaves no engine running, and the next command with the
//! passphrase works.

#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::process::{Command, Output};
use std::time::Duration;

use a2m_actions::Paths;
use a2m_local::EngineLock;
use anything2mcp::PASSPHRASE_VARIABLE;
use common::{Folder, IN_PROCESS_VARIABLE, Machine};
use example_app::harness::TestApp;
use serde_json::Value as Json;

const PLACEHOLDER: &str = "http://127.0.0.1:4010";
const READ: [&str; 5] = [
    "tool",
    "call",
    "example_order_get",
    "--input",
    r#"{"order_id": "A-100"}"#,
];

/// How long a test waits for what should happen at once. Only a failure
/// waits this long.
const PATIENCE: Duration = Duration::from_secs(120);

/// The real program, which starts an engine when it finds none.
fn starting(machine: &Machine, args: &[&str]) -> Command {
    let mut command = machine.program(args);
    command.env_remove(IN_PROCESS_VARIABLE);
    command
}

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

/// Waits until no engine holds the machine's lock.
async fn no_engine(machine: &Machine) {
    let paths = Paths::under(machine.home.path());
    let give_up = tokio::time::Instant::now() + PATIENCE;
    while EngineLock::held(&paths).unwrap() {
        assert!(
            tokio::time::Instant::now() < give_up,
            "an engine still runs"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// A machine as a first run finds it, with the `orders` target published by
/// a person in its project.
async fn first_machine(app: &TestApp) -> Machine {
    let machine =
        Machine::new("orders", &[(PLACEHOLDER, &app.url(""))]).with_default_vault();
    // No settings file of any layer: the default chooses the route.
    let paths = Paths::under(machine.home.path());
    assert!(!paths.installation_config.join("settings.toml").exists());
    assert!(!paths.config.join("settings.toml").exists());
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    world.close().await;
    machine
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_first_run_with_a_passphrase_and_no_settings_file_inits_adds_and_reads() {
    let app = TestApp::start().await.unwrap();
    let machine = first_machine(&app).await;

    // `init` and `target add`, in an empty folder of the person's own.
    let empty = Folder::new("first");
    let mut init = starting(&machine, &["init"]);
    init.current_dir(empty.path());
    let inited = output(init).await;
    assert_eq!(inited.status.code(), Some(0), "{inited:?}");
    assert!(empty.path().join("anything2mcp.toml").is_file());
    let address = app.url("");
    let mut add = starting(
        &machine,
        &["target", "add", "example", "--address", &address, "--kind", "http"],
    );
    add.current_dir(empty.path());
    let added = output(add).await;
    assert_eq!(added.status.code(), Some(0), "{added:?}");
    assert!(
        empty
            .path()
            .join("targets")
            .join("example")
            .join("target.yaml")
            .is_file()
    );

    // A read, through an engine started on demand that opened its vault.
    let read = output(starting(&machine, &READ)).await;
    assert_eq!(read.status.code(), Some(0), "{read:?}");
    let held = app.stored_order("A-100").await.unwrap().unwrap();
    assert_eq!(
        object(&read)["result"]["result"]["customer"],
        held.customer.as_str()
    );
    let stopped = output(machine.program(&["engine", "stop"])).await;
    assert_eq!(stopped.status.code(), Some(0), "{stopped:?}");
    no_engine(&machine).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_the_passphrase_a_command_stops_by_name_and_leaves_no_engine() {
    let app = TestApp::start().await.unwrap();
    let machine = first_machine(&app).await;

    let mut keyless = starting(&machine, &READ);
    keyless.env_remove(PASSPHRASE_VARIABLE);
    let refused = output(keyless).await;
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let error = &object(&refused)["error"];
    assert_eq!(error["code"], "vault_passphrase_missing", "{error}");
    assert!(
        error["fix"].as_str().unwrap().contains(PASSPHRASE_VARIABLE),
        "{error}"
    );
    no_engine(&machine).await;

    // The next command, given the passphrase, starts an engine and reads.
    let read = output(starting(&machine, &READ)).await;
    assert_eq!(read.status.code(), Some(0), "{read:?}");
    let held = app.stored_order("A-100").await.unwrap().unwrap();
    assert_eq!(
        object(&read)["result"]["result"]["customer"],
        held.customer.as_str()
    );
    let stopped = output(machine.program(&["engine", "stop"])).await;
    assert_eq!(stopped.status.code(), Some(0), "{stopped:?}");
    no_engine(&machine).await;
}

/// An engine started on demand that cannot open its vault ends at once,
/// with the vault's failure on its line for the command that started it,
/// and holds the engine lock no longer (decision 0220).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_engine_started_on_demand_without_its_vault_ends_and_says_why() {
    let app = TestApp::start().await.unwrap();
    let machine = first_machine(&app).await;
    let mut serve = machine.program(&["serve", "--started-on-demand"]);
    serve.env_remove(PASSPHRASE_VARIABLE);
    let ended = tokio::time::timeout(PATIENCE, output(serve)).await.unwrap();
    assert_eq!(ended.status.code(), Some(1), "{ended:?}");
    let said = String::from_utf8_lossy(&ended.stderr);
    let line = said
        .lines()
        .find(|line| line.starts_with(a2m_local::STOPPED_LINE))
        .unwrap_or_else(|| panic!("no line of the stop: {said}"));
    assert!(line.contains("vault_passphrase_missing"), "{line}");
    assert!(!line.contains(common::PASSPHRASE), "{line}");
    no_engine(&machine).await;
}
