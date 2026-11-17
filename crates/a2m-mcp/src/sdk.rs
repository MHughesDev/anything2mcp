//! The seam: everything that names the SDK crate.
//!
//! The SDK parses MCP messages, answers the handshake and discovery from
//! what [`Handler`] says of itself, agrees a revision for a request that
//! names one, and carries a call's progress and its result over Streamable
//! HTTP with no session. The handler lists and calls tools through
//! [`Core`], and takes the caller from the request that [`crate::http`]
//! already checked. Nothing outside this module sees an SDK type, so the
//! SDK can be replaced behind the list of messages in the crate's design.

use std::borrow::Cow;
use std::sync::Arc;

use a2m_actions::Caller;
use axum::body::Body;
use axum::http::request::Parts;
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock,
    Implementation, ListToolsResult, MetaObject, PaginatedRequestParams, ProgressNotificationParam,
    ProtocolVersion, ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::{Value as Json, json};

use crate::http::{HttpRequest, HttpResponse};
use crate::settings::Revision;
use crate::tools::{Called, Core, Rendered, ToolEntry};

/// The name the server gives itself.
pub(crate) const SERVER_NAME: &str = "anything2mcp";
/// The key of the server's name and version in a result's `_meta`.
const SERVER_INFO_KEY: &str = "io.modelcontextprotocol/serverInfo";
/// How long a client may keep a tool list, in milliseconds.
const LIST_TTL_MS: u64 = 60_000;
/// The steps a call's progress counts: handed to the dispatcher, answered.
const PROGRESS_STEPS: f64 = 2.0;

/// The caller of one request, as the HTTP layer checked it.
#[derive(Clone)]
pub(crate) struct Verified(pub(crate) Caller);

#[derive(Clone)]
struct Handler {
    core: Arc<Core>,
}

fn revision(revision: Revision) -> ProtocolVersion {
    match revision {
        Revision::Handshake => ProtocolVersion::V_2025_11_25,
        Revision::Stateless => ProtocolVersion::V_2026_07_28,
    }
}

fn server_info() -> Json {
    json!({"name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION")})
}

/// The `_meta` of a result. A request that named its revision is answered
/// with the server's name in it, as the stateless revision asks.
fn result_meta(
    context: &RequestContext<RoleServer>,
    mut meta: serde_json::Map<String, Json>,
) -> Option<MetaObject> {
    if context.meta.protocol_version().is_some() {
        meta.insert(SERVER_INFO_KEY.to_owned(), server_info());
    }
    (!meta.is_empty()).then_some(MetaObject(meta))
}

fn caller(context: &RequestContext<RoleServer>) -> Result<Caller, ErrorData> {
    context
        .extensions
        .get::<Parts>()
        .and_then(|parts| parts.extensions.get::<Verified>())
        .map(|verified| verified.0.clone())
        .ok_or_else(|| ErrorData::internal_error("The request carries no checked caller.", None))
}

fn tool(entry: ToolEntry) -> Tool {
    let notes = entry.annotations;
    Tool::new_with_raw(
        entry.name,
        Some(Cow::Owned(entry.description)),
        entry.input_schema,
    )
    .with_title(entry.title)
    .with_annotations(ToolAnnotations::from_raw(
        None,
        Some(notes.read_only),
        Some(notes.destructive),
        Some(notes.idempotent),
        Some(notes.open_world),
    ))
}

impl Handler {
    fn instructions(&self) -> String {
        format!(
            "The `{}` toolset of Anything2MCP. Every tool is deterministic: the same input does the same thing. A failed call answers with one JSON object as text: class, code, message, fix, and retry_safe; do what `fix` says. A write waits for a person's approval and says so in its result.",
            self.core.toolset
        )
    }
}

impl ServerHandler for Handler {
    fn get_info(&self) -> ServerConfig {
        // A capability is named only when it is served.
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(SERVER_NAME, env!("CARGO_PKG_VERSION")))
            .with_instructions(self.instructions())
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Owned(
            self.core
                .settings
                .revisions
                .iter()
                .copied()
                .map(revision)
                .collect(),
        )
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        // A list is one page, so no cursor was ever issued.
        if request.and_then(|params| params.cursor).is_some() {
            return Err(ErrorData::invalid_params(
                "The cursor is not one this server issued.",
                None,
            ));
        }
        let caller = caller(&context)?;
        let list = self.core.list(&caller).await.map_err(|error| {
            ErrorData::internal_error(error.message.clone(), Some(error.to_json()))
        })?;
        let mut result =
            ListToolsResult::with_all_items(list.tools.into_iter().map(tool).collect());
        if context.meta.protocol_version().is_some() {
            result = result
                .with_ttl_ms(LIST_TTL_MS)
                .with_cache_scope(CacheScope::Private);
        }
        result.meta = result_meta(&context, serde_json::Map::new());
        Ok(result)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let caller = caller(&context)?;
        let token = context.meta.get_progress_token();
        let progress = |step: f64| {
            let token = token.clone();
            let peer = context.peer.clone();
            async move {
                if let Some(token) = token {
                    let note =
                        ProgressNotificationParam::new(token, step).with_total(PROGRESS_STEPS);
                    // A caller that stopped listening loses nothing: the
                    // result says everything progress does.
                    let _ = peer.notify_progress(note).await;
                }
            }
        };
        progress(1.0).await;
        // The dispatch runs to its end whether or not the caller waits for
        // it, so that a closed stream never tears a dispatch in the middle.
        // The run it started is stopped with `a2m_run_cancel`.
        let core = Arc::clone(&self.core);
        let name = request.name.to_string();
        let arguments = request.arguments.unwrap_or_default();
        let work = tokio::spawn(async move { core.call(&name, arguments, &caller).await });
        let called = tokio::select! {
            () = context.ct.cancelled() => {
                self.core.cancelled.send_modify(|count| *count += 1);
                return Err(ErrorData::internal_error("The request was cancelled.", None));
            }
            joined = work => joined.map_err(|_| {
                ErrorData::internal_error("The call ended without an answer.", None)
            })?,
        };
        let Called::Answered(rendered) = called else {
            return Err(ErrorData::invalid_params(
                format!("Unknown tool: {}", request.name),
                None,
            ));
        };
        let Rendered { face, meta } = *rendered;
        progress(PROGRESS_STEPS).await;
        let mut result = match face.structured {
            Some(structured) if !face.is_error => CallToolResult::structured(structured),
            _ => CallToolResult::error(Vec::new()),
        };
        result.is_error = Some(face.is_error);
        result.content = vec![ContentBlock::text(face.text)];
        result.meta = result_meta(&context, meta);
        Ok(result.into())
    }
}

/// The SDK's Streamable HTTP service over the handler: no session for
/// either revision, and a JSON answer where no stream is needed.
#[derive(Clone)]
pub(crate) struct Sdk {
    service: StreamableHttpService<Handler, NeverSessionManager>,
}

impl Sdk {
    pub(crate) fn new(core: Arc<Core>) -> Self {
        let config = StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_allowed_hosts(core.settings.allowed_hosts.clone())
            .with_max_request_body_bytes(core.settings.max_request_bytes);
        let handler = Handler { core };
        Self {
            service: StreamableHttpService::new(
                move || Ok(handler.clone()),
                Arc::new(NeverSessionManager::default()),
                config,
            ),
        }
    }

    /// Hands a checked request to the SDK and returns its answer.
    pub(crate) async fn handle(&self, request: HttpRequest) -> HttpResponse {
        self.service.handle(request).await.map(Body::new)
    }
}
