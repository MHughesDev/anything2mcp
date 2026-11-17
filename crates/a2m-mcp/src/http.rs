//! The endpoint: what is checked before a message reaches the SDK.
//!
//! In order: the `Host`, the HTTP method, the caller, the size of the
//! body, that the body is JSON, that it is one request or one
//! notification, and that the revision it names is served. Each is
//! answered here, with a JSON-RPC error and a named failure, so that none
//! of them depends on what the service stands on. A notification is
//! answered here too: a stateless server has nothing to do with one.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use a2m_actions::{BuiltInToolset, Caller, CallerCredential, FrontDoor, Kernel};
use a2m_core::{Failure, Secret};
use a2m_policy::ToolsetName;
use axum::body::Body;
use axum::http::header::{ALLOW, AUTHORIZATION, CONTENT_TYPE, HOST};
use axum::http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode};
use serde_json::{Value as Json, json};

use crate::sdk::{Sdk, Verified};
use crate::settings::{McpSettings, Revision};
use crate::tools::Core;
use crate::{clean, codes};

/// An HTTP request, as the endpoint takes it.
pub type HttpRequest = Request<Body>;
/// An HTTP response, as the endpoint gives it.
pub type HttpResponse = Response<Body>;

/// The header that names the revision of a request.
pub(crate) const VERSION_HEADER: &str = "mcp-protocol-version";
/// The key of the revision in a request's `_meta`.
pub(crate) const VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";

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

/// One MCP endpoint, for one toolset. Cheap to clone; it holds no state
/// between requests.
#[derive(Clone)]
pub struct McpService {
    core: Arc<Core>,
    sdk: Sdk,
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
        let core = Arc::new(Core {
            kernel,
            toolset,
            builtin,
            settings: settings.checked()?,
            dispatched: AtomicU64::new(0),
            cancelled: tokio::sync::watch::channel(0).0,
        });
        Ok(Self {
            sdk: Sdk::new(Arc::clone(&core)),
            core,
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
        let answer = self.answer(request).await;
        clean::outgoing(self.core.kernel.redact(), answer).await
    }

    async fn answer(&self, request: HttpRequest) -> HttpResponse {
        let (mut parts, body) = request.into_parts();
        let settings = &self.core.settings;
        if !host_allowed(&parts.headers, &settings.allowed_hosts) {
            return plain(
                StatusCode::FORBIDDEN,
                "The Host of this request is not served.",
            );
        }
        if parts.method != Method::POST {
            // No stream is opened by the server and there is no session to
            // end, so `POST` is the one method.
            let mut response = plain(StatusCode::METHOD_NOT_ALLOWED, "Send a POST.");
            response
                .headers_mut()
                .insert(ALLOW, HeaderValue::from_static("POST"));
            return response;
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
            return plain(StatusCode::PAYLOAD_TOO_LARGE, "The request is too large.");
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
            None => return empty(StatusCode::ACCEPTED),
            Some(id) if id.is_string() || id.is_i64() || id.is_u64() => id,
            Some(_) => {
                return malformed(
                    codes::INVALID_REQUEST,
                    &Json::Null,
                    "A request's id is text or a whole number.",
                );
            }
        };
        if let Some(refused) = self.revision_rule(&mut parts.headers, &message, method, id) {
            return refused;
        }
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
        parts.extensions.insert(Verified(caller));
        self.sdk
            .handle(Request::from_parts(parts, Body::from(bytes)))
            .await
    }

    /// The caller of a request: one the mount already had checked, the
    /// peer the channel saw, or the bearer token of a server's caller. The
    /// kernel checks the last two; this build's kernel refuses every token. The token is taken out of the request,
    /// so nothing after this point can log it.
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

    /// How a revision is agreed (decision 0071), for the parts that must
    /// not depend on the SDK. Returns the refusal, or nothing when the
    /// request goes on.
    fn revision_rule(
        &self,
        headers: &mut HeaderMap,
        message: &Json,
        method: &str,
        id: &Json,
    ) -> Option<HttpResponse> {
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
        // A client that speaks both revisions falls back to the handshake
        // when discovery is an unknown method, so this comes first.
        if method == "server/discover" && !settings.serves(Revision::Stateless) {
            return Some(rpc_error(
                StatusCode::OK,
                id,
                codes::METHOD_NOT_FOUND,
                "server/discover",
                None,
            ));
        }
        let served = |text: &str| Revision::parse(text).is_some_and(|r| settings.serves(r));
        let in_header = headers
            .get(VERSION_HEADER)
            .map(|value| value.to_str().unwrap_or("").to_owned());
        if let Some(text) = in_header.as_deref().filter(|text| !served(text)) {
            return Some(unserved(text));
        }
        let in_body = message
            .pointer("/params/_meta")
            .and_then(|meta| meta.get(VERSION_KEY))
            .and_then(Json::as_str);
        if let Some(text) = in_body.filter(|text| !served(text)) {
            return Some(unserved(text));
        }
        match method {
            "initialize" if !settings.serves(Revision::Handshake) => {
                let requested = message
                    .pointer("/params/protocolVersion")
                    .and_then(Json::as_str)
                    .unwrap_or(Revision::Handshake.as_str());
                Some(unserved(requested))
            }
            "initialize" | "server/discover" => None,
            _ if in_body.is_some() => None,
            // A request that names no revision in its body is of the
            // handshake revision.
            _ if settings.serves(Revision::Handshake) => {
                if in_header.is_none() {
                    headers.insert(
                        VERSION_HEADER,
                        HeaderValue::from_static(Revision::Handshake.as_str()),
                    );
                }
                None
            }
            _ => Some(rpc_error(
                StatusCode::BAD_REQUEST,
                id,
                codes::INVALID_PARAMS,
                "The request's _meta does not name its protocol version.",
                Some(codes::failure_data(&codes::input(
                    codes::SCHEMA_INVALID,
                    "The request's `_meta` does not name its protocol version.",
                    format!(
                        "Send `{VERSION_KEY}` in `params._meta`; this server serves {}.",
                        Revision::Stateless.as_str()
                    ),
                ))),
            )),
        }
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

fn plain(status: StatusCode, text: &'static str) -> HttpResponse {
    let mut response = Response::new(Body::from(text));
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

fn empty(status: StatusCode) -> HttpResponse {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;
    response
}

/// A JSON-RPC error as an HTTP response.
pub(crate) fn rpc_error(
    status: StatusCode,
    id: &Json,
    code: i64,
    message: &str,
    data: Option<Json>,
) -> HttpResponse {
    let mut error = json!({"code": code, "message": message});
    if let Some(data) = data {
        error["data"] = data;
    }
    let body = json!({"jsonrpc": "2.0", "id": id, "error": error}).to_string();
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

/// The answer to a message that is not well formed: the JSON-RPC error,
/// and the one failure this crate adds, `input`/`schema_invalid`.
fn malformed(code: i64, id: &Json, message: &str) -> HttpResponse {
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
