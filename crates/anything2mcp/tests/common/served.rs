//! The real program as child processes: `serve` as the local engine, a
//! command run to its end, and `bridge` driven by raw protocol messages.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use anything2mcp::PASSPHRASE_VARIABLE;
use anything2mcp::cli::READY_LINE;
use serde_json::{Value as Json, json};

use super::Machine;

/// How long a test waits for a child process to say or do what it should
/// do at once. Only a failure waits this long.
pub const PATIENCE: Duration = Duration::from_secs(180);

/// What a child wrote on one of its streams, line by line, as it comes.
pub fn lines_of(stream: impl std::io::Read + Send + 'static) -> Receiver<String> {
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

/// Waits for a child to end, and gives its exit code and what was left on
/// its standard output.
pub fn ended(mut child: Child) -> (Option<i32>, String) {
    let stdout = child.stdout.take();
    let (send, done) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut stdout) = stdout {
            let _ = std::io::Read::read_to_string(&mut stdout, &mut text);
        }
        let _ = send.send((child.wait().unwrap().code(), text));
    });
    done.recv_timeout(PATIENCE).expect("the process ended")
}

/// The local engine as a child process: the real program's `serve`.
pub struct Served {
    child: Option<Child>,
    /// Kept so that what the engine says goes on being read.
    said: Receiver<String>,
    /// What it said before it was ready.
    heard: Vec<String>,
}

impl Served {
    /// Starts `serve` and waits for the line it writes when it answers on
    /// the local channel.
    pub fn start(machine: &Machine) -> Self {
        Self::start_with(machine, &[])
    }

    /// The same, with `variables` added to the engine's own environment.
    pub fn start_with(machine: &Machine, variables: &[(&str, &str)]) -> Self {
        let mut child = machine
            .program(&["serve"])
            .envs(variables.iter().copied())
            .spawn()
            .unwrap();
        let said = lines_of(child.stderr.take().unwrap());
        let mut heard = Vec::new();
        loop {
            match said.recv_timeout(PATIENCE) {
                Ok(line) if line == READY_LINE => break,
                Ok(line) => heard.push(line),
                Err(_) => {
                    let _ = child.kill();
                    panic!("`serve` did not become ready; it said {heard:?}");
                }
            }
        }
        // Before it is ready, `serve` says what approvals on this machine
        // need: the kind of presence check, and the reason.
        let presence = heard
            .iter()
            .find(|line| line.starts_with(anything2mcp::PRESENCE_LINE))
            .unwrap_or_else(|| panic!("`serve` did not say what approvals need: {heard:?}"));
        assert!(
            presence.contains(" os_prompt, ready") || presence.contains(" not_available, "),
            "{presence}"
        );
        Self {
            child: Some(child),
            said,
            heard,
        }
    }

    /// Waits for the engine's process to end by itself.
    pub fn ended(mut self) -> (Option<i32>, String) {
        ended(self.child.take().unwrap())
    }

    /// What can be seen of the engine now, without waiting, for the message
    /// of a test that failed: whether its process has ended, and how, and
    /// every line it has written on its standard error so far. The lines
    /// read here are kept, so [`Served::ended_saying`] still gives them.
    pub fn so_far(&mut self) -> String {
        let process = match self.child.as_mut().map(Child::try_wait) {
            Some(Ok(Some(status))) => format!("ended, {status}"),
            Some(Ok(None)) => "still running".to_owned(),
            Some(Err(error)) => format!("not known: {error}"),
            None => "already waited for".to_owned(),
        };
        while let Ok(line) = self.said.try_recv() {
            self.heard.push(line);
        }
        format!(
            "the engine's process is {process}; it said {:?}",
            self.heard
        )
    }

    /// The same, and every line it wrote on its standard error.
    pub fn ended_saying(mut self) -> (Option<i32>, String, Vec<String>) {
        let (code, printed) = ended(self.child.take().unwrap());
        let mut said = std::mem::take(&mut self.heard);
        // The stream ends with the process; the wait is bounded all the
        // same.
        while let Ok(line) = self.said.recv_timeout(PATIENCE) {
            said.push(line);
        }
        (code, printed, said)
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        // Only a test that failed leaves the process running.
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Runs a command of the real program to its end, off the test's own
/// threads, which the test application needs.
pub async fn output(mut command: Command) -> Output {
    tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap()
}

pub fn object(output: &Output) -> Json {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "not one JSON object: {} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// The real program's `bridge`, driven by raw protocol messages over its
/// standard streams.
pub struct Bridge {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    answers: Receiver<String>,
}

impl Bridge {
    pub fn start(machine: &Machine, args: &[&str]) -> Self {
        let mut command = machine.program(args);
        // The bridge holds no key and opens no store: it only copies.
        command
            .env_remove(PASSPHRASE_VARIABLE)
            .stdin(Stdio::piped());
        let mut child = command.spawn().unwrap();
        Self {
            stdin: child.stdin.take(),
            answers: lines_of(child.stdout.take().unwrap()),
            child: Some(child),
        }
    }

    pub fn send(&mut self, message: &Json) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    /// The next line of standard output, which must be one protocol
    /// message.
    pub fn next(&mut self) -> Json {
        let line = self
            .answers
            .recv_timeout(PATIENCE)
            .expect("the bridge answered");
        let message: Json = serde_json::from_str(&line)
            .unwrap_or_else(|_| panic!("standard output held something else: {line}"));
        assert_eq!(message["jsonrpc"], "2.0", "{line}");
        message
    }

    /// The response to request `id`; what comes before it is progress.
    pub fn answer(&mut self, id: u64) -> Json {
        loop {
            let message = self.next();
            if message["id"] == json!(id) {
                return message;
            }
            assert!(message.get("id").is_none(), "{message}");
        }
    }

    pub fn ask(&mut self, message: &Json) -> Json {
        self.send(message);
        self.answer(message["id"].as_u64().unwrap())
    }

    /// Ends the input, and gives the exit code, what was left on standard
    /// output, and standard error.
    pub fn close(mut self) -> (Option<i32>, Vec<String>, String) {
        drop(self.stdin.take());
        let mut child = self.child.take().unwrap();
        let stderr = lines_of(child.stderr.take().unwrap());
        let (code, _) = ended(child);
        let left: Vec<String> = self.answers.iter().collect();
        (code, left, stderr.iter().collect::<Vec<_>>().join("\n"))
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        drop(self.stdin.take());
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A request of the handshake revision.
pub fn request(id: u64, method: &str, params: Json) -> Json {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

/// A request of the stateless revision: its `_meta` names the revision,
/// the client, and the client's capabilities.
pub fn stateless(id: u64, method: &str, mut params: Json) -> Json {
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": {"name": "raw", "version": "0"},
    });
    request(id, method, params)
}

/// Every file under `folder` that can be read, with its text, read as
/// UTF-8 where it is not.
pub fn texts_under(folder: &Path, out: &mut Vec<(PathBuf, String)>) {
    let Ok(listing) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in listing {
        let path = entry.unwrap().path();
        if path.is_dir() {
            texts_under(&path, out);
        } else if let Ok(bytes) = std::fs::read(&path) {
            out.push((path, String::from_utf8_lossy(&bytes).into_owned()));
        }
    }
}
