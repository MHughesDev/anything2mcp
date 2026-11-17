//! Spike 0.5, variant B: JSON-RPC 2.0 over stdio by hand, with `serde_json` and nothing else.
//!
//! Usage: spike-mcp-by-hand <tools.json> [revision,revision,...]
//!
//! One process serves both eras of the protocol, as the 2026-07-28 revision's
//! "Versioning and Compatibility" page allows:
//!   - a request whose `_meta` names a protocol version is served statelessly (modern);
//!   - an `initialize` request selects the handshake revisions (legacy) for this process.

use std::{
    collections::HashMap,
    io::{BufRead, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde_json::{Map, Value, json};

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;

/// The first revision with no handshake. Revisions are dates, so text order is time order.
const FIRST_MODERN: &str = "2026-07-28";
const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";

struct Server {
    data: Value,
    /// Revisions served, oldest first.
    revisions: Vec<String>,
    out: Mutex<std::io::Stdout>,
    /// Requests in flight, by the JSON text of their id.
    in_flight: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// Set by `initialize`: the handshake revision agreed for this process.
    legacy: Mutex<Option<String>>,
}

enum Era {
    Modern,
    Legacy,
}

struct RpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

fn err(code: i64, message: impl Into<String>) -> RpcError {
    RpcError {
        code,
        message: message.into(),
        data: None,
    }
}

impl Server {
    fn send(&self, message: &Value) {
        if let Ok(mut out) = self.out.lock() {
            // One message per line; serde_json never writes a raw newline.
            let _ = writeln!(out, "{message}");
            let _ = out.flush();
        }
    }

    fn respond(&self, id: &Value, outcome: Result<Value, RpcError>) {
        let message = match outcome {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(e) => {
                let mut error = json!({"code": e.code, "message": e.message});
                if let Some(data) = e.data {
                    error["data"] = data;
                }
                json!({"jsonrpc": "2.0", "id": id, "error": error})
            }
        };
        self.send(&message);
    }

    fn legacy_revisions(&self) -> Vec<&String> {
        self.revisions
            .iter()
            .filter(|r| r.as_str() < FIRST_MODERN)
            .collect()
    }

    fn unsupported(&self, requested: &str) -> RpcError {
        RpcError {
            code: UNSUPPORTED_PROTOCOL_VERSION,
            message: "Unsupported protocol version".to_owned(),
            data: Some(json!({"supported": self.revisions, "requested": requested})),
        }
    }

    /// Which era a request belongs to. A version in `_meta` wins; otherwise the
    /// process must have been opened with `initialize`.
    fn era(&self, params: &Value) -> Result<Era, RpcError> {
        let meta = &params["_meta"];
        // A server set to handshake revisions only is a plain handshake server: it does not
        // read the modern keys at all, so that a dual-era client's probe gets an error that
        // is not a modern one and the client falls back to `initialize`.
        let serves_modern = self.revisions.iter().any(|r| r.as_str() >= FIRST_MODERN);
        if let Some(version) = meta.get(META_VERSION).filter(|_| serves_modern) {
            let Some(version) = version.as_str() else {
                return Err(err(
                    INVALID_PARAMS,
                    format!("_meta.{META_VERSION} must be a string"),
                ));
            };
            if version < FIRST_MODERN || !self.revisions.iter().any(|r| r == version) {
                return Err(self.unsupported(version));
            }
            if !meta[META_CAPABILITIES].is_object() {
                return Err(err(
                    INVALID_PARAMS,
                    format!("_meta.{META_CAPABILITIES} is required"),
                ));
            }
            return Ok(Era::Modern);
        }
        match self.legacy.lock().map(|l| l.is_some()) {
            Ok(true) => Ok(Era::Legacy),
            _ => Err(err(
                INVALID_PARAMS,
                format!(
                    "no initialize handshake and no _meta.{META_VERSION}; served: {}",
                    self.revisions.join(", ")
                ),
            )),
        }
    }

    fn server_info(&self) -> Value {
        self.data["server"].clone()
    }

    fn capabilities(&self) -> Value {
        json!({"tools": {"listChanged": true}})
    }

    /// Adds what every modern result carries. Legacy results are left as they are.
    fn finish(&self, era: &Era, mut result: Value) -> Value {
        if let Era::Modern = era {
            result["resultType"] = json!("complete");
            result["_meta"] = json!({META_SERVER_INFO: self.server_info()});
        }
        result
    }

    fn initialize(&self, params: &Value) -> Result<Value, RpcError> {
        let Some(requested) = params["protocolVersion"].as_str() else {
            return Err(err(INVALID_PARAMS, "protocolVersion is required"));
        };
        let legacy = self.legacy_revisions();
        // The handshake rule: the client's revision if we serve it, else our newest handshake revision.
        let Some(agreed) = legacy
            .iter()
            .find(|r| r.as_str() == requested)
            .or(legacy.last())
        else {
            return Err(self.unsupported(requested));
        };
        if let Ok(mut slot) = self.legacy.lock() {
            *slot = Some((*agreed).clone());
        }
        Ok(json!({
            "protocolVersion": agreed,
            "capabilities": self.capabilities(),
            "serverInfo": self.server_info(),
            "instructions": self.data["instructions"],
        }))
    }

    fn tools(&self) -> Vec<Value> {
        let listed = [
            "name",
            "title",
            "description",
            "inputSchema",
            "outputSchema",
            "annotations",
        ];
        self.data["tools"]
            .as_array()
            .map(|tools| {
                tools
                    .iter()
                    .map(|t| {
                        let mut tool = Map::new();
                        for key in listed {
                            if let Some(v) = t.get(key) {
                                tool.insert(key.to_owned(), v.clone());
                            }
                        }
                        Value::Object(tool)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Everything except `tools/call`, which runs on its own thread.
    fn request(&self, method: &str, params: &Value) -> Result<Value, RpcError> {
        if method == "initialize" {
            return self.initialize(params);
        }
        // The handshake revisions allow a ping before `initialize`; the modern ones have no ping.
        if method == "ping"
            && params["_meta"].get(META_VERSION).is_none()
            && !self.legacy_revisions().is_empty()
        {
            return Ok(json!({}));
        }
        if method == "server/discover" && !self.revisions.iter().any(|r| r.as_str() >= FIRST_MODERN)
        {
            return Err(err(METHOD_NOT_FOUND, format!("Method not found: {method}")));
        }
        let era = self.era(params)?;
        match (method, &era) {
            ("server/discover", Era::Modern) => Ok(self.finish(
                &era,
                json!({
                    "supportedVersions": self.revisions,
                    "capabilities": self.capabilities(),
                    "instructions": self.data["instructions"],
                    "ttlMs": 0,
                    "cacheScope": "public",
                }),
            )),
            ("tools/list", _) => {
                if !params["cursor"].is_null() {
                    return Err(err(INVALID_PARAMS, "Invalid cursor"));
                }
                let mut result = json!({"tools": self.tools()});
                if let Era::Modern = era {
                    result["ttlMs"] = json!(60_000);
                    result["cacheScope"] = json!("public");
                }
                Ok(self.finish(&era, result))
            }
            _ => Err(err(METHOD_NOT_FOUND, format!("Method not found: {method}"))),
        }
    }

    /// `None` means the call was cancelled: nothing more is sent for it.
    fn call_tool(&self, params: &Value, cancelled: &AtomicBool) -> Option<Result<Value, RpcError>> {
        let era = match self.era(params) {
            Ok(era) => era,
            Err(e) => return Some(Err(e)),
        };
        let name = params["name"].as_str().unwrap_or_default();
        let tool = self.data["tools"]
            .as_array()
            .and_then(|ts| ts.iter().find(|t| t["name"] == name));
        let Some(tool) = tool else {
            return Some(Err(err(INVALID_PARAMS, format!("Unknown tool: {name}"))));
        };
        let args = &params["arguments"];
        let tool_error = |text: String, structured: Option<Value>| {
            let mut result = json!({"content": [{"type": "text", "text": text}], "isError": true});
            if let Some(s) = structured {
                result["structuredContent"] = s;
            }
            Some(Ok(self.finish(&era, result)))
        };
        let Some(order_id) = args["order_id"].as_str() else {
            return tool_error(
                "input/schema_invalid: order_id is required and must be a string.".to_owned(),
                None,
            );
        };
        if let Some(wait_ms) = args["wait_ms"].as_u64() {
            let token = params["_meta"].get("progressToken").cloned();
            for step in 1..=4u64 {
                std::thread::sleep(Duration::from_millis(wait_ms / 4));
                if cancelled.load(Ordering::SeqCst) {
                    eprintln!("spike: request cancelled at step {step}");
                    return None;
                }
                if let Some(token) = &token {
                    self.send(
                        &json!({"jsonrpc": "2.0", "method": "notifications/progress",
                        "params": {"progressToken": token, "progress": step, "total": 4}}),
                    );
                }
            }
        }
        let Some(record) = tool["records"].get(order_id) else {
            return tool_error(
                format!("input/not_found: no order {order_id}."),
                Some(
                    json!({"error": {"class": "input", "code": "not_found", "retry_safe": false,
                    "message": format!("No order {order_id}.")}}),
                ),
            );
        };
        let text = format!(
            "Order {order_id}: {}, total {} {}.",
            record["status"].as_str().unwrap_or_default(),
            record["total"].as_str().unwrap_or_default(),
            record["currency"].as_str().unwrap_or_default()
        );
        Some(Ok(self.finish(
            &era,
            json!({"content": [{"type": "text", "text": text}], "structuredContent": record, "isError": false}),
        )))
    }

    fn handle(self: &Arc<Self>, line: &str) {
        let message: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => return self.respond(&Value::Null, Err(err(PARSE_ERROR, "Parse error"))),
        };
        let Some(object) = message.as_object() else {
            // Batches left the protocol in 2025-06-18; an array is not a request.
            return self.respond(&Value::Null, Err(err(INVALID_REQUEST, "Invalid Request")));
        };
        let id = object.get("id").cloned();
        let id_ok = id.as_ref().is_none_or(|i| i.is_string() || i.is_number());
        let Some(method) = object.get("method").and_then(Value::as_str) else {
            if object.contains_key("result") || object.contains_key("error") {
                return; // A response; this server sends no requests, so it has nothing to match.
            }
            return self.respond(
                &id.unwrap_or(Value::Null),
                Err(err(INVALID_REQUEST, "Invalid Request")),
            );
        };
        if object.get("jsonrpc") != Some(&json!("2.0")) || !id_ok {
            return self.respond(&Value::Null, Err(err(INVALID_REQUEST, "Invalid Request")));
        }
        let params = object.get("params").cloned().unwrap_or(Value::Null);
        if !(params.is_null() || params.is_object()) {
            return match id {
                Some(id) => self.respond(&id, Err(err(INVALID_PARAMS, "params must be an object"))),
                None => (),
            };
        }
        let Some(id) = id else {
            // A notification is never answered.
            if method == "notifications/cancelled" {
                let key = params["requestId"].to_string();
                if let Ok(map) = self.in_flight.lock() {
                    if let Some(flag) = map.get(&key) {
                        flag.store(true, Ordering::SeqCst);
                    }
                }
            }
            return;
        };
        if method != "tools/call" {
            return self.respond(&id, self.request(method, &params));
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        if let Ok(mut map) = self.in_flight.lock() {
            map.insert(id.to_string(), cancelled.clone());
        }
        let server = self.clone();
        std::thread::spawn(move || {
            let outcome = server.call_tool(&params, &cancelled);
            if let Ok(mut map) = server.in_flight.lock() {
                map.remove(&id.to_string());
            }
            if let Some(outcome) = outcome {
                server.respond(&id, outcome);
            }
        });
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: spike-mcp-by-hand <tools.json> [revisions]")?;
    let mut revisions: Vec<String> = args
        .next()
        .unwrap_or_else(|| "2025-11-25,2026-07-28".to_owned())
        .split(',')
        .map(str::to_owned)
        .collect();
    revisions.sort();
    let data: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    let server = Arc::new(Server {
        data,
        revisions,
        out: Mutex::new(std::io::stdout()),
        in_flight: Mutex::new(HashMap::new()),
        legacy: Mutex::new(None),
    });
    // End of input is the shutdown signal.
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        if !line.trim().is_empty() {
            server.handle(&line);
        }
    }
    // Let calls in flight answer before the process ends. A spike may poll.
    while server
        .in_flight
        .lock()
        .map(|m| !m.is_empty())
        .unwrap_or(false)
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}
