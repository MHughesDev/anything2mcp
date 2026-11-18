//! The stdio form of the stand: reads newline-delimited JSON-RPC messages on
//! standard input and answers on standard output. Its state folder is named
//! by `EXAMPLE_MCP_STATE`; without one, the state lives in memory and ends
//! with the program.
//!
//! Every start is written down in the state folder, as the program saw it:
//! its arguments element for element, the names of its environment, and its
//! process identifier. The arguments are data; four of them also change
//! what it does, so that a test can make it hard to stop:
//!
//! - `--stderr-megabyte`: writes a megabyte to standard error before it
//!   serves.
//! - `--grandchild`: starts a process of its own, this program again with
//!   `--linger`, which outlives it unless it is ended with it.
//! - `--silent`: reads its input and never answers.
//! - `--linger`: does nothing for ten minutes and exits; only a grandchild
//!   is started so.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{ExitCode, Stdio};
use std::time::Duration;

use example_mcp::state::StartRecord;
use example_mcp::stdio::{STATE_ENV, TOKEN_ENV, record_start, run, run_silent};

/// How long a grandchild lingers. Far longer than any test; the test asks
/// the system whether it is gone.
const LINGER: Duration = Duration::from_secs(600);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let has = |flag: &str| args.iter().any(|arg| arg == flag);
    if has("--linger") {
        std::thread::sleep(LINGER);
        return ExitCode::SUCCESS;
    }
    let folder = std::env::var_os(STATE_ENV).map(PathBuf::from);
    let grandchild = if has("--grandchild") {
        match std::env::current_exe().and_then(|me| {
            std::process::Command::new(me)
                .arg("--linger")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
        }) {
            Ok(child) => Some(child.id()),
            Err(e) => {
                eprintln!("example-mcp: could not start its grandchild: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    if let Some(folder) = &folder {
        let start = StartRecord {
            process: std::process::id(),
            args: args.clone(),
            env: std::env::vars_os()
                .map(|(name, _)| name.to_string_lossy().into_owned())
                .collect(),
            grandchild,
        };
        if let Err(e) = record_start(folder, start) {
            eprintln!("example-mcp: could not write down its start: {e}");
            return ExitCode::FAILURE;
        }
    }
    if has("--stderr-megabyte") {
        let line = [b'e'; 1023];
        let mut err = std::io::stderr().lock();
        for _ in 0..1024 {
            // Standard error that nobody reads may be closed; that is not
            // this program's failure.
            let _ = err.write_all(&line).and_then(|()| err.write_all(b"\n"));
        }
    }
    let served = if has("--silent") {
        run_silent()
    } else {
        // The credential is compared and never printed.
        let token = std::env::var(TOKEN_ENV).ok();
        run(folder.as_deref(), token.as_deref())
    };
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("example-mcp: stopped: {e}");
            ExitCode::FAILURE
        }
    }
}
