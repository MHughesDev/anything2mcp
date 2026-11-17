//! The stdio form: one program that reads newline-delimited JSON-RPC messages
//! on its standard input and writes its answers on its standard output.
//!
//! The program keeps its state in a folder, so that the harness can read the
//! ground truth and set faults from outside, and so that a program started
//! again over the same folder still holds what an earlier one committed.

use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use crate::protocol::{self, Incoming, Reply};
use crate::state::{Backing, Conn, Transport};

/// The environment variable that names the state folder.
pub const STATE_ENV: &str = "EXAMPLE_MCP_STATE";
/// The environment variable that carries the credential, when one is asked for.
pub const TOKEN_ENV: &str = "EXAMPLE_MCP_TOKEN";
/// The exit status of a program that dropped its connection on purpose.
pub const EXIT_DROPPED: i32 = 3;

/// How often the program looks in its state folder for a request to announce
/// that the tool list changed. It also looks each time a message arrives.
const WATCH_EVERY: Duration = Duration::from_millis(20);

type Out = Arc<Mutex<io::Stdout>>;

fn write_line(out: &Out, line: &str) {
    let mut out = out.lock().unwrap_or_else(PoisonError::into_inner);
    // A client that stopped reading has gone; there is nobody to tell.
    let _ = writeln!(out, "{line}").and_then(|()| out.flush());
}

fn deliver(out: &Out, reply: Reply) {
    match reply {
        Reply::Message(message) => write_line(out, &message.to_string()),
        Reply::Raw(text) => write_line(out, &text),
        Reply::Drop => std::process::exit(EXIT_DROPPED),
        // A status is an HTTP answer; the fault that gives one never applies here.
        Reply::Nothing | Reply::Status(_) => {}
    }
}

fn send_all(out: &Out, notifications: &[Value]) {
    for notification in notifications {
        write_line(out, &notification.to_string());
    }
}

/// Serves one connection on this process's standard input and output until
/// the input closes. With no state folder the state lives in memory and ends
/// with the program.
///
/// # Errors
///
/// Returns an error if the input cannot be read or the state folder cannot be
/// read or written.
pub fn run(state_folder: Option<&Path>, token: Option<&str>) -> io::Result<()> {
    let backing = Arc::new(match state_folder {
        Some(folder) => Backing::Folder(folder.to_owned()),
        None => Backing::memory(),
    });
    let conn = Arc::new(Mutex::new(Conn::new(Transport::Stdio)));
    let out: Out = Arc::new(Mutex::new(io::stdout()));

    {
        let (backing, conn, out) = (backing.clone(), conn.clone(), out.clone());
        thread::spawn(move || {
            loop {
                thread::sleep(WATCH_EVERY);
                let mut conn = conn.lock().unwrap_or_else(PoisonError::into_inner);
                if let Ok(owed) = backing.with(|state| protocol::announcements(state, &mut conn)) {
                    send_all(&out, &owed);
                }
            }
        });
    }

    for line in io::stdin().lock().lines() {
        let line = line?;
        let raw = line.trim();
        if raw.is_empty() {
            continue;
        }
        // Held until the answer is written or handed to a waiting thread, so
        // that a notice found while handling a message is sent before its answer.
        let mut conn = conn.lock().unwrap_or_else(PoisonError::into_inner);
        let outcome = backing.with(|state| {
            let incoming = Incoming {
                raw,
                header_revision: None,
                credential: state.credential(token),
            };
            protocol::dispatch(state, &mut conn, &incoming)
        })?;
        send_all(&out, &outcome.notifications);
        if outcome.delay_ms == 0 {
            deliver(&out, outcome.reply);
        } else {
            // A slow answer does not hold up the messages behind it, so
            // answers can leave in another order than their requests came.
            let out = out.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(outcome.delay_ms));
                deliver(&out, outcome.reply);
            });
        }
    }

    let conn = conn.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(id) = &conn.id {
        backing.with(|state| state.record(id, Transport::Stdio).open = false)?;
    }
    Ok(())
}
