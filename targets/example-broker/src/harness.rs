//! The ground-truth harness: starts a broker for a test and reports, through the
//! control interface, what it actually received, delivered, and holds.
//!
//! A test never proves a publish using Anything2MCP's own read path alone. It asks here.
//! The control interface is this handle, in the test's own process. No fault applies to it.

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout_at};

use crate::Shared;
use crate::client::{Client, ClientError};
use crate::faults::Fault;
use crate::model::{
    Acknowledgement, AppliedFault, ChannelContents, ChannelSpec, Delivery, Held, Message, Received,
    RequestRecord, Truth,
};

/// Why the harness could not do what a test asked.
#[derive(Debug)]
pub enum HarnessError {
    /// The broker could not be started.
    Start(std::io::Error),
    /// A condition a test waited for did not come true within its limit.
    TimedOut,
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start(e) => write!(f, "the test broker could not be started: {e}"),
            Self::TimedOut => write!(f, "the broker did not reach the awaited state in time"),
        }
    }
}

impl std::error::Error for HarnessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Start(e) => Some(e),
            Self::TimedOut => None,
        }
    }
}

/// One running test broker, on its own port, stopped when dropped.
pub struct TestBroker {
    address: SocketAddr,
    shared: Arc<Shared>,
    server: JoinHandle<()>,
}

impl fmt::Debug for TestBroker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestBroker")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl TestBroker {
    /// Starts a new broker with the seed data on a free local port.
    ///
    /// # Errors
    ///
    /// Returns an error if no port could be bound.
    pub async fn start() -> Result<Self, HarnessError> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(HarnessError::Start)?;
        let address = listener.local_addr().map_err(HarnessError::Start)?;
        let shared = Shared::new();
        // The task, and every connection it holds, is stopped when the `TestBroker` is dropped.
        let server = tokio::spawn(crate::server::serve(listener, Arc::clone(&shared)));
        Ok(Self {
            address,
            shared,
            server,
        })
    }

    /// The address the broker listens on: the loopback interface and the port it was given.
    #[must_use]
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// A new ordinary client, for a test that acts as something outside the product.
    ///
    /// # Errors
    ///
    /// Returns an error if the broker cannot be reached or refuses.
    pub async fn client(&self, name: &str) -> Result<Client, ClientError> {
        Client::connect(self.address, name).await
    }

    /// Restores the seed data and clears every record, fault, and setting.
    /// Open connections stay open; their subscriptions are gone.
    pub fn reset(&self) {
        self.shared.change(|s| s.reset());
    }

    /// Everything the broker received, did, and holds, at this moment.
    #[must_use]
    pub fn truth(&self) -> Truth {
        self.shared.lock().truth()
    }

    /// Every publish received since the last reset, in order, whatever became of it.
    #[must_use]
    pub fn received(&self) -> Vec<Received> {
        self.truth().received
    }

    /// Every delivery since the last reset, in order: which message went to whom.
    #[must_use]
    pub fn deliveries(&self) -> Vec<Delivery> {
        self.truth().deliveries
    }

    /// Every acknowledgement accepted since the last reset, in order.
    #[must_use]
    pub fn acknowledgements(&self) -> Vec<Acknowledgement> {
        self.truth().acknowledgements
    }

    /// Every request received since the last reset, in order.
    #[must_use]
    pub fn requests(&self) -> Vec<RequestRecord> {
        self.truth().requests
    }

    /// Everything one channel holds, if the channel exists.
    #[must_use]
    pub fn inspect(&self, channel: &str) -> Option<ChannelContents> {
        self.truth().channels.remove(channel)
    }

    /// What a queue holds: in flight first, then ready, each in order.
    #[must_use]
    pub fn queue(&self, channel: &str) -> Vec<Held> {
        self.inspect(channel).map(|c| c.queue).unwrap_or_default()
    }

    /// The retained value a channel holds for a key, if any.
    #[must_use]
    pub fn retained(&self, channel: &str, key: &str) -> Option<Message> {
        self.inspect(channel)?.retained.remove(key)
    }

    /// What a log holds, in order.
    #[must_use]
    pub fn log(&self, channel: &str) -> Vec<Message> {
        self.inspect(channel).map(|c| c.log).unwrap_or_default()
    }

    /// Switches a fault on.
    pub fn set_fault(&self, fault: Fault) {
        self.shared.change(|s| s.set_fault(fault));
    }

    /// The faults that are switched on now.
    #[must_use]
    pub fn faults(&self) -> Vec<Fault> {
        self.truth().faults
    }

    /// Every time a fault changed what the broker did, since the last reset.
    #[must_use]
    pub fn applied_faults(&self) -> Vec<AppliedFault> {
        self.truth().applied
    }

    /// Switches every fault off. The silent consumer gives back what it holds.
    pub fn clear_faults(&self) {
        self.shared.change(|s| s.clear_faults());
    }

    /// Declares a channel. One of the same address is replaced, with everything it held:
    /// this is how a test changes a channel's shape or key under the product.
    pub fn declare_channel(&self, spec: ChannelSpec) {
        self.shared.change(|s| s.declare_channel(spec));
    }

    /// Removes a channel. Returns whether there was one.
    pub fn remove_channel(&self, channel: &str) -> bool {
        self.shared.change(|s| s.remove_channel(channel))
    }

    /// Declares a durable subscription on a channel that is not a queue. It collects
    /// messages from now on, and one connection at a time may hold it.
    /// Returns false if the channel is missing or is a queue.
    pub fn declare_subscription(&self, channel: &str, name: &str) -> bool {
        self.shared
            .change(|s| s.declare_subscription(channel, name))
    }

    /// Sets, or with `None` removes, the header whose repeated value the channel refuses.
    /// Returns whether the channel exists.
    pub fn set_dedupe_header(&self, channel: &str, header: Option<&str>) -> bool {
        self.shared.change(|s| s.set_dedupe_header(channel, header))
    }

    /// Drops what a retained channel or a log keeps, as when its retention runs out.
    /// A log's positions carry on from where they were. Returns how many messages went.
    pub fn expire_retention(&self, channel: &str) -> usize {
        self.shared.change(|s| s.expire_retention(channel))
    }

    /// Takes back every unacknowledged delivery of a queue, as a broker does when its
    /// time limit for an acknowledgement runs out. Returns how many came back.
    pub fn redeliver_unacknowledged(&self, channel: &str) -> usize {
        self.shared.change(|s| s.redeliver_unacknowledged(channel))
    }

    /// Sets the token a connection must present in its `hello`, or with `None` asks for
    /// none. A connection that presented another is refused from its next request on.
    pub fn set_required_token(&self, token: Option<&str>) {
        self.shared.change(|s| s.set_required_token(token));
    }

    /// Drops every open connection, as when the broker restarts. What it holds stays.
    pub fn drop_connections(&self) {
        self.shared.kick.send_modify(|k| *k = k.wrapping_add(1));
    }

    /// Waits until the ground truth satisfies a condition, and returns it.
    ///
    /// # Errors
    ///
    /// Returns [`HarnessError::TimedOut`] if the condition is not true within the limit.
    pub async fn wait_for(
        &self,
        limit: Duration,
        condition: impl Fn(&Truth) -> bool,
    ) -> Result<Truth, HarnessError> {
        let deadline = Instant::now() + limit;
        let mut version = self.shared.version.subscribe();
        loop {
            version.borrow_and_update();
            let truth = self.truth();
            if condition(&truth) {
                return Ok(truth);
            }
            if !matches!(timeout_at(deadline, version.changed()).await, Ok(Ok(()))) {
                return Err(HarnessError::TimedOut);
            }
        }
    }
}

impl Drop for TestBroker {
    fn drop(&mut self) {
        self.server.abort();
    }
}
