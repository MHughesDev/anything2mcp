//! Test stand: a small local broker for the imaginary `example` target.
//!
//! It imitates what makes queues, topics, and logs hard to drive: a message that is
//! gone once taken, delivery at least once, a confirmation that can be lost after the
//! message was stored, and a look that must take nothing. It holds one channel of each
//! shape, speaks a line protocol of its own over TCP, and has a control interface that
//! reports what it actually received, delivered, and holds. `README.md` has the
//! protocol, the cases, and the faults.
//!
//! It is test equipment, not part of the product, so it sits outside the crate
//! layering. A product crate may use it in tests only.

pub mod client;
pub mod faults;
pub mod harness;
pub mod model;
mod server;
mod state;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::net::TcpListener;
use tokio::sync::watch;

pub use crate::state::{SEED_DEDUPE_HEADER, SEED_SUBSCRIPTION, SILENT_CONSUMER};
use crate::state::State;

/// The name and version of the wire protocol, as the broker gives it in answer to `hello`.
pub const PROTOCOL: &str = "example-broker/1";
/// The longest a `fetch` may ask to wait, in milliseconds.
pub const MAX_WAIT_MS: u64 = 60_000;
/// The most messages one `fetch` or `browse` may ask for.
pub const MAX_BATCH: u64 = 1000;
/// The longest request line the broker reads, in bytes.
pub const MAX_LINE_BYTES: usize = 1_048_576;

/// One running broker's state, with the signals that tell waiters it changed.
pub(crate) struct Shared {
    state: Mutex<State>,
    /// Goes up whenever the state may have changed.
    version: watch::Sender<u64>,
    /// Goes up when every connection is to be dropped.
    kick: watch::Sender<u64>,
}

impl Shared {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::seeded()),
            version: watch::Sender::new(0),
            kick: watch::Sender::new(0),
        })
    }

    /// The state, for a look or for a change the caller reports with [`Shared::touch`].
    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        // The state holds no invariant a panic elsewhere could break half-way.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Tells every waiter that the state may have changed.
    pub(crate) fn touch(&self) {
        self.version.send_modify(|v| *v = v.wrapping_add(1));
    }

    /// Changes the state and tells every waiter.
    pub(crate) fn change<R>(&self, change: impl FnOnce(&mut State) -> R) -> R {
        let result = change(&mut self.lock());
        self.touch();
        result
    }
}

/// Serves a new broker with the seed data on the given listener until the task is stopped.
///
/// A test wants [`harness::TestBroker::start`] instead, which also gives the control interface.
pub async fn serve(listener: TcpListener) {
    server::serve(listener, Shared::new()).await;
}
