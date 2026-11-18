//! Test stand: a small local server for the imaginary `example` target that
//! speaks the part of the NATS client protocol and of the JetStream interface
//! the stream pack uses.
//!
//! It imitates what makes this kind of broker hard to drive, and nothing else:
//! a confirmation that can be lost after the message was stored, a duplicate
//! window that forgets an id after a time, a work queue whose message is gone
//! once acknowledged, an acknowledgement whose answer can be lost, and a look
//! that must take nothing. It keeps everything in memory, its clock moves only
//! when a test moves it, and its control interface, the [`TestNats`] handle,
//! reports what it actually received, stored, delivered, and was told.
//!
//! It was written from the protocol's public documentation and the server's
//! public source; `README.md` names what it imitates, what it leaves out, and
//! what only a real server can show. It is test equipment, not part of the
//! product, and sits outside the crate layering.

pub mod model;
mod server;
mod state;

use std::collections::BTreeMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout_at};

use crate::model::{
    Acknowledgement, ApiCall, Connect, ConsumerSpec, Delivery, Fault, Received, StoredMessage,
    StreamSpec, StreamTruth, Truth,
};
use crate::server::Conn;
use crate::state::State;

/// The seeded stream that keeps the last status of each order.
pub const SEED_STATUS: &str = "ORDERS_STATUS";
/// The seeded stream that keeps every order event.
pub const SEED_EVENTS: &str = "ORDERS_EVENTS";
/// The seeded work queue of order tasks.
pub const SEED_WORK: &str = "ORDERS_WORK";
/// The seeded pull consumer of the work queue, with explicit acknowledgement.
pub const SEED_CONSUMER: &str = "example_runtime";
/// The header whose repeated value a stream refuses within its window.
pub const MSG_ID: &str = "Nats-Msg-Id";
/// The largest payload a connection may send, as the server's default.
pub const DEFAULT_MAX_PAYLOAD: usize = 1024 * 1024;
/// The longest control line the stand reads, as the server's.
pub const MAX_CONTROL_LINE: usize = 4096;
/// The version the stand gives in its `INFO`.
pub const SERVER_VERSION: &str = "2.12.0-example";

/// One running stand's state, with its connections and its signals.
pub(crate) struct Shared {
    state: Mutex<State>,
    conns: Mutex<BTreeMap<u64, Conn>>,
    /// Goes up whenever the state may have changed.
    version: watch::Sender<u64>,
    /// Goes up when every connection is to be dropped.
    kick: watch::Sender<u64>,
}

impl Shared {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::seeded()),
            conns: Mutex::new(BTreeMap::new()),
            version: watch::Sender::new(0),
            kick: watch::Sender::new(0),
        })
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        // The state holds no invariant a panic elsewhere could break half-way.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn conns(&self) -> MutexGuard<'_, BTreeMap<u64, Conn>> {
        self.conns.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn change<R>(&self, change: impl FnOnce(&mut State) -> R) -> R {
        let result = change(&mut self.lock());
        self.version.send_modify(|v| *v = v.wrapping_add(1));
        result
    }
}

/// Why the stand could not do what a test asked.
#[derive(Debug)]
pub enum StandError {
    /// The stand could not be started.
    Start(std::io::Error),
    /// A condition a test waited for did not come true within its limit.
    TimedOut,
}

impl fmt::Display for StandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start(e) => write!(f, "the test server could not be started: {e}"),
            Self::TimedOut => write!(f, "the server did not reach the awaited state in time"),
        }
    }
}

impl std::error::Error for StandError {}

/// One running stand, on its own loopback port, stopped when dropped.
pub struct TestNats {
    address: SocketAddr,
    shared: Arc<Shared>,
    server: JoinHandle<()>,
}

impl fmt::Debug for TestNats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestNats")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl TestNats {
    /// Starts a new stand with the seed data on a free loopback port.
    ///
    /// # Errors
    ///
    /// Returns an error if no port could be bound.
    pub async fn start() -> Result<Self, StandError> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(StandError::Start)?;
        let address = listener.local_addr().map_err(StandError::Start)?;
        let shared = Shared::new();
        let server = tokio::spawn(server::serve(listener, Arc::clone(&shared)));
        Ok(Self {
            address,
            shared,
            server,
        })
    }

    /// The address the stand listens on.
    #[must_use]
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Restores the seed data and clears every record, fault, and setting.
    pub fn reset(&self) {
        self.shared.change(State::reset);
    }

    /// Everything the stand received, did, and holds, at this moment.
    #[must_use]
    pub fn truth(&self) -> Truth {
        self.shared.lock().truth()
    }

    /// One stream as it stands now.
    #[must_use]
    pub fn stream(&self, name: &str) -> Option<StreamTruth> {
        self.truth().streams.remove(name)
    }

    /// What a stream holds, in order.
    #[must_use]
    pub fn messages(&self, stream: &str) -> Vec<StoredMessage> {
        self.stream(stream).map(|s| s.messages).unwrap_or_default()
    }

    /// Every publish received since the last reset, whatever became of it.
    #[must_use]
    pub fn received(&self) -> Vec<Received> {
        self.truth().received
    }

    /// Every delivery to a consumer since the last reset.
    #[must_use]
    pub fn deliveries(&self) -> Vec<Delivery> {
        self.truth().deliveries
    }

    /// Every acknowledgement processed since the last reset.
    #[must_use]
    pub fn acknowledgements(&self) -> Vec<Acknowledgement> {
        self.truth().acknowledgements
    }

    /// Every JetStream request since the last reset.
    #[must_use]
    pub fn api_calls(&self) -> Vec<ApiCall> {
        self.truth().api_calls
    }

    /// Every `CONNECT` since the last reset.
    #[must_use]
    pub fn connects(&self) -> Vec<Connect> {
        self.truth().connects
    }

    /// Switches a fault on, until it is cleared.
    pub fn set_fault(&self, fault: Fault) {
        self.shared.change(|s| s.faults.push(fault));
    }

    /// Switches every fault off.
    pub fn clear_faults(&self) {
        self.shared.change(|s| s.faults.clear());
    }

    /// Moves the stand's clock on. Nothing else moves it. Every delivery
    /// whose consumer's acknowledgement wait has run out goes out again.
    pub fn advance(&self, by: Duration) {
        let ms = u64::try_from(by.as_millis()).unwrap_or(u64::MAX);
        self.shared.change(|s| s.advance(ms));
    }

    /// Changes how long a consumer waits for an acknowledgement, keeping
    /// what it holds. Returns whether there was one.
    pub fn set_ack_wait(&self, stream: &str, consumer: &str, wait: Duration) -> bool {
        self.shared.change(|s| {
            s.streams
                .get_mut(stream)
                .and_then(|st| st.consumers.get_mut(consumer))
                .map(|c| c.spec.ack_wait = wait)
                .is_some()
        })
    }

    /// Removes one message from inside a stream, as a person deleting it
    /// by hand does. Returns whether there was one.
    pub fn remove_message(&self, stream: &str, seq: u64) -> bool {
        self.shared.change(|s| s.remove_message(stream, seq))
    }

    /// Declares a stream; one of the same name is replaced with everything it held.
    pub fn declare_stream(&self, spec: StreamSpec) {
        self.shared.change(|s| s.declare_stream(spec));
    }

    /// Removes a stream. Returns whether there was one.
    pub fn remove_stream(&self, name: &str) -> bool {
        self.shared.change(|s| s.streams.remove(name).is_some())
    }

    /// Declares a durable pull consumer on a stream, as a person would.
    /// Returns false when the stream is missing.
    pub fn declare_consumer(&self, stream: &str, spec: ConsumerSpec) -> bool {
        self.shared.change(|s| s.declare_consumer(stream, spec))
    }

    /// Removes a consumer. Returns whether there was one.
    pub fn remove_consumer(&self, stream: &str, name: &str) -> bool {
        self.shared.change(|s| {
            s.streams
                .get_mut(stream)
                .is_some_and(|st| st.consumers.remove(name).is_some())
        })
    }

    /// Takes back every delivery of a consumer that was not acknowledged, as
    /// the server does when its acknowledgement wait runs out.
    pub fn redeliver_unacknowledged(&self, stream: &str, consumer: &str) -> usize {
        self.shared
            .change(|s| s.redeliver_unacknowledged(stream, consumer))
    }

    /// Asks every new connection for this token, or with `None` for none.
    pub fn require_token(&self, token: Option<&str>) {
        self.shared
            .change(|s| s.required_token = token.map(str::to_owned));
    }

    /// Asks every new connection for this user and password.
    pub fn require_user(&self, user: Option<(&str, &str)>) {
        self.shared.change(|s| {
            s.required_user = user.map(|(u, p)| (u.to_owned(), p.to_owned()));
        });
    }

    /// The largest payload a new connection may send.
    pub fn set_max_payload(&self, bytes: usize) {
        self.shared.change(|s| s.max_payload = bytes);
    }

    /// Whether the stand's `INFO` says it requires TLS. It speaks none.
    pub fn set_tls_required(&self, required: bool) {
        self.shared.change(|s| s.tls_required = required);
    }

    /// Publishes as another producer would: stored where a stream listens,
    /// and copied to every matching subscription.
    pub fn publish(&self, subject: &str, headers: &[(&str, &str)], body: &str) {
        let headers: BTreeMap<String, String> = headers
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        let hdr = (!headers.is_empty()).then(|| {
            let mut block = String::from("NATS/1.0\r\n");
            for (k, v) in &headers {
                block.push_str(&format!("{k}: {v}\r\n"));
            }
            block.push_str("\r\n");
            block.into_bytes()
        });
        self.shared
            .route(subject, None, hdr.as_deref(), body.as_bytes());
        self.shared
            .change(|s| s.publish(0, subject, headers, body.as_bytes().to_vec()));
    }

    /// Drops every open connection, as when the server restarts.
    pub fn drop_connections(&self) {
        self.shared.kick.send_modify(|k| *k = k.wrapping_add(1));
    }

    /// How many connections are open now.
    #[must_use]
    pub fn open_connections(&self) -> usize {
        self.shared.conns().len()
    }

    /// Waits until the ground truth satisfies a condition, and returns it.
    ///
    /// # Errors
    ///
    /// [`StandError::TimedOut`] if the condition is not true within the limit.
    pub async fn wait_for(
        &self,
        limit: Duration,
        condition: impl Fn(&Truth) -> bool,
    ) -> Result<Truth, StandError> {
        let deadline = Instant::now() + limit;
        let mut version = self.shared.version.subscribe();
        loop {
            version.borrow_and_update();
            let truth = self.truth();
            if condition(&truth) {
                return Ok(truth);
            }
            if !matches!(timeout_at(deadline, version.changed()).await, Ok(Ok(()))) {
                return Err(StandError::TimedOut);
            }
        }
    }
}

impl Drop for TestNats {
    fn drop(&mut self) {
        self.server.abort();
        self.drop_connections();
    }
}
