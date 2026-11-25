//! `anything2mcp channel`, through the real program (decision 0999): with
//! the engine's owner checked as every command checks it, it carries a
//! client's own bytes, its hello first, to the engine and back, and reads
//! none of them; with no engine it says so as one JSON object on standard
//! error, sends nothing, and starts none.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::io::{Read, Write};
use std::process::Stdio;

use a2m_actions::Paths;
use a2m_local::{
    CLIENT_LABEL_HEADER, CLIENT_VERSION_HEADER, EngineLock, LOCAL_API, LOCAL_API_HEADER,
};
use anything2mcp::cli::CHANNEL_OPEN_LINE;
use common::Machine;
use common::served::{PATIENCE, Served, lines_of};
use serde_json::Value as Json;

#[test]
fn the_channel_command_carries_the_clients_own_hello_and_calls_to_the_engine() {
    let machine = Machine::new("orders", &[]).with_passphrase();
    let engine = Served::start(&machine);
    let mut child = machine
        .program(&["channel"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let said = lines_of(child.stderr.take().unwrap());
    assert_eq!(said.recv_timeout(PATIENCE).unwrap(), CHANNEL_OPEN_LINE);
    let body = "{}";
    let request = format!(
        "GET /local/hello HTTP/1.1\r\nhost: localhost\r\n{CLIENT_VERSION_HEADER}: {}\r\n\
         {LOCAL_API_HEADER}: {LOCAL_API}\r\n{CLIENT_LABEL_HEADER}: sdk:tests\r\n\r\n\
         POST /api/v1/actions/engine.status HTTP/1.1\r\nhost: localhost\r\n\
         content-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        anything2mcp::version(),
        body.len()
    );
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(request.as_bytes()).unwrap();
    stdin.flush().unwrap();
    // The engine closes the connection after the second answer, and the
    // command ends with it, its standard input still open.
    let mut answers = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut answers)
        .unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(0), "{answers}");
    drop(stdin);
    assert!(answers.starts_with("HTTP/1.1 200"), "{answers}");
    assert!(answers.contains("\"engine_version\""), "{answers}");
    let last = &answers[answers.rfind("\r\n\r\n").unwrap() + 4..];
    let status: Json = serde_json::from_str(last).unwrap();
    assert_eq!(status["action"], "engine.status", "{status}");
    assert_eq!(status["ok"], true, "{status}");
    // Nothing else was said on standard error.
    assert!(said.try_recv().is_err());
    drop(engine);
}

#[test]
fn with_no_engine_the_channel_command_says_so_and_starts_none() {
    let machine = Machine::new("orders", &[]).with_passphrase();
    let mut command = machine.program(&["channel"]);
    // A command would start an engine here; this one never does.
    command.env_remove(common::IN_PROCESS_VARIABLE);
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let said = String::from_utf8(output.stderr).unwrap();
    assert_eq!(said.lines().count(), 1, "{said}");
    let refusal: Json = serde_json::from_str(said.trim()).unwrap();
    assert_eq!(refusal["ok"], false, "{refusal}");
    assert_eq!(refusal["error"]["code"], "engine_unreachable", "{refusal}");
    assert!(!EngineLock::held(&Paths::under(machine.home.path())).unwrap());
}
