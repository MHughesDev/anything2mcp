//! The control interface: what a test uses to reset the stand, switch on
//! faults, and ask what the stand received and stored. It is the same for
//! both forms, and no fault ever applies to it.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::faults::{Drift, Fault, Settings};
use crate::harness::HarnessError;
use crate::state::{Backing, CallRecord, ConnectionRecord, FiredFault, State, Truth};
use crate::store::{StoredOrder, WriteRecord};

/// How long `wait_for_announcements` waits before it gives up. Far longer
/// than the stand needs; reaching it means the notification never left.
const ANNOUNCE_TIMEOUT: Duration = Duration::from_secs(20);
const ANNOUNCE_POLL: Duration = Duration::from_millis(10);

/// The control interface of one stand.
#[derive(Clone, Debug)]
pub struct Control {
    backing: Arc<Backing>,
}

impl Control {
    pub(crate) fn new(backing: Arc<Backing>) -> Self {
        Self { backing }
    }

    fn with<R>(&self, change: impl FnOnce(&mut State) -> R) -> Result<R, HarnessError> {
        self.backing.with(change).map_err(HarnessError::State)
    }

    fn set(&self, change: impl FnOnce(&mut Settings)) -> Result<(), HarnessError> {
        self.with(|state| change(&mut state.settings))
    }

    /// Restores the seed data and clears every fault, drift, counter, setting,
    /// and session.
    ///
    /// # Errors
    ///
    /// Every method of the control interface returns an error only if the
    /// stand's state could not be read or written.
    pub fn reset(&self) -> Result<(), HarnessError> {
        self.with(|state| *state = State::default())
    }

    /// The whole ground truth at once.
    pub fn truth(&self) -> Result<Truth, HarnessError> {
        self.with(|state| state.truth.clone())
    }

    /// Every start of the stdio program since the last reset, oldest
    /// first, as the program itself saw it.
    pub fn starts(&self) -> Result<Vec<crate::state::StartRecord>, HarnessError> {
        self.with(|state| state.truth.starts.clone())
    }

    /// Every tool call received since the last reset, oldest first.
    pub fn calls(&self) -> Result<Vec<CallRecord>, HarnessError> {
        self.with(|state| state.truth.calls.clone())
    }

    /// How many calls named this tool, whatever came of them.
    pub fn calls_to(&self, tool: &str) -> Result<usize, HarnessError> {
        self.with(|state| state.truth.calls.iter().filter(|c| c.tool == tool).count())
    }

    /// How many messages with this method arrived, such as `tools/list`.
    pub fn request_count(&self, method: &str) -> Result<u64, HarnessError> {
        self.with(|state| state.truth.requests.get(method).copied().unwrap_or(0))
    }

    /// Exactly what is stored for an order, if anything.
    pub fn stored_order(&self, order_id: &str) -> Result<Option<StoredOrder>, HarnessError> {
        self.with(|state| state.truth.store.orders.get(order_id).cloned())
    }

    /// Every stored order, in order of identifier.
    pub fn stored_orders(&self) -> Result<Vec<StoredOrder>, HarnessError> {
        self.with(|state| state.truth.store.orders.values().cloned().collect())
    }

    /// Every stored write since the last reset, oldest first.
    pub fn writes(&self) -> Result<Vec<WriteRecord>, HarnessError> {
        self.with(|state| state.truth.writes.clone())
    }

    /// How many times one order has been written since the last reset.
    pub fn write_count(&self, order_id: &str) -> Result<usize, HarnessError> {
        self.with(|state| {
            let writes = state.truth.writes.iter();
            writes.filter(|w| w.order_id == order_id).count()
        })
    }

    /// Every connection seen since the last reset.
    pub fn connections(&self) -> Result<Vec<ConnectionRecord>, HarnessError> {
        self.with(|state| state.truth.connections.clone())
    }

    /// The connections that carried the credentials of more than one
    /// principal. Empty when no connection was shared between identities.
    pub fn shared_connections(&self) -> Result<BTreeSet<String>, HarnessError> {
        self.with(|state| {
            let connections = state.truth.connections.iter();
            connections
                .filter(|c| c.principals.len() > 1)
                .map(|c| c.id.clone())
                .collect()
        })
    }

    /// Every fault that applied to a request, oldest first.
    pub fn fired_faults(&self) -> Result<Vec<FiredFault>, HarnessError> {
        self.with(|state| state.truth.fired.clone())
    }

    /// How many requests were refused for a missing or unknown credential.
    pub fn unauthorized_count(&self) -> Result<u64, HarnessError> {
        self.with(|state| state.truth.unauthorized)
    }

    /// Switches a fault on.
    pub fn set_fault(&self, fault: Fault) -> Result<(), HarnessError> {
        self.set(|settings| settings.faults.push(fault))
    }

    /// The faults that are switched on now.
    pub fn faults(&self) -> Result<Vec<Fault>, HarnessError> {
        self.with(|state| state.settings.faults.clone())
    }

    /// Switches every fault off.
    pub fn clear_faults(&self) -> Result<(), HarnessError> {
        self.set(|settings| settings.faults.clear())
    }

    /// Changes the tool list. The change is silent until
    /// [`Control::announce_list_changed`] is called.
    pub fn set_drift(&self, drift: Drift) -> Result<(), HarnessError> {
        self.set(|settings| settings.drift.push(drift))
    }

    /// Puts the tool list back as it was.
    pub fn clear_drift(&self) -> Result<(), HarnessError> {
        self.set(|settings| settings.drift.clear())
    }

    /// Asks every ready stdio connection to send the notification that the
    /// tool list changed. The HTTP form opens no stream to send it on.
    pub fn announce_list_changed(&self) -> Result<(), HarnessError> {
        self.set(|settings| settings.announce += 1)
    }

    /// How many notifications that the tool list changed have been sent.
    pub fn announcements(&self) -> Result<u64, HarnessError> {
        self.with(|state| state.truth.announcements)
    }

    /// Waits until at least this many notifications have been sent.
    ///
    /// # Errors
    ///
    /// Returns [`HarnessError::Timeout`] if they have not been sent after
    /// twenty seconds, which means no ready stdio connection was there.
    pub async fn wait_for_announcements(&self, at_least: u64) -> Result<(), HarnessError> {
        let started = Instant::now();
        loop {
            if self.announcements()? >= at_least {
                return Ok(());
            }
            if started.elapsed() > ANNOUNCE_TIMEOUT {
                return Err(HarnessError::Timeout(
                    "the notification that the tool list changed was not sent",
                ));
            }
            tokio::time::sleep(ANNOUNCE_POLL).await;
        }
    }

    /// Sets how long the slow tool waits, in milliseconds.
    pub fn set_slow_ms(&self, ms: u64) -> Result<(), HarnessError> {
        self.set(|settings| settings.slow_ms = ms)
    }

    /// Sets how many tools one page of the tool list holds; 0 puts all on one page.
    pub fn set_page_size(&self, size: usize) -> Result<(), HarnessError> {
        self.set(|settings| settings.page_size = size)
    }

    /// Sets the version the server reports in `initialize`.
    pub fn set_server_version(&self, version: &str) -> Result<(), HarnessError> {
        self.set(|settings| version.clone_into(&mut settings.server_version))
    }

    /// Makes `initialize` answer a revision it does not serve with an error,
    /// in place of offering the one it serves.
    pub fn refuse_unknown_revisions(&self, refuse: bool) -> Result<(), HarnessError> {
        self.set(|settings| settings.refuse_unknown_revision = refuse)
    }

    /// Makes the stand serve the earlier handshake revision, 2025-06-18,
    /// alone, as an earlier release of a server does.
    pub fn serve_earlier_revision_only(&self, only: bool) -> Result<(), HarnessError> {
        self.set(|settings| settings.earlier_revision_only = only)
    }

    /// Makes the stand ask for a credential, and accept this made-up token as
    /// the named principal. Call it once for each identity a test needs.
    pub fn accept_token(&self, token: &str, principal: &str) -> Result<(), HarnessError> {
        self.set(|settings| {
            settings.credential_required = true;
            settings
                .tokens
                .insert(token.to_owned(), principal.to_owned());
        })
    }

    /// Stops accepting a token. The stand goes on asking for a credential,
    /// so a request that still carries this one is refused.
    pub fn revoke_token(&self, token: &str) -> Result<(), HarnessError> {
        self.set(|settings| {
            settings.tokens.remove(token);
        })
    }

    /// Forgets every HTTP session, as a server does when it restarts. The
    /// next request that names one is answered with 404.
    pub fn expire_sessions(&self) -> Result<(), HarnessError> {
        self.with(|state| state.sessions.clear())
    }
}
