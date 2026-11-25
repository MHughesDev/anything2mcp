//! The engine's own log, through the real program (decision 0266): it
//! says each start and each stop of the engine and what caused it, it never
//! holds a call's input, and `engine logs` reads it where the engine runs,
//! starting nothing and making nothing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use a2m_actions::Paths;
use anything2mcp::cli::READY_LINE;
use common::{Folder, IN_PROCESS_VARIABLE, Machine};
use serde_json::Value as Json;

/// How long a test waits for what should happen at once. Only a failure
/// waits this long.
const PATIENCE: Duration = Duration::from_secs(120);

/// A string no product code writes, given in the input of refused calls.
const MARKER: &str = "zz-marker-for-the-log-7f3a91";

fn object(output: &Output) -> Json {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "not one JSON object: {} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Every file under a folder, with what it holds.
fn files_under(folder: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(folder) else {
        return found;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files_under(&path));
        } else {
            let bytes = std::fs::read(&path).unwrap();
            found.push((path, bytes));
        }
    }
    found
}

/// The log lines `engine logs` gave whose message is `message`.
fn said<'a>(lines: &'a [Json], message: &str) -> Vec<&'a Json> {
    lines
        .iter()
        .filter(|line| line["message"] == message)
        .collect()
}

#[test]
fn the_log_says_each_start_and_stop_and_never_an_input() {
    let machine = Machine::new("orders", &[]).with_passphrase();
    let paths = Paths::under(machine.home.path());

    // An engine started by hand.
    let mut child = machine.program(&["serve"]).spawn().unwrap();
    let engine = child.id();
    let stderr = child.stderr.take().unwrap();
    let (send, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if send.send(line).is_err() {
                break;
            }
        }
    });
    loop {
        let line = lines.recv_timeout(PATIENCE).expect("`serve` became ready");
        if line == READY_LINE {
            break;
        }
    }

    // Calls the engine refuses, whose input holds the marker. A command
    // with no passphrase cannot run an engine of its own: the engine that
    // runs answers them.
    let without_key = |args: &[&str]| {
        let mut command = machine.program(args);
        command.env_remove(anything2mcp::PASSPHRASE_VARIABLE);
        command.output().unwrap()
    };
    for args in [
        ["run", "get", MARKER],
        ["tool", "describe", MARKER],
        ["docs", "get", MARKER],
    ] {
        let refused = without_key(&args);
        assert_ne!(refused.status.code(), Some(0), "{args:?}: {refused:?}");
        assert_eq!(object(&refused)["ok"], false, "{args:?}");
    }

    // Asked to stop, by a process of the user.
    let mut stop = machine.program(&["engine", "stop"]);
    stop.env_remove(anything2mcp::PASSPHRASE_VARIABLE)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let stopping = stop.spawn().unwrap();
    let stopper = stopping.id();
    assert_eq!(stopping.wait_with_output().unwrap().status.code(), Some(0));
    assert_eq!(child.wait().unwrap().code(), Some(0));

    // Read where the engine ran, with none running, and with nothing to
    // stop a command from starting one: it starts none.
    let mut logs = machine.program(&["engine", "logs", "--json"]);
    logs.env_remove(IN_PROCESS_VARIABLE);
    let read = logs.output().unwrap();
    assert_eq!(read.status.code(), Some(0), "{read:?}");
    let answer = object(&read);
    assert_eq!(answer["action"], "engine.logs", "{answer}");
    let result = &answer["result"];
    assert_eq!(result["exists"], true, "{answer}");
    assert_eq!(
        Path::new(result["file"].as_str().unwrap()),
        paths.logs.join(a2m_logs::LOG_FILE_NAME)
    );
    let lines = result["lines"].as_array().unwrap();

    let started = said(lines, "engine.started");
    assert_eq!(started.len(), 1, "{lines:?}");
    let fields = &started[0]["fields"];
    assert_eq!(fields["by"], "serve", "{fields}");
    assert_eq!(fields["on_demand"], false, "{fields}");
    assert_eq!(fields["pid"], Json::from(engine), "{fields}");
    assert_eq!(fields["version"], env!("CARGO_PKG_VERSION"), "{fields}");

    let stopped = said(lines, "engine.stopped");
    assert_eq!(stopped.len(), 1, "{lines:?}");
    let fields = &stopped[0]["fields"];
    assert_eq!(fields["why"], "asked", "{fields}");
    assert_eq!(fields["by"], "engine.stop", "{fields}");
    if cfg!(target_os = "linux") {
        assert_eq!(fields["process"], Json::from(stopper), "{fields}");
    }

    // The engine reading its log started nothing: no socket, no lock held.
    assert!(!paths.runtime.join("engine.sock").exists());
    assert!(!a2m_local::EngineLock::held(&paths).unwrap());

    // No call's input reached any file of the logs folder, nor the answer.
    assert!(!String::from_utf8_lossy(&read.stdout).contains(MARKER));
    let under = files_under(&paths.logs);
    assert!(!under.is_empty());
    for (path, bytes) in under {
        assert!(
            !bytes
                .windows(MARKER.len())
                .any(|window| window == MARKER.as_bytes()),
            "the marker is in {}",
            path.display()
        );
    }

    // `--lines` gives the last lines only, and refuses a count outside 1
    // to 1,000 as malformed.
    let last = machine
        .program(&["engine", "logs", "--lines", "1"])
        .output()
        .unwrap();
    let one = object(&last)["result"]["lines"].as_array().unwrap().clone();
    assert_eq!(one.len(), 1);
    assert_eq!(&one[0], lines.last().unwrap());
    for count in ["0", "1001"] {
        let wrong = machine
            .program(&["engine", "logs", "--lines", count])
            .output()
            .unwrap();
        assert_eq!(wrong.status.code(), Some(2), "{count}: {wrong:?}");
        assert_eq!(object(&wrong)["error"]["code"], "schema_invalid");
    }
}

#[test]
fn engine_logs_with_no_home_says_so_and_makes_none() {
    let folder = Folder::new("nohome");
    let home = folder.path().join("absent");
    let read = Command::new(env!("CARGO_BIN_EXE_anything2mcp"))
        .args(["engine", "logs"])
        .current_dir(folder.path())
        .env("A2M_HOME", &home)
        .env_remove(IN_PROCESS_VARIABLE)
        .env_remove(anything2mcp::PASSPHRASE_VARIABLE)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(read.status.code(), Some(0), "{read:?}");
    let result = &object(&read)["result"];
    assert_eq!(result["exists"], false, "{result}");
    assert_eq!(result["lines"], Json::Array(Vec::new()), "{result}");
    assert!(!home.exists(), "a home was made");
}
