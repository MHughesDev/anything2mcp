//! Throwaway spike code. The whole protocol client of candidate (a).
//!
//! The debugging pipe carries JSON messages, each ended by one NUL byte. A
//! request has an `id`; its reply has the same `id` and a `result` or an
//! `error`. A message without an `id` is an event. A message for a tab carries
//! the tab's `sessionId` at the top level ("flat" sessions).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{broadcast, oneshot};

/// The largest message the client accepts. A larger one closes the link.
const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub enum CdpError {
    /// The browser answered with a protocol error.
    Protocol { code: i64, message: String },
    /// The pipe closed. `sent` says whether the request had been written.
    Disconnected { sent: bool },
    /// The deadline passed. The request had been written.
    Timeout,
    /// The browser sent something that is not a protocol message. The link is closed.
    Malformed,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub session: Option<String>,
    pub method: String,
    pub params: Value,
}

type Pending = HashMap<u64, oneshot::Sender<Result<Value, CdpError>>>;

/// `None` once the link is closed, so a late caller is told at once.
struct Shared {
    pending: Mutex<Option<Pending>>,
    events: broadcast::Sender<Event>,
}

#[derive(Clone)]
pub struct Cdp {
    shared: Arc<Shared>,
    writer: Arc<tokio::sync::Mutex<Box<dyn AsyncWrite + Send + Unpin>>>,
    next_id: Arc<AtomicU64>,
}

impl Cdp {
    pub fn connect<R, W>(reader: R, writer: W) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let (events, _) = broadcast::channel(1024);
        let shared = Arc::new(Shared {
            pending: Mutex::new(Some(HashMap::new())),
            events,
        });
        tokio::spawn(read_loop(BufReader::new(reader), shared.clone()));
        Self {
            shared,
            writer: Arc::new(tokio::sync::Mutex::new(Box::new(writer))),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Subscribe before sending the request whose event is awaited.
    pub fn events(&self) -> broadcast::Receiver<Event> {
        self.shared.events.subscribe()
    }

    /// One request, one reply, under a deadline.
    pub async fn call(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
        deadline: Duration,
    ) -> Result<Value, CdpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        match self.shared.pending.lock().expect("lock").as_mut() {
            Some(pending) => pending.insert(id, tx),
            None => return Err(CdpError::Disconnected { sent: false }),
        };
        let mut message = json!({"id": id, "method": method, "params": params});
        if let Some(session) = session {
            message["sessionId"] = json!(session);
        }
        let mut bytes = serde_json::to_vec(&message).expect("a request serializes");
        bytes.push(0);
        {
            let mut writer = self.writer.lock().await;
            if writer.write_all(&bytes).await.is_err() || writer.flush().await.is_err() {
                self.forget(id);
                // A failed write may still have delivered a whole message.
                return Err(CdpError::Disconnected { sent: true });
            }
        }
        match tokio::time::timeout(deadline, rx).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(_)) => Err(CdpError::Disconnected { sent: true }),
            Err(_) => {
                self.forget(id);
                Err(CdpError::Timeout)
            }
        }
    }

    fn forget(&self, id: u64) {
        if let Some(pending) = self.shared.pending.lock().expect("lock").as_mut() {
            pending.remove(&id);
        }
    }
}

/// Waits for the first event that `pick` accepts, under a deadline. A closed
/// link ends the wait. Events that arrived after `events` was made are not lost.
pub async fn wait_event<T>(
    events: &mut broadcast::Receiver<Event>,
    deadline: Duration,
    mut pick: impl FnMut(&Event) -> Option<T>,
) -> Result<T, CdpError> {
    let wait = async {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if let Some(found) = pick(&event) {
                        return Ok(found);
                    }
                }
                // The spike treats a reader that fell behind as a broken link.
                Err(broadcast::error::RecvError::Lagged(_)) => return Err(CdpError::Malformed),
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(CdpError::Disconnected { sent: true });
                }
            }
        }
    };
    tokio::time::timeout(deadline, wait)
        .await
        .unwrap_or(Err(CdpError::Timeout))
}

async fn read_loop<R: AsyncRead + Unpin>(mut reader: BufReader<R>, shared: Arc<Shared>) {
    let mut buffer = Vec::new();
    let end = loop {
        buffer.clear();
        match reader.read_until(0, &mut buffer).await {
            Ok(n) if n > 0 && buffer.last() == Some(&0) && n <= MAX_MESSAGE_BYTES => {}
            // End of the pipe, a read error, a partial message, or one too large.
            _ => break CdpError::Disconnected { sent: true },
        }
        buffer.pop();
        let Ok(mut message) = serde_json::from_slice::<Value>(&buffer) else {
            break CdpError::Malformed;
        };
        if let Some(id) = message.get("id").and_then(Value::as_u64) {
            let reply = match message.get_mut("error") {
                Some(error) => Err(CdpError::Protocol {
                    code: error["code"].as_i64().unwrap_or(0),
                    message: error["message"].as_str().unwrap_or("").to_owned(),
                }),
                None => Ok(message["result"].take()),
            };
            let waiter = shared
                .pending
                .lock()
                .expect("lock")
                .as_mut()
                .and_then(|p| p.remove(&id));
            if let Some(waiter) = waiter {
                let _ = waiter.send(reply);
            }
        } else if let Some(method) = message.get("method").and_then(Value::as_str) {
            let _ = shared.events.send(Event {
                session: message
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                method: method.to_owned(),
                params: message["params"].take(),
            });
        } else {
            break CdpError::Malformed;
        }
    };
    // A broken stream is not repaired: every pending request fails, and later ones fail at once.
    if let Some(pending) = shared.pending.lock().expect("lock").take() {
        for (_, waiter) in pending {
            let _ = waiter.send(Err(end.clone()));
        }
    }
}
