//! The runtime folder of two processes of one user whose environments
//! differ (decision 0432): an engine started with the variable that names
//! the runtime folder, and a command started without it.
//!
//! On Linux the engine is started with `XDG_RUNTIME_DIR` naming a stand-in
//! for the session manager's folder of the user. A command whose
//! environment lacks the variable is answered by that engine when the
//! session folder is there, the user's and closed, and otherwise is refused
//! `engine_running` with the engine's runtime folder named in the fix. On
//! macOS the engine is started with `TMPDIR`, which no longer leads
//! anywhere, and a command without it is answered by the same engine.
//!
//! The command runs in this process, as the command line runs it, so that
//! the session folder it is given is the stand-in and never the machine's
//! own. Windows is not affected: its runtime folder is in `LOCALAPPDATA`,
//! which every process of a user has.

#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anything2mcp::PASSPHRASE_VARIABLE;
use anything2mcp::cli::{Finished, Host, READY_LINE, run_with};
use common::served::{PATIENCE, ended, lines_of};
use common::{Machine, PASSPHRASE, USER};
use serde_json::Value as Json;

/// The variables of the real environment that would say where the user's
/// folders are; each process of the test is given its own.
const PLACES: [&str; 6] = [
    "A2M_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_RUNTIME_DIR",
    "TMPDIR",
];

/// A folder of this user's that no other account may enter, with a short
/// path: macOS takes a socket path of 103 bytes at most, and the data
/// folder is three folders below the home folder there.
struct Short(PathBuf);

impl Short {
    fn new(name: &str) -> Self {
        let path = Path::new("/tmp").join(format!("a2m-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}

impl Drop for Short {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `serve` with `HOME` and the variable given, and none of the real
/// environment's places.
struct Engine(Option<Child>);

impl Engine {
    fn start(machine: &Machine, home: &Path, variable: (&str, &Path)) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_anything2mcp"));
        command.arg("serve").current_dir(machine.project.path());
        for place in PLACES {
            command.env_remove(place);
        }
        let mut child = command
            .env("HOME", home)
            .env("USER", USER)
            .env(PASSPHRASE_VARIABLE, PASSPHRASE)
            .env(variable.0, variable.1)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
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
        Self(Some(child))
    }

    fn ended(mut self) -> (Option<i32>, String) {
        ended(self.0.take().unwrap())
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Only a test that failed leaves the process running.
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A command run as the command line runs it, in this process, with `HOME`
/// alone of the places and the session folder given.
async fn command(
    machine: &Machine,
    home: &Path,
    session_folder: Option<PathBuf>,
    args: &[&str],
) -> (Finished, Json) {
    let host = Host {
        cwd: machine.project.path().to_path_buf(),
        environment: BTreeMap::from([
            ("HOME".to_owned(), home.display().to_string()),
            ("USER".to_owned(), USER.to_owned()),
            (PASSPHRASE_VARIABLE.to_owned(), PASSPHRASE.to_owned()),
        ]),
        session_folder,
        stdout_is_terminal: false,
        person_at_terminal: false,
        person: None,
        key_store: None,
        program: None,
        prompt: None,
    };
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let finished = run_with(&args, &host).await;
    let json = serde_json::from_str(finished.stdout.trim())
        .unwrap_or_else(|_| panic!("not one JSON object: {finished:?}"));
    (finished, json)
}

/// The engine answers the stop, and its process ends by itself.
async fn stopped_by(machine: &Machine, home: &Path, session: Option<PathBuf>, engine: Engine) {
    let (stop, said) = command(machine, home, session, &["engine", "stop"]).await;
    assert_eq!(stop.code, 0, "{stop:?}");
    assert_eq!(said["result"]["stopping"], true, "{said}");
    let (code, printed) = engine.ended();
    assert_eq!(code, Some(0), "{printed}");
}

#[cfg(not(target_os = "macos"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_command_without_the_runtime_variable_finds_the_engine_or_is_told_where_it_is() {
    let machine = Machine::new("orders", &[]).with_default_vault();
    let home = Short::new("rh");
    // The stand-in for `/run/user/<uid>`: the user's, and closed.
    let session = Short::new("rs");
    let engine = Engine::start(&machine, &home.0, ("XDG_RUNTIME_DIR", &session.0));
    let runtime = session.0.join("anything2mcp");
    assert!(runtime.join("engine.lock").is_file());

    // Without the session folder, the command's runtime folder is in the
    // data folder: no engine is there, and none is waited for. The
    // refusal names the engine's runtime folder and the variable that
    // leads there.
    let (refused, said) = command(&machine, &home.0, None, &["engine", "status"]).await;
    // Exit code 4: the engine is busy elsewhere, as the output contract says.
    assert_eq!(refused.code, 4, "{refused:?}");
    let error = &said["error"];
    assert_eq!(error["code"], "engine_running", "{said}");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains(runtime.to_str().unwrap()), "{said}");
    let fix = error["fix"].as_str().unwrap();
    assert!(
        fix.contains(&format!(
            "`XDG_RUNTIME_DIR` set to `{}`",
            session.0.display()
        )),
        "{said}"
    );

    // With the session folder, the command's runtime folder is the
    // engine's: the same engine answers, and stops when asked.
    let (status, said) = command(
        &machine,
        &home.0,
        Some(session.0.clone()),
        &["engine", "status"],
    )
    .await;
    assert_eq!(status.code, 0, "{status:?}");
    assert_eq!(said["action"], "engine.status", "{said}");
    stopped_by(&machine, &home.0, Some(session.0.clone()), engine).await;
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_command_without_tmpdir_is_answered_by_the_engine_started_with_it() {
    let machine = Machine::new("orders", &[]).with_default_vault();
    let home = Short::new("rh");
    let temporary = Short::new("rt");
    let engine = Engine::start(&machine, &home.0, ("TMPDIR", &temporary.0));
    // Nothing of the engine's is in the temporary folder.
    assert!(!temporary.0.join("anything2mcp").exists());
    let (status, said) = command(&machine, &home.0, None, &["engine", "status"]).await;
    assert_eq!(status.code, 0, "{status:?}");
    assert_eq!(said["action"], "engine.status", "{said}");
    stopped_by(&machine, &home.0, None, engine).await;
}
