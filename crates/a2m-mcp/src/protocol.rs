//! The protocol: the methods this server answers, in both revisions.
//!
//! A request arrives here checked: it is one JSON-RPC request from a
//! caller the kernel let in, and the revision it is of is known. What is
//! served is small: the handshake and `ping` of revision 2025-11-25,
//! `server/discover` of revision 2026-07-28, and `tools/list` and
//! `tools/call` in both. Everything else is an unknown method.
//!
//! A result of the stateless revision says its type and names the server
//! in its `_meta`; a result of the handshake revision carries neither. A
//! tool call whose caller asked for progress is answered as a stream: its
//! progress, then its response. Any other request is answered with one
//! message.
//!
//! This layer is this crate's own (decision 0130). It names no outside
//! protocol library.

use std::sync::Arc;

use a2m_actions::Caller;
use axum::http::StatusCode;
use serde_json::{Map, Value as Json, json};
use tokio::sync::mpsc;

use crate::codes;
use crate::http::{Answer, error_message, rpc_error};
use crate::settings::Revision;
use crate::tools::{Called, Core, Rendered, ToolEntry};

/// The name the server gives itself.
const SERVER_NAME: &str = "anything2mcp";
/// The key of the server's name and version in a result's `_meta`.
const SERVER_INFO_KEY: &str = "io.modelcontextprotocol/serverInfo";
/// How long a client may keep what discovery or a tool list said, in
/// milliseconds.
const TTL_MS: u64 = 60_000;
/// The steps a call's progress counts: handed to the dispatcher, answered.
const PROGRESS_STEPS: u64 = 2;

fn server_info() -> Json {
    json!({"name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION")})
}

/// A capability is named only when it is served.
fn capabilities() -> Json {
    json!({"tools": {}})
}

fn instructions(core: &Core) -> String {
    format!(
        "The `{}` toolset of Anything2MCP. Every tool is deterministic: the same input does the same thing. A failed call answers with one JSON object as text: class, code, message, fix, and retry_safe; do what `fix` says. A write waits for a person's approval and says so in its result.",
        core.toolset
    )
}

/// A result as the revision of its request wants it.
fn result(stateless: bool, mut body: Map<String, Json>, mut meta: Map<String, Json>) -> Json {
    if stateless {
        body.insert("resultType".to_owned(), json!("complete"));
        meta.insert(SERVER_INFO_KEY.to_owned(), server_info());
    }
    if !meta.is_empty() {
        body.insert("_meta".to_owned(), Json::Object(meta));
    }
    Json::Object(body)
}

fn response(id: &Json, result: Json) -> Json {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn object(value: Json) -> Map<String, Json> {
    match value {
        Json::Object(map) => map,
        _ => Map::new(),
    }
}

/// The answer to a method this server does not serve.
pub(crate) fn unknown_method(id: &Json) -> Answer {
    rpc_error(
        StatusCode::OK,
        id,
        codes::METHOD_NOT_FOUND,
        "Method not found",
        None,
    )
}

/// Serves one checked request.
pub(crate) async fn serve(
    core: &Arc<Core>,
    caller: Caller,
    message: &Json,
    method: &str,
    id: &Json,
    stateless: bool,
) -> Answer {
    let params = message.get("params").unwrap_or(&Json::Null);
    match (method, stateless) {
        ("initialize", _) => Answer::Message(StatusCode::OK, response(id, initialize(core))),
        // Gone from the stateless revision.
        ("ping", false) => Answer::Message(StatusCode::OK, response(id, json!({}))),
        ("server/discover", true) => Answer::Message(StatusCode::OK, response(id, discover(core))),
        ("tools/list", _) => list(core, &caller, params, id, stateless).await,
        ("tools/call", _) => call(core, caller, params, id, stateless).await,
        _ => unknown_method(id),
    }
}

/// The handshake, answered from settings: the handshake revision whatever
/// was asked, and no session.
fn initialize(core: &Core) -> Json {
    json!({
        "protocolVersion": Revision::Handshake.as_str(),
        "capabilities": capabilities(),
        "serverInfo": server_info(),
        "instructions": instructions(core),
    })
}

fn discover(core: &Core) -> Json {
    let versions: Vec<&str> = core.settings.revisions.iter().map(|r| r.as_str()).collect();
    let body = object(json!({
        "supportedVersions": versions,
        "capabilities": capabilities(),
        "serverInfo": server_info(),
        "instructions": instructions(core),
        "ttlMs": TTL_MS,
        "cacheScope": "public",
    }));
    result(true, body, Map::new())
}

fn tool(entry: ToolEntry) -> Json {
    let notes = entry.annotations;
    json!({
        "name": entry.name,
        "title": entry.title,
        "description": entry.description,
        "inputSchema": entry.input_schema,
        "annotations": {
            "readOnlyHint": notes.read_only,
            "destructiveHint": notes.destructive,
            "idempotentHint": notes.idempotent,
            "openWorldHint": notes.open_world,
        },
    })
}

async fn list(core: &Core, caller: &Caller, params: &Json, id: &Json, stateless: bool) -> Answer {
    // A list is one page, so no cursor was ever issued.
    if params.get("cursor").is_some_and(|cursor| !cursor.is_null()) {
        return rpc_error(
            StatusCode::OK,
            id,
            codes::INVALID_PARAMS,
            "The cursor is not one this server issued.",
            None,
        );
    }
    let listed = match core.list(caller).await {
        Ok(listed) => listed,
        Err(error) => {
            return rpc_error(
                StatusCode::OK,
                id,
                codes::INTERNAL_ERROR,
                &error.message,
                Some(error.to_json()),
            );
        }
    };
    let tools: Vec<Json> = listed.tools.into_iter().map(tool).collect();
    let mut body = object(json!({"tools": tools}));
    if stateless {
        body.insert("ttlMs".to_owned(), json!(TTL_MS));
        // What a list holds is read as the caller, so it is the caller's.
        body.insert("cacheScope".to_owned(), json!("private"));
    }
    Answer::Message(
        StatusCode::OK,
        response(id, result(stateless, body, Map::new())),
    )
}

/// Counts a call whose caller went away before it was answered.
struct Waiting<'a> {
    core: &'a Core,
    answered: bool,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        if !self.answered {
            self.core.cancelled.send_modify(|count| *count += 1);
        }
    }
}

/// The message that answers a call: the result, or the protocol error for
/// a name that is no tool.
fn called(id: &Json, name: &str, outcome: Option<Called>, stateless: bool) -> Json {
    let rendered = match outcome {
        Some(Called::Answered(rendered)) => rendered,
        Some(Called::Unknown) => {
            return error_message(
                id,
                codes::INVALID_PARAMS,
                &format!("Unknown tool: {name}"),
                None,
            );
        }
        None => {
            return error_message(
                id,
                codes::INTERNAL_ERROR,
                "The call ended without an answer.",
                None,
            );
        }
    };
    let Rendered { face, meta } = *rendered;
    let mut body = Map::new();
    body.insert(
        "content".to_owned(),
        json!([{"type": "text", "text": face.text}]),
    );
    // A failure carries text alone: structured content must fit the
    // tool's output schema, and a failure does not.
    if let (false, Some(structured)) = (face.is_error, face.structured) {
        body.insert("structuredContent".to_owned(), structured);
    }
    body.insert("isError".to_owned(), json!(face.is_error));
    response(id, result(stateless, body, meta))
}

async fn call(
    core: &Arc<Core>,
    caller: Caller,
    params: &Json,
    id: &Json,
    stateless: bool,
) -> Answer {
    // The shape of the name and the arguments was checked before the
    // request came here.
    let name = params["name"].as_str().unwrap_or_default().to_owned();
    let arguments = object(params["arguments"].clone());
    let token = match params.pointer("/_meta/progressToken") {
        None | Some(Json::Null) => None,
        Some(token) if token.is_string() || token.is_i64() || token.is_u64() => Some(token.clone()),
        Some(_) => {
            return rpc_error(
                StatusCode::OK,
                id,
                codes::INVALID_PARAMS,
                "A progress token is text or a whole number.",
                None,
            );
        }
    };
    // The dispatch runs on its own task, to its end, whether or not the
    // caller waits for it: a closed stream never tears a dispatch in the
    // middle. The run it started is stopped with `a2m_run_cancel`.
    let work = {
        let (core, name) = (Arc::clone(core), name.clone());
        tokio::spawn(async move { core.call(&name, arguments, &caller).await })
    };
    let Some(token) = token else {
        // No progress was asked for: one message, when the call is done.
        let mut waiting = Waiting {
            core,
            answered: false,
        };
        let outcome = work.await.ok();
        waiting.answered = true;
        return Answer::Message(StatusCode::OK, called(id, &name, outcome, stateless));
    };
    let (messages, stream) = mpsc::channel(4);
    let (core, id) = (Arc::clone(core), id.clone());
    tokio::spawn(async move {
        // Progress says nothing the result does not: a rising count.
        let progress = |step: u64| {
            json!({
                "jsonrpc": "2.0",
                "method": "notifications/progress",
                "params": {"progressToken": token, "progress": step, "total": PROGRESS_STEPS},
            })
        };
        if messages.send(progress(1)).await.is_err() {
            core.cancelled.send_modify(|count| *count += 1);
            return;
        }
        let outcome = tokio::select! {
            // The caller cancelled, or closed the stream: stop waiting,
            // and send nothing more for this request.
            () = messages.closed() => {
                core.cancelled.send_modify(|count| *count += 1);
                return;
            }
            joined = work => joined.ok(),
        };
        if matches!(outcome, Some(Called::Answered(_))) {
            let _ = messages.send(progress(PROGRESS_STEPS)).await;
        }
        let _ = messages
            .send(called(&id, &name, outcome, stateless))
            .await;
    });
    Answer::Stream(stream)
}
