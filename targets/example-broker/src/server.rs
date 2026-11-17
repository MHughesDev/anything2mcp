//! The wire: one JSON object per line each way, one answer per request.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio::time::{Instant, timeout_at};

use crate::faults::FaultKind;
use crate::model::Op;
use crate::state::{AckEnd, PublishEnd, PublishRequest, Refusal};
use crate::{MAX_BATCH, MAX_LINE_BYTES, MAX_WAIT_MS, Shared};

/// What the connection does after one request.
enum Step {
    Reply(Value),
    Silent,
    Close,
}

/// Accepts connections until the task is stopped. Stopping it stops every connection.
pub(crate) async fn serve(listener: TcpListener, shared: Arc<Shared>) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    connections.spawn(connection(stream, Arc::clone(&shared)));
                }
                // One caller that gave up while connecting does not stop the broker.
                Err(_) => tokio::task::yield_now().await,
            },
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

async fn connection(stream: TcpStream, shared: Arc<Shared>) {
    let mut kick = shared.kick.subscribe();
    let number = shared.change(|s| s.connect());
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    let mut token: Option<String> = None;
    loop {
        // `next_line` loses nothing when the other branch wins.
        let line = tokio::select! {
            line = lines.next_line() => line,
            _ = kick.changed() => break,
        };
        let Ok(Some(line)) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        match request(&shared, number, &mut token, &mut kick, &line).await {
            Step::Reply(reply) => {
                if send(&mut write, &reply).await.is_err() {
                    break;
                }
            }
            Step::Silent => {}
            Step::Close => break,
        }
    }
    shared.change(|s| s.disconnect(number));
}

async fn send(write: &mut OwnedWriteHalf, reply: &Value) -> std::io::Result<()> {
    let mut line = reply.to_string();
    line.push('\n');
    write.write_all(line.as_bytes()).await?;
    write.flush().await
}

fn answer(id: &Value, result: Result<Value, Refusal>) -> Step {
    let mut reply = Map::new();
    reply.insert("re".to_owned(), id.clone());
    match result {
        Ok(fields) => {
            reply.insert("ok".to_owned(), Value::Bool(true));
            if let Value::Object(fields) = fields {
                reply.extend(fields);
            }
        }
        Err(refusal) => {
            reply.insert("ok".to_owned(), Value::Bool(false));
            reply.insert("error".to_owned(), json!(refusal.code));
            reply.insert("message".to_owned(), json!(refusal.message));
            reply.extend(refusal.extra);
        }
    }
    Step::Reply(Value::Object(reply))
}

fn bad_request(what: impl Into<String>) -> Refusal {
    Refusal::new("bad_request", what)
}

fn text<'a>(request: &'a Value, name: &str) -> Result<&'a str, Refusal> {
    request
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| bad_request(format!("The request needs the text field {name}.")))
}

fn optional_text<'a>(request: &'a Value, name: &str) -> Result<Option<&'a str>, Refusal> {
    match request.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(bad_request(format!("The field {name} is text."))),
    }
}

fn optional_number(request: &Value, name: &str) -> Result<Option<u64>, Refusal> {
    match request.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_u64().map(Some).ok_or_else(|| {
            bad_request(format!("The field {name} is a whole number, zero or more."))
        }),
    }
}

fn optional_flag(request: &Value, name: &str) -> Result<Option<bool>, Refusal> {
    match request.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(bad_request(format!("The field {name} is true or false."))),
    }
}

/// How many messages a `fetch` or `browse` asks for: 1 to [`MAX_BATCH`].
fn batch(request: &Value) -> Result<usize, Refusal> {
    optional_number(request, "max")?
        .filter(|max| (1..=MAX_BATCH).contains(max))
        .and_then(|max| usize::try_from(max).ok())
        .ok_or_else(|| bad_request(format!("The request needs max, from 1 to {MAX_BATCH}.")))
}

fn publish_request(request: &Value) -> Result<PublishRequest, Refusal> {
    let mut headers = BTreeMap::new();
    match request.get("headers") {
        None | Some(Value::Null) => {}
        Some(Value::Object(fields)) => {
            for (name, value) in fields {
                let value = value
                    .as_str()
                    .ok_or_else(|| bad_request("Every header value is text."))?;
                headers.insert(name.clone(), value.to_owned());
            }
        }
        Some(_) => return Err(bad_request("The field headers is an object.")),
    }
    Ok(PublishRequest {
        channel: text(request, "channel")?.to_owned(),
        key: optional_text(request, "key")?.map(str::to_owned),
        headers,
        body: request
            .get("body")
            .cloned()
            .ok_or_else(|| bad_request("The request needs a body."))?,
        retain: optional_flag(request, "retain")?,
    })
}

/// Handles one request line.
async fn request(
    shared: &Shared,
    connection: u64,
    token: &mut Option<String>,
    kick: &mut watch::Receiver<u64>,
    line: &str,
) -> Step {
    if line.len() > MAX_LINE_BYTES {
        return answer(
            &Value::Null,
            Err(bad_request("The request line is too long.")),
        );
    }
    let request = match serde_json::from_str::<Value>(line) {
        Ok(request @ Value::Object(_)) => request,
        _ => {
            let refusal = bad_request("A request is one JSON object on one line.");
            return answer(&Value::Null, Err(refusal));
        }
    };
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let Some(op) = request
        .get("op")
        .and_then(Value::as_str)
        .and_then(Op::parse)
    else {
        let refusal = Refusal::new("unknown_op", "The broker has no such request.");
        return answer(&id, Err(refusal));
    };
    if op == Op::Hello {
        *token = request
            .get("token")
            .and_then(Value::as_str)
            .map(str::to_owned);
    }
    let (channel, authorized) = shared.change(|s| {
        let channel = s.channel_of(op, &request);
        s.record_request(connection, op, channel.as_deref());
        (channel, s.authorized(token.as_deref()))
    });
    if !authorized {
        let refusal = Refusal::new("not_authorized", "The broker refuses the credentials.");
        return answer(&id, Err(refusal));
    }
    let delay = |k: &FaultKind| matches!(k, FaultKind::Delay { .. });
    let delayed =
        shared.change(|s| s.take_fault(Some(connection), Some(op), channel.as_deref(), delay));
    if let Some(FaultKind::Delay { ms }) = delayed {
        // The fault is the wait: a slow broker, on demand.
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
    let unexpected = |k: &FaultKind| matches!(k, FaultKind::UnexpectedReply);
    if shared
        .change(|s| s.take_fault(Some(connection), Some(op), channel.as_deref(), unexpected))
        .is_some()
    {
        return Step::Reply(json!({"re": id, "surprise": true}));
    }
    let result = match op {
        Op::Publish => {
            let end = match publish_request(&request) {
                Ok(publish) => shared.change(|s| s.publish(connection, publish)),
                Err(refusal) => PublishEnd::Refuse(refusal),
            };
            match end {
                PublishEnd::Reply(fields) => Ok(fields),
                PublishEnd::Refuse(refusal) => Err(refusal),
                PublishEnd::Drop => return Step::Close,
                PublishEnd::Silent => return Step::Silent,
            }
        }
        Op::Fetch => match fetch(shared, connection, kick, &request).await {
            Some(result) => result,
            None => return Step::Close,
        },
        Op::Ack => {
            let named = text(&request, "subscription")
                .and_then(|handle| Ok((handle, text(&request, "delivery")?)));
            match named {
                Ok((handle, delivery)) => {
                    match shared.change(|s| s.ack(connection, handle, delivery)) {
                        AckEnd::Answer(result) => result,
                        AckEnd::Drop => return Step::Close,
                    }
                }
                Err(refusal) => Err(refusal),
            }
        }
        _ => shared.change(|s| simple(s, connection, op, &request)),
    };
    answer(&id, result)
}

/// Every request that neither waits nor ends the connection.
fn simple(
    state: &mut crate::state::State,
    connection: u64,
    op: Op,
    request: &Value,
) -> Result<Value, Refusal> {
    match op {
        Op::Hello => {
            let client = optional_text(request, "client")?.unwrap_or("anonymous");
            Ok(state.hello(connection, client))
        }
        Op::Channels => Ok(state.channels()),
        Op::Describe => state.describe(text(request, "channel")?),
        Op::Latest => state.latest(connection, text(request, "channel")?, text(request, "key")?),
        Op::Browse => state.browse(
            connection,
            text(request, "channel")?,
            batch(request)?,
            optional_number(request, "from")?,
        ),
        Op::Subscribe => state.subscribe(
            connection,
            text(request, "channel")?,
            optional_text(request, "name")?,
            optional_number(request, "from")?,
            optional_flag(request, "with_retained")?.unwrap_or(true),
        ),
        Op::Release => state.release(
            connection,
            text(request, "subscription")?,
            text(request, "delivery")?,
        ),
        Op::Unsubscribe => state.unsubscribe(connection, text(request, "subscription")?),
        Op::Publish | Op::Fetch | Op::Ack => Err(bad_request("This request is handled elsewhere.")),
    }
}

/// A bounded read: answers as soon as one message can be handed over, or with none
/// when the wait runs out. `None` means the connection is to be dropped.
async fn fetch(
    shared: &Shared,
    connection: u64,
    kick: &mut watch::Receiver<u64>,
    request: &Value,
) -> Option<Result<Value, Refusal>> {
    let parsed = (|| {
        let handle = text(request, "subscription")?;
        let max = batch(request)?;
        let wait = optional_number(request, "wait_ms")?.unwrap_or(0);
        if wait > MAX_WAIT_MS {
            let refusal = Refusal::new("wait_too_long", "The wait is over the broker's limit.");
            return Err(refusal);
        }
        Ok((handle, max, wait))
    })();
    let (handle, max, wait) = match parsed {
        Ok(parsed) => parsed,
        Err(refusal) => return Some(Err(refusal)),
    };
    let deadline = Instant::now() + Duration::from_millis(wait);
    let mut version = shared.version.subscribe();
    loop {
        version.borrow_and_update();
        // Looked at under the lock; waiters are told only when something was handed over.
        let fetched = shared.lock().fetch(connection, handle, max);
        match fetched {
            Err(refusal) => return Some(Err(refusal)),
            Ok(fetched) if !fetched.messages.is_empty() => {
                shared.touch();
                return Some(Ok(
                    json!({"messages": fetched.messages, "next": fetched.next}),
                ));
            }
            Ok(fetched) => {
                let waited = tokio::select! {
                    waited = timeout_at(deadline, version.changed()) => waited,
                    _ = kick.changed() => return None,
                };
                if !matches!(waited, Ok(Ok(()))) {
                    return Some(Ok(json!({"messages": [], "next": fetched.next})));
                }
            }
        }
    }
}
