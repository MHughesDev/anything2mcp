//! The failure codes this crate names, and the JSON-RPC error codes it
//! answers with.

use a2m_core::{Failure, FailureClass, FailureCode};

/// A message that is not a well-formed MCP message. Refused before any
/// dispatch.
pub const SCHEMA_INVALID: FailureCode = FailureCode::SCHEMA_INVALID;

/// The service was asked for a toolset or a revision it does not serve.
pub const NOT_SERVED: FailureCode = FailureCode::fixed("mcp_not_served");

/// A request came with nothing that names its caller.
pub const CALLER_MISSING: FailureCode = FailureCode::fixed("caller_missing");

/// The proxy could not read its input.
pub const STDIO_READ_FAILED: FailureCode = FailureCode::fixed("stdio_read_failed");

/// The engine may have had a tool call, and its answer was lost: the call
/// may have started a run that writes. Class `possible_commit`.
pub const ANSWER_LOST: FailureCode = FailureCode::fixed("answer_lost");

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
        "Do not call the tool again: a second call may start a second run and write twice. Find this call's run first in your recent runs (`run.list`), or tell the person that the answer was lost so that they can check.",
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
