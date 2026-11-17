//! The protocol client: one reader task, one writer task, requests matched
//! to replies by number, events handed to the subscriber of their session,
//! and a deadline on every call.
//!
//! It works over any two byte streams, so it is tested against a scripted
//! peer with no browser. A message is JSON ended by one NUL byte. A message
//! that is too large, is not a protocol message, or answers a request that
//! was never made closes the link, and so does the end of the stream. When
//! the link closes every pending request fails, and each failure says
//! whether its request had been written.
//!
//! Writing is the writer task's alone, so dropping a call never leaves half
//! a message on the stream. A request that is abandoned before the writer
//! reached it is never written.

use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use a2m_provider::Deadline;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;

use crate::error::Dispatch;
use crate::protocol::{self, Command, Event, EventKind, Incoming, ReplyBody};

const NOT_WRITTEN: u8 = 0;
const WRITING: u8 = 1;
const WRITTEN: u8 = 2;

/// Why the link closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Why {
    /// The stream ended: the browser is gone.
    Ended,
    /// The browser sent something that is not a protocol message.
    Malformed,
    /// A message was larger than the ceiling.
    Oversized,
    /// A reply named a request that was never made.
    Unanswered,
    /// Writing to the browser failed.
    WriteFailed,
    /// The engine closed the link itself.
    ShutDown,
}

/// How a call ended without its reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClientError {
    /// The browser received the request and refused it.
    Protocol { code: i64, message: String },
    /// The link is closed.
    Closed { dispatch: Dispatch, why: Why },
    /// The deadline passed.
    Timeout { dispatch: Dispatch },
    /// The reply is not the shape its command declares.
    Shape { method: &'static str },
}

struct Pending {
    reply: oneshot::Sender<Result<ReplyBody, ClientError>>,
    written: Arc<AtomicU8>,
}

struct State {
    next_id: u64,
    pending: BTreeMap<u64, Pending>,
    /// The subscriber of each session; the empty name is the browser's own.
    routes: BTreeMap<String, mpsc::UnboundedSender<Event>>,
    closed: Option<Why>,
}

struct Shared {
    state: Mutex<State>,
}

impl Shared {
    /// The state, also after a panic elsewhere poisoned the lock: the state
    /// is plain data and every change to it is whole.
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Closes the link once: every pending request fails, and every
    /// subscriber sees its stream end.
    fn close(&self, why: Why) {
        let (pending, routes) = {
            let mut state = self.state();
            if state.closed.is_some() {
                return;
            }
            state.closed = Some(why);
            (
                std::mem::take(&mut state.pending),
                std::mem::take(&mut state.routes),
            )
        };
        for (_, waiter) in pending {
            // The receiver fills in whether its own request was written.
            let _ = waiter.reply.send(Err(ClientError::Closed {
                dispatch: Dispatch::Unknown,
                why,
            }));
        }
        drop(routes);
    }
}

struct Outgoing {
    id: u64,
    bytes: Vec<u8>,
}

struct Handle {
    shared: Arc<Shared>,
    out: mpsc::UnboundedSender<Outgoing>,
    reader: AbortHandle,
    writer: AbortHandle,
}

impl Handle {
    fn close(&self) {
        self.shared.close(Why::ShutDown);
        // The tasks own the two streams; ending them closes both, which is
        // how the browser learns that its owner is gone.
        self.reader.abort();
        self.writer.abort();
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.close();
    }
}

/// The client. Cheap to clone; every clone is the same link.
#[derive(Clone)]
pub(crate) struct Client {
    handle: Arc<Handle>,
}

fn dispatch_of(written: &AtomicU8) -> Dispatch {
    match written.load(Ordering::SeqCst) {
        NOT_WRITTEN => Dispatch::NotSent,
        WRITTEN => Dispatch::Sent,
        _ => Dispatch::Unknown,
    }
}

impl Client {
    /// Starts the reader and the writer over two streams. `max_message`
    /// is the ceiling on one message from the browser, in bytes.
    pub(crate) fn connect<R, W>(from_browser: R, to_browser: W, max_message: usize) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                next_id: 1,
                pending: BTreeMap::new(),
                routes: BTreeMap::new(),
                closed: None,
            }),
        });
        let (out, queue) = mpsc::unbounded_channel();
        let reader = tokio::spawn(read_loop(from_browser, Arc::clone(&shared), max_message));
        let writer = tokio::spawn(write_loop(to_browser, queue, Arc::clone(&shared)));
        Self {
            handle: Arc::new(Handle {
                shared,
                out,
                reader: reader.abort_handle(),
                writer: writer.abort_handle(),
            }),
        }
    }

    /// Why the link is closed, when it is.
    pub(crate) fn closed(&self) -> Option<Why> {
        self.handle.shared.state().closed
    }

    /// Closes the link. Both streams are dropped.
    pub(crate) fn close(&self) {
        self.handle.close();
    }

    /// The events of one session, or of the browser itself for `None`, in
    /// the order they arrived. A session attached under this one is heard
    /// here too. A second subscriber replaces the first. The stream ends
    /// when the link closes.
    pub(crate) fn subscribe(&self, session: Option<&str>) -> mpsc::UnboundedReceiver<Event> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut state = self.handle.shared.state();
        if state.closed.is_none() {
            state
                .routes
                .insert(session.unwrap_or_default().to_owned(), sender);
        }
        receiver
    }

    /// Ends the subscription of a session.
    pub(crate) fn unsubscribe(&self, session: &str) {
        self.handle.shared.state().routes.remove(session);
    }

    /// Queues one request for the writer.
    pub(crate) fn begin<C: Command>(
        &self,
        session: Option<&str>,
        command: &C,
    ) -> Result<InFlight<C>, ClientError> {
        let shared = &self.handle.shared;
        let written = Arc::new(AtomicU8::new(NOT_WRITTEN));
        let (reply, receiver) = oneshot::channel();
        let id = {
            let mut state = shared.state();
            if let Some(why) = state.closed {
                return Err(ClientError::Closed {
                    dispatch: Dispatch::NotSent,
                    why,
                });
            }
            let id = state.next_id;
            state.next_id += 1;
            state.pending.insert(
                id,
                Pending {
                    reply,
                    written: Arc::clone(&written),
                },
            );
            id
        };
        let flight = InFlight {
            shared: Arc::clone(shared),
            id,
            written,
            receiver,
            finished: false,
            command: PhantomData,
        };
        let queued = protocol::encode(id, session, command)
            .is_some_and(|bytes| self.handle.out.send(Outgoing { id, bytes }).is_ok());
        if queued {
            Ok(flight)
        } else {
            // Dropping the flight forgets the request.
            Err(ClientError::Closed {
                dispatch: Dispatch::NotSent,
                why: shared.state().closed.unwrap_or(Why::WriteFailed),
            })
        }
    }

    /// One request, one reply, under a deadline.
    pub(crate) async fn call<C: Command>(
        &self,
        session: Option<&str>,
        command: &C,
        deadline: Deadline,
    ) -> Result<C::Reply, ClientError> {
        self.begin(session, command)?.finish(deadline).await
    }
}

/// A request on its way. Dropping it before its reply abandons it: a late
/// reply is dropped, and a request the writer has not reached is never
/// written.
pub(crate) struct InFlight<C: Command> {
    shared: Arc<Shared>,
    id: u64,
    written: Arc<AtomicU8>,
    receiver: oneshot::Receiver<Result<ReplyBody, ClientError>>,
    finished: bool,
    command: PhantomData<fn() -> C>,
}

impl<C: Command> InFlight<C> {
    /// Whether the request has been written to the browser.
    pub(crate) fn dispatch(&self) -> Dispatch {
        dispatch_of(&self.written)
    }

    /// Forgets the request, and says whether it had been written. The
    /// writer marks a request under the same lock, so the answer is final.
    fn abandon(&mut self) -> Dispatch {
        self.finished = true;
        self.shared.state().pending.remove(&self.id);
        self.dispatch()
    }

    /// Waits for the reply until the deadline.
    pub(crate) async fn finish(&mut self, deadline: Deadline) -> Result<C::Reply, ClientError> {
        let waited = tokio::time::timeout_at(deadline.at(), &mut self.receiver).await;
        match waited {
            Err(_) => Err(ClientError::Timeout {
                dispatch: self.abandon(),
            }),
            Ok(Err(_)) => Err(ClientError::Closed {
                dispatch: self.abandon(),
                why: Why::ShutDown,
            }),
            Ok(Ok(Err(ClientError::Closed { why, .. }))) => Err(ClientError::Closed {
                dispatch: self.abandon(),
                why,
            }),
            Ok(Ok(Err(other))) => {
                self.finished = true;
                Err(other)
            }
            Ok(Ok(Ok(body))) => {
                self.finished = true;
                body.read::<C>()
                    .ok_or(ClientError::Shape { method: C::METHOD })
            }
        }
    }
}

impl<C: Command> Drop for InFlight<C> {
    fn drop(&mut self) {
        if !self.finished {
            self.shared.state().pending.remove(&self.id);
        }
    }
}

async fn write_loop<W: AsyncWrite + Unpin>(
    mut to_browser: W,
    mut queue: mpsc::UnboundedReceiver<Outgoing>,
    shared: Arc<Shared>,
) {
    while let Some(Outgoing { id, bytes }) = queue.recv().await {
        let written = {
            let state = shared.state();
            if state.closed.is_some() {
                return;
            }
            // Abandoned before it was written: it is never written.
            let Some(waiter) = state.pending.get(&id) else {
                continue;
            };
            waiter.written.store(WRITING, Ordering::SeqCst);
            Arc::clone(&waiter.written)
        };
        if to_browser.write_all(&bytes).await.is_err() || to_browser.flush().await.is_err() {
            shared.close(Why::WriteFailed);
            return;
        }
        written.store(WRITTEN, Ordering::SeqCst);
    }
}

/// Hands one message to whoever waits for it. `Err` closes the link.
fn deliver(shared: &Shared, frame: &[u8]) -> Result<(), Why> {
    match protocol::decode(frame).ok_or(Why::Malformed)? {
        Incoming::Ignored => {}
        Incoming::Reply { id, outcome } => {
            let waiter = {
                let mut state = shared.state();
                match state.pending.remove(&id) {
                    Some(waiter) => Some(waiter),
                    // A request that was abandoned is answered late; one
                    // that was never made is not answered at all.
                    None if id < state.next_id => None,
                    None => return Err(Why::Unanswered),
                }
            };
            if let Some(waiter) = waiter {
                let _ = waiter
                    .reply
                    .send(outcome.map_err(|error| ClientError::Protocol {
                        code: error.code,
                        message: error.message,
                    }));
            }
        }
        Incoming::Event(event) => {
            let mut state = shared.state();
            let from = event.session.clone().unwrap_or_default();
            match &event.kind {
                // A worker or a frame attached under a session is heard by
                // that session's subscriber, from its first event on.
                EventKind::Attached(attached) => {
                    if let Some(route) = state.routes.get(&from).cloned() {
                        state.routes.insert(attached.session_id.clone(), route);
                    }
                }
                EventKind::Detached(detached) => {
                    state.routes.remove(&detached.session_id);
                }
                _ => {}
            }
            let gone = state
                .routes
                .get(&from)
                .is_some_and(|route| route.send(event).is_err());
            if gone {
                state.routes.remove(&from);
            }
        }
    }
    Ok(())
}

async fn read_loop<R: AsyncRead + Unpin>(
    mut from_browser: R,
    shared: Arc<Shared>,
    max_message: usize,
) {
    let mut buffer: Vec<u8> = Vec::new();
    let mut chunk = vec![0_u8; 64 * 1024];
    let why = 'link: loop {
        let read = match from_browser.read(&mut chunk).await {
            Ok(0) | Err(_) => break Why::Ended,
            Ok(read) => read,
        };
        buffer.extend_from_slice(&chunk[..read]);
        let mut start = 0;
        while let Some(length) = buffer[start..].iter().position(|byte| *byte == 0) {
            if length > max_message {
                break 'link Why::Oversized;
            }
            if let Err(why) = deliver(&shared, &buffer[start..start + length]) {
                break 'link why;
            }
            start += length + 1;
        }
        buffer.drain(..start);
        if buffer.len() > max_message {
            break Why::Oversized;
        }
    };
    // A broken stream is not repaired.
    shared.close(why);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{DuplexStream, duplex};

    use super::*;
    use crate::protocol::{GetVersion, RunIfWaiting};

    /// The peer's two ends: what the client wrote, and what the client
    /// will read.
    struct Peer {
        requests: DuplexStream,
        replies: DuplexStream,
    }

    impl Peer {
        /// The next request the client wrote, without its NUL.
        async fn request(&mut self) -> serde_json::Value {
            let mut bytes = Vec::new();
            loop {
                let byte = self.requests.read_u8().await.unwrap();
                if byte == 0 {
                    break;
                }
                bytes.push(byte);
            }
            serde_json::from_slice(&bytes).unwrap()
        }

        async fn send(&mut self, message: &str) {
            self.replies.write_all(message.as_bytes()).await.unwrap();
            self.replies.write_all(&[0]).await.unwrap();
        }
    }

    fn link(pipe: usize, max_message: usize) -> (Client, Peer) {
        let (to_browser, requests) = duplex(pipe);
        let (replies, from_browser) = duplex(pipe);
        (
            Client::connect(from_browser, to_browser, max_message),
            Peer { requests, replies },
        )
    }

    fn soon() -> Deadline {
        Deadline::after(Duration::from_secs(30))
    }

    const VERSION: &str = r#""result":{"protocolVersion":"1.3","product":"Example/1"}"#;

    #[tokio::test]
    async fn replies_are_matched_by_number_whatever_their_order() {
        let (client, mut peer) = link(4096, 4096);
        let first = client.begin(Some("tab"), &GetVersion {}).unwrap();
        let second = client.begin(None, &RunIfWaiting {}).unwrap();
        let one = peer.request().await;
        let two = peer.request().await;
        assert_eq!(
            (one["id"].as_u64(), one["method"].as_str(), one["sessionId"].as_str()),
            (Some(1), Some("Browser.getVersion"), Some("tab"))
        );
        assert_eq!((two["id"].as_u64(), two.get("sessionId")), (Some(2), None));
        // The second is answered first.
        peer.send(r#"{"id":2,"result":{}}"#).await;
        peer.send(&format!(r#"{{"id":1,{VERSION}}}"#)).await;
        let (mut first, mut second) = (first, second);
        second.finish(soon()).await.unwrap();
        let version = first.finish(soon()).await.unwrap();
        assert_eq!(version.product, "Example/1");
        assert_eq!(first.dispatch(), Dispatch::Sent);
    }

    #[tokio::test]
    async fn the_browsers_own_error_is_the_calls_error_and_the_link_stays_open() {
        let (client, mut peer) = link(4096, 4096);
        let call = tokio::spawn({
            let client = client.clone();
            async move { client.call(None, &GetVersion {}, soon()).await }
        });
        peer.request().await;
        peer.send(r#"{"id":1,"error":{"code":-32000,"message":"No such thing"}}"#)
            .await;
        assert_eq!(
            call.await.unwrap().unwrap_err(),
            ClientError::Protocol {
                code: -32000,
                message: "No such thing".to_owned()
            }
        );
        assert_eq!(client.closed(), None);
    }

    #[tokio::test]
    async fn an_event_goes_to_the_subscriber_of_its_session_and_of_its_parent() {
        let (client, mut peer) = link(4096, 4096);
        let mut browser = client.subscribe(None);
        let mut tab = client.subscribe(Some("tab"));
        peer.send(r#"{"method":"Inspector.targetCrashed","params":{}}"#)
            .await;
        peer.send(r#"{"method":"Page.javascriptDialogClosed","params":{},"sessionId":"tab"}"#)
            .await;
        // An event nobody listens for is dropped, and is not malformed.
        peer.send(r#"{"method":"Example.unheardOf","params":{"x":1},"sessionId":"tab"}"#)
            .await;
        // A worker attaches under the tab; its events reach the tab's subscriber.
        peer.send(r#"{"method":"Target.attachedToTarget","sessionId":"tab","params":{"sessionId":"worker","targetInfo":{"type":"worker"},"waitingForDebugger":true}}"#)
            .await;
        peer.send(r#"{"method":"Runtime.executionContextsCleared","params":{},"sessionId":"worker"}"#)
            .await;
        assert!(matches!(
            browser.recv().await.unwrap().kind,
            EventKind::TargetCrashed
        ));
        assert!(matches!(tab.recv().await.unwrap().kind, EventKind::DialogClosed));
        assert!(matches!(tab.recv().await.unwrap().kind, EventKind::Attached(_)));
        let from_worker = tab.recv().await.unwrap();
        assert_eq!(from_worker.session.as_deref(), Some("worker"));
        assert!(matches!(from_worker.kind, EventKind::ContextsCleared));
        // The end of the link is the end of every subscription.
        drop(peer);
        assert!(tab.recv().await.is_none());
        assert!(browser.recv().await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn a_deadline_ends_a_call_and_its_late_reply_is_dropped() {
        let (client, mut peer) = link(4096, 4096);
        let failed = client
            .call(None, &GetVersion {}, Deadline::after(Duration::from_secs(5)))
            .await
            .unwrap_err();
        // The peer never read it, yet the writer had handed it over whole.
        assert_eq!(
            failed,
            ClientError::Timeout {
                dispatch: Dispatch::Sent
            }
        );
        peer.request().await;
        peer.send(&format!(r#"{{"id":1,{VERSION}}}"#)).await;
        // The link is still good for the next call.
        let call = tokio::spawn({
            let client = client.clone();
            async move { client.call(None, &GetVersion {}, soon()).await }
        });
        assert_eq!(peer.request().await["id"].as_u64(), Some(2));
        peer.send(&format!(r#"{{"id":2,{VERSION}}}"#)).await;
        call.await.unwrap().unwrap();
        assert_eq!(client.closed(), None);
    }

    async fn closes_with(message: &[u8], max_message: usize) -> (Why, ClientError, ClientError) {
        let (client, mut peer) = link(4096, max_message);
        let mut pending = client.begin(None, &GetVersion {}).unwrap();
        peer.request().await;
        peer.replies.write_all(message).await.unwrap();
        let failed = pending.finish(soon()).await.unwrap_err();
        let later = client.call(None, &GetVersion {}, soon()).await.unwrap_err();
        (client.closed().unwrap(), failed, later)
    }

    #[tokio::test]
    async fn a_message_that_is_not_the_protocol_closes_the_link_and_fails_what_is_pending() {
        for (message, why) in [
            (&b"not json\0"[..], Why::Malformed),
            (&b"[1,2]\0"[..], Why::Malformed),
            (&br#"{"neither":"reply nor event"}"#[..], Why::Malformed),
            // A reply to a request that was never made.
            (&br#"{"id":77,"result":{}}"#[..], Why::Unanswered),
        ] {
            let mut message = message.to_vec();
            if message.last() != Some(&0) {
                message.push(0);
            }
            let (closed, failed, later) = closes_with(&message, 4096).await;
            assert_eq!(closed, why);
            // The pending request had been written; the later one never was.
            assert_eq!(
                failed,
                ClientError::Closed {
                    dispatch: Dispatch::Sent,
                    why
                }
            );
            assert_eq!(
                later,
                ClientError::Closed {
                    dispatch: Dispatch::NotSent,
                    why
                }
            );
        }
    }

    #[tokio::test]
    async fn a_message_over_the_ceiling_closes_the_link_with_or_without_its_end() {
        let whole = format!(r#"{{"method":"Example.big","params":{{"x":"{}"}}}}"#, "a".repeat(64));
        let mut ended = whole.clone().into_bytes();
        ended.push(0);
        for message in [ended, whole.into_bytes()] {
            let (closed, failed, _) = closes_with(&message, 32).await;
            assert_eq!(closed, Why::Oversized);
            assert_eq!(
                failed,
                ClientError::Closed {
                    dispatch: Dispatch::Sent,
                    why: Why::Oversized
                }
            );
        }
    }

    #[tokio::test]
    async fn each_pending_request_says_whether_it_was_written_when_the_browser_goes() {
        // A pipe so small that the first request cannot be written whole,
        // and a peer that reads nothing.
        let (client, peer) = link(8, 4096);
        let mut first = client.begin(None, &GetVersion {}).unwrap();
        let mut second = client.begin(None, &GetVersion {}).unwrap();
        // The browser goes: both of its ends close.
        drop(peer);
        let first = first.finish(soon()).await.unwrap_err();
        let second = second.finish(soon()).await.unwrap_err();
        // The first was being written when the link broke; the second was
        // still queued behind it and never reached the stream.
        assert!(
            matches!(
                first,
                ClientError::Closed {
                    dispatch: Dispatch::Unknown | Dispatch::NotSent,
                    ..
                }
            ),
            "{first:?}"
        );
        assert!(
            matches!(
                second,
                ClientError::Closed {
                    dispatch: Dispatch::NotSent,
                    ..
                }
            ),
            "{second:?}"
        );
    }

    #[tokio::test]
    async fn a_request_dropped_before_the_writer_reached_it_is_never_written() {
        let (client, mut peer) = link(8, 4096);
        // The first fills the small pipe and holds the writer.
        let _first = client.begin(None, &GetVersion {}).unwrap();
        let second = client.begin(None, &RunIfWaiting {}).unwrap();
        assert_eq!(second.dispatch(), Dispatch::NotSent);
        drop(second);
        let third = client.begin(None, &GetVersion {}).unwrap();
        assert_eq!(peer.request().await["id"].as_u64(), Some(1));
        // The second was skipped: the next on the stream is the third.
        assert_eq!(peer.request().await["id"].as_u64(), Some(3));
        drop(third);
    }

    #[tokio::test]
    async fn closing_the_link_ends_both_streams() {
        let (client, mut peer) = link(4096, 4096);
        client.close();
        assert_eq!(client.closed(), Some(Why::ShutDown));
        // The peer reads the end of the stream: this is how a browser
        // learns that its owner is gone.
        let mut rest = Vec::new();
        peer.requests.read_to_end(&mut rest).await.unwrap();
        assert!(rest.is_empty());
    }
}
