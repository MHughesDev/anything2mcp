//! The pack's first standard wire protocol: `nats`, the NATS client
//! protocol with the JetStream interface (decision 0167; research note 22).
//!
//! A small client of the pack's own on Tokio, written from the protocol's
//! public documentation and the server's public source. Text lines and
//! counted payloads over one connection; JetStream is requests and JSON
//! answers on subjects under `$JS.API.`. The connection is used for one
//! request at a time, so every answer is the answer to the one request in
//! flight, and a connection whose request was cut off carries no other.
//!
//! How a channel's address names what it reads:
//!
//! - `STREAM/subject`: a stream, and the subject its messages are published
//!   on. A message with a key goes to `subject.key`; one without, to
//!   `subject`. The channel is the whole stream, so every subject the
//!   stream lists must be `subject` or below it.
//! - `subject`: a topic of the core protocol, which keeps nothing.
//!
//! What the pack's safety rests on, and where the server's source says so:
//!
//! - A look takes nothing: `STREAM.MSG.GET` reads one stored message by
//!   sequence, by the next at or after a sequence, or by the last of a
//!   subject, through the stream's leader, and makes no consumer.
//! - The acknowledgement of a publish names the stream and the sequence,
//!   and `"duplicate": true` with the sequence first stored when the
//!   message id was seen within the stream's duplicate window.
//! - An acknowledgement of a pulled message is sent with a reply subject,
//!   and the server answers it with an empty message once it has carried
//!   it out ("double acknowledgement"); no answer leaves it in doubt.
//! - A status `503` means nobody listens on the subject: nothing was
//!   stored and nothing acknowledged.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::BuildHasher;
use std::io::ErrorKind;
use std::time::Duration;

use a2m_provider::{Deadline, Sent};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::channel::Shape;
use crate::json::Json;
use crate::wire::{
    ChannelFacts, Credential, Delivered, Link, Message, Opened, Outgoing, Page, Protocol, Receipt,
    Wire, WireError, WireErrorKind,
};

/// The protocol's name, as a surface states it.
pub(crate) const NAME: &str = "nats";

/// The header whose repeated value a stream refuses within its window.
pub(crate) const MSG_ID: &str = "Nats-Msg-Id";

/// The name the pack gives itself in `CONNECT`.
const CLIENT: &str = "anything2mcp";

/// The longest control line read from the server. Its own limit on what it
/// reads is 4096 bytes; an `INFO` may be longer, so this is larger.
const LONGEST_LINE: usize = 64 * 1024;

/// What a message's headers may add to the server's payload limit.
const HEADER_ROOM: usize = 64 * 1024;

/// The subscription every answer arrives on.
const INBOX_SID: &str = "1";

/// How many pages of the stream list a discovery reads.
const LIST_PAGES: u64 = 20;

/// How long an ephemeral consumer outlives its last use on the server.
const INACTIVE: Duration = Duration::from_secs(30);

fn error(kind: WireErrorKind, sent: Sent, detail: impl Into<String>) -> WireError {
    WireError {
        kind,
        sent,
        detail: detail.into(),
    }
}

fn unexpected(what: &str) -> WireError {
    error(WireErrorKind::Unexpected, Sent::Unknown, what)
}

fn unfit(what: &str) -> WireError {
    error(WireErrorKind::Unfit, Sent::NotSent, what)
}

// ---- names the protocol can carry ----

/// A stream's or a consumer's name: printable, and none of the characters
/// the server refuses in one.
pub(crate) fn is_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 255
        && !text
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || ".*>/\\".contains(c))
}

/// One token of a subject: printable, and none of `.`, `*`, `>`.
pub(crate) fn is_token(text: &str) -> bool {
    !text.is_empty()
        && !text
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || ".*>".contains(c))
}

/// A subject with no wildcard.
pub(crate) fn is_subject(text: &str) -> bool {
    text.len() <= 4096 && text.split('.').all(is_token)
}

/// Whether a subject is one of the server's own: its JetStream interface
/// and acknowledgements (`$JS`, `$JSC`), its system account (`$SYS`), its
/// key-value and object stores (`$KV`, `$O`), anything else under `$`, and
/// the reply subjects of other clients (`_INBOX`). A channel there would
/// let a caller's key choose a request to the server, or answer another
/// client's request.
pub(crate) fn is_system_subject(text: &str) -> bool {
    let first = text.split('.').next().unwrap_or_default();
    first.starts_with('$') || first.eq_ignore_ascii_case("_INBOX")
}

/// Where a channel's address points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place<'a> {
    /// A stream, and the subject its messages are published on.
    Stream { stream: &'a str, subject: &'a str },
    /// A subject of the core protocol, which keeps nothing.
    Topic { subject: &'a str },
}

/// Reads an address, or says why the protocol cannot name it.
pub(crate) fn place(address: &str) -> Result<Place<'_>, &'static str> {
    match address.split_once('/') {
        Some((stream, subject)) => {
            if !is_name(stream) {
                return Err("the stream's name holds a character a stream name cannot hold");
            }
            if !is_subject(subject) {
                return Err("the subject is not a subject without wildcards");
            }
            if is_system_subject(subject) {
                return Err("the subject is one of the server's own, under `$` or `_INBOX`");
            }
            Ok(Place::Stream { stream, subject })
        }
        None if is_system_subject(address) => {
            Err("the subject is one of the server's own, under `$` or `_INBOX`")
        }
        None if is_subject(address) => Ok(Place::Topic { subject: address }),
        None => Err("the address is neither `STREAM/subject` nor a subject without wildcards"),
    }
}

fn keyed(subject: &str, key: Option<&str>) -> Result<String, WireError> {
    match key {
        None => Ok(subject.to_owned()),
        // A key is one token: a `.`, `*`, or `>` in it would send the
        // message to another subject than the channel's.
        Some(key) if is_token(key) => Ok(format!("{subject}.{key}")),
        Some(_) => Err(unfit(
            "the key cannot be one token of a subject: it is empty or holds a space, `.`, `*`, or `>`",
        )),
    }
}

fn key_of(subject: &str, channel: &str) -> Option<String> {
    subject
        .strip_prefix(channel)
        .and_then(|rest| rest.strip_prefix('.'))
        .map(str::to_owned)
}

// ---- base64, for the stream's own get ----

pub(crate) fn base64_decode(text: &str) -> Option<Vec<u8>> {
    fn value(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let pad = chunk.iter().rev().take_while(|c| **c == b'=').count();
        if pad > 2 {
            return None;
        }
        let mut n = 0_u32;
        for (i, c) in chunk.iter().enumerate() {
            let v = if i >= 4 - pad { 0 } else { value(*c)? };
            n = (n << 6) | v;
        }
        let [_, a, b, c] = n.to_be_bytes();
        out.push(a);
        if pad < 2 {
            out.push(b);
        }
        if pad < 1 {
            out.push(c);
        }
    }
    Some(out)
}

// ---- headers ----

/// A headers block, as `HPUB` and `HMSG` carry it.
fn header_block(headers: &BTreeMap<String, String>) -> Result<Vec<u8>, WireError> {
    let mut block = String::from("NATS/1.0\r\n");
    for (name, value) in headers {
        let name_fits = is_token(name) && !name.contains(':');
        let value_fits = !value.chars().any(|c| c == '\r' || c == '\n');
        if !name_fits || !value_fits {
            return Err(unfit(
                "a header's name or value holds a character the protocol cannot carry",
            ));
        }
        block.push_str(name);
        block.push_str(": ");
        block.push_str(value);
        block.push_str("\r\n");
    }
    block.push_str("\r\n");
    Ok(block.into_bytes())
}

/// A status, where the block has one, and the headers. A header repeated
/// keeps its first value.
fn parse_headers(block: &[u8]) -> Option<(Option<String>, BTreeMap<String, String>)> {
    let text = std::str::from_utf8(block).ok()?;
    let mut lines = text.split("\r\n");
    let first = lines.next()?.strip_prefix("NATS/1.0")?;
    let status = Some(first.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let mut headers = BTreeMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':')?;
        headers
            .entry(name.trim().to_owned())
            .or_insert_with(|| value.trim().to_owned());
    }
    Some((status, headers))
}

// ---- the connection ----

/// One message the server sent.
#[derive(Debug, Clone)]
struct Incoming {
    subject: String,
    sid: String,
    reply: Option<String>,
    status: Option<String>,
    headers: BTreeMap<String, String>,
    data: Vec<u8>,
}

impl Incoming {
    fn status_code(&self) -> Option<&str> {
        self.status.as_deref().and_then(|s| s.split(' ').next())
    }
}

enum Event {
    Message(Incoming),
    Pong,
    Error(String),
}

/// A message line read, waiting for its payload.
struct Head {
    subject: String,
    sid: String,
    reply: Option<String>,
    header_len: usize,
    total: usize,
}

/// One subscription the link holds.
enum Sub {
    /// A subscription of the core protocol, on a topic.
    Core { sid: String, subject: String },
    /// A pull consumer of a stream.
    Pull {
        stream: String,
        consumer: String,
        /// The channel's subject, for the key of what it delivers.
        subject: String,
        /// Whether each delivery is acknowledged: on a work queue only.
        explicit: bool,
        /// Made by this link; deleted when let go.
        ephemeral: bool,
        /// Deliveries handed over and not yet acknowledged or given back.
        held: BTreeSet<String>,
    },
}

struct NatsLink {
    stream: BufReader<TcpStream>,
    /// A control line read in part. Kept here so that a read cut off by a
    /// wait's end loses nothing.
    line: Vec<u8>,
    /// A message line whose payload is being read, and what was read.
    head: Option<(Head, Vec<u8>)>,
    max_payload: usize,
    inbox: String,
    next_reply: u64,
    next_sid: u64,
    /// True from the moment a request starts until its whole answer is
    /// read, and for ever after a failure of the connection.
    broken: bool,
    subs: BTreeMap<String, Sub>,
    /// Messages of a topic that arrived while something else was read.
    queued: BTreeMap<String, Vec<Incoming>>,
    /// A number for each message of a topic, which the protocol does not
    /// identify.
    topic_count: u64,
}

/// The largest message the client reads or sends when no setting says
/// otherwise: 8 MiB, eight times the server's default `max_payload`.
pub(crate) const DEFAULT_CEILING: usize = 8 * 1024 * 1024;

/// The most messages of a topic held for their fetch while something else
/// is read.
const QUEUED_MOST: usize = 1000;

/// The protocol.
pub(crate) struct NatsProtocol {
    /// The pack's own bound on the size of any message, whatever a server
    /// states: the setting `message_ceiling_bytes`.
    ceiling: usize,
}

impl NatsProtocol {
    /// The protocol, reading and sending no message larger than `ceiling`.
    pub(crate) fn new(ceiling: usize) -> Self {
        Self { ceiling }
    }
}

/// A prefix for the connection's reply subjects that another client of the
/// same server cannot guess: it is not shown, hashed, or compared anywhere.
fn inbox_prefix() -> String {
    let first = std::collections::hash_map::RandomState::new().hash_one(std::process::id());
    let second = std::collections::hash_map::RandomState::new().hash_one(first);
    format!("_INBOX.{first:016x}{second:016x}")
}

impl Protocol for NatsProtocol {
    fn name(&self) -> &'static str {
        NAME
    }

    fn carries(&self, _credential: &Credential) -> bool {
        // A token as `auth_token`, a user and password as `user` and `pass`.
        true
    }

    fn connect<'a>(
        &'a self,
        address: &'a str,
        credential: Option<&'a Credential>,
        deadline: Deadline,
    ) -> Wire<'a, Box<dyn Link>> {
        Box::pin(async move {
            // Plain TCP carries the session's credentials in the clear, so it
            // goes to this machine's own loopback address and nowhere else.
            let resolved = crate::wire::plain_tcp_address(address)?;
            let unreachable = || {
                error(
                    WireErrorKind::Unreachable,
                    Sent::NotSent,
                    "the broker did not accept a connection",
                )
            };
            let stream = tokio::time::timeout_at(deadline.at(), TcpStream::connect(resolved))
                .await
                .map_err(|_| unreachable())?
                .map_err(|_| unreachable())?;
            let _ = stream.set_nodelay(true);
            let mut link = NatsLink {
                stream: BufReader::new(stream),
                line: Vec::new(),
                head: None,
                max_payload: 0,
                inbox: inbox_prefix(),
                next_reply: 1,
                next_sid: 2,
                broken: true,
                subs: BTreeMap::new(),
                queued: BTreeMap::new(),
                topic_count: 0,
            };
            let info = match tokio::time::timeout_at(deadline.at(), link.read_line()).await {
                Ok(Ok(Some(line))) => line,
                _ => return Err(unreachable()),
            };
            let info = info
                .strip_prefix("INFO ")
                .and_then(|text| Json::parse(text).ok())
                .ok_or_else(|| {
                    error(
                        WireErrorKind::Unexpected,
                        Sent::NotSent,
                        "the server's first line is not the protocol's `INFO`",
                    )
                })?;
            if info.get("tls_required").and_then(Json::as_bool) == Some(true) {
                return Err(error(
                    WireErrorKind::Transport,
                    Sent::NotSent,
                    "the server requires TLS, and this build speaks plain TCP only",
                ));
            }
            if info.get("headers").and_then(Json::as_bool) != Some(true) {
                return Err(error(
                    WireErrorKind::Unexpected,
                    Sent::NotSent,
                    "the server does not carry headers, so a record's token cannot travel",
                ));
            }
            // The server's own limit, never above the pack's: a server that
            // states more is held to the ceiling, so that what it states
            // never decides what the client takes in.
            link.max_payload = info
                .get("max_payload")
                .and_then(Json::as_u64)
                .map(|n| usize::try_from(n).unwrap_or(usize::MAX))
                .filter(|n| *n > 0)
                .ok_or_else(|| {
                    error(
                        WireErrorKind::Unexpected,
                        Sent::NotSent,
                        "the server's `INFO` states no payload limit",
                    )
                })?
                .min(self.ceiling);
            let mut options = BTreeMap::from([
                ("verbose".to_owned(), Json::Bool(false)),
                ("pedantic".to_owned(), Json::Bool(false)),
                ("tls_required".to_owned(), Json::Bool(false)),
                ("name".to_owned(), Json::text(CLIENT)),
                ("lang".to_owned(), Json::text("rust")),
                ("version".to_owned(), Json::text(env!("CARGO_PKG_VERSION"))),
                ("protocol".to_owned(), Json::number(1)),
                ("echo".to_owned(), Json::Bool(false)),
                ("headers".to_owned(), Json::Bool(true)),
                ("no_responders".to_owned(), Json::Bool(true)),
            ]);
            match credential {
                Some(Credential::Token(token)) => {
                    options.insert("auth_token".to_owned(), Json::text(token.expose()));
                }
                Some(Credential::UserPassword { user, password }) => {
                    options.insert("user".to_owned(), Json::text(user.as_str()));
                    options.insert("pass".to_owned(), Json::text(password.expose()));
                }
                None => {}
            }
            // The line holds the credential: it is written and dropped, and
            // no error below can carry it.
            let hello = format!(
                "CONNECT {}\r\nSUB {}.* {INBOX_SID}\r\nPING\r\n",
                Json::Object(options).to_line(),
                link.inbox
            );
            link.write(hello.as_bytes(), deadline).await?;
            match link.next_event(deadline).await? {
                Event::Pong => {}
                Event::Error(said) => return Err(err_line(&said)),
                Event::Message(_) => return Err(unexpected("the server sent a message unasked")),
            }
            link.broken = false;
            Ok(Box::new(link) as Box<dyn Link>)
        })
    }
}

/// A `-ERR` line that arrives while a publish or an acknowledgement is in
/// flight on the connection. Nothing ties a `-ERR` to the request: the
/// server sends one on a timer (a credential that expires) whatever it has
/// already carried out, so once the bytes of a write have left, every
/// `-ERR` leaves the write in doubt, and the reconcile decides.
fn err_in_flight(said: &str) -> WireError {
    error(WireErrorKind::Unexpected, Sent::Unknown, said)
}

/// A `-ERR` line while nothing that writes is in flight: at `CONNECT`, or
/// during a look, as this pack names it.
fn err_line(said: &str) -> WireError {
    let lower = said.to_ascii_lowercase();
    if lower.contains("authorization violation") || lower.contains("authentication") {
        error(WireErrorKind::NotAuthorized, Sent::Answered, said)
    } else if lower.contains("maximum payload") {
        // The server refuses the message at the door and closes the
        // connection: nothing was routed.
        error(WireErrorKind::TooLarge, Sent::Answered, said)
    } else {
        // Any other: a permission refused, a stale connection, a line it
        // could not read. Nothing ties it to the request in flight.
        error(WireErrorKind::Unexpected, Sent::Unknown, said)
    }
}

/// A JetStream error object, as this pack names it. A code with no rule
/// has an unknown fate.
fn api_refusal(code: u64, said: &str) -> WireError {
    let kind = match code {
        10059 => WireErrorKind::ChannelMissing,
        10014 => WireErrorKind::SubscriptionMissing,
        // A second consumer on a work queue: something else reads it.
        10099 | 10100 => WireErrorKind::NotSupported,
        // Wrong last sequence or message id, another stream expected, a
        // sealed stream, a store that refused, no room: nothing stored.
        10071 | 10070 | 10060 | 10109 | 10077 | 10023 | 10002 => WireErrorKind::Refused,
        10054 | 10097 => WireErrorKind::TooLarge,
        // JetStream not enabled, or not available now.
        10076 | 10039 | 10008 => WireErrorKind::Unreachable,
        _ => {
            return error(
                WireErrorKind::Unexpected,
                Sent::Unknown,
                format!("the server refused with the code {code}: {said}"),
            );
        }
    };
    error(kind, Sent::Answered, said)
}

/// The error object of a JetStream answer, if it has one.
fn api_error(answer: &Json) -> Option<WireError> {
    let error = answer.get("error")?;
    let code = error.get("err_code").and_then(Json::as_u64).unwrap_or(0);
    let said = error
        .get("description")
        .and_then(Json::as_str)
        .unwrap_or_default();
    Some(api_refusal(code, said))
}

fn number(value: &Json, key: &str) -> Result<u64, WireError> {
    value
        .get(key)
        .and_then(Json::as_u64)
        .ok_or_else(|| unexpected("a count or a sequence is missing or not a whole number"))
}

impl NatsLink {
    /// Reads one line; `None` at the end of the stream. Cancel-safe: what
    /// was read stays in `self.line`.
    async fn read_line(&mut self) -> std::io::Result<Option<String>> {
        loop {
            let buffer = self.stream.fill_buf().await?;
            if buffer.is_empty() {
                return Ok(None);
            }
            let end = buffer.iter().position(|byte| *byte == b'\n');
            let take = end.map_or(buffer.len(), |end| end + 1);
            if self.line.len() + take > LONGEST_LINE {
                return Err(std::io::Error::new(
                    ErrorKind::InvalidData,
                    "a line is too long",
                ));
            }
            self.line.extend_from_slice(&buffer[..take]);
            self.stream.consume(take);
            if end.is_some() {
                let mut line = std::mem::take(&mut self.line);
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return String::from_utf8(line)
                    .map(Some)
                    .map_err(|_| std::io::Error::new(ErrorKind::InvalidData, "not UTF-8"));
            }
        }
    }

    /// Reads one thing the server sent. Cancel-safe.
    async fn read_raw(&mut self) -> Result<Option<Event>, WireError> {
        let lost = || {
            error(
                WireErrorKind::Lost,
                Sent::Unknown,
                "the connection to the broker ended before it answered",
            )
        };
        loop {
            if let Some((head, body)) = &mut self.head {
                let need = head.total.saturating_add(2);
                while body.len() < need {
                    let buffer = self.stream.fill_buf().await.map_err(|_| lost())?;
                    if buffer.is_empty() {
                        return Err(lost());
                    }
                    let take = buffer.len().min(need - body.len());
                    body.extend_from_slice(&buffer[..take]);
                    self.stream.consume(take);
                }
                let Some((head, mut body)) = self.head.take() else {
                    return Err(lost());
                };
                if !body.ends_with(b"\r\n") {
                    return Err(unexpected(
                        "a message's payload is not ended as the protocol says",
                    ));
                }
                body.truncate(head.total);
                let data = body.split_off(head.header_len);
                let (status, headers) = if head.header_len > 0 {
                    parse_headers(&body).ok_or_else(|| {
                        unexpected("a message's headers are not in the protocol's form")
                    })?
                } else {
                    (None, BTreeMap::new())
                };
                return Ok(Some(Event::Message(Incoming {
                    subject: head.subject,
                    sid: head.sid,
                    reply: head.reply,
                    status,
                    headers,
                    data,
                })));
            }
            let line = match self.read_line().await {
                Ok(Some(line)) => line,
                Ok(None) => return Err(lost()),
                Err(failed) if failed.kind() == ErrorKind::InvalidData => {
                    return Err(unexpected(
                        "the server sent a line the protocol does not allow",
                    ));
                }
                Err(_) => return Err(lost()),
            };
            let (op, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
            match op.to_ascii_uppercase().as_str() {
                "PING" => {
                    // The server asks whether the client is still there.
                    let wrote = self.stream.get_mut().write_all(b"PONG\r\n").await;
                    wrote.map_err(|_| lost())?;
                }
                "PONG" => return Ok(Some(Event::Pong)),
                "+OK" | "INFO" => {}
                "-ERR" => {
                    let said = rest.trim().trim_matches('\'').to_owned();
                    return Ok(Some(Event::Error(said)));
                }
                word @ ("MSG" | "HMSG") => {
                    let parts: Vec<&str> = rest.split_whitespace().collect();
                    let counts = if word == "MSG" { 1 } else { 2 };
                    if parts.len() != 2 + counts && parts.len() != 3 + counts {
                        return Err(unexpected("a message line is not in the protocol's form"));
                    }
                    let numbers: Option<Vec<usize>> = parts[parts.len() - counts..]
                        .iter()
                        .map(|n| n.parse().ok())
                        .collect();
                    let numbers = numbers
                        .ok_or_else(|| unexpected("a message's size is not a whole number"))?;
                    let (header_len, total) = if counts == 1 {
                        (0, numbers[0])
                    } else {
                        (numbers[0], numbers[1])
                    };
                    // Bounded by the pack's ceiling, whatever the server
                    // stated, before anything is set aside for it.
                    if header_len > total || total > self.max_payload.saturating_add(HEADER_ROOM) {
                        return Err(unexpected(
                            "a message is larger than the server's limit or the pack's setting `message_ceiling_bytes` allows",
                        ));
                    }
                    self.head = Some((
                        Head {
                            subject: parts[0].to_owned(),
                            sid: parts[1].to_owned(),
                            reply: (parts.len() == 3 + counts).then(|| parts[2].to_owned()),
                            header_len,
                            total,
                        },
                        // Grown as the bytes arrive.
                        Vec::with_capacity(total.saturating_add(2).min(LONGEST_LINE)),
                    ));
                }
                _ => {
                    return Err(unexpected(
                        "the server sent a line the protocol does not have",
                    ));
                }
            }
        }
    }

    /// The next event, by the deadline.
    async fn next_event(&mut self, deadline: Deadline) -> Result<Event, WireError> {
        match tokio::time::timeout_at(deadline.at(), self.read_raw()).await {
            Err(_) => Err(error(
                WireErrorKind::TimedOut,
                Sent::Unknown,
                "the broker did not answer in time",
            )),
            Ok(Err(failed)) => Err(failed),
            Ok(Ok(Some(event))) => Ok(event),
            Ok(Ok(None)) => Err(unexpected("the server sent nothing the protocol describes")),
        }
    }

    async fn write(&mut self, bytes: &[u8], deadline: Deadline) -> Result<(), WireError> {
        let wrote = tokio::time::timeout_at(deadline.at(), async {
            self.stream.get_mut().write_all(bytes).await?;
            self.stream.get_mut().flush().await
        })
        .await;
        match wrote {
            Err(_) => Err(error(
                WireErrorKind::TimedOut,
                Sent::Unknown,
                "the broker did not take the request in time",
            )),
            Ok(Err(_)) => Err(error(
                WireErrorKind::Lost,
                Sent::Unknown,
                "the connection to the broker ended while the request was sent",
            )),
            Ok(Ok(())) => Ok(()),
        }
    }

    fn start(&mut self) -> Result<(), WireError> {
        if self.broken {
            return Err(error(
                WireErrorKind::Lost,
                Sent::NotSent,
                "the connection to the broker cannot carry another request",
            ));
        }
        self.broken = true;
        Ok(())
    }

    /// The bytes of one publish, checked against the server's limit.
    fn publish_bytes(
        &self,
        subject: &str,
        reply: Option<&str>,
        headers: &BTreeMap<String, String>,
        body: &[u8],
    ) -> Result<Vec<u8>, WireError> {
        let reply = reply.map(|r| format!(" {r}")).unwrap_or_default();
        let mut out = if headers.is_empty() {
            if body.len() > self.max_payload {
                return Err(error(
                    WireErrorKind::TooLarge,
                    Sent::NotSent,
                    "the message is larger than the server's payload limit",
                ));
            }
            format!("PUB {subject}{reply} {}\r\n", body.len()).into_bytes()
        } else {
            let block = header_block(headers)?;
            let total = block.len() + body.len();
            if total > self.max_payload {
                return Err(error(
                    WireErrorKind::TooLarge,
                    Sent::NotSent,
                    "the message is larger than the server's payload limit",
                ));
            }
            let mut out = format!("HPUB {subject}{reply} {} {total}\r\n", block.len()).into_bytes();
            out.extend_from_slice(&block);
            out
        };
        out.extend_from_slice(body);
        out.extend_from_slice(b"\r\n");
        Ok(out)
    }

    /// Keeps a message of a topic for its fetch, or says it has no rule.
    fn keep(&mut self, message: Incoming) -> Result<(), WireError> {
        let topic = self
            .subs
            .values()
            .any(|sub| matches!(sub, Sub::Core { sid, .. } if *sid == message.sid));
        if topic {
            let queue = self.queued.entry(message.sid.clone()).or_default();
            if queue.len() >= QUEUED_MOST {
                return Err(unexpected(
                    "more messages of a topic arrived while something else was read than the client holds",
                ));
            }
            queue.push(message);
            Ok(())
        } else {
            Err(unexpected(
                "the server sent a message no request of this connection asked for",
            ))
        }
    }

    /// Sends one request and reads the one answer that belongs to it.
    /// `writes` says the request is a publish or an acknowledgement: a
    /// `-ERR` then leaves it in doubt.
    async fn request(
        &mut self,
        subject: &str,
        headers: &BTreeMap<String, String>,
        body: &[u8],
        writes: bool,
        deadline: Deadline,
    ) -> Result<Incoming, WireError> {
        let reply = format!("{}.{}", self.inbox, self.next_reply);
        self.next_reply += 1;
        let bytes = self.publish_bytes(subject, Some(&reply), headers, body)?;
        self.start()?;
        self.write(&bytes, deadline).await?;
        loop {
            match self.next_event(deadline).await? {
                Event::Message(message) if message.sid == INBOX_SID && message.subject == reply => {
                    self.broken = false;
                    return Ok(message);
                }
                Event::Message(message) => self.keep(message)?,
                Event::Pong => return Err(unexpected("the server answered a ping no one sent")),
                Event::Error(said) if writes => return Err(err_in_flight(&said)),
                Event::Error(said) => return Err(err_line(&said)),
            }
        }
    }

    /// A JetStream request with a JSON body, and its JSON answer. An error
    /// object is left in the answer for the caller to read.
    async fn api(
        &mut self,
        subject: &str,
        body: &Json,
        deadline: Deadline,
    ) -> Result<Json, WireError> {
        let line = if body.is_null() {
            String::new()
        } else {
            body.to_line()
        };
        let answer = self
            .request(subject, &BTreeMap::new(), line.as_bytes(), false, deadline)
            .await?;
        if answer.status_code() == Some("503") {
            return Err(error(
                WireErrorKind::Unreachable,
                Sent::Answered,
                "nothing on the server answers the JetStream interface",
            ));
        }
        if answer.status.is_some() {
            return Err(unexpected(
                "the JetStream interface answered with a status it has no rule for",
            ));
        }
        std::str::from_utf8(&answer.data)
            .ok()
            .and_then(|text| Json::parse(text).ok())
            .filter(|json| json.as_object().is_some())
            .ok_or_else(|| {
                error(
                    WireErrorKind::Unexpected,
                    Sent::Answered,
                    "a JetStream answer is not one JSON object",
                )
            })
    }

    /// A JetStream answer with its error object turned into a failure.
    async fn api_ok(
        &mut self,
        subject: &str,
        body: &Json,
        deadline: Deadline,
    ) -> Result<Json, WireError> {
        let answer = self.api(subject, body, deadline).await?;
        match api_error(&answer) {
            Some(failed) => Err(failed),
            None => Ok(answer),
        }
    }

    /// Writes a ping and reads to its pong: everything the server sent
    /// before it is in hand. `writes` says a publish went before it: a
    /// `-ERR` then leaves the publish in doubt.
    async fn round_trip(
        &mut self,
        writes: bool,
        deadline: Deadline,
    ) -> Result<Vec<Incoming>, WireError> {
        self.start()?;
        self.write(b"PING\r\n", deadline).await?;
        let mut before = Vec::new();
        loop {
            match self.next_event(deadline).await? {
                Event::Pong => {
                    self.broken = false;
                    return Ok(before);
                }
                Event::Message(message) => before.push(message),
                Event::Error(said) if writes => return Err(err_in_flight(&said)),
                Event::Error(said) => return Err(err_line(&said)),
            }
        }
    }

    async fn stream_info(&mut self, stream: &str, deadline: Deadline) -> Result<Json, WireError> {
        self.api_ok(
            &format!("$JS.API.STREAM.INFO.{stream}"),
            &Json::Null,
            deadline,
        )
        .await
    }

    async fn consumer_names(
        &mut self,
        stream: &str,
        deadline: Deadline,
    ) -> Result<Vec<String>, WireError> {
        let answer = self
            .api_ok(
                &format!("$JS.API.CONSUMER.NAMES.{stream}"),
                &Json::Null,
                deadline,
            )
            .await?;
        Ok(match answer.get("consumers") {
            None | Some(Json::Null) => Vec::new(),
            Some(list) => list
                .as_list()
                .ok_or_else(|| unexpected("a list of consumers is not a list"))?
                .iter()
                .map(|name| {
                    name.as_str()
                        .filter(|name| is_name(name))
                        .map(str::to_owned)
                })
                .collect::<Option<Vec<String>>>()
                .ok_or_else(|| {
                    unexpected("a consumer the server lists has no name a consumer can have")
                })?,
        })
    }

    /// What one stream reports, as the pack's facts about a channel.
    fn facts(
        address: String,
        subject: &str,
        info: &Json,
        subscriptions: Vec<String>,
    ) -> Result<ChannelFacts, WireError> {
        let config = info
            .get("config")
            .ok_or_else(|| unexpected("a stream's description holds no configuration"))?;
        let state = info
            .get("state")
            .ok_or_else(|| unexpected("a stream's description holds no state"))?;
        let one_per_subject =
            matches!(config.get("max_msgs_per_subject"), Some(Json::Number(n)) if n == "1");
        // The channel is the whole stream: a subject the stream stores
        // that is not the channel's would be read as the channel's.
        let within = config
            .get("subjects")
            .and_then(Json::as_list)
            .is_some_and(|subjects| {
                subjects.iter().all(|s| {
                    s.as_str().is_some_and(|s| {
                        s == subject
                            || s.strip_prefix(subject)
                                .is_some_and(|rest| rest.starts_with('.'))
                    })
                })
            });
        let shape = match config.get("retention").and_then(Json::as_str) {
            _ if !within => None,
            Some("workqueue") => Some(Shape::Queue),
            Some("limits") if one_per_subject => Some(Shape::Retained),
            Some("limits") => Some(Shape::Log),
            _ => None,
        };
        let window_ns = config
            .get("duplicate_window")
            .and_then(Json::as_u64)
            .unwrap_or(0);
        // What lets the stream drop a message from inside its range, not
        // only from its front: a limit per subject other than one (which
        // is a retained channel's, read by its key) that evicts, a time to
        // live per message, or a retention that removes a message once it
        // is read or nobody is interested. A field the stream states in a
        // form with no rule is read as able to lose.
        let per_subject_unlimited = match config.get("max_msgs_per_subject") {
            None => true,
            Some(Json::Number(n)) => n == "-1" || n == "0" || n == "1",
            Some(_) => false,
        };
        let ttl = match config.get("allow_msg_ttl") {
            None | Some(Json::Bool(false)) => false,
            Some(_) => true,
        };
        let removes_read = !matches!(
            config.get("retention").and_then(Json::as_str),
            Some("limits")
        );
        let may_lose_inside = !per_subject_unlimited || ttl || removes_read;
        // The server keys a stored message by its whole subject, and the
        // channel's key is the token below the channel's subject: the
        // stream keys by subject where it stores a subject below it.
        let key_from_subject =
            config
                .get("subjects")
                .and_then(Json::as_list)
                .is_some_and(|subjects| {
                    subjects.iter().any(|s| {
                        s.as_str()
                            .and_then(|s| s.strip_prefix(subject))
                            .is_some_and(|rest| rest.starts_with('.'))
                    })
                });
        let last = number(state, "last_seq")?;
        let first = number(state, "first_seq")?;
        Ok(ChannelFacts {
            address,
            shape,
            key_field: None,
            key_from_subject: within && key_from_subject,
            schema: None,
            dedupe_header: (window_ns > 0).then(|| MSG_ID.to_owned()),
            dedupe_window_ms: Some(window_ns / 1_000_000),
            held: number(state, "messages")?,
            start: Some(first.max(1).min(last + 1)),
            end: Some(last + 1),
            subscriptions,
            may_lose_inside,
        })
    }

    /// One stored message, from the stream's own get.
    fn stored(&self, subject: &str, answer: &Json) -> Result<Message, WireError> {
        let malformed = || unexpected("a stored message is not in the protocol's form");
        let message = answer.get("message").ok_or_else(malformed)?;
        let seq = number(message, "seq")?;
        let on = message
            .get("subject")
            .and_then(Json::as_str)
            .ok_or_else(malformed)?;
        let decode = |key: &str| -> Result<Vec<u8>, WireError> {
            match message.get(key) {
                None | Some(Json::Null) => Ok(Vec::new()),
                Some(text) => text.as_str().and_then(base64_decode).ok_or_else(malformed),
            }
        };
        let headers = decode("hdrs")?;
        let headers = if headers.is_empty() {
            BTreeMap::new()
        } else {
            parse_headers(&headers).ok_or_else(malformed)?.1
        };
        let body = body_of(&decode("data")?)?;
        Ok(Message {
            id: seq.to_string(),
            key: key_of(on, subject),
            headers,
            body,
            position: Some(seq),
        })
    }

    async fn get(
        &mut self,
        stream: &str,
        subject: &str,
        request: Json,
        deadline: Deadline,
    ) -> Result<Option<Message>, WireError> {
        let answer = self
            .api(
                &format!("$JS.API.STREAM.MSG.GET.{stream}"),
                &request,
                deadline,
            )
            .await?;
        if let Some(failed) = api_error(&answer) {
            let none = answer
                .get("error")
                .and_then(|e| e.get("err_code"))
                .and_then(Json::as_u64)
                == Some(10037);
            return if none { Ok(None) } else { Err(failed) };
        }
        self.stored(subject, &answer).map(Some)
    }

    fn stream_place<'a>(&self, address: &'a str) -> Result<(&'a str, &'a str), WireError> {
        match place(address).map_err(unfit)? {
            Place::Stream { stream, subject } => Ok((stream, subject)),
            Place::Topic { .. } => Err(error(
                WireErrorKind::NotSupported,
                Sent::NotSent,
                "a topic keeps nothing to look at",
            )),
        }
    }
}

/// A message's payload as the pack's JSON. One that is not JSON is no
/// message of a channel whose encoding is `json`.
fn body_of(data: &[u8]) -> Result<Json, WireError> {
    std::str::from_utf8(data)
        .ok()
        .and_then(|text| Json::parse(text).ok())
        .ok_or_else(|| {
            error(
                WireErrorKind::Unexpected,
                Sent::Answered,
                "a message's payload is not JSON",
            )
        })
}

/// What an acknowledgement subject names: the stream, the consumer, how
/// many times the message was delivered, and its stream sequence. The
/// 9-token form, and the longer form with a domain and an account.
struct AckMeta<'a> {
    stream: &'a str,
    consumer: &'a str,
    attempt: u64,
    seq: u64,
}

fn ack_meta(ack: &str) -> Option<AckMeta<'_>> {
    let tokens: Vec<&str> = ack.split('.').collect();
    if tokens.first() != Some(&"$JS") || tokens.get(1) != Some(&"ACK") {
        return None;
    }
    let offset = match tokens.len() {
        9 => 2,
        n if n >= 11 => 4,
        _ => return None,
    };
    Some(AckMeta {
        stream: tokens.get(offset)?,
        consumer: tokens.get(offset + 1)?,
        attempt: tokens.get(offset + 2)?.parse().ok()?,
        seq: tokens.get(offset + 3)?.parse().ok()?,
    })
}

/// The stream sequence an acknowledgement subject names.
fn sequence_of(ack: &str) -> Option<u64> {
    ack_meta(ack).map(|meta| meta.seq)
}

impl Link for NatsLink {
    fn alive(&self) -> bool {
        if self.broken
            || self.head.is_some()
            || !self.line.is_empty()
            || !self.stream.buffer().is_empty()
        {
            return false;
        }
        // Between requests a live connection has nothing to read, unless
        // the server pinged it; either way, one that has something waiting
        // is not reused, and one that reads nothing has ended.
        let mut probe = [0_u8; 1];
        matches!(
            self.stream.get_ref().try_read(&mut probe),
            Err(failed) if failed.kind() == ErrorKind::WouldBlock
        )
    }

    fn channels(&mut self, deadline: Deadline) -> Wire<'_, Vec<ChannelFacts>> {
        Box::pin(async move {
            let mut found = Vec::new();
            let mut offset = 0_u64;
            for _ in 0..LIST_PAGES {
                let request = Json::object([("offset", Json::number(offset))]);
                let page = self
                    .api_ok("$JS.API.STREAM.LIST", &request, deadline)
                    .await?;
                let total = number(&page, "total")?;
                let streams = match page.get("streams") {
                    None | Some(Json::Null) => Vec::new(),
                    Some(list) => list
                        .as_list()
                        .ok_or_else(|| unexpected("a list of streams is not a list"))?
                        .to_vec(),
                };
                for info in &streams {
                    let config = info.get("config");
                    let name = config
                        .and_then(|c| c.get("name"))
                        .and_then(Json::as_str)
                        .filter(|name| is_name(name))
                        .ok_or_else(|| {
                            unexpected("a stream the server lists has no name a stream can have")
                        })?
                        .to_owned();
                    let first = config
                        .and_then(|c| c.get("subjects"))
                        .and_then(Json::as_list)
                        .and_then(|s| s.first())
                        .and_then(Json::as_str)
                        .unwrap_or_default();
                    let subject = first
                        .strip_suffix(".>")
                        .or_else(|| first.strip_suffix(".*"))
                        .unwrap_or(first)
                        .to_owned();
                    if !subject.is_empty() && !is_subject(&subject) {
                        return Err(unexpected(
                            "a stream the server lists stores a subject the protocol cannot name",
                        ));
                    }
                    let consumers = self.consumer_names(&name, deadline).await?;
                    found.push(Self::facts(
                        format!("{name}/{subject}"),
                        &subject,
                        info,
                        consumers,
                    )?);
                }
                offset += u64::try_from(streams.len()).unwrap_or(u64::MAX);
                if offset >= total || streams.is_empty() {
                    return Ok(found);
                }
            }
            Err(error(
                WireErrorKind::Unexpected,
                Sent::Answered,
                "the server lists more streams than a discovery reads",
            ))
        })
    }

    fn describe<'a>(&'a mut self, address: &'a str, deadline: Deadline) -> Wire<'a, ChannelFacts> {
        Box::pin(async move {
            match place(address).map_err(unfit)? {
                Place::Stream { stream, subject } => {
                    let info = self.stream_info(stream, deadline).await?;
                    let consumers = self.consumer_names(stream, deadline).await?;
                    Self::facts(address.to_owned(), subject, &info, consumers)
                }
                Place::Topic { subject } => {
                    // A topic is a subject no stream stores: one that a
                    // stream stores keeps what is published to it.
                    let mut stored = false;
                    for filter in [subject.to_owned(), format!("{subject}.>")] {
                        let request = Json::object([("subject", Json::text(filter))]);
                        let names = self
                            .api_ok("$JS.API.STREAM.NAMES", &request, deadline)
                            .await?;
                        stored |= names
                            .get("streams")
                            .and_then(Json::as_list)
                            .is_some_and(|list| !list.is_empty());
                    }
                    Ok(ChannelFacts {
                        address: address.to_owned(),
                        shape: (!stored).then_some(Shape::Topic),
                        key_field: None,
                        key_from_subject: false,
                        schema: None,
                        dedupe_header: None,
                        dedupe_window_ms: None,
                        held: 0,
                        start: None,
                        end: None,
                        subscriptions: Vec::new(),
                        // A topic keeps nothing to lose.
                        may_lose_inside: false,
                    })
                }
            }
        })
    }

    fn latest<'a>(
        &'a mut self,
        address: &'a str,
        key: &'a str,
        deadline: Deadline,
    ) -> Wire<'a, Option<Message>> {
        Box::pin(async move {
            let (stream, subject) = self.stream_place(address)?;
            let wanted = keyed(subject, Some(key))?;
            let request = Json::object([("last_by_subj", Json::text(wanted))]);
            self.get(stream, subject, request, deadline).await
        })
    }

    fn browse<'a>(
        &'a mut self,
        address: &'a str,
        from: Option<u64>,
        max: u32,
        deadline: Deadline,
    ) -> Wire<'a, Page> {
        Box::pin(async move {
            let (stream, subject) = self.stream_place(address)?;
            let info = self.stream_info(stream, deadline).await?;
            let state = info
                .get("state")
                .ok_or_else(|| unexpected("a stream's description holds no state"))?;
            let last = number(state, "last_seq")?;
            let first = number(state, "first_seq")?.max(1).min(last + 1);
            let mut end = last + 1;
            if let Some(from) = from
                && (from < first || from > end)
            {
                return Err(error(
                    WireErrorKind::PositionOutOfRange,
                    Sent::Answered,
                    "the stream no longer holds that position, or has not reached it",
                ));
            }
            let mut seq = from.unwrap_or(first);
            let mut messages = Vec::new();
            let mut exhausted = false;
            while messages.len() < usize::try_from(max).unwrap_or(usize::MAX) {
                let request = Json::object([
                    ("seq", Json::number(seq)),
                    ("next_by_subj", Json::text(">")),
                ]);
                let Some(message) = self.get(stream, subject, request, deadline).await? else {
                    exhausted = true;
                    break;
                };
                let at = message.position.unwrap_or(seq);
                if at < seq {
                    return Err(unexpected(
                        "the stream answered with a message before the one asked for",
                    ));
                }
                seq = at + 1;
                end = end.max(seq);
                messages.push(message);
            }
            if exhausted {
                // The server says no message lies at or after `seq`: the
                // look reached the log's end, whatever gaps lie before it.
                end = seq;
            }
            let more = !exhausted && seq < end;
            Ok(Page {
                messages,
                more,
                start: Some(first),
                end: Some(end),
            })
        })
    }

    fn subscribe<'a>(
        &'a mut self,
        address: &'a str,
        name: Option<&'a str>,
        from: Option<u64>,
        deadline: Deadline,
    ) -> Wire<'a, Opened> {
        Box::pin(async move {
            let not_supported =
                |what: &str| error(WireErrorKind::NotSupported, Sent::NotSent, what);
            match place(address).map_err(unfit)? {
                Place::Topic { subject } => {
                    if name.is_some() || from.is_some() {
                        return Err(not_supported(
                            "a topic of the core protocol has no named subscription and no position",
                        ));
                    }
                    let sid = self.next_sid.to_string();
                    self.next_sid += 1;
                    self.start()?;
                    self.write(
                        format!("SUB {subject}.> {sid}\r\nSUB {subject} {sid}x\r\n").as_bytes(),
                        deadline,
                    )
                    .await?;
                    self.broken = false;
                    // The pong says the server holds the subscription, so a
                    // message published from now on is seen.
                    self.subs.insert(
                        format!("core.{sid}"),
                        Sub::Core {
                            sid: sid.clone(),
                            subject: subject.to_owned(),
                        },
                    );
                    self.subs.insert(
                        format!("core.{sid}x"),
                        Sub::Core {
                            sid: format!("{sid}x"),
                            subject: subject.to_owned(),
                        },
                    );
                    for message in self.round_trip(false, deadline).await? {
                        self.keep(message)?;
                    }
                    Ok(Opened {
                        handle: format!("core.{sid}"),
                        position: None,
                        ack_wait: None,
                    })
                }
                Place::Stream { stream, subject } => {
                    let info = self.stream_info(stream, deadline).await?;
                    let queue = info
                        .get("config")
                        .and_then(|c| c.get("retention"))
                        .and_then(Json::as_str)
                        == Some("workqueue");
                    let mut ack_wait = None;
                    let (consumer, ephemeral, answer) = match name {
                        Some(name) => {
                            if from.is_some() {
                                return Err(not_supported(
                                    "a declared consumer keeps its own position",
                                ));
                            }
                            if !is_name(name) {
                                return Err(unfit(
                                    "the subscription's name cannot be a consumer's",
                                ));
                            }
                            let answer = self
                                .api_ok(
                                    &format!("$JS.API.CONSUMER.INFO.{stream}.{name}"),
                                    &Json::Null,
                                    deadline,
                                )
                                .await?;
                            let config = answer.get("config").ok_or_else(|| {
                                unexpected("a consumer's description holds no configuration")
                            })?;
                            if config.get("deliver_subject").is_some_and(|d| !d.is_null()) {
                                return Err(not_supported(
                                    "the declared consumer pushes; the runtime pulls",
                                ));
                            }
                            // On a queue the runtime takes, and every message
                            // is acknowledged; elsewhere it only reads, and
                            // nothing is.
                            let wanted = if queue { "explicit" } else { "none" };
                            if config.get("ack_policy").and_then(Json::as_str) != Some(wanted) {
                                return Err(not_supported(
                                    "the declared consumer's acknowledgement policy is not the one this read needs",
                                ));
                            }
                            // A work queue's consumer hands a message out
                            // again after this wait; one that does not say
                            // it has no rule, since an acknowledgement could
                            // then reach the server after it.
                            let wait = match config.get("ack_wait") {
                                None if queue => {
                                    return Err(unexpected(
                                        "a work queue's consumer does not state its acknowledgement wait",
                                    ));
                                }
                                None => None,
                                Some(wait) => Some(wait.as_u64().ok_or_else(|| {
                                    unexpected(
                                        "a consumer's acknowledgement wait is not a whole number",
                                    )
                                })?),
                            };
                            ack_wait = wait.map(Duration::from_nanos);
                            (name.to_owned(), false, answer)
                        }
                        None if queue => {
                            return Err(not_supported(
                                "the runtime never makes a consumer on a work queue; the channel names the one a person made",
                            ));
                        }
                        None => {
                            let mut config = BTreeMap::from([
                                ("ack_policy".to_owned(), Json::text("none")),
                                ("replay_policy".to_owned(), Json::text("instant")),
                                ("mem_storage".to_owned(), Json::Bool(true)),
                                ("num_replicas".to_owned(), Json::number(1)),
                                (
                                    "inactive_threshold".to_owned(),
                                    Json::number(
                                        u64::try_from(INACTIVE.as_nanos()).unwrap_or(u64::MAX),
                                    ),
                                ),
                            ]);
                            match from {
                                Some(from) => {
                                    config.insert(
                                        "deliver_policy".to_owned(),
                                        Json::text("by_start_sequence"),
                                    );
                                    config.insert("opt_start_seq".to_owned(), Json::number(from));
                                }
                                None => {
                                    config.insert("deliver_policy".to_owned(), Json::text("new"));
                                }
                            }
                            let request = Json::object([
                                ("stream_name", Json::text(stream)),
                                ("config", Json::Object(config)),
                            ]);
                            let answer = self
                                .api_ok(
                                    &format!("$JS.API.CONSUMER.CREATE.{stream}"),
                                    &request,
                                    deadline,
                                )
                                .await?;
                            let made = answer
                                .get("name")
                                .and_then(Json::as_str)
                                .filter(|made| is_name(made))
                                .ok_or_else(|| unexpected("a consumer made has no name"))?
                                .to_owned();
                            (made, true, answer)
                        }
                    };
                    let delivered = answer
                        .get("delivered")
                        .ok_or_else(|| unexpected("a consumer's description holds no position"))?;
                    let position = number(delivered, "stream_seq")? + 1;
                    let handle = format!("pull.{stream}.{consumer}");
                    self.subs.insert(
                        handle.clone(),
                        Sub::Pull {
                            stream: stream.to_owned(),
                            consumer,
                            subject: subject.to_owned(),
                            explicit: queue,
                            ephemeral,
                            held: BTreeSet::new(),
                        },
                    );
                    Ok(Opened {
                        handle,
                        position: Some(position),
                        ack_wait,
                    })
                }
            }
        })
    }

    fn fetch<'a>(
        &'a mut self,
        handle: &'a str,
        max: u32,
        wait: Duration,
        deadline: Deadline,
    ) -> Wire<'a, Vec<Delivered>> {
        Box::pin(async move {
            let max = usize::try_from(max).unwrap_or(usize::MAX).max(1);
            let (stream, consumer, subject) = match self.subs.get(handle) {
                Some(Sub::Core { sid, .. }) => {
                    let sid = sid.clone();
                    return self.fetch_topic(handle, &sid, max, wait, deadline).await;
                }
                Some(Sub::Pull {
                    stream,
                    consumer,
                    subject,
                    ..
                }) => (stream.clone(), consumer.clone(), subject.clone()),
                None => {
                    return Err(error(
                        WireErrorKind::Unexpected,
                        Sent::NotSent,
                        "the link holds no such subscription",
                    ));
                }
            };
            let reply = format!("{}.{}", self.inbox, self.next_reply);
            self.next_reply += 1;
            let mut request = BTreeMap::from([(
                "batch".to_owned(),
                Json::number(u64::try_from(max).unwrap_or(1)),
            )]);
            if wait.is_zero() {
                request.insert("no_wait".to_owned(), Json::Bool(true));
            } else {
                request.insert(
                    "expires".to_owned(),
                    Json::number(u64::try_from(wait.as_nanos()).unwrap_or(u64::MAX)),
                );
            }
            let bytes = self.publish_bytes(
                &format!("$JS.API.CONSUMER.MSG.NEXT.{stream}.{consumer}"),
                Some(&reply),
                &BTreeMap::new(),
                Json::Object(request).to_line().as_bytes(),
            )?;
            self.start()?;
            self.write(&bytes, deadline).await?;
            let mut delivered = Vec::new();
            let mut ended = false;
            while !ended && delivered.len() < max {
                match self.next_event(deadline).await? {
                    Event::Message(message)
                        if message.sid == INBOX_SID && message.subject == reply =>
                    {
                        match message.status_code() {
                            Some("408" | "404") => ended = true,
                            Some("100") => {}
                            Some("503") => {
                                self.broken = false;
                                return Err(error(
                                    WireErrorKind::SubscriptionMissing,
                                    Sent::Answered,
                                    "nothing on the server serves the consumer",
                                ));
                            }
                            Some("409")
                                if message
                                    .status
                                    .as_deref()
                                    .is_some_and(|s| s.contains("Consumer Deleted")) =>
                            {
                                // The pull is over: the link can go on.
                                self.broken = false;
                                return Err(error(
                                    WireErrorKind::SubscriptionMissing,
                                    Sent::Answered,
                                    "the consumer was deleted",
                                ));
                            }
                            _ => {
                                return Err(unexpected(
                                    "a pull was answered with a status this client has no rule for",
                                ));
                            }
                        }
                    }
                    Event::Message(message)
                        if message.sid == INBOX_SID
                            && message
                                .reply
                                .as_deref()
                                .is_some_and(|r| r.starts_with("$JS.ACK.")) =>
                    {
                        delivered.push(message);
                    }
                    Event::Message(message) => self.keep(message)?,
                    Event::Pong => {
                        return Err(unexpected("the server answered a ping no one sent"));
                    }
                    Event::Error(said) => return Err(err_line(&said)),
                }
            }
            self.broken = false;
            if !ended {
                // As many as were asked for came. A ping's answer shows
                // whether the server handed over more than that.
                for message in self.round_trip(false, deadline).await? {
                    if message.sid == INBOX_SID
                        && message
                            .reply
                            .as_deref()
                            .is_some_and(|r| r.starts_with("$JS.ACK."))
                    {
                        delivered.push(message);
                    } else if message.sid == INBOX_SID && message.subject == reply {
                        // The pull's own end, after the batch: nothing more.
                    } else {
                        self.keep(message)?;
                    }
                }
            }
            let mut out = Vec::new();
            for message in delivered {
                let ack = message.reply.clone().unwrap_or_default();
                let meta = ack_meta(&ack).ok_or_else(|| {
                    unexpected("a delivery's acknowledgement subject is not in the protocol's form")
                })?;
                // A delivery is this pull's only when its acknowledgement
                // names the stream and the consumer pulled: another client
                // that sees the inbox cannot hand over one of its choosing.
                if meta.stream != stream || meta.consumer != consumer {
                    return Err(unexpected(
                        "a delivery's acknowledgement names another stream or consumer than the one pulled",
                    ));
                }
                let (seq, attempt) = (meta.seq, meta.attempt);
                if let Some(Sub::Pull {
                    held,
                    explicit: true,
                    ..
                }) = self.subs.get_mut(handle)
                {
                    held.insert(ack.clone());
                }
                out.push(Delivered {
                    delivery: ack,
                    message: Message {
                        id: seq.to_string(),
                        key: key_of(&message.subject, &subject),
                        headers: message.headers,
                        body: body_of(&message.data)?,
                        position: Some(seq),
                    },
                    attempt: Some(attempt),
                });
            }
            Ok(out)
        })
    }

    fn acknowledge<'a>(
        &'a mut self,
        handle: &'a str,
        delivery: &'a str,
        deadline: Deadline,
    ) -> Wire<'a, ()> {
        Box::pin(async move { self.settle(handle, delivery, b"+ACK", deadline).await })
    }

    fn release<'a>(
        &'a mut self,
        handle: &'a str,
        delivery: &'a str,
        deadline: Deadline,
    ) -> Wire<'a, ()> {
        Box::pin(async move { self.settle(handle, delivery, b"-NAK", deadline).await })
    }

    fn unsubscribe<'a>(&'a mut self, handle: &'a str, deadline: Deadline) -> Wire<'a, ()> {
        Box::pin(async move {
            match self.subs.get(handle) {
                None => Ok(()),
                Some(Sub::Core { sid, .. }) => {
                    let sid = sid.clone();
                    let twin = format!("{sid}x");
                    self.start()?;
                    self.write(
                        format!("UNSUB {sid}\r\nUNSUB {twin}\r\n").as_bytes(),
                        deadline,
                    )
                    .await?;
                    self.broken = false;
                    self.subs.remove(handle);
                    self.subs.remove(&format!("core.{twin}"));
                    self.queued.remove(&sid);
                    self.queued.remove(&twin);
                    // What was in flight to the subscription arrives before
                    // the pong, and is dropped with it.
                    self.round_trip(false, deadline).await.map(|_| ())
                }
                Some(Sub::Pull {
                    stream,
                    consumer,
                    ephemeral,
                    held,
                    ..
                }) => {
                    let (stream, consumer, ephemeral) =
                        (stream.clone(), consumer.clone(), *ephemeral);
                    for delivery in held.clone() {
                        self.settle(handle, &delivery, b"-NAK", deadline).await?;
                    }
                    self.subs.remove(handle);
                    if ephemeral {
                        let deleted = self
                            .api_ok(
                                &format!("$JS.API.CONSUMER.DELETE.{stream}.{consumer}"),
                                &Json::Null,
                                deadline,
                            )
                            .await;
                        match deleted {
                            Ok(_) => {}
                            Err(failed) if failed.kind == WireErrorKind::SubscriptionMissing => {}
                            Err(failed) => return Err(failed),
                        }
                    }
                    Ok(())
                }
            }
        })
    }

    fn publish<'a>(&'a mut self, message: &'a Outgoing, deadline: Deadline) -> Wire<'a, Receipt> {
        Box::pin(async move {
            let body = message.body.to_line();
            match place(&message.address).map_err(unfit)? {
                Place::Topic { subject } => {
                    let subject = keyed(subject, message.key.as_deref())?;
                    let bytes =
                        self.publish_bytes(&subject, None, &message.headers, body.as_bytes())?;
                    self.start()?;
                    self.write(&bytes, deadline).await?;
                    self.broken = false;
                    // The core protocol confirms nothing; a pong says the
                    // server read the message and refused nothing before it.
                    for incoming in self.round_trip(true, deadline).await? {
                        self.keep(incoming)?;
                    }
                    Ok(Receipt {
                        id: String::new(),
                        position: None,
                        duplicate: false,
                    })
                }
                Place::Stream { stream, subject } => {
                    let subject = keyed(subject, message.key.as_deref())?;
                    let answer = self
                        .request(&subject, &message.headers, body.as_bytes(), true, deadline)
                        .await?;
                    match answer.status_code() {
                        None => {}
                        // Nobody listens on the subject: no stream stored it.
                        Some("503") => {
                            return Err(error(
                                WireErrorKind::ChannelMissing,
                                Sent::Answered,
                                "no stream stores the channel's subject",
                            ));
                        }
                        Some(_) => {
                            return Err(unexpected(
                                "a publish was answered with a status this client has no rule for",
                            ));
                        }
                    }
                    let ack = std::str::from_utf8(&answer.data)
                        .ok()
                        .and_then(|text| Json::parse(text).ok())
                        .filter(|json| json.as_object().is_some())
                        .ok_or_else(|| {
                            unexpected("a publish's acknowledgement is not one JSON object")
                        })?;
                    if let Some(failed) = api_error(&ack) {
                        return Err(failed);
                    }
                    // A message stored by another stream than the channel's
                    // was stored, and not where the target reads it.
                    if ack.get("stream").and_then(Json::as_str) != Some(stream) {
                        return Err(unexpected(
                            "the acknowledgement names another stream than the channel's",
                        ));
                    }
                    let seq = number(&ack, "seq")?;
                    if seq == 0 {
                        return Err(unexpected("the acknowledgement names no stored message"));
                    }
                    Ok(Receipt {
                        id: seq.to_string(),
                        position: Some(seq),
                        duplicate: ack
                            .get("duplicate")
                            .and_then(Json::as_bool)
                            .unwrap_or(false),
                    })
                }
            }
        })
    }
}

impl NatsLink {
    async fn fetch_topic(
        &mut self,
        handle: &str,
        sid: &str,
        max: usize,
        wait: Duration,
        deadline: Deadline,
    ) -> Result<Vec<Delivered>, WireError> {
        let twin = format!("{sid}x");
        let ends = Deadline::after(wait.min(deadline.remaining()));
        let mut got: Vec<Incoming> = Vec::new();
        let take = |link: &mut Self, got: &mut Vec<Incoming>| {
            for one in [sid, twin.as_str()] {
                if let Some(queue) = link.queued.get_mut(one) {
                    while got.len() < max && !queue.is_empty() {
                        got.push(queue.remove(0));
                    }
                }
            }
        };
        take(self, &mut got);
        while got.is_empty() && !ends.remaining().is_zero() {
            self.start()?;
            // The read is cut off when the wait ends; the reader keeps what
            // it read in part, so nothing is lost.
            let read = tokio::time::timeout_at(ends.at(), self.read_raw()).await;
            self.broken = false;
            match read {
                Err(_) => break,
                Ok(Err(failed)) => {
                    self.broken = true;
                    return Err(failed);
                }
                Ok(Ok(Some(Event::Message(message)))) => self.keep(message)?,
                Ok(Ok(Some(Event::Error(said)))) => {
                    self.broken = true;
                    return Err(err_line(&said));
                }
                Ok(Ok(_)) => {
                    self.broken = true;
                    return Err(unexpected("the server answered a ping no one sent"));
                }
            }
            take(self, &mut got);
        }
        let subject = match self.subs.get(handle) {
            Some(Sub::Core { subject, .. }) => subject.clone(),
            _ => String::new(),
        };
        got.into_iter()
            .map(|message| {
                self.topic_count += 1;
                let id = message
                    .headers
                    .get(MSG_ID)
                    .cloned()
                    .unwrap_or_else(|| format!("seen-{}", self.topic_count));
                Ok(Delivered {
                    delivery: String::new(),
                    message: Message {
                        id,
                        key: key_of(&message.subject, &subject),
                        headers: message.headers,
                        body: body_of(&message.data)?,
                        position: None,
                    },
                    attempt: None,
                })
            })
            .collect::<Result<Vec<_>, WireError>>()
    }

    /// An acknowledgement or a giving back, sent with a reply subject: the
    /// server's empty answer says it carried it out.
    async fn settle(
        &mut self,
        handle: &str,
        delivery: &str,
        word: &[u8],
        deadline: Deadline,
    ) -> Result<(), WireError> {
        let held = matches!(self.subs.get(handle), Some(Sub::Pull { held, .. }) if held.contains(delivery));
        if !held || sequence_of(delivery).is_none() {
            return Err(error(
                WireErrorKind::Unexpected,
                Sent::NotSent,
                "the delivery is not one this link holds",
            ));
        }
        let answer = self
            .request(delivery, &BTreeMap::new(), word, true, deadline)
            .await?;
        if answer.status_code() == Some("503") {
            // Nobody listens on the acknowledgement's subject: the consumer
            // is gone, and nothing was acknowledged.
            return Err(error(
                WireErrorKind::SubscriptionMissing,
                Sent::Answered,
                "the consumer that delivered the message is gone",
            ));
        }
        if answer.status.is_some() || !answer.data.is_empty() {
            return Err(unexpected(
                "an acknowledgement was answered in a way this client has no rule for",
            ));
        }
        if let Some(Sub::Pull { held, .. }) = self.subs.get_mut(handle) {
            held.remove(delivery);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test stops when its helper cannot work"
    )]

    use a2m_core::Secret;

    use super::*;

    #[tokio::test]
    async fn a_credential_reaches_the_server_in_connect_and_plain_text_goes_to_loopback_only() {
        let stand = example_nats::TestNats::start().await.unwrap();
        let address = stand.address().to_string();
        let deadline = Deadline::after(Duration::from_secs(10));
        let token = |text: &str| Credential::Token(Secret::new(text));

        stand.require_token(Some("example-token-for-tests"));
        let opened = NatsProtocol::new(DEFAULT_CEILING)
            .connect(&address, Some(&token("example-token-for-tests")), deadline)
            .await;
        assert!(opened.is_ok());
        let wrong = NatsProtocol::new(DEFAULT_CEILING)
            .connect(&address, Some(&token("example-token-wrong")), deadline)
            .await
            .err()
            .unwrap();
        assert_eq!(
            (wrong.kind, wrong.sent),
            (WireErrorKind::NotAuthorized, Sent::Answered)
        );
        assert!(!wrong.detail.contains("example-token-wrong"));

        stand.require_token(None);
        stand.require_user(Some(("example-user", "secret-for-tests-pass")));
        let user = Credential::UserPassword {
            user: "example-user".to_owned(),
            password: Secret::new("secret-for-tests-pass"),
        };
        assert!(
            NatsProtocol::new(DEFAULT_CEILING)
                .connect(&address, Some(&user), deadline)
                .await
                .is_ok()
        );
        let seen: Vec<(&str, bool)> = stand
            .connects()
            .iter()
            .map(|c| (c.presented, c.accepted))
            .collect();
        assert_eq!(seen, [("token", true), ("token", false), ("user", true)]);

        // Not this machine's own address: refused before anything is sent.
        let far = NatsProtocol::new(DEFAULT_CEILING)
            .connect("192.0.2.10:4222", Some(&user), deadline)
            .await
            .err()
            .unwrap();
        assert_eq!(
            (far.kind, far.sent),
            (WireErrorKind::Transport, Sent::NotSent)
        );
    }

    /// A server that states a payload limit no real one has, and then a
    /// message far larger than the pack's ceiling.
    #[tokio::test]
    async fn a_message_larger_than_the_ceiling_stops_before_anything_is_set_aside() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = BufReader::new(socket);
            socket
                .get_mut()
                .write_all(b"INFO {\"headers\":true,\"max_payload\":18446744073709551615}\r\n")
                .await
                .unwrap();
            let mut line = String::new();
            // `CONNECT`, `SUB`, and `PING`.
            for _ in 0..3 {
                line.clear();
                socket.read_line(&mut line).await.unwrap();
            }
            socket.get_mut().write_all(b"PONG\r\n").await.unwrap();
            line.clear();
            socket.read_line(&mut line).await.unwrap();
            let reply = line.split_whitespace().nth(2).unwrap().to_owned();
            socket
                .get_mut()
                .write_all(format!("MSG {reply} 1 1099511627776\r\n").as_bytes())
                .await
                .unwrap();
            // Held open until the client lets go.
            let mut rest = Vec::new();
            let _ = tokio::io::AsyncReadExt::read_to_end(&mut socket, &mut rest).await;
        });
        let deadline = Deadline::after(Duration::from_secs(10));
        let mut link = NatsProtocol::new(DEFAULT_CEILING)
            .connect(&address, None, deadline)
            .await
            .ok()
            .unwrap();
        let stopped = link
            .describe("ORDERS/example.orders", deadline)
            .await
            .err()
            .unwrap();
        assert_eq!(stopped.kind, WireErrorKind::Unexpected);
        assert!(
            stopped.detail.contains("message_ceiling_bytes"),
            "{}",
            stopped.detail
        );
        drop(link);
        server.await.unwrap();
    }

    #[test]
    fn a_channel_is_never_one_of_the_servers_own_subjects() {
        for address in [
            "$JS.API.STREAM.DELETE",
            "_INBOX.abc",
            "S/$KV.orders",
            "S/$SYS.x",
            "$O.bucket",
        ] {
            assert!(place(address).is_err(), "{address}");
        }
    }

    #[test]
    fn an_address_names_a_stream_and_a_subject_or_a_topic() {
        assert_eq!(
            place("ORDERS/example.orders.status"),
            Ok(Place::Stream {
                stream: "ORDERS",
                subject: "example.orders.status"
            })
        );
        assert_eq!(
            place("example.orders.notices"),
            Ok(Place::Topic {
                subject: "example.orders.notices"
            })
        );
        assert!(place("ORDERS/example.*").is_err());
        assert!(place("OR.DERS/example").is_err());
        assert!(place("example..notices").is_err());
    }

    #[test]
    fn a_key_never_moves_a_message_to_another_subject() {
        assert_eq!(
            keyed("example.orders", Some("A-100")).unwrap_or_default(),
            "example.orders.A-100"
        );
        for key in ["A.100", "*", ">", "A 100", ""] {
            assert_eq!(
                keyed("example.orders", Some(key)).map_err(|e| e.kind),
                Err(WireErrorKind::Unfit)
            );
        }
    }

    #[test]
    fn base64_reads_what_the_server_writes() {
        assert_eq!(
            base64_decode("eyJhIjoxfQ==").unwrap_or_default(),
            br#"{"a":1}"#.to_vec()
        );
        assert_eq!(base64_decode("YWJj").unwrap_or_default(), b"abc".to_vec());
        assert_eq!(base64_decode("YQ").map(|_| ()), None);
    }

    #[test]
    fn an_acknowledgement_subject_names_its_sequence_in_both_forms() {
        assert_eq!(sequence_of("$JS.ACK.S.C.1.42.7.1800000000000.0"), Some(42));
        let meta = ack_meta("$JS.ACK.S.C.3.42.7.1800000000000.0").unwrap();
        assert_eq!((meta.stream, meta.consumer, meta.attempt), ("S", "C", 3));
        assert_eq!(
            sequence_of("$JS.ACK.dom.acc.S.C.1.42.7.1800000000000.0.r"),
            Some(42)
        );
        assert_eq!(sequence_of("$JS.ACK.S.C.1"), None);
    }

    #[test]
    fn a_header_that_would_end_its_line_is_never_sent() {
        let headers = BTreeMap::from([("x".to_owned(), "a\r\nNats-Msg-Id: forged".to_owned())]);
        assert_eq!(
            header_block(&headers).map_err(|e| e.kind),
            Err(WireErrorKind::Unfit)
        );
    }
}
