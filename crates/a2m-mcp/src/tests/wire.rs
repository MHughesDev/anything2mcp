//! A small conformance client: hand-written JSON-RPC over the endpoint's
//! HTTP service in process, and through the proxy with in-memory streams.
//! It knows nothing of what the service stands on.

use std::time::Duration;

use a2m_core::Failure;
use a2m_policy::ToolsetName;
use axum::body::Body;
use axum::http::Request;
use serde_json::{Value as Json, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};
use tokio::task::JoinHandle;

use crate::{HttpRequest, McpPeer, McpService, mcp_path, stdio_proxy};

pub const H: &str = "2025-11-25";
pub const M: &str = "2026-07-28";
const PV: &str = "io.modelcontextprotocol/protocolVersion";
const CC: &str = "io.modelcontextprotocol/clientCapabilities";
const CI: &str = "io.modelcontextprotocol/clientInfo";
pub const SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";

/// How long a test waits for something that should come at once.
const PATIENCE: Duration = Duration::from_secs(60);

/// A request of the handshake revision.
pub fn h(id: impl Into<Json>, method: &str, params: Json) -> Json {
    json!({"jsonrpc": "2.0", "id": id.into(), "method": method, "params": params})
}

/// A request of the stateless revision: its `_meta` names the revision,
/// the client, and the client's capabilities.
pub fn m(id: impl Into<Json>, method: &str, params: Json) -> Json {
    m_at(M, id, method, params)
}

pub fn m_at(version: &str, id: impl Into<Json>, method: &str, mut params: Json) -> Json {
    let meta = params["_meta"].take();
    let mut meta = meta.as_object().cloned().unwrap_or_default();
    meta.insert(PV.to_owned(), json!(version));
    meta.insert(CC.to_owned(), json!({}));
    meta.insert(
        CI.to_owned(),
        json!({"name": "conformance", "version": "0"}),
    );
    params["_meta"] = Json::Object(meta);
    h(id, method, params)
}

/// What came back over HTTP.
pub struct Reply {
    pub status: u16,
    pub session: bool,
    pub body: Vec<u8>,
    pub messages: Vec<Json>,
}

impl Reply {
    pub fn only(&self) -> &Json {
        assert_eq!(self.messages.len(), 1, "{:?}", self.messages);
        &self.messages[0]
    }
}

/// The messages of a Streamable HTTP response body: one JSON document, or
/// the data of each event of a stream.
fn messages(content_type: &str, body: &[u8]) -> Vec<Json> {
    if body.is_empty() {
        return Vec::new();
    }
    if content_type.starts_with("text/event-stream") {
        let text = String::from_utf8(body.to_vec()).unwrap();
        return text
            .split("\n\n")
            .filter_map(|event| {
                let data: String = event
                    .lines()
                    .filter_map(|line| line.strip_prefix("data:"))
                    .map(str::trim_start)
                    .collect();
                (!data.is_empty()).then(|| serde_json::from_str(&data).unwrap())
            })
            .collect();
    }
    serde_json::from_slice(body)
        .map(|message| vec![message])
        .unwrap_or_default()
}

/// The endpoint, called in process.
#[derive(Clone)]
pub struct Http {
    pub service: McpService,
    pub peer: Option<McpPeer>,
}

impl Http {
    pub fn new(service: McpService) -> Self {
        Self {
            service,
            peer: Some(super::world::peer()),
        }
    }

    /// One POST with exactly these bytes and headers.
    pub async fn send(&self, body: &[u8], headers: &[(&str, &str)]) -> Reply {
        let mut request = Request::post(mcp_path(self.service.toolset()))
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if !headers.iter().any(|(name, _)| *name == "host") {
            request = request.header("host", "localhost");
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let mut request = request.body(Body::from(body.to_vec())).unwrap();
        if let Some(peer) = &self.peer {
            request.extensions_mut().insert(peer.clone());
        }
        let response = tokio::time::timeout(PATIENCE, self.service.handle(request))
            .await
            .expect("the endpoint answered");
        let status = response.status().as_u16();
        let session = response.headers().contains_key("mcp-session-id");
        let content_type = response
            .headers()
            .get("content-type")
            .map(|value| value.to_str().unwrap().to_owned())
            .unwrap_or_default();
        let body = tokio::time::timeout(
            PATIENCE,
            axum::body::to_bytes(response.into_body(), usize::MAX),
        )
        .await
        .expect("the body ended")
        .unwrap()
        .to_vec();
        Reply {
            status,
            session,
            messages: messages(&content_type, &body),
            body,
        }
    }

    /// One message, with the headers the stateless revision asks of a
    /// request that names it.
    pub async fn post(&self, message: &Json) -> Reply {
        let mut headers: Vec<(&str, &str)> = Vec::new();
        if let Some(version) = message["params"]["_meta"][PV].as_str() {
            headers.push(("mcp-protocol-version", version));
            headers.push(("mcp-method", message["method"].as_str().unwrap_or("")));
            if let Some(name) = message["params"]["name"].as_str() {
                headers.push(("mcp-name", name));
            }
        }
        self.send(message.to_string().as_bytes(), &headers).await
    }
}

/// The proxy, running over in-memory streams against the endpoint.
pub struct Stdio {
    input: Option<DuplexStream>,
    output: BufReader<DuplexStream>,
    done: JoinHandle<Result<(), Failure>>,
}

impl Stdio {
    pub fn start(service: McpService) -> Self {
        let toolset: ToolsetName = service.toolset().clone();
        let peer = super::world::peer();
        let upstream = move |mut request: HttpRequest| {
            let service = service.clone();
            let peer = peer.clone();
            async move {
                // What the local channel does: it names the peer it saw.
                request.extensions_mut().insert(peer);
                Ok(service.handle(request).await)
            }
        };
        let (input, proxy_in) = tokio::io::duplex(1 << 16);
        let (proxy_out, output) = tokio::io::duplex(1 << 16);
        let done =
            tokio::spawn(async move { stdio_proxy(upstream, &toolset, proxy_in, proxy_out).await });
        Self {
            input: Some(input),
            output: BufReader::new(output),
            done,
        }
    }

    pub async fn write(&mut self, line: &[u8]) {
        let input = self.input.as_mut().unwrap();
        input.write_all(line).await.unwrap();
        input.write_all(b"\n").await.unwrap();
    }

    /// The next line of the proxy's output, without its end. Every line
    /// must be one JSON message.
    pub async fn line(&mut self) -> Option<Vec<u8>> {
        let mut line = Vec::new();
        let read = tokio::time::timeout(PATIENCE, self.output.read_until(b'\n', &mut line))
            .await
            .expect("the proxy wrote a line or ended")
            .unwrap();
        if read == 0 {
            return None;
        }
        assert_eq!(line.pop(), Some(b'\n'));
        let message: Json = serde_json::from_slice(&line).expect("one JSON message per line");
        assert_eq!(message["jsonrpc"], "2.0", "{message}");
        Some(line)
    }

    pub async fn message(&mut self) -> Json {
        serde_json::from_slice(&self.line().await.expect("a line")).unwrap()
    }

    /// Ends the input, and returns what the proxy still wrote and how it
    /// ended.
    pub async fn close(mut self) -> (Vec<Json>, Result<(), Failure>) {
        drop(self.input.take());
        let mut rest = Vec::new();
        while let Some(line) = self.line().await {
            rest.push(serde_json::from_slice(&line).unwrap());
        }
        let ended = tokio::time::timeout(PATIENCE, self.done)
            .await
            .expect("the proxy ended")
            .unwrap();
        (rest, ended)
    }
}

/// Either way to the endpoint, for the checks that hold on both.
pub enum Wire {
    Http(Http),
    Stdio(Box<Stdio>),
}

impl Wire {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Http(_) => "http",
            Self::Stdio(_) => "stdio",
        }
    }

    /// Sends a request and returns every message answered to it, in
    /// order: the notifications it caused, then its response.
    pub async fn ask(&mut self, message: &Json) -> Vec<Json> {
        match self {
            Self::Http(http) => http.post(message).await.messages,
            Self::Stdio(stdio) => {
                stdio.write(message.to_string().as_bytes()).await;
                let mut seen = Vec::new();
                loop {
                    let line = stdio.message().await;
                    let last = line.get("id").is_some();
                    if last {
                        // A response to anything else would be a message
                        // the proxy or the server made up.
                        assert_eq!(line["id"], message["id"], "{line}");
                    }
                    seen.push(line);
                    if last {
                        return seen;
                    }
                }
            }
        }
    }

    /// The response to a request that causes no notification.
    pub async fn answer(&mut self, message: &Json) -> Json {
        let mut seen = self.ask(message).await;
        assert_eq!(seen.len(), 1, "{seen:?}");
        seen.remove(0)
    }

    /// Sends bytes that are answered with a null id, and returns the
    /// answer.
    pub async fn broken(&mut self, bytes: &[u8]) -> Json {
        match self {
            Self::Http(http) => {
                let reply = http.send(bytes, &[]).await;
                assert_eq!(reply.status, 400, "{:?}", reply.messages);
                reply.only().clone()
            }
            Self::Stdio(stdio) => {
                stdio.write(bytes).await;
                stdio.message().await
            }
        }
    }

    /// Sends a notification. Nothing may come back: over HTTP the status
    /// is 202 with no body; over stdio the next thing read is the answer
    /// to whatever is asked next.
    pub async fn tell(&mut self, message: &Json) {
        match self {
            Self::Http(http) => {
                let reply = http.post(message).await;
                assert_eq!(reply.status, 202, "{:?}", reply.messages);
                assert!(reply.body.is_empty());
            }
            Self::Stdio(stdio) => stdio.write(message.to_string().as_bytes()).await,
        }
    }
}
