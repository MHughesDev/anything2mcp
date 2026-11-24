//! The protocol layer: one JSON-RPC 2.0 message in, what to send back out.
//! Written by hand, with no MCP library, so that the stand does not share a
//! mistake with whatever the product uses to speak the protocol.

use serde_json::{Map, Value, json};

use crate::faults::{FaultKind, Malformed};
use crate::state::{CallOutcome, CallRecord, Conn, Credential, Phase, State, Transport};
use crate::store::INSTRUCTION_TEXT;
use crate::tools;

/// The revision served through the `initialize` handshake.
pub const HANDSHAKE_REVISION: &str = "2025-11-25";
/// The earlier revision served through the same handshake, as an earlier
/// release of a server serves it alone.
pub const EARLIER_HANDSHAKE_REVISION: &str = "2025-06-18";
/// The revision served with no handshake: every request names it.
pub const STATELESS_REVISION: &str = "2026-07-28";
/// Every revision the stand serves, newest first.
pub const REVISIONS: [&str; 3] = [
    STATELESS_REVISION,
    HANDSHAKE_REVISION,
    EARLIER_HANDSHAKE_REVISION,
];
/// The revisions served through the handshake, newest first.
const HANDSHAKE_REVISIONS: [&str; 2] = [HANDSHAKE_REVISION, EARLIER_HANDSHAKE_REVISION];
/// What a stand set to the earlier revision alone serves.
const EARLIER_ONLY: [&str; 1] = [EARLIER_HANDSHAKE_REVISION];

/// The revisions served through the handshake now, newest first.
fn handshake_revisions(state: &State) -> &'static [&'static str] {
    if state.settings.earlier_revision_only {
        &EARLIER_ONLY
    } else {
        &HANDSHAKE_REVISIONS
    }
}

/// Every revision served now, newest first.
fn served_revisions(state: &State) -> &'static [&'static str] {
    if state.settings.earlier_revision_only {
        &EARLIER_ONLY
    } else {
        &REVISIONS
    }
}
/// Where a request with no handshake names its revision, under `params._meta`.
///
/// Verified against the text of the 2026-07-28 revision: research note 18
/// read the key in the specification's own files.
pub const STATELESS_VERSION_META_KEY: &str = "io.modelcontextprotocol/protocolVersion";
/// The HTTP header that names the revision of a request.
pub const PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";
/// The name the server reports.
pub const SERVER_NAME: &str = "example-mcp";

/// JSON-RPC: the message is not JSON.
pub const PARSE_ERROR: i64 = -32700;
/// JSON-RPC: the message is not a request the stand can take.
pub const INVALID_REQUEST: i64 = -32600;
/// JSON-RPC: no such method.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC: bad parameters. Also an unknown tool, an unknown cursor, and a
/// revision refused in the handshake, as the protocol says.
pub const INVALID_PARAMS: i64 = -32602;
/// The protocol's own error for a request with no handshake that names a
/// revision the server does not serve. Its `data` holds `supported` and
/// `requested` (research note 18).
pub const UNSUPPORTED_VERSION: i64 = -32022;
/// The stdio program's answer when the credential it was started with is not
/// accepted. The protocol names no code for this over stdio; this one is the
/// stand's own.
pub const CREDENTIAL_REFUSED: i64 = -32001;

const LIST_CHANGED: &str = "notifications/tools/list_changed";

/// What the transport does once a message was handled.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Reply {
    /// Nothing: the message was a notification.
    Nothing,
    /// Send this message.
    Message(Value),
    /// Send this text as it is; it is not a response.
    Raw(String),
    /// Drop the connection without answering.
    Drop,
    /// Answer with this HTTP status and no message.
    Status(u16),
}

/// What handling one message gave.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Outcome {
    /// Notifications to send before the reply. Stdio only.
    pub(crate) notifications: Vec<Value>,
    /// How long to wait before the reply, in milliseconds.
    pub(crate) delay_ms: u64,
    pub(crate) reply: Reply,
    /// The HTTP status that goes with a `Reply::Message`.
    pub(crate) http_status: u16,
}

impl Outcome {
    fn of(reply: Reply) -> Self {
        Self {
            notifications: Vec::new(),
            delay_ms: 0,
            reply,
            http_status: 200,
        }
    }
}

struct RpcError {
    code: i64,
    message: String,
    data: Option<Value>,
    http_status: u16,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
            http_status: 200,
        }
    }

    fn status(mut self, http_status: u16) -> Self {
        self.http_status = http_status;
        self
    }

    /// The refusal of a revision in the handshake.
    fn unsupported_revision(requested: &str, supported: &[&str]) -> Self {
        Self {
            data: Some(json!({"supported": supported, "requested": requested})),
            ..Self::new(INVALID_PARAMS, "Unsupported protocol version").status(400)
        }
    }

    /// The refusal of a request with no handshake that names a revision the
    /// stand does not serve.
    fn unserved_revision(requested: &str, supported: &[&str]) -> Self {
        Self {
            code: UNSUPPORTED_VERSION,
            ..Self::unsupported_revision(requested, supported)
        }
    }

    fn not_initialized() -> Self {
        Self::new(
            INVALID_REQUEST,
            "The connection is not initialized, and the request names no revision that needs no handshake.",
        )
        .status(400)
    }

    fn into_outcome(self, id: &Value) -> Outcome {
        let mut error = json!({"code": self.code, "message": self.message});
        if let Some(data) = self.data {
            error["data"] = data;
        }
        Outcome {
            http_status: self.http_status,
            ..Outcome::of(Reply::Message(
                json!({"jsonrpc": "2.0", "id": id, "error": error}),
            ))
        }
    }
}

/// One message as a transport received it.
pub(crate) struct Incoming<'a> {
    /// The message text.
    pub(crate) raw: &'a str,
    /// The revision the HTTP header names, if any.
    pub(crate) header_revision: Option<&'a str>,
    /// Whether the credential that came with it is accepted.
    pub(crate) credential: Credential,
}

struct Executed {
    result: Value,
    delay_ms: u64,
}

fn broken_answer(shape: Malformed, id: &Value) -> String {
    match shape {
        Malformed::NotJson => {
            format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{\"content\":[{{\"type\":\"te")
        }
        Malformed::NotAResponse => json!({"jsonrpc": "2.0", "id": id}).to_string(),
    }
}

/// The notification that the tool list changed, if this connection is owed one.
pub(crate) fn announcements(state: &mut State, conn: &mut Conn) -> Vec<Value> {
    let asked = state.settings.announce;
    let owed = asked > conn.announced;
    let ready = conn.transport == Transport::Stdio && conn.phase == Phase::Ready;
    if ready || !owed {
        conn.announced = asked;
    }
    if ready && owed {
        state.truth.announcements += 1;
        vec![json!({"jsonrpc": "2.0", "method": LIST_CHANGED})]
    } else {
        Vec::new()
    }
}

fn initialize(state: &mut State, conn: &mut Conn, params: &Value) -> Result<Executed, RpcError> {
    let Some(requested) = params.get("protocolVersion").and_then(Value::as_str) else {
        return Err(RpcError::new(
            INVALID_PARAMS,
            "initialize needs a protocolVersion.",
        ));
    };
    // The handshake's rule: answer with the requested revision if it is
    // served, and otherwise with one that is. The client ends the connection
    // if it cannot speak that one. A test may ask for a plain refusal instead.
    let served = handshake_revisions(state);
    let answered = match served.iter().find(|revision| **revision == requested) {
        Some(revision) => *revision,
        None if state.settings.refuse_unknown_revision => {
            return Err(RpcError::unsupported_revision(requested, served).status(200));
        }
        None => served[0],
    };
    conn.phase = Phase::Initialized;
    let prefix = match conn.transport {
        Transport::Stdio => "stdio",
        Transport::Http => "session",
    };
    let id = state.register(conn, prefix);
    state.record(&id, conn.transport).revision = Some(answered.to_owned());
    Ok(Executed {
        result: json!({
            "protocolVersion": answered,
            "capabilities": {"tools": {"listChanged": conn.transport == Transport::Stdio}},
            "serverInfo": {
                "name": SERVER_NAME,
                "title": "Example order entry",
                "version": state.settings.server_version
            },
            "instructions": "Tools over the orders of the example order-entry system."
        }),
        delay_ms: 0,
    })
}

fn list_tools(state: &State, params: &Value) -> Result<Executed, RpcError> {
    let all = tools::definitions(&state.settings);
    let start = match params.get("cursor") {
        None | Some(Value::Null) => 0,
        Some(cursor) => cursor
            .as_str()
            .and_then(|c| c.strip_prefix("offset-"))
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|n| *n > 0 && *n < all.len())
            .ok_or_else(|| RpcError::new(INVALID_PARAMS, "Invalid cursor"))?,
    };
    let size = match state.settings.page_size {
        0 => all.len(),
        size => size,
    };
    let end = all.len().min(start + size);
    let mut result = json!({"tools": all[start..end]});
    if end < all.len() {
        result["nextCursor"] = json!(format!("offset-{end}"));
    }
    Ok(Executed {
        result,
        delay_ms: 0,
    })
}

fn call_tool(
    state: &mut State,
    connection: &str,
    principal: Option<&str>,
    params: &Value,
    fault: Option<&FaultKind>,
) -> Result<Executed, RpcError> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err(RpcError::new(INVALID_PARAMS, "tools/call needs a name."));
    };
    let no_arguments = Map::new();
    let arguments = match params.get("arguments") {
        None | Some(Value::Null) => &no_arguments,
        Some(Value::Object(arguments)) => arguments,
        Some(_) => {
            return Err(RpcError::new(
                INVALID_PARAMS,
                "The arguments of tools/call must be an object.",
            ));
        }
    };
    let seq = state.truth.calls.len() as u64 + 1;
    let mut record = CallRecord {
        seq,
        connection: connection.to_owned(),
        principal: principal.map(str::to_owned),
        tool: name.to_owned(),
        arguments: Value::Object(arguments.clone()),
        outcome: CallOutcome::UnknownTool,
        fault: fault.cloned(),
    };
    let listed = tools::definitions(&state.settings)
        .iter()
        .any(|tool| tool["name"] == name);
    if !listed {
        state.truth.calls.push(record);
        return Err(RpcError::new(
            INVALID_PARAMS,
            format!("Unknown tool: {name}"),
        ));
    }
    let mut run = tools::run(state, seq, name, arguments);
    record.outcome = if run.is_error {
        CallOutcome::ErrorResult
    } else {
        CallOutcome::Answered
    };
    state.truth.calls.push(record);
    match fault {
        Some(FaultKind::InjectText) => {
            if let Some(content) = run.result["content"].as_array_mut() {
                content.push(json!({"type": "text", "text": INSTRUCTION_TEXT}));
            }
        }
        Some(FaultKind::ResultOffSchema) => {
            run.result["structuredContent"] = json!({"unexpected": true});
        }
        Some(FaultKind::ResultStructured { content }) => {
            run.result["structuredContent"] = content.clone();
        }
        _ => {}
    }
    Ok(Executed {
        result: run.result,
        delay_ms: run.delay_ms,
    })
}

/// Which connection record a request belongs to, once it may be served.
fn admit(
    state: &mut State,
    conn: &mut Conn,
    params: &Value,
    header_revision: Option<&str>,
) -> Result<String, RpcError> {
    match conn.phase {
        Phase::Ready => Ok(conn.id.clone().unwrap_or_default()),
        Phase::Initialized => Err(RpcError::new(
            INVALID_REQUEST,
            "The client has not sent notifications/initialized.",
        )
        .status(400)),
        Phase::New => {
            let named = params
                .get("_meta")
                .and_then(|meta| meta.get(STATELESS_VERSION_META_KEY))
                .and_then(Value::as_str)
                .or(header_revision);
            match named {
                Some(STATELESS_REVISION) if !state.settings.earlier_revision_only => {
                    let id = match conn.transport {
                        Transport::Stdio => state.register(conn, "stdio"),
                        Transport::Http => "stateless".to_owned(),
                    };
                    state.record(&id, conn.transport).revision =
                        Some(STATELESS_REVISION.to_owned());
                    Ok(id)
                }
                Some(named) if handshake_revisions(state).contains(&named) => {
                    Err(RpcError::not_initialized())
                }
                None => Err(RpcError::not_initialized()),
                Some(other) => Err(RpcError::unserved_revision(other, served_revisions(state))),
            }
        }
    }
}

fn execute(
    state: &mut State,
    conn: &mut Conn,
    method: &str,
    params: &Value,
    incoming: &Incoming<'_>,
    fault: Option<&FaultKind>,
) -> Result<Executed, RpcError> {
    let nothing = || Executed {
        result: json!({}),
        delay_ms: 0,
    };
    match method {
        "initialize" => initialize(state, conn, params),
        "ping" => Ok(nothing()),
        "tools/list" | "tools/call" => {
            let connection = admit(state, conn, params, incoming.header_revision)?;
            let principal = incoming.credential.principal();
            if let Some(principal) = principal {
                let record = state.record(&connection, conn.transport);
                record.principals.insert(principal.to_owned());
            }
            if conn.id.is_none() {
                // A request with no session: it is counted here, because no
                // connection of its own was there to count it on.
                state.record(&connection, conn.transport).messages += 1;
            }
            if method == "tools/list" {
                list_tools(state, params)
            } else {
                call_tool(state, &connection, principal, params, fault)
            }
        }
        other => Err(RpcError::new(
            METHOD_NOT_FOUND,
            format!("Method not found: {other}"),
        )),
    }
}

/// Handles one message on one connection.
pub(crate) fn dispatch(state: &mut State, conn: &mut Conn, incoming: &Incoming<'_>) -> Outcome {
    let no_id = Value::Null;
    let Ok(message) = serde_json::from_str::<Value>(incoming.raw) else {
        return RpcError::new(PARSE_ERROR, "Parse error")
            .status(400)
            .into_outcome(&no_id);
    };
    let Some(object) = message
        .as_object()
        .filter(|o| o.get("jsonrpc") == Some(&json!("2.0")))
    else {
        // An array is a batch, which the revisions served do not have.
        return RpcError::new(INVALID_REQUEST, "Not a JSON-RPC 2.0 message")
            .status(400)
            .into_outcome(&no_id);
    };
    let id = object.get("id").filter(|id| !id.is_null());
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        if object.contains_key("result") || object.contains_key("error") {
            // A response. The stand asks its client nothing, so it matches none.
            return Outcome::of(Reply::Nothing);
        }
        return RpcError::new(INVALID_REQUEST, "The message has no method")
            .status(400)
            .into_outcome(id.unwrap_or(&no_id));
    };
    let no_params = Value::Null;
    let params = object.get("params").unwrap_or(&no_params);

    *state.truth.requests.entry(method.to_owned()).or_default() += 1;
    if conn.transport == Transport::Stdio {
        state.register(conn, "stdio");
    }
    if let Some(connection) = conn.id.clone() {
        let record = state.record(&connection, conn.transport);
        record.messages += 1;
        if let Some(principal) = incoming.credential.principal() {
            record.principals.insert(principal.to_owned());
        }
    }
    if let Some(named) = incoming.header_revision
        && method != "initialize"
        && !served_revisions(state).contains(&named)
    {
        return RpcError::unserved_revision(named, served_revisions(state))
            .into_outcome(id.unwrap_or(&no_id));
    }

    let Some(id) = id else {
        if method == "notifications/initialized" && conn.phase == Phase::Initialized {
            conn.phase = Phase::Ready;
            // A connection is told of changes made after it became ready.
            conn.announced = state.settings.announce;
        }
        return Outcome {
            notifications: announcements(state, conn),
            http_status: 202,
            ..Outcome::of(Reply::Nothing)
        };
    };

    if matches!(
        incoming.credential,
        Credential::Missing | Credential::Refused
    ) {
        state.truth.unauthorized += 1;
        return RpcError::new(
            CREDENTIAL_REFUSED,
            "Unauthorized: the credential the program was started with was not accepted.",
        )
        .into_outcome(id);
    }

    let tool = match method {
        "tools/call" => params.get("name").and_then(Value::as_str),
        _ => None,
    };
    let fault = state.take_fault(method, tool, conn.transport);
    match &fault {
        Some(FaultKind::DropBeforeHandling) => return Outcome::of(Reply::Drop),
        Some(FaultKind::HttpStatus { status }) => return Outcome::of(Reply::Status(*status)),
        Some(FaultKind::ProtocolError { code, message }) => {
            return RpcError::new(*code, message.clone()).into_outcome(id);
        }
        _ => {}
    }

    let mut outcome = match execute(state, conn, method, params, incoming, fault.as_ref()) {
        Ok(done) => Outcome {
            delay_ms: done.delay_ms,
            ..Outcome::of(Reply::Message(
                json!({"jsonrpc": "2.0", "id": id, "result": done.result}),
            ))
        },
        Err(error) => error.into_outcome(id),
    };
    match fault {
        Some(FaultKind::Delay { ms }) => outcome.delay_ms += ms,
        Some(FaultKind::DropAfterCommit) => outcome.reply = Reply::Drop,
        Some(FaultKind::Malformed { shape }) => {
            outcome.reply = Reply::Raw(broken_answer(shape, id));
            outcome.http_status = 200;
        }
        _ => {}
    }
    outcome.notifications = announcements(state, conn);
    outcome
}
