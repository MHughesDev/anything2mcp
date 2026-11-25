//! `channel`: the local channel as bytes on standard input and output, for
//! a client that cannot make the check of the engine itself.
//!
//! It opens the engine's channel with the connect every client of this
//! program makes, which checks that what serves it runs as this user
//! before anything is sent (`Connector::open_checked`), and then copies
//! bytes both ways and reads none of them: no hello, no request, no
//! answer is made or looked at here. The Node client uses it on Windows,
//! where Node's own modules cannot ask who serves a named pipe (decision
//! 0466). It holds no engine logic, is not an action, and starts no
//! engine.
//!
//! What it says is on standard error, one line: [`CHANNEL_OPEN_LINE`] once
//! the checked connection is made, or else the refusal as one JSON object,
//! after which it ends. Standard output carries the engine's bytes and
//! nothing else. It ends when standard input ends, when the engine closes
//! the connection, or when standard output can no longer be written.

use std::io::{Read, Write};

use a2m_actions::{ExitClass, PROGRAM, Paths, codes};
use a2m_core::{Failure, FailureClass};
use a2m_local::{CheckedStream, ClientLabel, Connector};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{Finished, Host, render};
use crate::local::program_version;

/// The line `channel` writes to standard error once the engine's owner is
/// checked and the connection is made. A client sends nothing it has not
/// queued before it reads this line.
pub const CHANNEL_OPEN_LINE: &str = "anything2mcp: the local channel is open.";

/// How much is read at a time from either side.
const CHUNK: usize = 64 * 1024;

/// Writes one line to standard error now.
fn say(line: &str) {
    let mut error = std::io::stderr().lock();
    // A closed pipe is not this program's failure to report.
    let _ = writeln!(error, "{line}");
    let _ = error.flush();
}

/// Runs `channel`. It takes no arguments.
pub(crate) async fn run(rest: &[String], host: &Host) -> Finished {
    let refused = |failure: &Failure, exit: ExitClass| {
        say(&render::refused("channel", failure, exit)
            .to_json()
            .to_string());
        Finished {
            code: exit.exit_code(),
            stdout: String::new(),
            stderr: String::new(),
        }
    };
    if let Some(word) = rest.first() {
        return refused(
            &Failure::new(
                FailureClass::Input,
                codes::SCHEMA_INVALID,
                format!(
                    "`{PROGRAM} channel` takes no argument, and was given `{}`.",
                    super::shown(word)
                ),
                format!("Run `{PROGRAM} channel` alone."),
            ),
            ExitClass::Malformed,
        );
    }
    let opened = async {
        let paths = Paths::resolve(&host.env())?;
        let connector = Connector::new(&paths, program_version()?, ClientLabel::CommandLine)?;
        connector.open_checked().await
    };
    let stream = match opened.await {
        Ok(stream) => stream,
        Err(failure) => return refused(&failure, ExitClass::Failed),
    };
    say(CHANNEL_OPEN_LINE);
    carry(stream).await;
    Finished {
        code: 0,
        stdout: String::new(),
        stderr: String::new(),
    }
}

/// Standard input, read on a thread of its own and handed over in chunks;
/// `None` once it has ended. A thread of its own, and not the runtime's
/// blocking pool, so that a read that waits on input does not keep the
/// program from ending once the engine has closed the connection.
fn standard_input() -> tokio::sync::mpsc::Receiver<Vec<u8>> {
    let (send, receive) = tokio::sync::mpsc::channel::<Vec<u8>>(4);
    std::thread::spawn(move || {
        let mut input = std::io::stdin().lock();
        let mut buffer = vec![0_u8; CHUNK];
        loop {
            match input.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if send.blocking_send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    receive
}

/// Copies bytes both ways until one side ends.
async fn carry(stream: CheckedStream) {
    let (mut from_engine, mut to_engine) = tokio::io::split(stream);
    let mut input = standard_input();
    let mut output = tokio::io::stdout();
    let mut buffer = vec![0_u8; CHUNK];
    loop {
        tokio::select! {
            chunk = input.recv() => {
                let Some(chunk) = chunk else { break };
                if to_engine.write_all(&chunk).await.is_err() || to_engine.flush().await.is_err() {
                    break;
                }
            }
            read = from_engine.read(&mut buffer) => {
                let read = match read {
                    Ok(0) | Err(_) => break,
                    Ok(read) => read,
                };
                if output.write_all(&buffer[..read]).await.is_err() || output.flush().await.is_err() {
                    break;
                }
            }
        }
    }
    let _ = output.flush().await;
}
