//! The stdio proxy: a line copier between a client's standard streams and
//! an MCP endpoint.
//!
//! Each line read is posted to the endpoint as it came, byte for byte, and
//! each message the endpoint answers is written as one line. The proxy has
//! no tools, keeps nothing between lines but the requests still open, and
//! parses a line only to do what the two transports force on it:
//!
//! 1. A `notifications/cancelled` line closes the HTTP stream of the
//!    request it names, which is how a request is cancelled over HTTP. The
//!    line itself is not sent on.
//! 2. Three values are copied out of the body into headers, as the
//!    stateless revision asks of every HTTP request: the revision, the
//!    method, and the name of the tool, prompt, or resource.
//!
//! A line that is not JSON is answered here with -32700, because no body
//! was ever sent. When the endpoint cannot be reached, or answers with
//! something that is not an MCP message, a request is answered with error
//! -32000 so that its caller is not left waiting.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2m_core::{Failure, FailureClass};
use a2m_policy::ToolsetName;
use axum::body::{Body, HttpBody};
use axum::http::header::{ACCEPT, CONTENT_TYPE, HOST};
use axum::http::{Method, Request};
use serde_json::{Value as Json, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinSet};

use crate::codes;
use crate::http::{HttpRequest, HttpResponse, VERSION_HEADER, VERSION_KEY, mcp_path};

/// A future the proxy awaits.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The way to the engine's MCP endpoint: on one machine, the local
/// channel. Any function from a request to a response is one.
pub trait Upstream: Send + Sync + 'static {
    /// Sends one request and returns the response, whose body may still be
    /// arriving.
    fn send(&self, request: HttpRequest) -> BoxFuture<'static, Result<HttpResponse, Failure>>;
}

impl<F, Fut> Upstream for F
where
    F: Fn(HttpRequest) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<HttpResponse, Failure>> + Send + 'static,
{
    fn send(&self, request: HttpRequest) -> BoxFuture<'static, Result<HttpResponse, Failure>> {
        Box::pin(self(request))
    }
}

/// Copies MCP messages between `input` and `output` and the endpoint of
/// `toolset` behind `upstream`, until `input` ends. Requests still open at
/// the end of input are answered first. `output` carries one JSON message
/// per line and nothing else.
///
/// # Errors
///
/// `transient`/`stdio_read_failed` when `input` cannot be read. An
/// `output` that is closed ends the proxy quietly: the client has gone.
pub async fn stdio_proxy<U, R, W>(
    upstream: U,
    toolset: &ToolsetName,
    input: R,
    mut output: W,
) -> Result<(), Failure>
where
    U: Upstream,
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let upstream: Arc<dyn Upstream> = Arc::new(upstream);
    let path = mcp_path(toolset);
    let (lines, mut written) = mpsc::channel::<Vec<u8>>(64);
    let writer = tokio::spawn(async move {
        while let Some(mut line) = written.recv().await {
            line.push(b'\n');
            if output.write_all(&line).await.is_err() || output.flush().await.is_err() {
                break;
            }
        }
    });
    let mut open: BTreeMap<String, AbortHandle> = BTreeMap::new();
    let mut exchanges = JoinSet::new();
    let mut reader = BufReader::new(input);
    let mut line = Vec::new();
    let read = loop {
        line.clear();
        match reader.read_until(b'\n', &mut line).await {
            Ok(0) => break Ok(()),
            Ok(_) => {}
            Err(_) => {
                break Err(Failure::new(
                    FailureClass::Transient,
                    codes::STDIO_READ_FAILED,
                    "The proxy could not read its input.",
                    "Start the client again.",
                ));
            }
        }
        while line
            .last()
            .is_some_and(|byte| matches!(byte, b'\n' | b'\r'))
        {
            line.pop();
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let Ok(message) = serde_json::from_slice::<Json>(&line) else {
            let answer = error_line(
                &Json::Null,
                codes::PARSE_ERROR,
                "The line is not JSON.",
                None,
            );
            if lines.send(answer).await.is_err() {
                break Ok(());
            }
            continue;
        };
        let method = message.get("method").and_then(Json::as_str);
        if method == Some("notifications/cancelled") {
            if let Some(handle) = message
                .pointer("/params/requestId")
                .and_then(|id| open.remove(&id.to_string()))
            {
                handle.abort();
            }
            continue;
        }
        let id = message.get("id").filter(|id| !id.is_null()).cloned();
        let request = post(&path, &message, std::mem::take(&mut line));
        let handle = exchanges.spawn(exchange(
            Arc::clone(&upstream),
            request,
            id.clone(),
            lines.clone(),
        ));
        open.retain(|_, handle| !handle.is_finished());
        if let Some(id) = id {
            open.insert(id.to_string(), handle);
        }
    };
    while exchanges.join_next().await.is_some() {}
    drop(lines);
    let _ = writer.await;
    read
}

/// The HTTP request that carries one line: the line as the body, and the
/// values the stateless revision wants in headers, copied out of it.
fn post(path: &str, message: &Json, body: Vec<u8>) -> Result<HttpRequest, axum::http::Error> {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(HOST, "localhost")
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/event-stream");
    let text = |pointer: &str| message.pointer(pointer).and_then(Json::as_str);
    if let Some(version) = message
        .pointer("/params/_meta")
        .and_then(|meta| meta.get(VERSION_KEY))
        .and_then(Json::as_str)
    {
        request = request.header(VERSION_HEADER, version);
        if let Some(method) = text("/method") {
            request = request.header("mcp-method", method);
        }
        let name = match text("/method") {
            Some("tools/call" | "prompts/get") => text("/params/name"),
            Some("resources/read") => text("/params/uri"),
            _ => None,
        };
        if let Some(name) = name {
            request = request.header("mcp-name", name);
        }
    }
    request.body(Body::from(body))
}

/// One request and everything the endpoint answers to it.
async fn exchange(
    upstream: Arc<dyn Upstream>,
    request: Result<HttpRequest, axum::http::Error>,
    id: Option<Json>,
    lines: mpsc::Sender<Vec<u8>>,
) {
    // A caller with a request open is always answered; a notification is
    // owed nothing.
    let unanswered = |message: &str, data: Option<Json>| {
        id.as_ref()
            .map(|id| error_line(id, codes::ENGINE_UNREACHABLE, message, data))
    };
    let Ok(request) = request else {
        // A value copied into a header was not one a header can carry.
        if let Some(line) = unanswered("The message names something a header cannot carry.", None) {
            let _ = lines.send(line).await;
        }
        return;
    };
    let response = match upstream.send(request).await {
        Ok(response) => response,
        Err(failure) => {
            if let Some(line) = unanswered(failure.message(), Some(codes::failure_data(&failure))) {
                let _ = lines.send(line).await;
            }
            return;
        }
    };
    let status = response.status();
    let stream = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/event-stream"));
    let mut body = response.into_body();
    let mut buffer: Vec<u8> = Vec::new();
    let mut data: Vec<u8> = Vec::new();
    let mut answered = false;
    loop {
        let frame = std::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await;
        let Some(Ok(frame)) = frame else {
            break;
        };
        let Ok(chunk) = frame.into_data() else {
            continue;
        };
        buffer.extend_from_slice(&chunk);
        if !stream {
            continue;
        }
        while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
            let mut field: Vec<u8> = buffer.drain(..=end).collect();
            field.pop();
            if field.last() == Some(&b'\r') {
                field.pop();
            }
            if let Some(value) = field.strip_prefix(b"data:") {
                data.extend_from_slice(value.strip_prefix(b" ").unwrap_or(value));
            } else if field.is_empty() && !data.is_empty() {
                answered = true;
                if lines.send(std::mem::take(&mut data)).await.is_err() {
                    return;
                }
            }
        }
    }
    if !stream {
        // A JSON body is one message. Line ends between its tokens are
        // framing, and are the only bytes taken out.
        buffer.retain(|byte| !matches!(byte, b'\n' | b'\r'));
        if !buffer.is_empty() && serde_json::from_slice::<Json>(&buffer).is_ok() {
            answered = true;
            let _ = lines.send(buffer).await;
        }
    }
    if !answered
        && let Some(line) = unanswered(
            "The engine answered with something that is not an MCP message.",
            Some(json!({"status": status.as_u16()})),
        )
    {
        let _ = lines.send(line).await;
    }
}

fn error_line(id: &Json, code: i64, message: &str, data: Option<Json>) -> Vec<u8> {
    let mut error = json!({"code": code, "message": message});
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({"jsonrpc": "2.0", "id": id, "error": error})
        .to_string()
        .into_bytes()
}
