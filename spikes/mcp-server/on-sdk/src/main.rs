//! Spike 0.5, variant A: the server stands on the official Rust SDK.
//!
//! Usage: spike-mcp-on-sdk <tools.json> [revision,revision,...]
//!
//! The tool list is built from the JSON file at start. No macro declares a tool.
//!
//! With the `http` feature: spike-mcp-on-sdk <tools.json> <revisions> --http
//! serves stateless Streamable HTTP at /mcp on a port the system picks, and prints the port.

use std::{borrow::Cow, sync::Arc, time::Duration};

use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    model::{
        CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock,
        Implementation, JsonObject, ListToolsResult, PaginatedRequestParams,
        ProgressNotificationParam, ProtocolVersion, ServerCapabilities, ServerConfig, Tool,
        ToolAnnotations,
    },
    service::RequestContext,
    transport::stdio,
};
use serde_json::{Value, json};

#[derive(Clone)]
struct Spike {
    name: String,
    version: String,
    instructions: String,
    tools: Vec<Tool>,
    records: Vec<(String, JsonObject)>,
    revisions: Vec<ProtocolVersion>,
}

fn object(value: &Value) -> JsonObject {
    value.as_object().cloned().unwrap_or_default()
}

impl Spike {
    fn load(path: &str, revisions: Vec<ProtocolVersion>) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let data: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
        let mut tools = Vec::new();
        let mut records = Vec::new();
        for t in data["tools"].as_array().cloned().unwrap_or_default() {
            let name = t["name"].as_str().unwrap_or_default().to_owned();
            let description = t["description"].as_str().map(|s| Cow::Owned(s.to_owned()));
            let a = &t["annotations"];
            let annotations = ToolAnnotations::from_raw(
                None,
                a["readOnlyHint"].as_bool(),
                a["destructiveHint"].as_bool(),
                a["idempotentHint"].as_bool(),
                a["openWorldHint"].as_bool(),
            );
            let mut tool = Tool::new_with_raw(name.clone(), description, object(&t["inputSchema"]))
                .with_raw_output_schema(Arc::new(object(&t["outputSchema"])))
                .with_annotations(annotations);
            if let Some(title) = t["title"].as_str() {
                tool = tool.with_title(title);
            }
            tools.push(tool);
            records.push((name, object(&t["records"])));
        }
        Ok(Self {
            name: data["server"]["name"]
                .as_str()
                .unwrap_or("spike")
                .to_owned(),
            version: data["server"]["version"].as_str().unwrap_or("0").to_owned(),
            instructions: data["instructions"].as_str().unwrap_or_default().to_owned(),
            tools,
            records,
            revisions,
        })
    }
}

impl ServerHandler for Spike {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .build(),
        )
        .with_server_info(Implementation::new(self.name.clone(), self.version.clone()))
        .with_instructions(self.instructions.clone())
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Owned(self.revisions.clone())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tools.clone())
            .with_ttl_ms(60_000)
            .with_cache_scope(CacheScope::Public))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let Some((_, records)) = self.records.iter().find(|(n, _)| *n == request.name) else {
            return Err(ErrorData::invalid_params(
                format!("Unknown tool: {}", request.name),
                None,
            ));
        };
        let args = request.arguments.unwrap_or_default();
        let Some(order_id) = args.get("order_id").and_then(Value::as_str) else {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "input/schema_invalid: order_id is required and must be a string.",
            )])
            .into());
        };
        if let Some(wait_ms) = args.get("wait_ms").and_then(Value::as_u64) {
            let token = context.meta.get_progress_token();
            for step in 1..=4u32 {
                tokio::select! {
                    () = context.ct.cancelled() => {
                        eprintln!("spike: request {} cancelled at step {step}", context.id);
                        return Err(ErrorData::internal_error("cancelled", None));
                    }
                    () = tokio::time::sleep(Duration::from_millis(wait_ms / 4)) => {}
                }
                if let Some(token) = token.clone() {
                    let p = ProgressNotificationParam::new(token, f64::from(step)).with_total(4.0);
                    let _ = context.peer.notify_progress(p).await;
                }
            }
        }
        let Some(record) = records.get(order_id) else {
            let mut result = CallToolResult::structured_error(json!({
                "error": {"class": "input", "code": "not_found", "retry_safe": false,
                          "message": format!("No order {order_id}.")}
            }));
            result.content = vec![ContentBlock::text(format!(
                "input/not_found: no order {order_id}."
            ))];
            return Ok(result.into());
        };
        let mut result = CallToolResult::structured(record.clone());
        result.content = vec![ContentBlock::text(format!(
            "Order {order_id}: {}, total {} {}.",
            record["status"].as_str().unwrap_or_default(),
            record["total"].as_str().unwrap_or_default(),
            record["currency"].as_str().unwrap_or_default()
        ))];
        Ok(result.into())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: spike-mcp-on-sdk <tools.json> [revisions]")?;
    let revisions: Vec<ProtocolVersion> = args
        .next()
        .unwrap_or_else(|| "2025-11-25,2026-07-28".to_owned())
        .split(',')
        .map(|s| serde_json::from_value(Value::String(s.to_owned())))
        .collect::<Result<_, _>>()?;
    let spike = Spike::load(&path, revisions)?;
    #[cfg(feature = "http")]
    if args.next().as_deref() == Some("--http") {
        return serve_http(spike).await;
    }
    let running = spike.serve(stdio()).await?;
    running.waiting().await?;
    Ok(())
}

/// Stateless Streamable HTTP: no session for either era, JSON answers where no stream is needed.
#[cfg(feature = "http")]
async fn serve_http(spike: Spike) -> Result<(), Box<dyn std::error::Error>> {
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
    };
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true);
    let service = StreamableHttpService::new(
        move || Ok(spike.clone()),
        Arc::new(NeverSessionManager::default()),
        config,
    );
    let app = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    println!("{}", listener.local_addr()?.port());
    axum::serve(listener, app).await?;
    Ok(())
}
