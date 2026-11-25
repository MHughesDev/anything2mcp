//! The keeper's rows: sessions, their timeline, their saved state, and the
//! stored credentials, each sealed.
//!
//! The keeper owns the meaning of these rows; the store owns the connection,
//! the transactions, and the migration that creates the tables (decision
//! 0080). Every change of state is one compare-and-set on the session's
//! state and epoch, written in one transaction with its timeline event and
//! its saved state, so that two keepers on one store cannot both win a
//! renewal. A lease, a passed check, and a restore are written only while
//! the session's epoch is the one they were made for, so none of them is
//! recorded for credentials that were replaced or ended meanwhile.

use std::collections::BTreeMap;

use a2m_core::{
    DecisionId, Digest, Failure, RunId, RunnerId, SessionId, SessionState, Timestamp, WorkspaceId,
};
use a2m_ledger::{Durability, Store};
use a2m_vault::Sealed;

use crate::key::SessionKey;
use crate::name::{BrowserProfileRef, CredentialRef, ProviderName};
use crate::person::PersonReason;
use crate::tables;

/// One session's record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    /// The session.
    pub id: SessionId,
    /// What it is keyed by.
    pub key: SessionKey,
    /// The provider that signed it in.
    pub provider: ProviderName,
    /// Where it stands.
    pub state: SessionState,
    /// The number of its credentials. It increases whenever they are
    /// replaced or ended, and a lease is good only for the epoch it names.
    pub epoch: u64,
    /// The runner that holds the live state. `None` for the local engine.
    pub home: Option<RunnerId>,
    /// Whether its state may be saved and restored.
    pub movable: bool,
    /// The engine-owned profile, when the live state is a directory.
    pub profile: Option<BrowserProfileRef>,
    /// When the state last changed.
    pub state_since: Timestamp,
    /// When the present sign-in was made.
    pub signed_in_at: Option<Timestamp>,
    /// When the credentials themselves run out, if they say.
    pub expires_at: Option<Timestamp>,
    /// When a check last passed.
    pub checked_at: Option<Timestamp>,
    /// When a run last used it: the start of the keep-alive window.
    pub last_used_at: Option<Timestamp>,
    /// Attempts with a stored credential since the other side last
    /// confirmed it: automatic ones whose credential was not confirmed,
    /// refusals, and sign-ins whose answer nobody could read.
    pub stored_attempts: u32,
    /// Why a person is wanted, while the state is `needs_person`.
    pub wants: Option<PersonReason>,
    /// The stored credential `stored_attempts` counts attempts with, as it
    /// was stored then: the mark of its sealed envelopes, never of a value
    /// (decision 0269). A count with another mark than the credential
    /// stored now is about one the person has stored again since, and is
    /// not counted. `None` when no attempt is counted.
    pub attempted_with: Option<Digest>,
    /// Whether the last sign-in was declared by the target's drafts rather
    /// than by a published version. Such a session is never renewed without
    /// its person (decision 0248). Kept in the record, so that a restart
    /// does not forget it (decision 0374); a record that does not say, as
    /// one kept before the mark was, reads as made under drafts.
    pub made_under_drafts: bool,
}

/// What happened, in the words of the session timeline (spec section 215).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
    /// The session's record was made.
    Created,
    /// A sign-in started.
    SignInStarted,
    /// A sign-in finished and the session is valid.
    SignedIn,
    /// A run was given a lease.
    Leased,
    /// A check was made, or a sign of expiry was reported.
    Checked,
    /// A renewal started.
    RenewalStarted,
    /// The credentials were renewed.
    Renewed,
    /// A person was asked.
    Prompted,
    /// The session ended and could not be renewed.
    Expired,
    /// The session was ended on purpose.
    Revoked,
    /// A sign-in was abandoned or could not be made.
    Abandoned,
    /// Saved state was restored to the home runner.
    Restored,
}

impl EventKind {
    /// The kind as the timeline writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::SignInStarted => "sign_in_started",
            Self::SignedIn => "signed_in",
            Self::Leased => "leased",
            Self::Checked => "checked",
            Self::RenewalStarted => "renewal_started",
            Self::Renewed => "renewed",
            Self::Prompted => "prompted",
            Self::Expired => "expired",
            Self::Revoked => "revoked",
            Self::Abandoned => "abandoned",
            Self::Restored => "restored",
        }
    }

    /// The kind a timeline word names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == name)
    }

    /// Every kind.
    pub const ALL: [Self; 12] = [
        Self::Created,
        Self::SignInStarted,
        Self::SignedIn,
        Self::Leased,
        Self::Checked,
        Self::RenewalStarted,
        Self::Renewed,
        Self::Prompted,
        Self::Expired,
        Self::Revoked,
        Self::Abandoned,
        Self::Restored,
    ];
}

/// One line of a session's timeline. It holds names, times, and causes, and
/// never a cookie, a token, or a password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEvent {
    /// The session.
    pub session: SessionId,
    /// When.
    pub at: Timestamp,
    /// What happened.
    pub kind: EventKind,
    /// The state after it.
    pub state: SessionState,
    /// The epoch after it.
    pub epoch: u64,
    /// Why, as a fixed word such as `sign_refused` or `max_age`.
    pub cause: String,
    /// The run a lease was given to.
    pub run: Option<RunId>,
    /// The policy decision about whom that run acts as.
    pub decided: Option<DecisionId>,
}

/// What a change of state does to the session's saved state, in the same
/// write.
#[derive(Debug, Clone)]
pub(crate) enum Saved {
    /// Leave it as it is.
    Keep,
    /// Replace it.
    Put(Sealed),
    /// Remove it.
    Delete,
}

/// Where the keeper's rows are kept: the store, through the tables this
/// crate owns. A clone is the same store.
#[derive(Debug, Clone)]
pub struct SessionStore {
    store: Store,
}

impl SessionStore {
    /// The keeper's rows in `store`. The store's migrations have made the
    /// tables.
    #[must_use]
    pub fn new(store: Store) -> Self {
        Self { store }
    }

    pub(crate) async fn find(&self, key: &SessionKey) -> Result<Option<SessionRow>, Failure> {
        let key = key.clone();
        self.store.read(move |tx| tables::find(tx, &key)).await
    }

    pub(crate) async fn get(&self, id: SessionId) -> Result<Option<SessionRow>, Failure> {
        self.store.read(move |tx| tables::get(tx, id)).await
    }

    /// Inserts `row` unless a session of its key exists, and returns the
    /// record that is stored: the new one, or the one that was there first.
    pub(crate) async fn create(
        &self,
        row: SessionRow,
        event: SessionEvent,
    ) -> Result<SessionRow, Failure> {
        self.store
            .write(Durability::Synced, move |tx| {
                tables::create(tx, &row, &event)
            })
            .await
    }

    /// The compare-and-set: writes `next`, its event, and the change to the
    /// saved state, only if the session still has the state and the epoch
    /// `seen` has. `false` when someone else changed it first; nothing is
    /// written then.
    pub(crate) async fn advance(
        &self,
        seen: &SessionRow,
        next: SessionRow,
        event: SessionEvent,
        saved: Saved,
    ) -> Result<bool, Failure> {
        let seen = seen.clone();
        self.store
            .write(Durability::Synced, move |tx| {
                tables::advance(tx, &seen, &next, &event, &saved)
            })
            .await
    }

    /// Notes a use, a passed check, or a restore, and appends its event,
    /// only if the session's epoch is still the one the event names.
    /// `false` when the session was renewed or ended first; nothing is
    /// written then.
    pub(crate) async fn note(
        &self,
        used_at: Option<Timestamp>,
        checked_at: Option<Timestamp>,
        confirmed: bool,
        event: SessionEvent,
    ) -> Result<bool, Failure> {
        self.store
            .write(Durability::Relaxed, move |tx| {
                tables::note(
                    tx,
                    event.session,
                    event.epoch,
                    used_at,
                    checked_at,
                    confirmed,
                    &event,
                )
            })
            .await
    }

    pub(crate) async fn saved(&self, id: SessionId) -> Result<Option<Sealed>, Failure> {
        self.store.read(move |tx| tables::saved(tx, id)).await
    }

    pub(crate) async fn all(&self) -> Result<Vec<SessionRow>, Failure> {
        self.store.read(tables::all).await
    }

    pub(crate) async fn events(&self, id: SessionId) -> Result<Vec<SessionEvent>, Failure> {
        self.store.read(move |tx| tables::events(tx, id)).await
    }

    /// The newest sign-ins of a session that made it hold, newest first:
    /// each cause and time, at most `limit`.
    pub(crate) async fn sign_ins(
        &self,
        id: SessionId,
        limit: u32,
    ) -> Result<Vec<(String, Timestamp)>, Failure> {
        self.store
            .read(move |tx| tables::sign_ins(tx, id, limit))
            .await
    }

    /// The mark of the stored credentials an auth entry names for the
    /// session's identity, as they are stored now. Nothing is opened.
    pub(crate) async fn credential_mark(
        &self,
        key: &SessionKey,
        references: &BTreeMap<String, CredentialRef>,
    ) -> Result<Digest, Failure> {
        let workspace = key.workspace;
        let owner = key.identity.to_string();
        let references = references.clone();
        self.store
            .read(move |tx| {
                tables::credential_mark(
                    tx,
                    &workspace,
                    &owner,
                    references
                        .iter()
                        .map(|(role, reference)| (role.as_str(), reference)),
                )
            })
            .await
    }

    pub(crate) async fn put_credential(
        &self,
        workspace: WorkspaceId,
        owner: String,
        reference: CredentialRef,
        sealed: Sealed,
    ) -> Result<(), Failure> {
        self.store
            .write(Durability::Synced, move |tx| {
                tables::put_credential(tx, &workspace, &owner, &reference, &sealed)
            })
            .await
    }

    pub(crate) async fn credential(
        &self,
        workspace: WorkspaceId,
        owner: String,
        reference: CredentialRef,
    ) -> Result<Option<Sealed>, Failure> {
        self.store
            .read(move |tx| tables::credential(tx, &workspace, &owner, &reference))
            .await
    }
}
