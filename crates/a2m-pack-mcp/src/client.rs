//! The pack's own small MCP client: JSON-RPC 2.0 over a stream, in the two
//! forms the protocol has. With the handshake (revision 2025-11-25, or the
//! earlier 2025-06-18) a connection starts with `initialize`; without it
//! (revision 2026-07-28)
//! every request names the revision. It lists tools with cursors, calls one
//! tool, and cancels a call it stops waiting for.
//!
//! It is written here, and no SDK is used, because a client that must say
//! of every failure whether the request left needs to own the transport
//! (decision 0108). Nothing an upstream server says is put in a failure.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use a2m_core::Secret;
use a2m_provider::{Deadline, ScopedSupervisor, StepFailure, SubsystemId};
use serde_json::{Value as Json, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;

use crate::codes;
use crate::json::ToolDefinition;
use crate::mapping::{REVISION_HANDSHAKE, REVISION_HANDSHAKE_EARLIER, REVISION_NO_HANDSHAKE};

/// The `_meta` key under which a request with no handshake names its
/// revision. Verified against the specification's text in research note 18.
const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
const META_CLIENT: &str = "io.modelcontextprotocol/clientInfo";

/// The protocol's error for a revision the server does not serve.
const UNSUPPORTED_VERSION: i64 = -32022;
/// The protocol's error for an unknown tool or a malformed request.
const INVALID_PARAMS: i64 = -32602;

/// The most pages of one tool list that are followed.
const MAX_PAGES: usize = 200;

/// Which form of the protocol a connection speaks. It is stated by the
/// surface and never probed: an answer in another revision is a stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Revision {
    /// 2025-11-25: `initialize`, then `notifications/initialized`.
    Handshake,
    /// 2025-06-18: the same handshake, asking for the earlier revision. A
    /// server's own error for a call's bad input may come as the protocol
    /// error -32602 in it, which is a stop as a changed tool, never a
    /// retry (decision 0399).
    HandshakeEarlier,
    /// 2026-07-28: no handshake; every request names the revision.
    NoHandshake,
}

impl Revision {
    pub(crate) fn of(text: Option<&str>) -> Self {
        match text {
            Some(REVISION_NO_HANDSHAKE) => Self::NoHandshake,
            Some(REVISION_HANDSHAKE_EARLIER) => Self::HandshakeEarlier,
            _ => Self::Handshake,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Handshake => REVISION_HANDSHAKE,
            Self::HandshakeEarlier => REVISION_HANDSHAKE_EARLIER,
            Self::NoHandshake => REVISION_NO_HANDSHAKE,
        }
    }
}

/// What the server said it is. Its own words: compared, never shown as
/// anything but untrusted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// The result of one tool call, as the server gave it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ToolResult {
    /// Whether the tool refused the call with its own error result.
    pub is_error: bool,
    /// The text blocks of the result, in order. Untrusted.
    pub texts: Vec<String>,
    /// The structured content, when there is any.
    pub structured: Option<Json>,
    /// The whole result, for the capture.
    pub raw: Json,
}

/// The limits of one connection, from the pack's settings.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    pub max_message_bytes: usize,
    pub max_tools: usize,
    /// How old the tool list a read is compared with may be, on a
    /// connection that gives no notice of a change.
    pub list_max_age: std::time::Duration,
}

fn client_info() -> Json {
    json!({"name": "anything2mcp", "version": env!("CARGO_PKG_VERSION")})
}

/// One answer to one request.
enum Answer {
    Result(Json),
    Error(i64, Json),
}

/// Reads a JSON-RPC response for the request `id`. `None` for anything that
/// is not one.
fn read_answer(body: &Json, id: u64) -> Option<Answer> {
    let object = body.as_object()?;
    if object.get("jsonrpc").and_then(Json::as_str) != Some("2.0")
        || object.get("id").and_then(Json::as_u64) != Some(id)
    {
        return None;
    }
    match (object.get("result"), object.get("error")) {
        (Some(result), None) => Some(Answer::Result(result.clone())),
        (None, Some(error)) => {
            let code = error.get("code")?.as_i64()?;
            Some(Answer::Error(
                code,
                error.get("data").cloned().unwrap_or(Json::Null),
            ))
        }
        _ => None,
    }
}

/// The failure for an answer that is not a response. What became of the
/// request is not known.
fn not_a_response() -> StepFailure {
    StepFailure::unknown(codes::unexpected(
        "The upstream server answered with something that is not a JSON-RPC response.",
    ))
}

/// Revisions a server listed, kept only when each has the shape of a
/// revision's name, so that nothing else the server wrote is shown.
fn listed_revisions(data: &Json) -> String {
    let listed: Vec<&str> = data
        .get("supported")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(Json::as_str)
        .filter(|text| {
            text.len() == 10
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'-')
        })
        .take(8)
        .collect();
    if listed.is_empty() {
        "it names no revision it serves".to_owned()
    } else {
        format!("it serves {}", listed.join(", "))
    }
}

/// The failure for a JSON-RPC error. The server answered, so the request's
/// fate is known: it was not carried out.
fn protocol_error(code: i64, data: &Json, method: &str, tool: Option<&str>) -> StepFailure {
    let failure = match (code, tool) {
        (UNSUPPORTED_VERSION, _) => codes::unexpected(format!(
            "The upstream server does not serve the protocol revision the surface states; {}.",
            listed_revisions(data)
        )),
        (INVALID_PARAMS, Some(tool)) if method == "tools/call" => codes::tool_changed(
            tool,
            "is no longer accepted by the server as its mapping pins it",
        ),
        _ => codes::unexpected(format!(
            "The upstream server answered `{method}` with the protocol error {code}, which the mapping does not foresee."
        )),
    };
    StepFailure::answered(failure)
}

// ---------------------------------------------------------------- stdio

type Waiting = BTreeMap<u64, oneshot::Sender<Option<Json>>>;

/// What the reader of a local program's output shares with its callers.
struct StdioShared {
    /// The requests waiting for an answer, by id. `None` is sent to each
    /// when the connection is lost.
    waiting: Mutex<Waiting>,
    lost: AtomicBool,
    list_changed: AtomicBool,
}

impl StdioShared {
    /// Ends the connection: every waiting request is told it was lost.
    fn fail_all(&self) {
        self.end_with(None);
    }

    /// Ends the connection after output that is not a message: every
    /// waiting request is given something that is not a response.
    fn poison(&self) {
        self.end_with(Some(Json::Null));
    }

    fn end_with(&self, answer: Option<Json>) {
        self.lost.store(true, Ordering::SeqCst);
        if let Ok(mut waiting) = self.waiting.lock() {
            for (_, sender) in std::mem::take(&mut *waiting) {
                let _ = sender.send(answer.clone());
            }
        }
    }

    fn take(&self, id: u64) -> Option<oneshot::Sender<Option<Json>>> {
        self.waiting.lock().ok()?.remove(&id)
    }
}

type Writer = Arc<tokio::sync::Mutex<Box<dyn AsyncWrite + Send + Unpin>>>;

/// A local server program on its standard streams: one JSON message a line.
pub(crate) struct StdioLink {
    writer: Writer,
    shared: Arc<StdioShared>,
    reader: tokio::task::JoinHandle<()>,
    subsystem: Mutex<Option<(ScopedSupervisor, SubsystemId)>>,
}

/// Reads the program's output, one line a message, and hands each answer to
/// the request that waits for it. Its end is the end of the program.
async fn read_lines(
    from_child: Box<dyn AsyncRead + Send + Unpin>,
    shared: Arc<StdioShared>,
    max_message_bytes: usize,
) {
    let mut reader = BufReader::new(from_child);
    let limit = u64::try_from(max_message_bytes).unwrap_or(u64::MAX);
    loop {
        let mut line = Vec::new();
        let read = (&mut reader)
            .take(limit.saturating_add(1))
            .read_until(b'\n', &mut line)
            .await;
        match read {
            // The end of the stream, a broken stream, or a line longer than
            // the limit: the connection is over.
            Ok(0) | Err(_) => break,
            Ok(_) if line.last() != Some(&b'\n') => break,
            Ok(_) => {}
        }
        let Ok(message) = serde_json::from_slice::<Json>(&line) else {
            // A line that is not JSON answers nobody in particular, so
            // nothing after it on this connection can be trusted.
            shared.poison();
            return;
        };
        let id = message.get("id").and_then(Json::as_u64);
        let is_request = message.get("method").is_some();
        match (id, is_request) {
            (Some(id), false) => {
                if let Some(sender) = shared.take(id) {
                    let _ = sender.send(Some(message));
                }
            }
            (None, true)
                if message.get("method").and_then(Json::as_str)
                    == Some("notifications/tools/list_changed") =>
            {
                shared.list_changed.store(true, Ordering::SeqCst);
            }
            // Any other notification, and any request from the server, is
            // ignored: the pack serves no client-side feature.
            _ => {}
        }
    }
    shared.fail_all();
}

async fn write_line(writer: &Writer, message: &Json) -> std::io::Result<()> {
    let mut line = message.to_string().into_bytes();
    line.push(b'\n');
    let mut writer = writer.lock().await;
    writer.write_all(&line).await?;
    writer.flush().await
}

/// Forgets a waiting request when its caller stops waiting, and tells the
/// server the request is cancelled.
struct CancelOnDrop {
    shared: Arc<StdioShared>,
    writer: Writer,
    id: u64,
    armed: bool,
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        drop(self.shared.take(self.id));
        if self.shared.lost.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let writer = Arc::clone(&self.writer);
            let cancel = json!({
                "jsonrpc": "2.0",
                "method": "notifications/cancelled",
                "params": {"requestId": self.id, "reason": "The caller stopped waiting."}
            });
            runtime.spawn(async move {
                let _ = write_line(&writer, &cancel).await;
            });
        }
    }
}

impl StdioLink {
    /// A link over the two ends of a program's standard streams.
    pub(crate) fn new(
        to_child: Box<dyn AsyncWrite + Send + Unpin>,
        from_child: Box<dyn AsyncRead + Send + Unpin>,
        subsystem: Option<(ScopedSupervisor, SubsystemId)>,
        limits: Limits,
    ) -> Self {
        let shared = Arc::new(StdioShared {
            waiting: Mutex::new(BTreeMap::new()),
            lost: AtomicBool::new(false),
            list_changed: AtomicBool::new(false),
        });
        let reader = tokio::spawn(read_lines(
            from_child,
            Arc::clone(&shared),
            limits.max_message_bytes,
        ));
        Self {
            writer: Arc::new(tokio::sync::Mutex::new(to_child)),
            shared,
            reader,
            subsystem: Mutex::new(subsystem),
        }
    }

    async fn request(
        &self,
        id: u64,
        message: &Json,
        deadline: Deadline,
    ) -> Result<Json, StepFailure> {
        if self.shared.lost.load(Ordering::SeqCst) {
            return Err(StepFailure::not_sent(codes::connection_lost(
                "before the request was sent",
            )));
        }
        let (sender, receiver) = oneshot::channel();
        match self.shared.waiting.lock() {
            Ok(mut waiting) => {
                waiting.insert(id, sender);
            }
            Err(_) => {
                return Err(StepFailure::not_sent(codes::connection_lost(
                    "before the request was sent",
                )));
            }
        }
        let mut guard = CancelOnDrop {
            shared: Arc::clone(&self.shared),
            writer: Arc::clone(&self.writer),
            id,
            armed: true,
        };
        if write_line(&self.writer, message).await.is_err() {
            guard.armed = false;
            drop(self.shared.take(id));
            self.shared.fail_all();
            // A write that failed may still have delivered the line.
            return Err(StepFailure::unknown(codes::connection_lost(
                "while the request was being sent",
            )));
        }
        match tokio::time::timeout_at(deadline.at(), receiver).await {
            Ok(Ok(Some(answer))) => {
                guard.armed = false;
                Ok(answer)
            }
            Ok(Ok(None) | Err(_)) => {
                guard.armed = false;
                Err(StepFailure::unknown(codes::connection_lost(
                    "after the request was sent and before its answer came",
                )))
            }
            // The guard cancels the request as it is dropped.
            Err(_) => Err(StepFailure::unknown(codes::timeout("the request"))),
        }
    }

    async fn notify(&self, message: &Json) -> Result<(), StepFailure> {
        write_line(&self.writer, message).await.map_err(|_| {
            self.shared.fail_all();
            StepFailure::unknown(codes::connection_lost("while a notice was being sent"))
        })
    }

    fn take_subsystem(&self) -> Option<(ScopedSupervisor, SubsystemId)> {
        self.subsystem.lock().ok().and_then(|mut held| held.take())
    }

    /// Ends the program's group and waits for it.
    async fn close(&self) -> a2m_provider::Cleanup {
        self.reader.abort();
        self.shared.fail_all();
        match self.take_subsystem() {
            Some((supervisor, id)) => supervisor.stop(id).await,
            None => a2m_provider::Cleanup::complete(),
        }
    }
}

impl Drop for StdioLink {
    fn drop(&mut self) {
        self.reader.abort();
        // A link dropped without `close` still ends its program.
        if let (Some((supervisor, id)), Ok(runtime)) =
            (self.take_subsystem(), tokio::runtime::Handle::try_current())
        {
            runtime.spawn(async move {
                let _ = supervisor.stop(id).await;
            });
        }
    }
}

// ----------------------------------------------------------------- HTTP

/// A remote server over HTTP: one JSON-RPC message a request, answered with
/// JSON.
pub(crate) struct HttpLink {
    client: reqwest::Client,
    url: String,
    /// The headers the session's provider placed. Held for this connection
    /// and nowhere else, and never shown.
    credentials: Vec<(String, Secret)>,
    /// Whether the credentials would cross plain `http` to another machine
    /// with the surface's acceptance missing: then nothing is sent.
    refuse_credentials: bool,
    session: Mutex<Option<String>>,
    lost: AtomicBool,
    max_message_bytes: usize,
}

/// What an HTTP exchange gave: the status, and the body when it is JSON.
struct Exchange {
    status: u16,
    body: Option<Json>,
    session: Option<String>,
    event_stream: bool,
}

impl HttpLink {
    /// A link to `url`, which the caller has checked against the run's
    /// destinations.
    pub(crate) fn new(
        url: &str,
        credentials: Vec<(String, Secret)>,
        unencrypted_accepted: bool,
        limits: Limits,
    ) -> Result<Self, StepFailure> {
        // No redirect is followed and no proxy of the environment is used:
        // nothing but the surface's declared address decides where a
        // request goes. The client trusts what the engine trusts, states its
        // own TLS provider, and for an `https` address sends nothing that is
        // not `https`.
        let trust = a2m_auth::tls::installed().map_err(StepFailure::not_sent)?;
        let client = a2m_auth::tls::client_builder(&trust)
            .https_only(url.starts_with("https://"))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| {
                StepFailure::not_sent(codes::unavailable(
                    "The HTTP client for the upstream server could not be made.",
                ))
            })?;
        Ok(Self {
            client,
            url: url.to_owned(),
            refuse_credentials: !credentials.is_empty()
                && !unencrypted_accepted
                && crate::mapping::plain_to_another_machine(url),
            credentials,
            session: Mutex::new(None),
            lost: AtomicBool::new(false),
            max_message_bytes: limits.max_message_bytes,
        })
    }

    fn session(&self) -> Option<String> {
        self.session.lock().ok().and_then(|session| session.clone())
    }

    async fn post(
        &self,
        message: &Json,
        revision: Revision,
        tool: Option<&str>,
        deadline: Deadline,
    ) -> Result<Exchange, StepFailure> {
        if self.refuse_credentials {
            return Err(StepFailure::not_sent(codes::link_not_encrypted()));
        }
        let mut request = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", revision.as_str());
        if revision == Revision::NoHandshake {
            if let Some(method) = message.get("method").and_then(Json::as_str) {
                request = request.header("Mcp-Method", method);
            }
            if let Some(tool) = tool {
                request = request.header("Mcp-Name", tool);
            }
        }
        if let Some(session) = self.session() {
            request = request.header("Mcp-Session-Id", session);
        }
        for (name, value) in &self.credentials {
            request = request.header(name.as_str(), value.expose());
        }
        let exchange = async {
            let mut response = request.body(message.to_string()).send().await?;
            let status = response.status().as_u16();
            let header = |name: &str| {
                response
                    .headers()
                    .get(name)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned)
            };
            let session = header("mcp-session-id");
            let event_stream =
                header("content-type").is_some_and(|kind| kind.starts_with("text/event-stream"));
            let mut bytes = Vec::new();
            if !event_stream {
                while let Some(chunk) = response.chunk().await? {
                    if bytes.len() + chunk.len() > self.max_message_bytes {
                        return Ok(None);
                    }
                    bytes.extend_from_slice(&chunk);
                }
            }
            Ok::<_, reqwest::Error>(Some(Exchange {
                status,
                body: serde_json::from_slice(&bytes).ok(),
                session,
                event_stream,
            }))
        };
        match tokio::time::timeout_at(deadline.at(), exchange).await {
            // Dropping the exchange closes the stream, which is how a
            // request is cancelled over HTTP.
            Err(_) => Err(StepFailure::unknown(codes::timeout("the request"))),
            Ok(Ok(Some(exchange))) => Ok(exchange),
            Ok(Ok(None)) => Err(StepFailure::unknown(codes::unexpected(
                "The upstream server's answer is longer than the limit `max_message_bytes`.",
            ))),
            // A certificate or a handshake that failed is named; nothing
            // was sent either way.
            Ok(Err(error)) if error.is_connect() => Err(StepFailure::not_sent(
                a2m_auth::tls::refusal(&error, "the upstream server").unwrap_or_else(|| {
                    codes::unavailable("The upstream server could not be reached at its address.")
                }),
            )),
            Ok(Err(_)) => Err(StepFailure::unknown(codes::connection_lost(
                "after the request was sent and before its answer came",
            ))),
        }
    }

    /// The failure for a status that is not 200, when it is one the pack
    /// has a rule for.
    fn status_failure(&self, status: u16) -> Option<StepFailure> {
        Some(match status {
            401 => StepFailure::answered(a2m_core::Failure::new(
                a2m_core::FailureClass::SessionExpired,
                a2m_auth::codes::NEEDS_LOGIN,
                "The upstream server refused the credential.",
                "Sign in with `anything2mcp session login`, then resume the run, which reconciles any write it began before it writes again.",
            )),
            404 if self.session().is_some() => {
                self.lost.store(true, Ordering::SeqCst);
                StepFailure::answered(codes::connection_lost(
                    "because the server no longer knows the session",
                ))
            }
            429 | 502 | 503 | 504 => StepFailure::answered(codes::unavailable(format!(
                "The upstream server answered with the status {status}: it cannot serve now."
            ))),
            _ => return None,
        })
    }

    async fn request(
        &self,
        id: u64,
        message: &Json,
        revision: Revision,
        tool: Option<&str>,
        deadline: Deadline,
    ) -> Result<Json, StepFailure> {
        if self.lost.load(Ordering::SeqCst) {
            return Err(StepFailure::not_sent(codes::connection_lost(
                "before the request was sent",
            )));
        }
        let exchange = self.post(message, revision, tool, deadline).await?;
        if let Some(failure) = self.status_failure(exchange.status) {
            return Err(failure);
        }
        if exchange.event_stream {
            return Err(StepFailure::unknown(codes::unexpected(
                "The upstream server answered with an event stream, which this build does not read.",
            )));
        }
        if let (Some(session), Ok(mut held)) = (exchange.session, self.session.lock())
            && held.is_none()
        {
            *held = Some(session);
        }
        match exchange.body {
            Some(body) if read_answer(&body, id).is_some() => Ok(body),
            // A status with no JSON-RPC answer in it. Below 500 the server
            // refused the request; from 500 on, nothing says what it did.
            _ if (400..500).contains(&exchange.status) => {
                Err(StepFailure::answered(codes::unexpected(format!(
                    "The upstream server answered with the status {}, which the mapping does not foresee.",
                    exchange.status
                ))))
            }
            _ if exchange.status != 200 => Err(StepFailure::unknown(codes::unexpected(format!(
                "The upstream server answered with the status {}, which the mapping does not foresee.",
                exchange.status
            )))),
            _ => Err(not_a_response()),
        }
    }

    async fn notify(
        &self,
        message: &Json,
        revision: Revision,
        deadline: Deadline,
    ) -> Result<(), StepFailure> {
        let exchange = self.post(message, revision, None, deadline).await?;
        match self.status_failure(exchange.status) {
            Some(failure) => Err(failure),
            None if (200..300).contains(&exchange.status) => Ok(()),
            None => Err(StepFailure::answered(codes::unexpected(format!(
                "The upstream server answered a notice with the status {}.",
                exchange.status
            )))),
        }
    }
}

// --------------------------------------------------------------- client

/// The stream a client speaks over.
pub(crate) enum Link {
    Stdio(StdioLink),
    Http(HttpLink),
}

/// One connection to one upstream server, for one identity.
pub(crate) struct Client {
    link: Link,
    revision: Revision,
    next_id: AtomicU64,
    limits: Limits,
    /// What the server said it is, when the revision has a way to ask.
    pub(crate) server: Option<ServerInfo>,
    /// Whether the server said, in the handshake, that it gives notice
    /// when its tool list changes.
    notice_declared: bool,
}

impl Client {
    /// Connects over `link`: with the handshake, `initialize` and the
    /// notice that follows it; without it, nothing is sent.
    pub(crate) async fn connect(
        link: Link,
        revision: Revision,
        limits: Limits,
        deadline: Deadline,
    ) -> Result<Self, StepFailure> {
        let mut client = Self {
            link,
            revision,
            next_id: AtomicU64::new(1),
            limits,
            server: None,
            notice_declared: false,
        };
        if revision != Revision::NoHandshake {
            let result = client
                .request(
                    "initialize",
                    json!({
                        "protocolVersion": revision.as_str(),
                        "capabilities": {},
                        "clientInfo": client_info(),
                    }),
                    None,
                    deadline,
                )
                .await?;
            // The handshake's rule: the server offers a revision it serves,
            // and the client ends the connection if it cannot speak it.
            if result.get("protocolVersion").and_then(Json::as_str) != Some(revision.as_str()) {
                return Err(StepFailure::answered(codes::unexpected(format!(
                    "The upstream server offers another protocol revision than {}, the one the surface states.",
                    revision.as_str()
                ))));
            }
            let info = |key: &str| {
                result
                    .get("serverInfo")
                    .and_then(|server| server.get(key))
                    .and_then(Json::as_str)
                    .map(str::to_owned)
            };
            if let (Some(name), Some(version)) = (info("name"), info("version")) {
                client.server = Some(ServerInfo { name, version });
            }
            client.notice_declared = result
                .get("capabilities")
                .and_then(|capabilities| capabilities.get("tools"))
                .and_then(|tools| tools.get("listChanged"))
                .and_then(Json::as_bool)
                == Some(true);
            client
                .notify("notifications/initialized", json!({}), deadline)
                .await?;
        }
        Ok(client)
    }

    /// Whether the connection can carry no more requests.
    pub(crate) fn lost(&self) -> bool {
        match &self.link {
            Link::Stdio(link) => link.shared.lost.load(Ordering::SeqCst),
            Link::Http(link) => link.lost.load(Ordering::SeqCst),
        }
    }

    /// Whether a change to the server's tool list would be heard on this
    /// connection: the server said in the handshake that it gives notice,
    /// and the link is one a notice can arrive on. Over HTTP none can: this
    /// build opens no stream for the server to send one on. With no
    /// handshake the server has said nothing.
    pub(crate) fn gives_notice(&self) -> bool {
        match &self.link {
            Link::Stdio(_) => self.notice_declared,
            Link::Http(_) => false,
        }
    }

    /// Whether the server gave notice that its tool list changed since the
    /// last time this was asked. Never over HTTP, where no notice is read.
    pub(crate) fn take_list_changed(&self) -> bool {
        match &self.link {
            Link::Stdio(link) => link.shared.list_changed.swap(false, Ordering::SeqCst),
            Link::Http(_) => false,
        }
    }

    /// Ends the connection, and the program when there is one.
    pub(crate) async fn close(&self) -> a2m_provider::Cleanup {
        match &self.link {
            Link::Stdio(link) => link.close().await,
            Link::Http(link) => {
                link.lost.store(true, Ordering::SeqCst);
                a2m_provider::Cleanup::complete()
            }
        }
    }

    fn params(&self, mut params: Json) -> Json {
        if self.revision == Revision::NoHandshake
            && let Some(object) = params.as_object_mut()
        {
            object.insert(
                "_meta".to_owned(),
                json!({
                    META_VERSION: self.revision.as_str(),
                    META_CAPABILITIES: {},
                    META_CLIENT: client_info(),
                }),
            );
        }
        params
    }

    async fn request(
        &self,
        method: &str,
        params: Json,
        tool: Option<&str>,
        deadline: Deadline,
    ) -> Result<Json, StepFailure> {
        if deadline.passed() {
            return Err(StepFailure::not_sent(codes::timeout(
                "because the time for the step had passed before the request was sent",
            )));
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": self.params(params),
        });
        let body = match &self.link {
            Link::Stdio(link) => link.request(id, &message, deadline).await?,
            Link::Http(link) => {
                link.request(id, &message, self.revision, tool, deadline)
                    .await?
            }
        };
        match read_answer(&body, id) {
            Some(Answer::Result(result)) => Ok(result),
            Some(Answer::Error(code, data)) => Err(protocol_error(code, &data, method, tool)),
            None => Err(not_a_response()),
        }
    }

    async fn notify(
        &self,
        method: &str,
        params: Json,
        deadline: Deadline,
    ) -> Result<(), StepFailure> {
        let message = json!({"jsonrpc": "2.0", "method": method, "params": params});
        match &self.link {
            Link::Stdio(link) => link.notify(&message).await,
            Link::Http(link) => link.notify(&message, self.revision, deadline).await,
        }
    }

    /// The server's whole tool list, following its cursors. Listing is a
    /// read the protocol defines; it calls no tool.
    pub(crate) async fn list_tools(
        &self,
        deadline: Deadline,
    ) -> Result<Vec<ToolDefinition>, StepFailure> {
        let unreadable = || {
            StepFailure::answered(codes::unexpected(
                "The upstream server's tool list does not have the shape the protocol gives it.",
            ))
        };
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        let mut seen = Vec::new();
        for _ in 0..MAX_PAGES {
            let params = match &cursor {
                Some(cursor) => json!({"cursor": cursor}),
                None => json!({}),
            };
            let page = self.request("tools/list", params, None, deadline).await?;
            let listed = page
                .get("tools")
                .and_then(Json::as_array)
                .ok_or_else(unreadable)?;
            for entry in listed {
                tools.push(ToolDefinition::from_listed(entry).ok_or_else(unreadable)?);
                if tools.len() > self.limits.max_tools {
                    return Err(StepFailure::answered(codes::unexpected(
                        "The upstream server lists more tools than the limit `max_tools`.",
                    )));
                }
            }
            match page.get("nextCursor").filter(|next| !next.is_null()) {
                None => return Ok(tools),
                Some(next) => {
                    let next = next.as_str().ok_or_else(unreadable)?.to_owned();
                    // A cursor that comes round again would never end.
                    if seen.contains(&next) {
                        return Err(unreadable());
                    }
                    seen.push(next.clone());
                    cursor = Some(next);
                }
            }
        }
        Err(unreadable())
    }

    /// Calls one tool, once.
    pub(crate) async fn call_tool(
        &self,
        tool: &str,
        arguments: Json,
        deadline: Deadline,
    ) -> Result<ToolResult, StepFailure> {
        let raw = self
            .request(
                "tools/call",
                json!({"name": tool, "arguments": arguments}),
                Some(tool),
                deadline,
            )
            .await?;
        // A result that asks for more input, or that stands for a task, is
        // one this build has no rule for.
        let complete = match raw.get("resultType") {
            None => true,
            Some(kind) => kind.as_str() == Some("complete"),
        };
        let content = raw.get("content").map(|content| content.as_array());
        let (true, Some(blocks)) = (
            complete && raw.is_object(),
            content.unwrap_or(Some(&Vec::new())).cloned(),
        ) else {
            return Err(not_a_response());
        };
        let texts = blocks
            .iter()
            .filter(|block| block.get("type").and_then(Json::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Json::as_str))
            .map(str::to_owned)
            .collect();
        Ok(ToolResult {
            is_error: raw.get("isError").and_then(Json::as_bool) == Some(true),
            texts,
            structured: raw
                .get("structuredContent")
                .filter(|s| !s.is_null())
                .cloned(),
            raw,
        })
    }
}
