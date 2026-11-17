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
//! A line that is not JSON is answered here with -32700, and a line longer
//! than [`LINE_LIMIT`] with -32600, because no body was ever sent. When the
//! endpoint cannot be reached, or answers with something that is not an
//! MCP message, a request is answered with error -32000 so that its caller
//! is not left waiting. The error's `data` says whether the call may be
//! made again: a request refused before it was handed to the engine may
//! be; a `tools/call` whose answer was lost after the engine may have had
//! it may not, because it may have started a run that writes (class
//! `possible_commit`, code `answer_lost`).
//!
//! At most [`OPEN_LIMIT`] requests are carried at once. The next line is
//! read when one of them is answered.

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

/// The longest line read, in bytes: the endpoint's own cap on a request
/// body. A longer line is answered with -32600 and the rest of it is read
/// and dropped, never kept.
pub const LINE_LIMIT: usize = 4 * 1024 * 1024;

/// The most requests carried to the endpoint at once, each on its own
/// connection.
pub const OPEN_LIMIT: usize = 64;

/// A future the proxy awaits.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The way to the engine's MCP endpoint: on one machine, the local
/// channel. Any function from a request to a response is one.
pub trait Upstream: Send + Sync + 'static {
    /// Sends one request and returns the response, whose body may still be
    /// arriving.
    ///
    /// A failure of class `busy`, `policy`, `input`, or `transient` says
    /// that the request was not handed to the engine: nothing answered,
    /// what answered was refused, or the connection ended before anything
    /// was sent. A failure of any other class, `possible_commit` above all
    /// (the local channel's `answer_lost`), says that the engine may have
    /// had it.
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
        // Exchanges that are over are let go, and no more than the limit
        // are carried at once: the next line waits for one to end.
        while exchanges.try_join_next().is_some() {}
        while exchanges.len() >= OPEN_LIMIT {
            exchanges.join_next().await;
        }
        match read_line(&mut reader, &mut line, LINE_LIMIT).await {
            Ok(Line::End) => break Ok(()),
            Ok(Line::Read) => {}
            Ok(Line::TooLong) => {
                let answer = error_line(
                    &Json::Null,
                    codes::INVALID_REQUEST,
                    "The line is longer than a request may be.",
                    None,
                );
                if lines.send(answer).await.is_err() {
                    break Ok(());
                }
                continue;
            }
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
        // Only a tool call can start a run, and so a write.
        let may_write = method == Some("tools/call");
        let request = post(&path, &message, std::mem::take(&mut line));
        let handle = exchanges.spawn(exchange(
            Arc::clone(&upstream),
            request,
            Asked {
                id: id.clone(),
                may_write,
            },
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

/// How a line was read.
enum Line {
    /// The input has ended.
    End,
    /// A line, without its end, is in the buffer.
    Read,
    /// The line was longer than the limit. It was read to its end and
    /// dropped.
    TooLong,
}

/// Reads one line into `line`, holding no more than `limit` bytes of it.
async fn read_line<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    line: &mut Vec<u8>,
    limit: usize,
) -> std::io::Result<Line> {
    let mut too_long = false;
    loop {
        let buffer = reader.fill_buf().await?;
        if buffer.is_empty() {
            return Ok(match (too_long, line.is_empty()) {
                (true, _) => Line::TooLong,
                (false, true) => Line::End,
                (false, false) => Line::Read,
            });
        }
        let (taken, ended) = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(at) => (at + 1, true),
            None => (buffer.len(), false),
        };
        if !too_long {
            if line.len() + taken > limit.saturating_add(1) {
                too_long = true;
                line.clear();
            } else {
                line.extend_from_slice(&buffer[..taken]);
            }
        }
        reader.consume(taken);
        if ended {
            return Ok(if too_long { Line::TooLong } else { Line::Read });
        }
    }
}

/// What the proxy knows of a request it carries.
struct Asked {
    /// Its id; `None` for a notification, which is owed no answer.
    id: Option<Json>,
    /// Whether it is a tool call, which may start a run that writes.
    may_write: bool,
}

/// The `data` of an error for a request that was not handed to the
/// engine: it may be made again.
fn not_sent(failure: &Failure) -> Json {
    let mut data = codes::failure_data(failure);
    data["retry_safe"] = json!(true);
    data
}

/// The `data` of an error for a tool call whose answer was lost after the
/// engine may have had it: a write in doubt, never "not sent".
fn in_doubt() -> Json {
    let failure = codes::answer_lost();
    let mut data = codes::failure_data(&failure);
    data["retry_safe"] = json!(false);
    data
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
    asked: Asked,
    lines: mpsc::Sender<Vec<u8>>,
) {
    // A caller with a request open is always answered; a notification is
    // owed nothing.
    let unanswered = |message: &str, data: Option<Json>| {
        asked
            .id
            .as_ref()
            .map(|id| error_line(id, codes::ENGINE_UNREACHABLE, message, data))
    };
    // A request the engine may have had, whose answer is lost: a tool
    // call is a write in doubt; any other request wrote nothing.
    let lost = |failure: &Failure| {
        if asked.may_write {
            unanswered(codes::answer_lost().message(), Some(in_doubt()))
        } else {
            unanswered(failure.message(), Some(not_sent(failure)))
        }
    };
    let Ok(request) = request else {
        // A value copied into a header was not one a header can carry.
        // Nothing was sent.
        if let Some(line) = unanswered(
            "The message names something a header cannot carry.",
            Some(json!({"retry_safe": true})),
        ) {
            let _ = lines.send(line).await;
        }
        return;
    };
    let response = match upstream.send(request).await {
        Ok(response) => response,
        Err(failure) => {
            let sent = !matches!(
                failure.class(),
                FailureClass::Busy
                    | FailureClass::Policy
                    | FailureClass::Input
                    | FailureClass::Transient
            );
            let line = if sent {
                lost(&failure)
            } else {
                unanswered(failure.message(), Some(not_sent(&failure)))
            };
            if let Some(line) = line {
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
                let message = std::mem::take(&mut data);
                // Only one JSON message goes on a line. The call is
                // answered by its response, not by a note of progress.
                let Ok(parsed) = serde_json::from_slice::<Json>(&message) else {
                    continue;
                };
                answered |= is_response(&parsed);
                if lines.send(message).await.is_err() {
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
    if answered {
        return;
    }
    // A refusal of the endpoint's own comes before any dispatch, with a
    // status of 400 to 499. Any other answer that is not a response, or
    // breaks off before one, may follow a dispatch.
    let line = if status.is_client_error() || !asked.may_write {
        unanswered(
            "The engine answered with something that is not an MCP message.",
            Some(
                json!({"status": status.as_u16(), "retry_safe": !asked.may_write || status.is_client_error()}),
            ),
        )
    } else {
        unanswered(codes::answer_lost().message(), Some(in_doubt()))
    };
    if let Some(line) = line {
        let _ = lines.send(line).await;
    }
}

/// Whether a message is the response to a request, not a note.
fn is_response(message: &Json) -> bool {
    message.get("method").is_none()
        && message.get("id").is_some()
        && (message.get("result").is_some() || message.get("error").is_some())
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
