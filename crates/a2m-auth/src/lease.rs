//! The lease: how one run reaches a session.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use a2m_core::{
    DecisionId, Failure, PrincipalId, REDACTED, Redact, RunId, RunnerId, Secret, SessionId,
    SessionState, Timestamp,
};
use a2m_vault::Vault;

use crate::failure;
use crate::key::{AuthEntry, SessionKey};
use crate::name::{BrowserProfileRef, ProviderName};
use crate::provider::{ExpirySign, RawSign};
use crate::sink::{ChallengeScheme, CredentialSink};
use crate::work::Live;

/// How long a call waits for a session that is signing in, being checked,
/// being renewed, or waiting for a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// An interactive call: it waits this long, for a person too.
    Interactive(Duration),
    /// A pipeline: it waits for the keeper's own work, a check or a silent
    /// renewal, and never for a person. It stops with `needs_login` and is
    /// resumed after the sign-in.
    NoWait,
}

impl Wait {
    /// The wait of an interactive call when nothing else is set: 5 minutes.
    pub const DEFAULT: Self = Self::Interactive(Duration::from_secs(300));
}

/// What a lease is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    /// A run.
    Run,
    /// The keeper's own session check, run as a read. It is granted while the
    /// session is `valid`, `suspect`, or `renewing`.
    Probe,
}

/// A request for a lease, made through `Control` on the engine's behalf.
#[derive(Debug, Clone)]
pub struct LeaseRequest {
    /// Which session.
    pub key: SessionKey,
    /// The session's entry in the target's auth file.
    pub entry: Arc<AuthEntry>,
    /// The run that asks.
    pub run: RunId,
    /// The policy decision about whom the run acts as. Recorded with the
    /// lease, not made again.
    pub decided: DecisionId,
    /// The person the run acts for: the caller themself, or the person who
    /// delegated to the calling agent. `None` when it acts for no person. A
    /// session signed in as a person is leased only to a run acting for
    /// that person.
    pub acting_for: Option<PrincipalId>,
    /// How long to wait.
    pub wait: Wait,
    /// The time a pipeline says the session must still have left.
    pub headroom: Option<Duration>,
    /// What the lease is for.
    pub purpose: Purpose,
}

/// What a lease says, as plain data: it can be written to the runner link
/// and holds no secret. The home runner turns it back into a lease with
/// [`KeeperWorker::bind`](crate::KeeperWorker::bind).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SessionGrant {
    /// The session.
    pub session: SessionId,
    /// The epoch of the credentials the lease is for.
    pub epoch: u64,
    /// The session's state when the lease was granted.
    pub state: SessionState,
    /// The runner that holds the live state. `None` for the local engine.
    pub home: Option<RunnerId>,
    /// The provider that signed the session in.
    pub provider: ProviderName,
    /// When it was granted.
    pub granted_at: Timestamp,
    /// The run it was granted to.
    pub run: RunId,
    /// The policy decision recorded with it.
    pub decided: DecisionId,
    /// What it is for.
    pub purpose: Purpose,
    /// The engine-owned profile, when the live state is a directory.
    pub profile: Option<BrowserProfileRef>,
}

/// A session as one run may use it. It carries no secret and cannot print
/// one: it names the session, and reaches the credentials the home runner
/// holds only to place them into a driver's client.
///
/// A lease is a snapshot, not a promise. It is good while the session's
/// epoch is the one it names and the session has not ended. A driver that
/// sees a sign of expiry asks
/// [`SessionLease::detect_expiry`], stops its step with `session_expired`,
/// and leaves the report to the engine; it never renews.
pub struct SessionLease {
    grant: SessionGrant,
    bound: Option<Bound>,
}

/// The credentials a lease reaches: the home runner's table of live
/// sessions, and the vault whose redactor every placed secret goes through.
pub(crate) struct Bound {
    pub(crate) live: Arc<Live>,
    pub(crate) vault: Arc<Vault>,
}

impl SessionLease {
    pub(crate) fn new(grant: SessionGrant, bound: Option<Bound>) -> Self {
        Self { grant, bound }
    }

    /// The session.
    #[must_use]
    pub fn session(&self) -> SessionId {
        self.grant.session
    }

    /// The epoch of the credentials this lease is for.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.grant.epoch
    }

    /// The session's state when the lease was granted.
    #[must_use]
    pub fn state(&self) -> SessionState {
        self.grant.state
    }

    /// The engine-owned profile directory's name, for a session whose live
    /// state is a directory. `None` otherwise.
    #[must_use]
    pub fn profile(&self) -> Option<&BrowserProfileRef> {
        self.grant.profile.as_ref()
    }

    /// Everything the lease says, as data that can cross the runner link.
    #[must_use]
    pub fn grant(&self) -> &SessionGrant {
        &self.grant
    }

    /// Places the session's credentials into the driver's client, through
    /// the provider's `inject`. Every secret is registered with the redactor
    /// before the sink sees it. Nothing is returned that holds one.
    ///
    /// # Errors
    ///
    /// `session_expired`/`device_required` when this process does not hold
    /// the session's credentials; `session_expired`/`lease_stale` when the
    /// credentials were replaced after the lease was granted, or the session
    /// was ended, whatever is held; the provider's failure, with every
    /// registered secret masked, when its material lacks a part.
    pub fn apply(&self, sink: &mut dyn CredentialSink) -> Result<(), Failure> {
        let bound = self.bound.as_ref().ok_or_else(failure::device_required)?;
        // The home runner's own word on the session: the credentials at the
        // lease's epoch, and no end at a later one. An end is remembered
        // there, so credentials that reach the runner after it, by a restore
        // that was slow, are neither held nor placed.
        let held = bound
            .live
            .usable(self.grant.session, self.grant.epoch)
            .ok_or_else(failure::lease_stale)?;
        let mut registering = Registering {
            sink,
            vault: &bound.vault,
        };
        let placed = held.provider.inject(&held.material, &mut registering);
        placed.map_err(|failure| bound.vault.redactor().redact_failure(&failure))
    }

    /// Whether something the driver observed is a sign that the session has
    /// ended, in the judgment of the session's provider. The driver stops
    /// its step and the engine reports the sign to the keeper.
    #[must_use]
    pub fn detect_expiry(&self, sign: &RawSign) -> Option<ExpirySign> {
        let bound = self.bound.as_ref()?;
        let held = bound.live.held(self.grant.session)?;
        held.provider.detect_expiry(sign)
    }
}

impl fmt::Debug for SessionLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionLease")
            .field("session", &self.grant.session)
            .field("epoch", &self.grant.epoch)
            .field("state", &self.grant.state)
            .field("provider", &self.grant.provider.as_str())
            .field("credentials", &REDACTED)
            .finish()
    }
}

/// The driver's sink, with the redactor in front of it: whatever a provider
/// places, including a value it built from its parts, is registered first.
struct Registering<'a> {
    sink: &'a mut dyn CredentialSink,
    vault: &'a Vault,
}

impl CredentialSink for Registering<'_> {
    fn header(&mut self, name: &str, value: &Secret) {
        self.vault.register(value);
        self.sink.header(name, value);
    }

    fn cookie(&mut self, origin: &str, name: &str, value: &Secret) {
        self.vault.register(value);
        self.sink.cookie(origin, name, value);
    }

    fn tls_identity(&mut self, certificate: &[u8], key: &Secret) {
        self.vault.register(key);
        self.sink.tls_identity(certificate, key);
    }

    fn challenge_credential(&mut self, scheme: ChallengeScheme, user: &str, secret: &Secret) {
        self.vault.register(secret);
        self.sink.challenge_credential(scheme, user, secret);
    }
}
