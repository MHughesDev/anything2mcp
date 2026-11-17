//! The keeper's rows: sessions, their timeline, their saved state, and the
//! stored credentials, each sealed.
//!
//! The keeper owns the meaning of these rows. Where they are kept is behind
//! [`SessionStore`]: in memory today, and in the store's tables once
//! `a2m-ledger` offers the interface through which a crate keeps its own.
//! Every change of state is one compare-and-set on the session's state and
//! epoch, written together with its timeline event and its saved state, so
//! that two keepers on one store cannot both win a renewal.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use a2m_core::{DecisionId, Failure, RunId, RunnerId, SessionId, SessionState, Timestamp};
use a2m_vault::Sealed;

use crate::key::SessionKey;
use crate::name::{CredentialRef, ProfileRef, ProviderName};
use crate::person::PersonReason;

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
    pub profile: Option<ProfileRef>,
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
    /// Automatic attempts with a stored credential since the other side last
    /// confirmed it.
    pub stored_attempts: u32,
    /// Why a person is wanted, while the state is `needs_person`.
    pub wants: Option<PersonReason>,
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
    pub cause: &'static str,
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

/// Where the keeper's rows are kept. A clone shares the same rows, so a test
/// keeps one across a simulated restart, as a database file would be kept.
#[derive(Debug, Clone)]
pub struct SessionStore {
    backing: Backing,
}

#[derive(Debug, Clone)]
enum Backing {
    Memory(Arc<Mutex<Memory>>),
}

#[derive(Debug, Default)]
struct Memory {
    rows: BTreeMap<SessionId, SessionRow>,
    by_key: BTreeMap<SessionKey, SessionId>,
    events: Vec<SessionEvent>,
    saved: BTreeMap<SessionId, Sealed>,
    credentials: BTreeMap<(String, CredentialRef), Sealed>,
}

impl SessionStore {
    /// Rows held in memory. For tests, and for a process that keeps nothing.
    #[must_use]
    pub fn in_memory() -> Self {
        Self {
            backing: Backing::Memory(Arc::new(Mutex::new(Memory::default()))),
        }
    }

    fn memory(&self) -> MutexGuard<'_, Memory> {
        let Backing::Memory(memory) = &self.backing;
        memory.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) async fn find(&self, key: &SessionKey) -> Result<Option<SessionRow>, Failure> {
        let memory = self.memory();
        Ok(memory
            .by_key
            .get(key)
            .and_then(|id| memory.rows.get(id))
            .cloned())
    }

    pub(crate) async fn get(&self, id: SessionId) -> Result<Option<SessionRow>, Failure> {
        Ok(self.memory().rows.get(&id).cloned())
    }

    /// Inserts `row` unless a session of its key exists, and returns the
    /// record that is stored: the new one, or the one that was there first.
    pub(crate) async fn create(
        &self,
        row: SessionRow,
        event: SessionEvent,
    ) -> Result<SessionRow, Failure> {
        let mut memory = self.memory();
        if let Some(existing) = memory
            .by_key
            .get(&row.key)
            .and_then(|id| memory.rows.get(id))
        {
            return Ok(existing.clone());
        }
        memory.by_key.insert(row.key.clone(), row.id);
        memory.rows.insert(row.id, row.clone());
        memory.events.push(event);
        Ok(row)
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
        let mut memory = self.memory();
        let current = memory.rows.get(&seen.id);
        if !current.is_some_and(|row| row.state == seen.state && row.epoch == seen.epoch) {
            return Ok(false);
        }
        match saved {
            Saved::Keep => {}
            Saved::Put(sealed) => {
                memory.saved.insert(next.id, sealed);
            }
            Saved::Delete => {
                memory.saved.remove(&next.id);
            }
        }
        memory.rows.insert(next.id, next);
        memory.events.push(event);
        Ok(true)
    }

    /// Notes a use or a passed check, and appends its event. A check that
    /// confirmed the credential clears the count of unconfirmed attempts. It
    /// changes neither the state nor the epoch, so it needs no
    /// compare-and-set.
    pub(crate) async fn note(
        &self,
        id: SessionId,
        used_at: Option<Timestamp>,
        checked_at: Option<Timestamp>,
        confirmed: bool,
        event: SessionEvent,
    ) -> Result<(), Failure> {
        let mut memory = self.memory();
        if let Some(row) = memory.rows.get_mut(&id) {
            row.last_used_at = used_at.or(row.last_used_at);
            row.checked_at = checked_at.or(row.checked_at);
            if confirmed {
                row.stored_attempts = 0;
            }
        }
        memory.events.push(event);
        Ok(())
    }

    pub(crate) async fn saved(&self, id: SessionId) -> Result<Option<Sealed>, Failure> {
        Ok(self.memory().saved.get(&id).cloned())
    }

    pub(crate) async fn all(&self) -> Result<Vec<SessionRow>, Failure> {
        Ok(self.memory().rows.values().cloned().collect())
    }

    pub(crate) async fn events(&self, id: SessionId) -> Result<Vec<SessionEvent>, Failure> {
        Ok(self
            .memory()
            .events
            .iter()
            .filter(|event| event.session == id)
            .cloned()
            .collect())
    }

    pub(crate) async fn put_credential(
        &self,
        owner: String,
        reference: CredentialRef,
        sealed: Sealed,
    ) -> Result<(), Failure> {
        self.memory().credentials.insert((owner, reference), sealed);
        Ok(())
    }

    pub(crate) async fn credential(
        &self,
        owner: &str,
        reference: &CredentialRef,
    ) -> Result<Option<Sealed>, Failure> {
        Ok(self
            .memory()
            .credentials
            .get(&(owner.to_owned(), reference.clone()))
            .cloned())
    }
}
