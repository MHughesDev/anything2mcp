//! The failure codes this crate names, and the JSON-RPC error codes it
//! answers with.
//!
//! `stdio_read_failed` is `transient` and does not pass by itself: the
//! proxy's input is the client's end of a pipe, and once a read of it fails
//! the same proxy cannot read it again; the client must start it anew.

use a2m_core::{Failure, FailureClass, FailureCode};

/// A message that is not a well-formed MCP message. Refused before any
/// dispatch.
pub const SCHEMA_INVALID: FailureCode = FailureCode::SCHEMA_INVALID;

a2m_core::failure_codes! {
    NOT_SERVED = Input, "mcp_not_served",
        meaning "The service was asked for a toolset or a revision it does not serve.",
        next "Ask for a toolset and a revision this build serves; the message names what was asked for.";
    CALLER_MISSING = Policy, "caller_missing",
        meaning "A request came with nothing that names its caller.",
        next "Call through the local channel of the engine.";
    STDIO_READ_FAILED = Transient, "stdio_read_failed",
        meaning "The proxy could not read its input.",
        next "Start the client again.",
        passes not_by_itself;
    ANSWER_LOST = PossibleCommit, "answer_lost",
        meaning "The engine may have had a tool call, and its answer was lost: the call may have started a run that writes.",
        next "Do not call the tool again without its idempotency key: find the call's run with `run.list` first, and do what that run's `next` says.";
}

/// JSON-RPC: the body or line is not JSON.
pub const PARSE_ERROR: i64 = -32700;
/// JSON-RPC: the message is not a request.
pub const INVALID_REQUEST: i64 = -32600;
/// JSON-RPC: the method is not served.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC: the parameters are not what the method takes.
pub const INVALID_PARAMS: i64 = -32602;
/// JSON-RPC: the server failed.
pub const INTERNAL_ERROR: i64 = -32603;
/// MCP: a header disagrees with the body.
pub const HEADER_MISMATCH: i64 = -32020;
/// MCP: the revision asked for is not served.
pub const UNSUPPORTED_VERSION: i64 = -32022;
/// This server's own: the caller was not let in.
pub const CALLER_REFUSED: i64 = -32001;
/// This server's own: the proxy could not reach the engine, or the engine
/// answered with something that is not an MCP message.
pub const ENGINE_UNREACHABLE: i64 = -32000;

/// The failure of a tool call whose answer was lost after the engine may
/// have had it. It names no run, because the answer that named one is
/// what was lost.
pub(crate) fn answer_lost() -> Failure {
    Failure::new(
        FailureClass::PossibleCommit,
        ANSWER_LOST,
        "The engine may have had the call, and its answer was lost: the call may have started a run, and that run may have written.",
        "Do not call the tool again: a second call may start a second run and write twice. Find this call's run first in your recent runs: call `a2m_action_run` with `action` set to `run.list` and `input` holding `since`, the moment the call was sent, and do what that run's `next` says.",
    )
}

/// What the proxy knows of a tool call whose answer was lost: whether it
/// carried an idempotency key, the published tool it named when it named
/// one, and when it was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LostCall {
    pub(crate) keyed: bool,
    pub(crate) tool: Option<String>,
    /// Whole milliseconds since 1970 in UTC, a minute before the call was
    /// sent, so that a clock a little apart from the engine's still finds
    /// the run.
    pub(crate) since: i64,
}

/// The same failure, with the exact next step: with a key, the same call
/// again with the same key; without one, the caller's runs since the call
/// was sent, by the exact call that lists them.
pub(crate) fn answer_lost_for(call: &LostCall) -> Failure {
    let fix = if call.keyed {
        "Make the same call again, with the same idempotency key and nothing changed: it gives back the run the first call started, finished or not, and starts no other.".to_owned()
    } else {
        let mut input = serde_json::Map::new();
        input.insert("since".to_owned(), serde_json::json!(call.since));
        if let Some(tool) = &call.tool {
            input.insert("tool".to_owned(), serde_json::json!(tool));
        }
        let listing = serde_json::json!({"action": "run.list", "input": input});
        format!(
            "Do not call the tool again: a second call may start a second run and write twice. Find this call's run first: call `a2m_action_run` with {listing}, and do what that run's `next` says. A call that carries an idempotency key (`idempotency_key` of `a2m_tool_call`, or `{}` in the call's `_meta`) may be made again with the same key instead.",
            crate::tools::KEY_META
        )
    };
    Failure::new(
        FailureClass::PossibleCommit,
        ANSWER_LOST,
        "The engine may have had the call, and its answer was lost: the call may have started a run, and that run may have written.",
        fix,
    )
}

pub(crate) fn input(
    code: FailureCode,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Failure {
    Failure::new(FailureClass::Input, code, message, fix)
}

pub(crate) fn policy(
    code: FailureCode,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Failure {
    Failure::new(FailureClass::Policy, code, message, fix)
}

/// A failure as the `data` of a JSON-RPC error: its class, code, message,
/// and next step. The proxy adds `retry_safe`.
pub(crate) fn failure_data(failure: &Failure) -> serde_json::Value {
    serde_json::json!({
        "class": failure.class().to_string(),
        "code": failure.code().as_str(),
        "message": failure.message(),
        "fix": failure.next_step(),
    })
}
