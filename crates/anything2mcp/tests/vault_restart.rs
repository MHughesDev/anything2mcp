//! The vault across a restart, through the real program (package 2.2): a
//! secret stored with `secret set` is read by the next engine after the
//! first one stopped; an engine given the wrong passphrase stops with
//! `vault_passphrase_mismatch`; and the secret, and the passphrase, are in no
//! file under the engine's home or the project.

#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use a2m_actions::Paths;
use a2m_local::EngineLock;
use anything2mcp::PASSPHRASE_VARIABLE;
use common::{IN_PROCESS_VARIABLE, Machine, PASSPHRASE};
use example_app::harness::{BEARER_TOKEN, TestApp};
use serde_json::Value as Json;

const READ: [&str; 5] = [
    "tool",
    "call",
    "example_order_get",
    "--input",
    r#"{"order_id": "A-100"}"#,
];
/// A made-up passphrase that is not the machine's.
const WRONG: &str = "invented-wrong-passphrase-0d2b71";

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

/// Stops the engine a command started, and waits until it is gone.
async fn stop(machine: &Machine) {
    let stopped = output(machine.program(&["engine", "stop"])).await;
    assert_eq!(stopped.status.code(), Some(0), "{stopped:?}");
    no_engine(machine).await;
}

fn files_under(folder: &Path, out: &mut Vec<PathBuf>) {
    let Ok(listing) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in listing {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Nothing under the machine's home or its project holds any of `needles`:
/// the store, the key records, the evidence, the traces, and the logs.
fn in_no_file(machine: &Machine, needles: &[&str]) {
    let mut files = Vec::new();
    files_under(machine.home.path(), &mut files);
    files_under(machine.project.path(), &mut files);
    assert!(!files.is_empty());
    for path in files {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        for needle in needles {
            assert!(!text.contains(needle), "{}", path.display());
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stored_secret_survives_a_restart_and_the_wrong_passphrase_stops_by_name() {
    let app = TestApp::start().await.unwrap();
    let machine =
        Machine::new("secured", &[("http://127.0.0.1:4010", &app.url(""))]).with_default_vault();
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    world.close().await;

    // The first engine stores the secret, and storing it gives nothing back.
    let stored = output(starting(
        &machine,
        &[
            "secret",
            "set",
            "example/service/token",
            "--value",
            BEARER_TOKEN,
        ],
    ))
    .await;
    assert_eq!(stored.status.code(), Some(0), "{stored:?}");
    assert!(!String::from_utf8_lossy(&stored.stdout).contains(BEARER_TOKEN));
    assert!(!String::from_utf8_lossy(&stored.stderr).contains(BEARER_TOKEN));
    stop(&machine).await;

    // An engine started afterwards, in a new process, reads with it.
    let read = output(starting(&machine, &READ)).await;
    assert_eq!(read.status.code(), Some(0), "{read:?}");
    let held = app.stored_order("A-100").await.unwrap().unwrap();
    assert_eq!(
        object(&read)["result"]["result"]["customer"],
        held.customer.as_str()
    );
    assert!(!String::from_utf8_lossy(&read.stdout).contains(BEARER_TOKEN));
    assert!(!String::from_utf8_lossy(&read.stderr).contains(BEARER_TOKEN));
    stop(&machine).await;

    // The wrong passphrase: the engine stops by the vault's own name, leaves
    // no engine running, and says nothing of either passphrase.
    let mut wrong = starting(&machine, &READ);
    wrong.env(PASSPHRASE_VARIABLE, WRONG);
    let refused = output(wrong).await;
    assert_eq!(refused.status.code(), Some(4), "{refused:?}");
    assert_eq!(
        object(&refused)["error"]["code"],
        "vault_passphrase_mismatch",
        "{refused:?}"
    );
    for stream in [&refused.stdout, &refused.stderr] {
        let text = String::from_utf8_lossy(stream);
        assert!(
            !text.contains(WRONG) && !text.contains(PASSPHRASE) && !text.contains(BEARER_TOKEN)
        );
    }
    no_engine(&machine).await;

    // The right passphrase still works, and the secret is in no file.
    let again = output(starting(&machine, &READ)).await;
    assert_eq!(again.status.code(), Some(0), "{again:?}");
    stop(&machine).await;
    in_no_file(&machine, &[BEARER_TOKEN, PASSPHRASE, WRONG]);
}
