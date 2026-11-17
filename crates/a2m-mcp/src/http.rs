//! The endpoint over HTTP: what is checked before a message is served,
//! and how an answer leaves.
//!
//! In order: the `Host`, the HTTP method, the content types, the caller,
//! the size of the body, that the body is JSON, that it is one request or
//! one notification, the revision it names, and the shape of a tool call.
//! Each is answered here, with a JSON-RPC error and a named failure. A
//! notification is answered here too: a stateless server has nothing to
//! do with one. What passes is served by [`crate::protocol`].
//!
//! An answer is one JSON message, or a stream of events when the caller
//! asked for progress. Every text in every message passes the redactor on
//! its way out.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::task::{Context, Poll, ready};

use a2m_actions::{BuiltInToolset, Caller, CallerCredential, FrontDoor, Kernel};
use a2m_core::{Failure, Redact, Secret};
use a2m_policy::ToolsetName;
use axum::body::{Body, Bytes};
use axum::http::header::{ACCEPT, ALLOW, AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, HOST};
use axum::http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode};
use serde_json::{Value as Json, json};
use tokio::sync::mpsc;

use crate::settings::{McpSettings, Revision};
use crate::tools::Core;
use crate::{clean, codes, protocol};

/// An HTTP request, as the endpoint takes it.
pub type HttpRequest = Request<Body>;
/// An HTTP response, as the endpoint gives it.
pub type HttpResponse = Response<Body>;

/// The header that names the revision of a request.
pub(crate) const VERSION_HEADER: &str = "mcp-protocol-version";
/// The header that repeats the method of a request.
pub(crate) const METHOD_HEADER: &str = "mcp-method";
/// The header that repeats the name of the tool, prompt, or resource.
pub(crate) const NAME_HEADER: &str = "mcp-name";
/// The key of the revision in a request's `_meta`.
pub(crate) const VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";
/// The key of the client's capabilities in a request's `_meta`.
const CAPABILITIES_KEY: &str = "io.modelcontextprotocol/clientCapabilities";

/// The path an endpoint is mounted at, and the proxy posts to.
#[must_use]
pub fn mcp_path(toolset: &ToolsetName) -> String {
    format!("/mcp/{toolset}")
}

/// Who the local channel saw connect. Whoever mounts the endpoint puts one
/// in each request's extensions, or a `Caller` the kernel already checked;
/// the endpoint has the kernel check a peer and trusts nothing else about
/// the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpPeer {
    /// The operating-system user at the other end of the channel.
    pub user: String,
    /// A label for the client, for the record. Never trusted.
    pub label: String,
}

/// What the endpoint answers, before it is put on the wire.
pub(crate) enum Answer {
    /// A status and a sentence, for what is refused before a message is
    /// read.
    Plain(StatusCode, &'static str),
    /// A status and no body.
    Empty(StatusCode),
    /// One JSON-RPC message.
    Message(StatusCode, Json),
    /// The messages of one request as they come: its progress, then its
    /// response. Dropping the receiver is how the sender learns that the
    /// caller has gone.
    Stream(mpsc::Receiver<Json>),
}

/// The messages of a stream as server-sent events, each redacted as it
/// passes.
struct Events {
    messages: mpsc::Receiver<Json>,
    redact: Arc<dyn Redact>,
}

impl futures_core::Stream for Events {
    type Item = Result<Bytes, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let next = ready!(self.messages.poll_recv(cx));
        Poll::Ready(next.map(|mut message| {
            clean::walk(self.redact.as_ref(), &mut message);
            Ok(Bytes::from(format!("data: {message}\n\n")))
        }))
    }
}

/// One MCP endpoint, for one toolset. Cheap to clone; it holds no state
/// between requests.
#[derive(Clone)]
pub struct McpService {
    core: Arc<Core>,
}

impl std::fmt::Debug for McpService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpService")
            .field("toolset", &self.core.toolset)
            .finish_non_exhaustive()
    }
}

impl McpService {
    /// The endpoint of `toolset`.
    ///
    /// # Errors
    ///
    /// `input`/`mcp_not_served` for a toolset other than `use` and `build`,
    /// which are the two this build lists, and for settings that name no
    /// revision.
    pub fn new(
        kernel: Kernel,
        toolset: ToolsetName,
        settings: McpSettings,
    ) -> Result<Self, Failure> {
        let builtin = [BuiltInToolset::Use, BuiltInToolset::Build]
            .into_iter()
            .find(|known| known.as_str() == toolset.as_str())
            .ok_or_else(|| {
                codes::input(
                    codes::NOT_SERVED,
                    format!("The toolset `{toolset}` is not served over MCP by this build."),
                    "Serve `use` or `build`.",
                )
            })?;
        Ok(Self {
            core: Arc::new(Core {
                kernel,
                toolset,
                builtin,
                settings: settings.checked()?,
                dispatched: AtomicU64::new(0),
                cancelled: tokio::sync::watch::channel(0).0,
            }),
        })
    }

    /// The toolset this endpoint serves.
    #[must_use]
    pub fn toolset(&self) -> &ToolsetName {
        &self.core.toolset
    }

    /// The endpoint as a router with one route, for the product crate to
    /// nest at [`mcp_path`]. Stateless Streamable HTTP.
    pub fn http(self) -> axum::Router {
        axum::Router::new().fallback(move |request: HttpRequest| {
            let service = self.clone();
            async move { service.handle(request).await }
        })
    }

    /// Answers one HTTP request. Everything in the answer passes the
    /// redactor on its way out, the protocol's own errors included: they
    /// can quote what the caller sent.
    pub async fn handle(&self, request: HttpRequest) -> HttpResponse {
        let redact = self.core.kernel.redact();
        let (status, kind, body) = match self.answer(request).await {
            Answer::Plain(status, text) => (status, "text/plain; charset=utf-8", Body::from(text)),
            Answer::Empty(status) => {
                let mut response = Response::new(Body::empty());
                *response.status_mut() = status;
                return response;
            }
            Answer::Message(status, mut message) => {
                clean::walk(redact.as_ref(), &mut message);
                (status, "application/json", Body::from(message.to_string()))
            }
            Answer::Stream(messages) => (
                StatusCode::OK,
                "text/event-stream",
                Body::from_stream(Events { messages, redact }),
            ),
        };
        let mut response = Response::new(body);
        *response.status_mut() = status;
        let headers = response.headers_mut();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(kind));
        if status == StatusCode::METHOD_NOT_ALLOWED {
            headers.insert(ALLOW, HeaderValue::from_static("POST"));
        }
        if kind == "text/event-stream" {
            headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        }
        response
    }

    async fn answer(&self, request: HttpRequest) -> Answer {
        let (mut parts, body) = request.into_parts();
        let settings = &self.core.settings;
        if !host_allowed(&parts.headers, &settings.allowed_hosts) {
            return Answer::Plain(
                StatusCode::FORBIDDEN,
                "The Host of this request is not served.",
            );
        }
        if parts.method != Method::POST {
            // No stream is opened by the server and there is no session to
            // end, so `POST` is the one method.
            return Answer::Plain(StatusCode::METHOD_NOT_ALLOWED, "Send a POST.");
        }
        if !header_says(&parts.headers, CONTENT_TYPE.as_str(), &["application/json"]) {
            return Answer::Plain(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Send the message as application/json.",
            );
        }
        // An answer is one JSON message or a stream of events, and the
        // caller must take either.
        if !header_says(&parts.headers, ACCEPT.as_str(), &["*/*"])
            && !header_says(
                &parts.headers,
                ACCEPT.as_str(),
                &["application/json", "text/event-stream"],
            )
        {
            return Answer::Plain(
                StatusCode::NOT_ACCEPTABLE,
                "Accept both application/json and text/event-stream.",
            );
        }
        let caller = match self.caller(&mut parts).await {
            Ok(caller) => caller,
            Err((status, failure)) => {
                return rpc_error(
                    status,
                    &Json::Null,
                    codes::CALLER_REFUSED,
                    "The caller was not let in.",
                    Some(codes::failure_data(&failure)),
                );
            }
        };
        let Ok(bytes) = axum::body::to_bytes(body, settings.max_request_bytes).await else {
            return Answer::Plain(StatusCode::PAYLOAD_TOO_LARGE, "The request is too large.");
        };
        let Ok(message) = serde_json::from_slice::<Json>(&bytes) else {
            return malformed(codes::PARSE_ERROR, &Json::Null, "The body is not JSON.");
        };
        let Some(method) = message
            .as_object()
            .filter(|object| object.get("jsonrpc") == Some(&json!("2.0")))
            .and_then(|object| object.get("method"))
            .and_then(Json::as_str)
        else {
            return malformed(
                codes::INVALID_REQUEST,
                &Json::Null,
                "The message is not one JSON-RPC 2.0 request or notification.",
            );
        };
        let id = match message.get("id") {
            // A notification. Nothing is owed and nothing is kept.
            None => return Answer::Empty(StatusCode::ACCEPTED),
            Some(id) if id.is_string() || id.is_i64() || id.is_u64() => id,
            Some(_) => {
                return malformed(
                    codes::INVALID_REQUEST,
                    &Json::Null,
                    "A request's id is text or a whole number.",
                );
            }
        };
        if message.get("params").is_some_and(|given| !given.is_object()) {
            return malformed(
                codes::INVALID_PARAMS,
                id,
                "A request's parameters are an object.",
            );
        }
        let stateless = match self.revision_rule(&parts.headers, &message, method, id) {
            Ok(stateless) => stateless,
            Err(refused) => return refused,
        };
        // The one method that reaches the dispatcher with a caller's
        // input: its parameters have their shape before anything else
        // reads them.
        if method == "tools/call"
            && !(message.pointer("/params/name").is_some_and(Json::is_string)
                && message
                    .pointer("/params/arguments")
                    .is_none_or(|given| given.is_object() || given.is_null()))
        {
            return malformed(
                codes::INVALID_PARAMS,
                id,
                "A tool call names its tool as text and gives its arguments as an object.",
            );
        }
        protocol::serve(&self.core, caller, &message, method, id, stateless).await
    }

    /// The caller of a request: one the mount already had checked, the
    /// peer the channel saw, or the bearer token of a server's caller. The
    /// kernel checks the last two; this build's kernel refuses every
    /// token. The token is taken out of the request first, so nothing
    /// after this point can read it.
    async fn caller(
        &self,
        parts: &mut axum::http::request::Parts,
    ) -> Result<Caller, (StatusCode, Failure)> {
        let token = parts
            .headers
            .remove(AUTHORIZATION)
            .and_then(|value| value.to_str().ok().map(str::to_owned))
            .and_then(|value| {
                let (scheme, token) = value.split_once(' ')?;
                scheme
                    .eq_ignore_ascii_case("bearer")
                    .then(|| Secret::new(token.trim()))
            });
        if let Some(checked) = parts.extensions.get::<Caller>() {
            // A caller the mount already had the kernel check, as the REST
            // router takes it (decision 0116). Only the kernel can make
            // one. The door, the toolset, and that an agent is calling
            // are this endpoint's to say.
            let mut caller = checked.clone();
            caller.door = FrontDoor::Mcp;
            caller.is_agent = true;
            caller.toolset = Some(self.core.toolset.clone());
            return Ok(caller);
        }
        let (credential, label, status) = match (parts.extensions.get::<McpPeer>(), token) {
            (Some(peer), _) => (
                CallerCredential::LocalUser(peer.user.clone()),
                peer.label.clone(),
                StatusCode::FORBIDDEN,
            ),
            (None, Some(token)) => (
                CallerCredential::Bearer(token),
                String::new(),
                StatusCode::UNAUTHORIZED,
            ),
            (None, None) => {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    codes::policy(
                        codes::CALLER_MISSING,
                        "The request names no caller.",
                        "Call through the local channel of the engine.",
                    ),
                ));
            }
        };
        let principal = self
            .core
            .kernel
            .verify_caller(credential)
            .await
            .map_err(|failure| (status, failure))?;
        // The MCP face always says its caller is an agent.
        let mut caller = Caller::new(principal, FrontDoor::Mcp, true);
        caller.client = label;
        caller.toolset = Some(self.core.toolset.clone());
        Ok(caller)
    }

    /// How a revision is agreed (decision 0071). Returns whether the
    /// request is of the stateless revision, or the refusal.
    fn revision_rule(
        &self,
        headers: &HeaderMap,
        message: &Json,
        method: &str,
        id: &Json,
    ) -> Result<bool, Answer> {
        let settings = &self.core.settings;
        let unserved = |requested: &str| {
            let supported: Vec<&str> = settings.revisions.iter().map(|r| r.as_str()).collect();
            rpc_error(
                StatusCode::BAD_REQUEST,
                id,
                codes::UNSUPPORTED_VERSION,
                "Unsupported protocol version",
                Some(json!({"supported": supported, "requested": requested})),
            )
        };
        let missing = |what: &str| {
            let sentence = format!("The request's `_meta` does not hold `{what}`.");
            rpc_error(
                StatusCode::BAD_REQUEST,
                id,
                codes::INVALID_PARAMS,
                &sentence,
                Some(codes::failure_data(&codes::input(
                    codes::SCHEMA_INVALID,
                    sentence.clone(),
                    format!(
                        "Send `{VERSION_KEY}` and `{CAPABILITIES_KEY}` in `params._meta`, as revision {} asks.",
                        Revision::Stateless.as_str()
                    ),
                ))),
            )
        };
        let mismatch = |header: &str| {
            rpc_error(
                StatusCode::BAD_REQUEST,
                id,
                codes::HEADER_MISMATCH,
                &format!("The `{header}` header does not say what the body says."),
                None,
            )
        };
        // A client that speaks both revisions falls back to the handshake
        // when discovery is an unknown method, so this comes first.
        if method == "server/discover" && !settings.serves(Revision::Stateless) {
            return Err(protocol::unknown_method(id));
        }
        let served = |text: &str| Revision::parse(text).is_some_and(|r| settings.serves(r));
        let header = |name: &str| {
            headers
                .get(name)
                .map(|value| value.to_str().unwrap_or("").to_owned())
        };
        let in_header = header(VERSION_HEADER);
        if let Some(text) = in_header.as_deref().filter(|text| !served(text)) {
            return Err(unserved(text));
        }
        let meta = message.pointer("/params/_meta");
        let in_body = meta
            .and_then(|meta| meta.get(VERSION_KEY))
            .and_then(Json::as_str);
        if let Some(text) = in_body.filter(|text| !served(text)) {
            return Err(unserved(text));
        }
        if method == "initialize" {
            // The handshake names its revision in its own parameters, and
            // is answered from settings.
            if !settings.serves(Revision::Handshake) {
                let requested = message
                    .pointer("/params/protocolVersion")
                    .and_then(Json::as_str)
                    .unwrap_or(Revision::Handshake.as_str());
                return Err(unserved(requested));
            }
            return Ok(false);
        }
        let Some(named) = in_body else {
            // A request that names no revision in its body is of the
            // handshake revision, when that is served and nothing says
            // otherwise.
            let stateless_asked = method == "server/discover"
                || in_header.as_deref() == Some(Revision::Stateless.as_str());
            if stateless_asked || !settings.serves(Revision::Handshake) {
                return Err(missing(VERSION_KEY));
            }
            return Ok(false);
        };
        // A request that names its revision says the same in its headers.
        if in_header.as_deref() != Some(named) {
            return Err(mismatch(VERSION_HEADER));
        }
        if header(METHOD_HEADER).is_some_and(|said| said != method) {
            return Err(mismatch(METHOD_HEADER));
        }
        let name = match method {
            "tools/call" | "prompts/get" => message.pointer("/params/name"),
            "resources/read" => message.pointer("/params/uri"),
            _ => None,
        }
        .and_then(Json::as_str);
        if header(NAME_HEADER).is_some_and(|said| Some(said.as_str()) != name) {
            return Err(mismatch(NAME_HEADER));
        }
        let stateless = named == Revision::Stateless.as_str();
        if stateless
            && !meta
                .and_then(|meta| meta.get(CAPABILITIES_KEY))
                .is_some_and(Json::is_object)
        {
            return Err(missing(CAPABILITIES_KEY));
        }
        Ok(stateless)
    }

    /// Told each time a call stops waiting because its caller went away.
    #[cfg(test)]
    pub(crate) fn cancellations(&self) -> tokio::sync::watch::Receiver<u64> {
        self.core.cancelled.subscribe()
    }

    /// How many requests this endpoint has handed the dispatcher.
    #[cfg(test)]
    pub(crate) fn dispatched(&self) -> u64 {
        self.core
            .dispatched
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Whether the request's `Host` is one this endpoint serves. A request
/// with none is refused.
fn host_allowed(headers: &HeaderMap, allowed: &[String]) -> bool {
    let Some(host) = headers.get(HOST).and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    // The name without its port; an IPv6 literal keeps its colons.
    let name = match host.strip_prefix('[') {
        Some(rest) => rest.split_once(']').map_or(rest, |(inner, _)| inner),
        None => host
            .rsplit_once(':')
            .map_or(host.as_str(), |(name, _)| name),
    };
    allowed
        .iter()
        .any(|entry| entry.eq_ignore_ascii_case(&host) || entry.eq_ignore_ascii_case(name))
}

/// Whether the header `name` lists every one of `types`.
fn header_says(headers: &HeaderMap, name: &str, types: &[&str]) -> bool {
    let said: Vec<String> = headers
        .get_all(name)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|part| {
            part.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        })
        .collect();
    types
        .iter()
        .all(|wanted| said.iter().any(|part| part == wanted))
}

/// A JSON-RPC error as an answer.
pub(crate) fn rpc_error(
    status: StatusCode,
    id: &Json,
    code: i64,
    message: &str,
    data: Option<Json>,
) -> Answer {
    Answer::Message(status, error_message(id, code, message, data))
}

/// A JSON-RPC error as a message.
pub(crate) fn error_message(id: &Json, code: i64, message: &str, data: Option<Json>) -> Json {
    let mut error = json!({"code": code, "message": message});
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({"jsonrpc": "2.0", "id": id, "error": error})
}

/// The answer to a message that is not well formed: the JSON-RPC error,
/// and the failure this crate adds, `input`/`schema_invalid`.
fn malformed(code: i64, id: &Json, message: &str) -> Answer {
    let failure = codes::input(
        codes::SCHEMA_INVALID,
        message,
        "Send one JSON-RPC 2.0 request or notification as the body, as the protocol's schema gives it.",
    );
    rpc_error(
        StatusCode::BAD_REQUEST,
        id,
        code,
        message,
        Some(codes::failure_data(&failure)),
    )
}
