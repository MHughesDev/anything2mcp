//! What asked the engine to stop, through the real program's `serve`
//! (decision 0266): the interrupt, the termination signal, and the hang-up
//! each stop it cleanly and are named in its last object and on standard
//! error; a signal sent while the kernel is still opening does the same
//! once it serves; and `engine.stop` is named with the process that sent
//! it.
//!
//! A test signals only the process it started itself, by the process id of
//! its own spawn: other tests on the same machine run engines of their own.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::io::{BufRead, BufReader};
#[cfg(unix)]
use std::process::Command;
use std::process::{Child, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use a2m_actions::Paths;
use anything2mcp::cli::READY_LINE;
use common::Machine;
use serde_json::Value as Json;

/// How long a test waits for what should happen at once. Only a failure
/// waits this long.
const PATIENCE: Duration = Duration::from_secs(120);

/// A machine with its store on disk and the vault's key from a passphrase,
/// as the real program finds it.
fn machine() -> Machine {
    Machine::new("orders", &[]).with_passphrase()
}

/// What a child writes on standard error, line by line, as it comes.
fn lines_of(stream: impl std::io::Read + Send + 'static) -> Receiver<String> {
    let (send, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            if send.send(line).is_err() {
                break;
            }
        }
    });
    lines
}

/// `serve`, started by this test.
struct Served {
    child: Option<Child>,
    said: Receiver<String>,
    heard: Vec<String>,
}

impl Served {
    fn spawn(machine: &Machine) -> Self {
        let mut child = machine.program(&["serve"]).spawn().unwrap();
        let said = lines_of(child.stderr.take().unwrap());
        Self {
            child: Some(child),
            said,
            heard: Vec::new(),
        }
    }

    /// Started and answering.
    fn ready(machine: &Machine) -> Self {
        let mut served = Self::spawn(machine);
        loop {
            match served.said.recv_timeout(PATIENCE) {
                Ok(line) if line == READY_LINE => break,
                Ok(line) => served.heard.push(line),
                Err(_) => panic!("`serve` did not become ready: {:?}", served.heard),
            }
        }
        served
    }

    #[cfg(unix)]
    fn id(&self) -> u32 {
        self.child.as_ref().unwrap().id()
    }

    /// Sends the signal named to this test's own child, by its process id.
    #[cfg(unix)]
    fn signal(&self, name: &str) {
        let sent = Command::new("kill")
            .arg(format!("-{name}"))
            .arg(self.id().to_string())
            .status()
            .unwrap();
        assert!(sent.success(), "kill -{name}");
    }

    /// Waits for the process to end, and gives its exit code, its last
    /// object, and everything it said on standard error.
    fn ended(mut self) -> (Option<i32>, Json, Vec<String>) {
        let mut child = self.child.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, done) = mpsc::channel();
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut { stdout }, &mut text);
            let _ = send.send((child.wait().unwrap().code(), text));
        });
        let (code, printed) = done.recv_timeout(PATIENCE).expect("`serve` ended");
        // Standard error is closed once the process has ended.
        let mut heard = std::mem::take(&mut self.heard);
        while let Ok(line) = self.said.recv_timeout(PATIENCE) {
            heard.push(line);
        }
        let last = printed
            .lines()
            .rev()
            .find_map(|line| serde_json::from_str::<Json>(line).ok())
            .unwrap_or_else(|| panic!("no last object: {printed} / {heard:?}"));
        (code, last, heard)
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        // Only a test that failed leaves its own child running.
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn socket(machine: &Machine) -> std::path::PathBuf {
    Paths::under(machine.home.path())
        .runtime
        .join("engine.sock")
}

/// Asserts that `serve` stopped cleanly because `by` asked: the last
/// object says so, the line on standard error says so, and the socket is
/// gone.
#[cfg(unix)]
fn asked_by(machine: &Machine, ended: &(Option<i32>, Json, Vec<String>), by: &str) {
    let (code, last, heard) = ended;
    assert_eq!(*code, Some(0), "{last} {heard:?}");
    assert_eq!(last["action"], "serve", "{last}");
    assert_eq!(last["result"]["stopped"], "asked", "{last}");
    assert_eq!(last["result"]["by"], by, "{last}");
    let told = format!("asked to stop by the signal {by}.");
    assert!(heard.iter().any(|line| line.ends_with(&told)), "{heard:?}");
    assert!(!socket(machine).exists(), "the socket was left behind");
}

#[cfg(unix)]
#[test]
fn serve_names_the_signal_that_stopped_it() {
    for (signal, name) in [("TERM", "SIGTERM"), ("INT", "SIGINT"), ("HUP", "SIGHUP")] {
        let machine = machine();
        let served = Served::ready(&machine);
        served.signal(signal);
        asked_by(&machine, &served.ended(), name);
    }
}

#[cfg(unix)]
#[test]
fn a_signal_while_the_kernel_opens_stops_the_engine_cleanly() {
    let machine = machine();
    let served = Served::spawn(&machine);
    // The socket is bound before the kernel opens and the vault's key is
    // derived from the passphrase: the signal is sent the moment it is
    // there, which is while the kernel opens.
    let give_up = Instant::now() + PATIENCE;
    while !socket(&machine).exists() {
        assert!(Instant::now() < give_up, "no socket was bound");
        std::thread::sleep(Duration::from_millis(1));
    }
    served.signal("TERM");
    asked_by(&machine, &served.ended(), "SIGTERM");
}

#[test]
fn engine_stop_is_named_with_its_process() {
    let machine = machine();
    let served = Served::ready(&machine);
    let mut stop = machine.program(&["engine", "stop"]);
    stop.env_remove(anything2mcp::PASSPHRASE_VARIABLE)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let stopping = stop.spawn().unwrap();
    let process = stopping.id();
    let answered = stopping.wait_with_output().unwrap();
    assert_eq!(answered.status.code(), Some(0), "{answered:?}");
    let (code, last, heard) = served.ended();
    assert_eq!(code, Some(0), "{last} {heard:?}");
    assert_eq!(last["result"]["stopped"], "asked", "{last}");
    assert_eq!(last["result"]["by"], "engine.stop", "{last}");
    // The process that sent it, where the system names the peer of a
    // connection: always on Linux.
    let named = last["result"].get("process");
    if cfg!(target_os = "linux") || named.is_some() {
        assert_eq!(named, Some(&Json::from(process)), "{last}");
    }
    assert!(
        heard
            .iter()
            .any(|line| line.contains("asked to stop by engine.stop from")),
        "{heard:?}"
    );
    assert!(!socket(&machine).exists(), "the socket was left behind");
}
