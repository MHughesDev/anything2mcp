//! The wire: the part of the NATS client protocol and of the JetStream
//! interface the stream pack uses, written from the protocol's public
//! documentation and the server's public source. Every rule about what is
//! stored is in `state.rs`.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value as J, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::net::tcp::OwnedReadHalf;
use tokio::sync::mpsc;
use tokio::time::{Instant, timeout_at};

use crate::model::{AckPolicy, ApiCall, Connect, ConsumerSpec, Fault, Retention};
use crate::state::{ApiError, Published, matches};
use crate::{MAX_CONTROL_LINE, Shared};

/// One open connection, as routing sees it.
pub(crate) struct Conn {
    pub tx: mpsc::UnboundedSender<Vec<u8>>,
    /// Subscriptions: sid to subject pattern.
    pub subs: BTreeMap<String, String>,
    pub no_responders: bool,
}

pub(crate) async fn serve(listener: TcpListener, shared: Arc<Shared>) {
    let mut next = 0_u64;
    while let Ok((stream, _)) = listener.accept().await {
        next += 1;
        let _ = stream.set_nodelay(true);
        tokio::spawn(connection(stream, next, Arc::clone(&shared)));
    }
}

fn err_line(text: &str) -> Vec<u8> {
    format!("-ERR '{text}'\r\n").into_bytes()
}

/// A headers block: the version line, a status where there is one, the
/// headers, and the empty line.
fn header_block(status: Option<&str>, headers: &BTreeMap<String, String>) -> Vec<u8> {
    let mut block = String::from("NATS/1.0");
    if let Some(status) = status {
        block.push(' ');
        block.push_str(status);
    }
    block.push_str("\r\n");
    for (name, value) in headers {
        block.push_str(&format!("{name}: {value}\r\n"));
    }
    block.push_str("\r\n");
    block.into_bytes()
}

/// A message line and its payload, with headers where there are any.
fn frame(
    subject: &str,
    sid: &str,
    reply: Option<&str>,
    hdr: Option<&[u8]>,
    data: &[u8],
) -> Vec<u8> {
    let reply = reply.map(|r| format!(" {r}")).unwrap_or_default();
    let mut out = match hdr {
        Some(hdr) => format!(
            "HMSG {subject} {sid}{reply} {} {}\r\n",
            hdr.len(),
            hdr.len() + data.len()
        )
        .into_bytes(),
        None => format!("MSG {subject} {sid}{reply} {}\r\n", data.len()).into_bytes(),
    };
    if let Some(hdr) = hdr {
        out.extend_from_slice(hdr);
    }
    out.extend_from_slice(data);
    out.extend_from_slice(b"\r\n");
    out
}

/// Parses a headers block into a status and the headers.
fn parse_headers(block: &[u8]) -> Option<BTreeMap<String, String>> {
    let text = std::str::from_utf8(block).ok()?;
    let mut lines = text.split("\r\n");
    if !lines.next()?.starts_with("NATS/1.0") {
        return None;
    }
    let mut headers = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':')?;
        headers
            .entry(name.trim().to_owned())
            .or_insert_with(|| value.trim().to_owned());
    }
    Some(headers)
}

pub(crate) fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

impl Shared {
    /// Delivers a message to every subscription whose pattern matches.
    /// Returns whether anyone was listening.
    pub(crate) fn route(
        &self,
        subject: &str,
        reply: Option<&str>,
        headers: Option<&[u8]>,
        data: &[u8],
    ) -> bool {
        let conns = self.conns();
        let mut anyone = false;
        for conn in conns.values() {
            for (sid, pattern) in &conn.subs {
                if matches(pattern, subject) {
                    anyone = true;
                    let _ = conn.tx.send(frame(subject, sid, reply, headers, data));
                }
            }
        }
        anyone
    }

    /// Sends a message to the connection's subscription that listens on
    /// `inbox`, under `subject`: how a pulled message reaches its puller.
    fn to_inbox(
        &self,
        connection: u64,
        inbox: &str,
        subject: &str,
        reply: Option<&str>,
        headers: Option<&[u8]>,
        data: &[u8],
    ) {
        let conns = self.conns();
        let Some(conn) = conns.get(&connection) else {
            return;
        };
        if let Some((sid, _)) = conn.subs.iter().find(|(_, p)| matches(p, inbox)) {
            let _ = conn.tx.send(frame(subject, sid, reply, headers, data));
        }
    }

    fn status(&self, connection: u64, inbox: &str, status: &str) {
        let block = header_block(Some(status), &BTreeMap::new());
        self.to_inbox(connection, inbox, inbox, None, Some(&block), b"");
    }

    fn no_responders(&self, connection: u64, reply: Option<&str>) {
        let wants = self
            .conns()
            .get(&connection)
            .is_some_and(|c| c.no_responders);
        if let (Some(reply), true) = (reply, wants) {
            self.status(connection, reply, "503");
        }
    }

    fn answer(&self, reply: Option<&str>, body: &J) {
        if let Some(reply) = reply {
            self.route(reply, None, None, body.to_string().as_bytes());
        }
    }
}

fn api_error(kind: &str, error: &ApiError) -> J {
    json!({
        "type": format!("io.nats.jetstream.api.v1.{kind}"),
        "error": {"code": error.code, "err_code": error.err_code, "description": error.description},
    })
}

async fn read_control(reader: &mut BufReader<OwnedReadHalf>) -> Option<Result<String, ()>> {
    let mut line = Vec::new();
    let limit = u64::try_from(MAX_CONTROL_LINE + 2).unwrap_or(u64::MAX);
    let read = (&mut *reader)
        .take(limit)
        .read_until(b'\n', &mut line)
        .await;
    match read {
        Ok(0) | Err(_) => None,
        Ok(_) if line.last() != Some(&b'\n') => Some(Err(())),
        Ok(_) => {
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            Some(String::from_utf8(line).map_err(|_| ()))
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one connection's loop: each operation of the protocol in one place"
)]
async fn connection(stream: tokio::net::TcpStream, id: u64, shared: Arc<Shared>) {
    let (read, mut write) = stream.into_split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let writer = tokio::spawn(async move {
        while let Some(bytes) = rx.recv().await {
            if write.write_all(&bytes).await.is_err() {
                break;
            }
        }
        let _ = write.shutdown().await;
    });
    let (info, max_payload, needs_auth) = {
        let state = shared.lock();
        let needs_auth = state.required_token.is_some() || state.required_user.is_some();
        let info = json!({
            "server_id": "example-nats",
            "server_name": "example-nats",
            "version": crate::SERVER_VERSION,
            "proto": 1,
            "go": "none",
            "host": "127.0.0.1",
            "port": 0,
            "headers": true,
            "auth_required": needs_auth,
            "tls_required": state.tls_required,
            "max_payload": state.max_payload,
            "jetstream": true,
            "client_id": id,
        });
        (info, state.max_payload, needs_auth)
    };
    let _ = tx.send(format!("INFO {info}\r\n").into_bytes());
    shared.conns().insert(
        id,
        Conn {
            tx: tx.clone(),
            subs: BTreeMap::new(),
            no_responders: false,
        },
    );
    let mut kick = shared.kick.subscribe();
    kick.borrow_and_update();
    let mut reader = BufReader::new(read);
    let mut connected = false;
    loop {
        let line = tokio::select! {
            _ = kick.changed() => break,
            line = read_control(&mut reader) => line,
        };
        let Some(line) = line else { break };
        let Ok(line) = line else {
            let _ = tx.send(err_line("Maximum Control Line Exceeded"));
            break;
        };
        let (op, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        let op = op.to_ascii_uppercase();
        if !connected && op != "CONNECT" && needs_auth {
            let _ = tx.send(err_line("Authorization Violation"));
            break;
        }
        match op.as_str() {
            "CONNECT" => {
                let options: J = serde_json::from_str(rest).unwrap_or(J::Null);
                let text = |k: &str| options.get(k).and_then(J::as_str).map(str::to_owned);
                let flag = |k: &str| options.get(k).and_then(J::as_bool).unwrap_or(false);
                let (presented, accepted) = {
                    let state = shared.lock();
                    let presented = if text("auth_token").is_some() {
                        "token"
                    } else if text("user").is_some() {
                        "user"
                    } else {
                        "none"
                    };
                    let accepted = match (&state.required_token, &state.required_user) {
                        (Some(token), _) => text("auth_token").as_ref() == Some(token),
                        (None, Some((user, pass))) => {
                            text("user").as_ref() == Some(user)
                                && text("pass").as_ref() == Some(pass)
                        }
                        (None, None) => true,
                    };
                    (presented, accepted)
                };
                shared.change(|s| {
                    s.connects.push(Connect {
                        connection: id,
                        presented,
                        accepted,
                        headers: flag("headers"),
                        no_responders: flag("no_responders"),
                        verbose: flag("verbose"),
                    });
                });
                if !accepted {
                    let _ = tx.send(err_line("Authorization Violation"));
                    break;
                }
                if let Some(conn) = shared.conns().get_mut(&id) {
                    conn.no_responders = flag("no_responders");
                }
                connected = true;
                if flag("verbose") {
                    let _ = tx.send(b"+OK\r\n".to_vec());
                }
            }
            "PING" => {
                if shared.lock().fault(&Fault::UnknownLine) {
                    let _ = tx.send(b"+WHAT\r\n".to_vec());
                } else {
                    let _ = tx.send(b"PONG\r\n".to_vec());
                }
            }
            "PONG" => {}
            "SUB" => {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if let (Some(subject), Some(sid)) = (parts.first(), parts.last())
                    && parts.len() >= 2
                    && let Some(conn) = shared.conns().get_mut(&id)
                {
                    conn.subs.insert((*sid).to_owned(), (*subject).to_owned());
                }
            }
            "UNSUB" => {
                if let Some(sid) = rest.split_whitespace().next()
                    && let Some(conn) = shared.conns().get_mut(&id)
                {
                    conn.subs.remove(sid);
                }
            }
            "PUB" | "HPUB" => {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                let counts = if op == "PUB" { 1 } else { 2 };
                if parts.len() < 1 + counts || parts.len() > 2 + counts {
                    let _ = tx.send(err_line("Unknown Protocol Operation"));
                    break;
                }
                let subject = parts[0].to_owned();
                let reply = (parts.len() == 2 + counts).then(|| parts[1].to_owned());
                let numbers: Option<Vec<usize>> = parts[parts.len() - counts..]
                    .iter()
                    .map(|n| n.parse().ok())
                    .collect();
                let Some(numbers) = numbers else {
                    let _ = tx.send(err_line("Unknown Protocol Operation"));
                    break;
                };
                let (header_len, total) = if counts == 1 {
                    (0, numbers[0])
                } else {
                    (numbers[0], numbers[1])
                };
                if total > max_payload {
                    let _ = tx.send(err_line("Maximum Payload Violation"));
                    break;
                }
                let mut payload = vec![0_u8; total + 2];
                if reader.read_exact(&mut payload).await.is_err() {
                    break;
                }
                payload.truncate(total);
                if header_len > total {
                    break;
                }
                let data = payload.split_off(header_len);
                let headers = if header_len > 0 {
                    match parse_headers(&payload) {
                        Some(h) => Some(h),
                        None => break,
                    }
                } else {
                    None
                };
                if reply.is_some() && shared.lock().fault(&Fault::UnknownLine) {
                    let _ = tx.send(b"+WHAT\r\n".to_vec());
                    continue;
                }
                let close = publish(&shared, id, &subject, reply.as_deref(), headers, data);
                if close {
                    break;
                }
            }
            _ => {
                let _ = tx.send(err_line("Unknown Protocol Operation"));
                break;
            }
        }
    }
    shared.conns().remove(&id);
    drop(tx);
    let _ = writer.await;
}

/// What the server sends when a connection's credential expires: its timer
/// fires whatever the connection has in flight.
fn expire(shared: &Shared, id: u64) {
    if let Some(conn) = shared.conns().get(&id) {
        let _ = conn.tx.send(err_line("User Authentication Expired"));
    }
}

/// One message from a client. Returns true when the connection is to close.
fn publish(
    shared: &Arc<Shared>,
    id: u64,
    subject: &str,
    reply: Option<&str>,
    headers: Option<BTreeMap<String, String>>,
    data: Vec<u8>,
) -> bool {
    if let Some(api) = subject.strip_prefix("$JS.API.") {
        shared.change(|s| {
            s.api_calls.push(ApiCall {
                connection: id,
                subject: subject.to_owned(),
            });
        });
        api_request(shared, id, api, reply, &data);
        return false;
    }
    if subject.starts_with("$JS.ACK.") {
        let done = shared.change(|s| s.acknowledge(id, subject, &data));
        match (done, reply) {
            (None, _) => shared.no_responders(id, reply),
            (Some(()), Some(reply)) => {
                let mut state = shared.lock();
                if state.fault(&Fault::ExpireAuthAfterAck) {
                    drop(state);
                    expire(shared, id);
                    return true;
                } else if state.fault(&Fault::DropAckReply) {
                } else if state.fault(&Fault::UnknownAckReply) {
                    drop(state);
                    shared.route(reply, None, None, b"+WHAT");
                } else {
                    drop(state);
                    shared.route(reply, None, None, b"");
                }
            }
            (Some(()), None) => {}
        }
        return false;
    }
    let hdr = headers.as_ref().map(|h| header_block(None, h));
    let listened = shared.route(subject, reply, hdr.as_deref(), &data);
    let published = shared.change(|s| s.publish(id, subject, headers.unwrap_or_default(), data));
    match published {
        Published::NoStream => {
            if !listened {
                shared.no_responders(id, reply);
            }
            false
        }
        Published::Refused { stream, error } => {
            let mut body = api_error("pub_ack_response", &error);
            body["stream"] = json!(stream);
            shared.answer(reply, &body);
            false
        }
        Published::Ack {
            stream,
            seq,
            duplicate,
        } => {
            let mut state = shared.lock();
            if !duplicate && state.fault(&Fault::DropPublishAck) {
                return false;
            }
            if !duplicate && state.fault(&Fault::CloseAfterStore) {
                return true;
            }
            if !duplicate && state.fault(&Fault::ExpireAuthAfterStore) {
                drop(state);
                expire(shared, id);
                return true;
            }
            let unknown = !duplicate && state.fault(&Fault::UnknownCodeAfterStore);
            drop(state);
            let Some(reply) = reply else { return false };
            // The server writes this one by hand, with a space before `true`.
            let text = if unknown {
                format!(
                    r#"{{"error":{{"code":503,"err_code":19999,"description":"a code the protocol does not have"}},"stream":"{stream}","seq":0}}"#
                )
            } else if duplicate {
                format!(r#"{{"stream":"{stream}","seq":{seq},"duplicate": true}}"#)
            } else {
                format!(r#"{{"stream":"{stream}","seq":{seq}}}"#)
            };
            shared.route(reply, None, None, text.as_bytes());
            false
        }
    }
}

fn stream_info(shared: &Shared, name: &str) -> Result<J, ApiError> {
    let state = shared.lock();
    let s = state
        .streams
        .get(name)
        .ok_or_else(ApiError::stream_not_found)?;
    let window = u64::try_from(s.spec.duplicate_window.as_nanos()).unwrap_or(u64::MAX);
    Ok(json!({
        "type": "io.nats.jetstream.api.v1.stream_info_response",
        "config": {
            "name": s.spec.name,
            "subjects": s.spec.subjects,
            "retention": match s.spec.retention { Retention::Limits => "limits", Retention::WorkQueue => "workqueue" },
            "max_consumers": -1,
            "max_msgs": -1,
            "max_bytes": -1,
            "max_age": 0,
            "max_msgs_per_subject": s.spec.max_msgs_per_subject,
            "allow_msg_ttl": s.spec.allow_msg_ttl,
            "max_msg_size": s.spec.max_msg_size.map_or(-1, |n| i64::try_from(n).unwrap_or(-1)),
            "discard": "old",
            "storage": "memory",
            "num_replicas": 1,
            "duplicate_window": window,
            "allow_direct": false,
        },
        "created": "2027-01-15T08:00:00Z",
        "state": {
            "messages": s.messages.len(),
            "bytes": s.messages.values().map(|m| m.data.len()).sum::<usize>(),
            "first_seq": s.first_seq(),
            "last_seq": s.last_seq,
            "consumer_count": s.consumers.len(),
        },
    }))
}

fn consumer_info(shared: &Shared, stream: &str, name: &str, kind: &str) -> Result<J, ApiError> {
    let state = shared.lock();
    let s = state
        .streams
        .get(stream)
        .ok_or_else(ApiError::stream_not_found)?;
    let c = s
        .consumers
        .get(name)
        .ok_or_else(ApiError::consumer_not_found)?;
    let mut config = json!({
        "name": name,
        "ack_policy": match c.spec.ack { AckPolicy::None => "none", AckPolicy::Explicit => "explicit" },
        "deliver_policy": "all",
        "ack_wait": u64::try_from(c.spec.ack_wait.as_nanos()).unwrap_or(u64::MAX),
        "max_deliver": -1,
        "replay_policy": "instant",
        "max_waiting": 512,
    });
    if !c.ephemeral {
        config["durable_name"] = json!(name);
    }
    if let Some(filter) = &c.spec.filter {
        config["filter_subject"] = json!(filter);
    }
    Ok(json!({
        "type": format!("io.nats.jetstream.api.v1.{kind}"),
        "stream_name": stream,
        "name": name,
        "created": "2027-01-15T08:00:00Z",
        "config": config,
        "delivered": {"consumer_seq": c.dseq, "stream_seq": c.next.saturating_sub(1)},
        "ack_floor": {"consumer_seq": 0, "stream_seq": 0},
        "num_ack_pending": c.pending.len(),
        "num_redelivered": 0,
        "num_waiting": 0,
        "num_pending": s.messages.range(c.next..).count(),
    }))
}

#[allow(
    clippy::too_many_lines,
    reason = "the JetStream requests the stand answers, each in one arm"
)]
fn api_request(shared: &Arc<Shared>, id: u64, api: &str, reply: Option<&str>, data: &[u8]) {
    let body: J = if data.is_empty() {
        J::Null
    } else {
        serde_json::from_slice(data).unwrap_or(J::Null)
    };
    let tokens: Vec<&str> = api.split('.').collect();
    let answer = |result: Result<J, ApiError>, kind: &str| match result {
        Ok(value) => shared.answer(reply, &value),
        Err(error) => shared.answer(reply, &api_error(kind, &error)),
    };
    match tokens.as_slice() {
        ["STREAM", "INFO", name] => answer(stream_info(shared, name), "stream_info_response"),
        ["STREAM", "LIST"] => {
            let names: Vec<String> = shared.lock().streams.keys().cloned().collect();
            let streams: Vec<J> = names
                .iter()
                .filter_map(|n| stream_info(shared, n).ok())
                .collect();
            answer(
                Ok(json!({
                    "type": "io.nats.jetstream.api.v1.stream_list_response",
                    "total": streams.len(), "offset": 0, "limit": 256, "streams": streams,
                })),
                "stream_list_response",
            );
        }
        ["STREAM", "NAMES"] => {
            let wanted = body.get("subject").and_then(J::as_str).map(str::to_owned);
            let state = shared.lock();
            let names: Vec<&String> = state
                .streams
                .values()
                .filter(|s| {
                    wanted.as_deref().is_none_or(|w| {
                        s.spec
                            .subjects
                            .iter()
                            .any(|p| matches(p, w) || matches(w, p))
                    })
                })
                .map(|s| &s.spec.name)
                .collect();
            let value = json!({
                "type": "io.nats.jetstream.api.v1.stream_names_response",
                "total": names.len(), "offset": 0, "limit": 1024,
                "streams": if names.is_empty() { J::Null } else { json!(names) },
            });
            drop(state);
            answer(Ok(value), "stream_names_response");
        }
        ["STREAM", "MSG", "GET", name] => {
            let state = shared.lock();
            let result = (|| {
                let s = state
                    .streams
                    .get(*name)
                    .ok_or_else(ApiError::stream_not_found)?;
                let seq = body.get("seq").and_then(J::as_u64);
                let last = body.get("last_by_subj").and_then(J::as_str);
                let next = body.get("next_by_subj").and_then(J::as_str);
                let found = match (seq, last, next) {
                    (Some(seq), None, None) => s.messages.get(&seq),
                    (None, Some(last), None) => {
                        s.messages.values().rev().find(|m| m.subject == last)
                    }
                    (seq, None, Some(next)) => s
                        .messages
                        .range(seq.unwrap_or(0)..)
                        .map(|(_, m)| m)
                        .find(|m| matches(next, &m.subject)),
                    _ => return Err(ApiError::bad_request()),
                };
                let m = found.ok_or_else(ApiError::no_message)?;
                let mut message = json!({
                    "subject": m.subject, "seq": m.seq,
                    "data": base64(&m.data), "time": "2027-01-15T08:00:00Z",
                });
                if !m.headers.is_empty() {
                    message["hdrs"] = json!(base64(&header_block(None, &m.headers)));
                }
                Ok(
                    json!({"type": "io.nats.jetstream.api.v1.stream_msg_get_response", "message": message}),
                )
            })();
            drop(state);
            answer(result, "stream_msg_get_response");
        }
        ["CONSUMER", "NAMES", stream] => {
            let state = shared.lock();
            let result = state
                .streams
                .get(*stream)
                .map(|s| {
                    let names: Vec<&String> = s.consumers.keys().collect();
                    json!({
                        "type": "io.nats.jetstream.api.v1.consumer_names_response",
                        "total": names.len(), "offset": 0, "limit": 1024, "consumers": names,
                    })
                })
                .ok_or_else(ApiError::stream_not_found);
            drop(state);
            answer(result, "consumer_names_response");
        }
        ["CONSUMER", "INFO", stream, name] => answer(
            consumer_info(shared, stream, name, "consumer_info_response"),
            "consumer_info_response",
        ),
        ["CONSUMER", "CREATE", stream, ..] => {
            let config = body.get("config").cloned().unwrap_or(J::Null);
            let ack = match config.get("ack_policy").and_then(J::as_str) {
                Some("none") => AckPolicy::None,
                _ => AckPolicy::Explicit,
            };
            let name = config
                .get("durable_name")
                .or_else(|| config.get("name"))
                .and_then(J::as_str)
                .unwrap_or_default();
            let policy = config.get("deliver_policy").and_then(J::as_str);
            let start = (policy == Some("by_start_sequence"))
                .then(|| config.get("opt_start_seq").and_then(J::as_u64))
                .flatten();
            let spec = ConsumerSpec {
                filter: config
                    .get("filter_subject")
                    .and_then(J::as_str)
                    .map(str::to_owned),
                deliver_new: policy == Some("new"),
                ..ConsumerSpec::new(name, ack)
            };
            let created =
                shared.change(|s| s.create_consumer(stream, spec, start, policy == Some("new")));
            let result = created
                .and_then(|name| consumer_info(shared, stream, &name, "consumer_create_response"));
            answer(result, "consumer_create_response");
        }
        ["CONSUMER", "DELETE", stream, name] => {
            let removed = shared.change(|s| {
                s.streams
                    .get_mut(*stream)
                    .ok_or_else(ApiError::stream_not_found)?
                    .consumers
                    .remove(*name)
                    .ok_or_else(ApiError::consumer_not_found)
            });
            answer(
                removed.map(|_| {
                    json!({"type": "io.nats.jetstream.api.v1.consumer_delete_response", "success": true})
                }),
                "consumer_delete_response",
            );
        }
        ["CONSUMER", "MSG", "NEXT", stream, consumer] => {
            let Some(reply) = reply else { return };
            let mut batch = body
                .get("batch")
                .and_then(J::as_u64)
                .and_then(|b| usize::try_from(b).ok())
                .unwrap_or(1)
                .max(1);
            if shared.lock().fault(&Fault::DeliverExtra) {
                batch += 1;
            }
            let expires = body.get("expires").and_then(J::as_u64).unwrap_or(0);
            let no_wait = body.get("no_wait").and_then(J::as_bool).unwrap_or(false);
            pull(
                Arc::clone(shared),
                id,
                (*stream).to_owned(),
                (*consumer).to_owned(),
                reply.to_owned(),
                batch,
                if no_wait {
                    None
                } else {
                    Some(Duration::from_nanos(expires))
                },
            );
        }
        _ => shared.no_responders(id, reply),
    }
}

/// A pull: hands over what is there now, and waits for the rest until it
/// expires, as the server's waiting request does.
fn pull(
    shared: Arc<Shared>,
    id: u64,
    stream: String,
    consumer: String,
    inbox: String,
    batch: usize,
    expires: Option<Duration>,
) {
    tokio::spawn(async move {
        let deadline = Instant::now() + expires.unwrap_or_default();
        let mut version = shared.version.subscribe();
        let mut left = batch;
        loop {
            version.borrow_and_update();
            let handed = shared.change(|s| s.pull(id, &stream, &consumer, left));
            let Ok(handed) = handed else {
                shared.status(id, &inbox, "409 Consumer Deleted");
                return;
            };
            for one in &handed {
                let hdr = (!one.message.headers.is_empty())
                    .then(|| header_block(None, &one.message.headers));
                shared.to_inbox(
                    id,
                    &inbox,
                    &one.message.subject,
                    Some(&one.ack_subject),
                    hdr.as_deref(),
                    &one.message.data,
                );
            }
            left = left.saturating_sub(handed.len());
            if left == 0 {
                return;
            }
            let Some(_) = expires else {
                if left == batch {
                    shared.status(id, &inbox, "404 No Messages");
                }
                return;
            };
            if !matches!(timeout_at(deadline, version.changed()).await, Ok(Ok(()))) {
                shared.status(id, &inbox, "408 Request Timeout");
                return;
            }
        }
    });
}
