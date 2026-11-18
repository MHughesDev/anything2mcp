//! The session keeper: the only code that signs in, checks, renews, and ends
//! a session, and the only way to a lease.
//!
//! The keeper decides; the work it decides on runs on the session's home
//! runner, through the [`KeeperWork`] it was handed. Every change of state is
//! a legal move of [`legal_move`], made by one compare-and-set on the
//! session's state and epoch. The caller that wins a move starts the work
//! that belongs to it; every other caller waits for the outcome.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use a2m_core::{
    Environment, Failure, FailureClass, FailureCode, Identity, PrincipalId, SessionId,
    SessionState, TargetName, Timestamp, WorkspaceId,
};
use a2m_telemetry::{Clock, Telemetry};
use a2m_vault::{OsRandom, RandomSource};
use tokio::sync::watch;

use crate::failure;
use crate::key::{AuthEntry, SessionKey, SessionRef};
use crate::lease::{LeaseRequest, Purpose, SessionGrant, SessionLease, Wait};
use crate::name::ProviderName;
use crate::person::{PersonAsk, PersonReason};
use crate::provider::{
    ExpirySign, ProviderNeeds, ProviderSet, Rung, SignInAddress, checked, sign_in_addresses,
};
use crate::state::{CallRule, call_rule, legal_move};
use crate::store::{EventKind, Saved, SessionEvent, SessionRow, SessionStore};
use crate::work::{KeeperJob, KeeperWork, KeeperWorker, ProbeOutcome, Work, WorkOutcome};

/// The keeper was asked for a move that is not in the table of states. It
/// is a fault of the keeper itself, and the move is not made.
pub const SESSION_MOVE_ILLEGAL: FailureCode = FailureCode::fixed("session_move_illegal");

/// Renewals that succeeded.
pub const RENEWALS: &str = "a2m.session.renewals";
/// Times a person was made to sign in again.
pub const FORCED_SIGN_INS: &str = "a2m.session.forced_sign_ins";
/// Checks that failed, and signs of expiry reported.
pub const CHECKS_FAILED: &str = "a2m.session.checks_failed";
/// How long a renewal took.
pub const RENEWAL_DURATION: &str = "a2m.session.renewal.duration";

/// The keeper's own settings. The defaults are the ones spec section 215
/// names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeeperSettings {
    /// How long after its last use a session is still checked: 8 hours.
    pub keep_alive: Duration,
    /// How often a session is probed when its auth file declares no idle
    /// timeout: 15 minutes. With a declared one, half of it.
    pub probe_every: Duration,
    /// How long a check, a renewal, or a stored-credential sign-in may take
    /// before a tick calls it interrupted, and how long a call that does not
    /// wait for a person waits for one: 10 minutes.
    pub work_limit: Duration,
    /// How long before a credential's own expiry it is renewed: 30 seconds.
    pub renew_ahead: Duration,
}

impl Default for KeeperSettings {
    fn default() -> Self {
        Self {
            keep_alive: Duration::from_secs(8 * 60 * 60),
            probe_every: Duration::from_secs(15 * 60),
            work_limit: Duration::from_secs(10 * 60),
            renew_ahead: Duration::from_secs(30),
        }
    }
}

/// Who ended a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndedBy {
    /// The person whose session it is: `session.revoke`.
    Owner,
    /// An administrator, who ends it without using it: `session.end`.
    Administrator,
}

/// What `session.login` gives back at once: which session, and where it
/// stands. The sign-in goes on behind it; `lease` and `status` show how it
/// ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignInTicket {
    /// The session.
    pub session: SessionId,
    /// Its state when the ticket was made.
    pub state: SessionState,
}

/// One session as `session.status` shows it. It holds no credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStatus {
    /// The session.
    pub session: SessionId,
    /// The target.
    pub target: TargetName,
    /// The environment.
    pub environment: Environment,
    /// Who the system sees.
    pub identity: Identity,
    /// The provider that signed it in.
    pub provider: ProviderName,
    /// Where it stands.
    pub state: SessionState,
    /// The number of its credentials.
    pub epoch: u64,
    /// Why a person is wanted, while the state is `needs_person`.
    pub wants: Option<PersonReason>,
    /// When the present sign-in was made.
    pub signed_in_at: Option<Timestamp>,
    /// When the credentials themselves run out, if they say.
    pub expires_at: Option<Timestamp>,
    /// When a check last passed.
    pub checked_at: Option<Timestamp>,
    /// When a run last used it.
    pub last_used_at: Option<Timestamp>,
}

/// The session keeper. A clone is the same keeper.
#[derive(Clone)]
pub struct Keeper {
    inner: Arc<Inner>,
}

struct Inner {
    store: SessionStore,
    providers: ProviderSet,
    work: Arc<dyn KeeperWork>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn RandomSource>,
    local: Option<Arc<KeeperWorker>>,
    settings: KeeperSettings,
    telemetry: Option<Telemetry>,
    /// Moves on every change of any session and on every tick, so that a
    /// waiting call looks again.
    changed: watch::Sender<u64>,
    /// The auth file entry each session was last used under. A drafts'
    /// entry never replaces a published version's (decision 0248).
    entries: Mutex<BTreeMap<SessionId, Arc<AuthEntry>>>,
    /// For each session, the addresses its person was shown and allowed
    /// when they carried out a sign-in declared by drafts, or that a
    /// published version declares for it: where a run under drafts may
    /// send its credentials (decision 0248). In memory only: after a
    /// restart a run under drafts is refused until the person signs in
    /// again.
    allowed: Mutex<BTreeMap<SessionId, BTreeSet<String>>>,
    /// The sessions whose last sign-in was declared by drafts. None of them
    /// is renewed without its person (decision 0248). In memory only.
    drafted: Mutex<BTreeSet<SessionId>>,
    /// Sessions with work in flight in this process.
    busy: Mutex<BTreeMap<SessionId, usize>>,
    /// Why each session's last sign-in or renewal failed, until one works.
    failures: Mutex<BTreeMap<SessionId, Failure>>,
    /// How many calls are waiting for a session now.
    waiting: AtomicUsize,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The time from `from` to `to`, or none when `to` is the earlier.
fn between(from: Timestamp, to: Timestamp) -> Duration {
    let millis = to.as_unix_millis().saturating_sub(from.as_unix_millis());
    Duration::from_millis(u64::try_from(millis).unwrap_or(0))
}

/// Marks a session as having work in flight until it is dropped.
struct Busy {
    inner: Arc<Inner>,
    session: SessionId,
}

impl Drop for Busy {
    fn drop(&mut self) {
        {
            // Marks are counted: two pieces of work on one session may
            // overlap, and the session is busy until the last one ends.
            let mut busy = locked(&self.inner.busy);
            match busy.get_mut(&self.session) {
                Some(marks) if *marks > 1 => *marks -= 1,
                _ => {
                    busy.remove(&self.session);
                }
            }
        }
        self.inner.wake();
    }
}

/// Counts one call as waiting until it is dropped, so that a call that is
/// cancelled while it waits is no longer counted.
struct Waiting<'a>(&'a AtomicUsize);

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Builds a [`Keeper`]. What is not set takes its default: the operating
/// system's random source, the settings of spec section 215, no telemetry,
/// and no worker in this process.
pub struct KeeperBuilder {
    inner: Inner,
}

impl KeeperBuilder {
    /// The worker of this same process, so that a lease for a session whose
    /// home is here reaches its credentials at once, and saved state is
    /// restored here after a restart.
    #[must_use]
    pub fn local_worker(mut self, worker: Arc<KeeperWorker>) -> Self {
        self.inner.local = Some(worker);
        self
    }

    /// Other settings than the defaults.
    #[must_use]
    pub fn settings(mut self, settings: KeeperSettings) -> Self {
        self.inner.settings = settings;
        self
    }

    /// A telemetry handle, for the keeper's metrics.
    #[must_use]
    pub fn telemetry(mut self, telemetry: Telemetry) -> Self {
        self.inner.telemetry = Some(telemetry);
        self
    }

    /// Where the random part of a session's identifier comes from.
    #[must_use]
    pub fn random(mut self, random: Arc<dyn RandomSource>) -> Self {
        self.inner.random = random;
        self
    }

    /// The keeper.
    #[must_use]
    pub fn build(self) -> Keeper {
        Keeper {
            inner: Arc::new(self.inner),
        }
    }
}

impl fmt::Debug for KeeperBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeeperBuilder").finish_non_exhaustive()
    }
}

impl Keeper {
    /// Starts a keeper over its rows, the providers, the work interface,
    /// and the clock. Nothing is read until the first call.
    #[must_use]
    pub fn builder(
        store: SessionStore,
        providers: ProviderSet,
        work: Arc<dyn KeeperWork>,
        clock: Arc<dyn Clock>,
    ) -> KeeperBuilder {
        KeeperBuilder {
            inner: Inner {
                store,
                providers,
                work,
                clock,
                random: Arc::new(OsRandom),
                local: None,
                settings: KeeperSettings::default(),
                telemetry: None,
                changed: watch::Sender::new(0),
                entries: Mutex::new(BTreeMap::new()),
                allowed: Mutex::new(BTreeMap::new()),
                drafted: Mutex::new(BTreeSet::new()),
                busy: Mutex::new(BTreeMap::new()),
                failures: Mutex::new(BTreeMap::new()),
                waiting: AtomicUsize::new(0),
            },
        }
    }

    /// What the provider of a name needs and can do.
    ///
    /// # Errors
    ///
    /// `input`/`auth_provider_unknown`.
    pub fn needs(&self, provider: &ProviderName) -> Result<ProviderNeeds, Failure> {
        Ok(self.inner.providers.get(provider)?.needs())
    }

    /// Every address a sign-in under `entry` for `target` sends a
    /// credential, a token, or the person to, after the checks the keeper
    /// makes before any request: the provider's own, that every credential
    /// belongs to the target, and that every address the provider sends to
    /// is one the target declares (decision 0248). What a person is shown
    /// before a sign-in or a publish.
    ///
    /// # Errors
    ///
    /// `input`/`auth_provider_unknown`; the provider's
    /// `input`/`auth_settings_invalid`; `input`/`credential_of_another_target`;
    /// `policy`/`destination_not_declared`.
    pub fn sign_in_addresses(
        &self,
        target: &TargetName,
        entry: &AuthEntry,
    ) -> Result<Vec<SignInAddress>, Failure> {
        let provider = self.inner.providers.get(&entry.provider)?;
        checked(provider.as_ref(), entry, target)?;
        Ok(sign_in_addresses(provider.as_ref(), entry))
    }

    /// Whether a run under `entry`, an entry the target's drafts declare,
    /// may use the session `session`: every address it would send the
    /// session's credentials to is one the session's person allowed. Always
    /// true for an entry of a published version.
    #[must_use]
    pub fn allows(&self, session: SessionId, entry: &AuthEntry) -> bool {
        self.inner.allows(session, entry)
    }

    /// The engine's only way to a session, through `Control`.
    ///
    /// A valid session gives a lease. A session that is signing in, being
    /// checked, or being renewed is waited for; one that needs a person is
    /// waited for up to an interactive call's wait, and not at all by a
    /// call that does not wait. A session whose credentials have reached
    /// their own expiry is renewed first, once, however many calls noticed.
    /// A session that is absent or expired is signed in from its stored
    /// credential when its provider needs no person and the attempt limit
    /// allows; otherwise the call stops with `needs_login`.
    ///
    /// A call returns as soon as the session's record decides it. The work
    /// that wrote the record may still have steps to take: telling the
    /// person, having the home runner drop what it held, counting a metric.
    /// [`Keeper::idle`] waits for those.
    ///
    /// # Errors
    ///
    /// `policy`/`identity_not_allowed` when the run does not act for the
    /// person the session belongs to; `session_expired` with `needs_login`,
    /// `needs_mfa`, `credential_refused`, `headroom_short`, or
    /// `device_required`; `input` when the auth entry is not usable; the
    /// failure of the sign-in or renewal the call waited for; the vault's
    /// `busy` failures when saved state cannot be opened.
    pub async fn lease(&self, req: LeaseRequest) -> Result<SessionLease, Failure> {
        self.inner.lease(req).await
    }

    /// A driver's sign of expiry, relayed by the engine. The driver has
    /// stopped its step; the keeper checks the session again once and, if
    /// the check fails, renews it once. A report for an epoch that is no
    /// longer the session's is a report about credentials already replaced,
    /// and changes nothing.
    pub async fn report(&self, lease: &SessionLease, sign: ExpirySign) {
        let inner = &self.inner;
        let Ok(Some(row)) = inner.store.get(lease.session()).await else {
            return;
        };
        if row.epoch != lease.epoch() || row.state != SessionState::Valid {
            return;
        }
        // Marked before the move, so that the session is never seen in a
        // state that work belongs to with nobody marked as doing the work.
        let busy = inner.mark_busy(row.id);
        let moved = inner
            .advance(
                &row,
                SessionState::Suspect,
                EventKind::Checked,
                sign.as_str(),
                |_| {},
                Saved::Keep,
            )
            .await;
        if let Ok(Some(suspect)) = moved {
            inner.count(CHECKS_FAILED);
            let inner = Arc::clone(inner);
            tokio::spawn(async move {
                let _busy = busy;
                inner.recheck(&suspect).await;
            });
        }
    }

    /// Starts a sign-in for a session: `session.login`. For a provider that
    /// needs a person, the person is asked through the work interface; for
    /// one that works from a stored credential, the credential is tried
    /// once more, whatever was refused before.
    ///
    /// # Errors
    ///
    /// `policy`/`identity_not_allowed`; `input` when the auth entry is not
    /// usable.
    pub async fn sign_in(
        &self,
        key: &SessionKey,
        entry: Arc<AuthEntry>,
        acting_for: Option<PrincipalId>,
    ) -> Result<SignInTicket, Failure> {
        let inner = &self.inner;
        owner_only(key, acting_for.as_ref())?;
        checked(
            inner.providers.get(&entry.provider)?.as_ref(),
            &entry,
            &key.target,
        )?;
        let surfaces = &entry.surfaces;
        if !surfaces.from_published && !surfaces.shown_to_person {
            return Err(failure::not_published(
                "it would sign in without the person being shown where it sends",
            ));
        }
        loop {
            let row = inner.find_or_create(key, &entry).await?;
            inner.remember_entry(row.id, &entry);
            // The person, shown every address, allowed this sign-in's: a
            // session that holds already may now be used there.
            inner.allow(row.id, &entry);
            if matches!(
                row.state,
                SessionState::Valid
                    | SessionState::SigningIn
                    | SessionState::Suspect
                    | SessionState::Renewing
            ) {
                return Ok(SignInTicket {
                    session: row.id,
                    state: row.state,
                });
            }
            let busy = inner.mark_busy(row.id);
            let moved = inner
                .advance(
                    &row,
                    SessionState::SigningIn,
                    EventKind::SignInStarted,
                    "requested",
                    |next| {
                        next.stored_attempts = 0;
                        next.provider = entry.provider.clone();
                    },
                    Saved::Keep,
                )
                .await?;
            if let Some(signing) = moved {
                let ticket = SignInTicket {
                    session: signing.id,
                    state: signing.state,
                };
                inner.spawn_sign_in(signing, entry, false, busy);
                return Ok(ticket);
            }
        }
    }

    /// The sessions of one workspace: those of one identity for
    /// `session.status`, or all of them for `session.list`.
    ///
    /// # Errors
    ///
    /// The store's failures.
    pub async fn status(
        &self,
        workspace: &WorkspaceId,
        identity: Option<&Identity>,
    ) -> Result<Vec<SessionStatus>, Failure> {
        let rows = self.inner.store.all().await?;
        Ok(rows
            .into_iter()
            .filter(|row| &row.key.workspace == workspace)
            .filter(|row| identity.is_none_or(|identity| &row.key.identity == identity))
            .map(status_of)
            .collect())
    }

    /// Waits until the sign-in [`Keeper::sign_in`] started for `session`
    /// has ended, for at most `limit`, and says how it ended: the session
    /// as `status` shows it once it holds credentials, or the failure the
    /// sign-in ended with, by name. A session that still signs in when the
    /// limit is reached is shown as it stands, in `signing_in`.
    ///
    /// It only watches: it starts no sign-in and asks no person.
    ///
    /// # Errors
    ///
    /// The failure the sign-in ended with: `session_expired` with
    /// `credential_refused`, `needs_login`, or `needs_mfa`, a provider's
    /// failure, or `input`/`credential_missing`; `needs_login` when the
    /// session ended without one being recorded; the store's failures.
    pub async fn signed_in(
        &self,
        session: SessionId,
        limit: Duration,
    ) -> Result<SessionStatus, Failure> {
        let inner = &self.inner;
        let mut changes = inner.changed.subscribe();
        let until = tokio::time::Instant::now() + limit;
        loop {
            let Some(row) = inner.store.get(session).await? else {
                return Err(failure::needs_login("no such session is kept"));
            };
            match row.state {
                SessionState::Valid | SessionState::Suspect | SessionState::Renewing => {
                    return Ok(status_of(row));
                }
                SessionState::SigningIn => {}
                SessionState::NeedsPerson => {
                    return Err(inner.last_failure(row.id).unwrap_or_else(|| {
                        if row.wants == Some(PersonReason::SecondFactor) {
                            failure::needs_mfa()
                        } else {
                            failure::needs_login("a person must sign in, and none has yet")
                        }
                    }));
                }
                SessionState::Absent | SessionState::Expired | SessionState::Revoked => {
                    return Err(inner.last_failure(row.id).unwrap_or_else(|| {
                        failure::needs_login("the sign-in did not end in a session")
                    }));
                }
            }
            match tokio::time::timeout_at(until, changes.changed()).await {
                Ok(Ok(())) => {}
                // The limit, or a keeper that is gone: the session as it
                // stands.
                Ok(Err(_)) | Err(_) => return Ok(status_of(row)),
            }
        }
    }

    /// A session's timeline: what happened to it, when, and why. No
    /// credential appears in it.
    ///
    /// # Errors
    ///
    /// The store's failures.
    pub async fn timeline(&self, session: SessionId) -> Result<Vec<SessionEvent>, Failure> {
        self.inner.store.events(session).await
    }

    /// Ends a session on purpose: `session.revoke` by its owner, or
    /// `session.end` by an administrator, who thereby uses nothing of it.
    /// The record is marked first, so no new lease is given; then the home
    /// runner signs out where the provider can and drops the credentials,
    /// and the saved state is deleted. Whether the caller may end this
    /// session is decided before the call, by policy.
    ///
    /// # Errors
    ///
    /// The store's failures.
    pub async fn revoke(&self, key: &SessionKey, by: EndedBy) -> Result<(), Failure> {
        let inner = &self.inner;
        let cause = match by {
            EndedBy::Owner => "revoked_by_owner",
            EndedBy::Administrator => "ended_by_administrator",
        };
        loop {
            let Some(row) = inner.store.find(key).await? else {
                return Ok(());
            };
            if matches!(row.state, SessionState::Absent | SessionState::Revoked) {
                return Ok(());
            }
            let moved = inner
                .advance(
                    &row,
                    SessionState::Revoked,
                    EventKind::Revoked,
                    cause,
                    |_| {},
                    Saved::Delete,
                )
                .await?;
            if let Some(revoked) = moved {
                // What the person allowed was for this session, which has
                // ended on purpose.
                locked(&inner.allowed).remove(&revoked.id);
                locked(&inner.drafted).remove(&revoked.id);
                let entry = inner
                    .entry_of(revoked.id)
                    .unwrap_or_else(|| Arc::new(AuthEntry::new(revoked.provider.clone())));
                let outcome = inner.perform(&revoked, &entry, Work::SignOut).await;
                if matches!(outcome, WorkOutcome::Failed(_)) {
                    tracing::warn!(
                        session = %revoked.id,
                        "the session is ended here; the sign-out on the other side did not succeed"
                    );
                }
                return Ok(());
            }
        }
    }

    /// How many calls are waiting for a session now: for one that is
    /// signing in, being checked, being renewed, or wanting a person.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.inner.waiting.load(Ordering::SeqCst)
    }

    /// Waits until no sign-in, check, or renewal that this keeper started is
    /// still in flight: from before the session's record says the work has
    /// begun until the task that does the work has ended, with everything it
    /// does after its last write: the notice to the person, the home runner
    /// dropping what it held, the metrics. For closing down in order, and
    /// for anything that reads what that work leaves behind.
    pub async fn idle(&self) {
        let mut changes = self.inner.changed.subscribe();
        while !locked(&self.inner.busy).is_empty() {
            if changes.changed().await.is_err() {
                return;
            }
        }
    }

    /// The keeper's timers. Call it at a steady pace, a few times a minute.
    /// It ends sessions that reached their declared maximum age, probes the
    /// sessions that are still wanted and due a check, ends work that was
    /// interrupted, and wakes every waiting call so that it looks at its
    /// wait. It returns when its checks are done.
    pub async fn tick(&self) {
        self.inner.tick().await;
    }
}

impl fmt::Debug for Keeper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keeper")
            .field("providers", &self.inner.providers)
            .field("settings", &self.inner.settings)
            .finish_non_exhaustive()
    }
}

/// A person's session is used only by that person or an agent acting for
/// them (decision 0035). An account's session has no person to compare with;
/// who may use it is policy's decision alone.
fn owner_only(key: &SessionKey, acting_for: Option<&PrincipalId>) -> Result<(), Failure> {
    match &key.identity {
        Identity::Person(owner) if acting_for != Some(owner) => {
            Err(failure::identity_not_allowed())
        }
        _ => Ok(()),
    }
}

fn status_of(row: SessionRow) -> SessionStatus {
    SessionStatus {
        session: row.id,
        target: row.key.target,
        environment: row.key.environment,
        identity: row.key.identity,
        provider: row.provider,
        state: row.state,
        epoch: row.epoch,
        wants: row.wants,
        signed_in_at: row.signed_in_at,
        expires_at: row.expires_at,
        checked_at: row.checked_at,
        last_used_at: row.last_used_at,
    }
}

fn session_ref(row: &SessionRow) -> SessionRef {
    SessionRef {
        session: row.id,
        key: row.key.clone(),
        epoch: row.epoch,
        home: row.home,
    }
}

impl Inner {
    fn now(&self) -> Timestamp {
        self.clock.now()
    }

    fn wake(&self) {
        self.changed
            .send_modify(|turn| *turn = turn.wrapping_add(1));
    }

    fn count(&self, name: &'static str) {
        if let Some(telemetry) = &self.telemetry {
            telemetry.count(name, &[], 1);
        }
    }

    fn entry_of(&self, session: SessionId) -> Option<Arc<AuthEntry>> {
        locked(&self.entries).get(&session).cloned()
    }

    fn remember_entry(&self, session: SessionId, entry: &Arc<AuthEntry>) {
        let mut entries = locked(&self.entries);
        let keeps_published = entries
            .get(&session)
            .is_some_and(|kept| kept.surfaces.from_published && !entry.surfaces.from_published);
        if !keeps_published {
            entries.insert(session, Arc::clone(entry));
        }
    }

    /// The addresses a run under `entry` sends the session's credentials
    /// to: those of the surfaces it covers, and those the provider itself
    /// sends to.
    fn reach(&self, entry: &AuthEntry) -> BTreeSet<String> {
        let mut reach: BTreeSet<String> = entry
            .surfaces
            .surfaces
            .iter()
            .map(|surface| surface.address.clone())
            .collect();
        if let Ok(provider) = self.providers.get(&entry.provider) {
            reach.extend(
                provider
                    .destinations(entry)
                    .into_iter()
                    .map(|sent| sent.address),
            );
        }
        reach
    }

    /// Notes the addresses `entry` sends to as allowed for the session,
    /// when a person was shown them or a published version declares them.
    fn allow(&self, session: SessionId, entry: &AuthEntry) {
        if entry.surfaces.shown_to_person || entry.surfaces.from_published {
            let reach = self.reach(entry);
            locked(&self.allowed)
                .entry(session)
                .or_default()
                .extend(reach);
        }
    }

    fn allows(&self, session: SessionId, entry: &AuthEntry) -> bool {
        if entry.surfaces.from_published {
            return true;
        }
        let reach = self.reach(entry);
        locked(&self.allowed)
            .get(&session)
            .is_some_and(|allowed| reach.is_subset(allowed))
    }

    /// Refuses a run under drafts that would send the session's
    /// credentials to an address its person did not allow.
    fn may_use(&self, row: &SessionRow, entry: &AuthEntry) -> Result<(), Failure> {
        if self.allows(row.id, entry) {
            Ok(())
        } else {
            Err(failure::not_published(
                "it would send the session's credentials to an address its person was not shown",
            ))
        }
    }

    fn mark_busy(self: &Arc<Self>, session: SessionId) -> Busy {
        *locked(&self.busy).entry(session).or_insert(0) += 1;
        Busy {
            inner: Arc::clone(self),
            session,
        }
    }

    fn is_busy(&self, session: SessionId) -> bool {
        locked(&self.busy).contains_key(&session)
    }

    fn fail(&self, session: SessionId, failure: Failure) {
        locked(&self.failures).insert(session, failure);
    }

    fn last_failure(&self, session: SessionId) -> Option<Failure> {
        locked(&self.failures).get(&session).cloned()
    }

    async fn perform(&self, row: &SessionRow, entry: &Arc<AuthEntry>, work: Work) -> WorkOutcome {
        self.work
            .perform(KeeperJob {
                session: session_ref(row),
                entry: Arc::clone(entry),
                work,
            })
            .await
    }

    async fn find_or_create(
        &self,
        key: &SessionKey,
        entry: &AuthEntry,
    ) -> Result<SessionRow, Failure> {
        if let Some(row) = self.store.find(key).await? {
            return Ok(row);
        }
        let now = self.now();
        let mut random = [0u8; 10];
        self.random.fill(&mut random)?;
        let row = SessionRow {
            id: SessionId::from_parts(now, random)?,
            key: key.clone(),
            provider: entry.provider.clone(),
            state: SessionState::Absent,
            epoch: 0,
            home: None,
            movable: entry.movable,
            profile: None,
            state_since: now,
            signed_in_at: None,
            expires_at: None,
            checked_at: None,
            last_used_at: None,
            stored_attempts: 0,
            wants: None,
        };
        let event = SessionEvent {
            session: row.id,
            at: now,
            kind: EventKind::Created,
            state: row.state,
            epoch: row.epoch,
            cause: "first_use".to_owned(),
            run: None,
            decided: None,
        };
        self.store.create(row, event).await
    }

    /// One legal move, by compare-and-set. `None` when someone else changed
    /// the session first. A session that ends gets a new epoch, so every
    /// lease of the old one is stale.
    async fn advance(
        &self,
        seen: &SessionRow,
        to: SessionState,
        kind: EventKind,
        cause: &'static str,
        edit: impl FnOnce(&mut SessionRow),
        saved: Saved,
    ) -> Result<Option<SessionRow>, Failure> {
        if !legal_move(seen.state, to) {
            return Err(Failure::new(
                FailureClass::UnknownVariant,
                SESSION_MOVE_ILLEGAL,
                format!(
                    "The session keeper was asked to move a session from `{}` to `{to}`, which is not a move it has.",
                    seen.state
                ),
                "This is a fault in the engine. Report it with the session's timeline.",
            ));
        }
        let now = self.now();
        let mut next = seen.clone();
        next.state = to;
        next.state_since = now;
        if to != SessionState::NeedsPerson {
            next.wants = None;
        }
        if matches!(
            to,
            SessionState::Absent | SessionState::Expired | SessionState::Revoked
        ) {
            next.epoch = next.epoch.saturating_add(1);
            next.expires_at = None;
        }
        edit(&mut next);
        let event = SessionEvent {
            session: next.id,
            at: now,
            kind,
            state: to,
            epoch: next.epoch,
            cause: cause.to_owned(),
            run: None,
            decided: None,
        };
        let won = self.store.advance(seen, next.clone(), event, saved).await?;
        if !won {
            return Ok(None);
        }
        tracing::info!(
            session = %next.id,
            from = seen.state.as_str(),
            to = to.as_str(),
            epoch = next.epoch,
            cause,
            "session state changed"
        );
        if to == SessionState::Valid {
            locked(&self.failures).remove(&next.id);
        }
        self.wake();
        Ok(Some(next))
    }

    /// Ends a session that could not be kept, and has the home runner drop
    /// what it holds.
    async fn expire(&self, row: &SessionRow, cause: &'static str) -> Result<(), Failure> {
        let moved = self
            .advance(
                row,
                SessionState::Expired,
                EventKind::Expired,
                cause,
                |next| {
                    // Lost state is not a refusal: the stored credential may
                    // be tried again.
                    if cause == "state_lost" {
                        next.stored_attempts = 0;
                    }
                },
                Saved::Delete,
            )
            .await?;
        if let Some(expired) = moved {
            self.forget(&expired, expired.epoch).await;
        }
        Ok(())
    }

    /// Moves a session to `needs_person` and tells the person, once.
    async fn want_person(
        &self,
        row: &SessionRow,
        reason: PersonReason,
        attempts: u32,
    ) -> Result<(), Failure> {
        let moved = self
            .advance(
                row,
                SessionState::NeedsPerson,
                EventKind::Prompted,
                reason.as_str(),
                |next| {
                    next.wants = Some(reason);
                    next.stored_attempts = attempts;
                },
                Saved::Keep,
            )
            .await?;
        if let Some(waiting) = moved {
            self.count(FORCED_SIGN_INS);
            self.work
                .ask_person(&session_ref(&waiting), PersonAsk::Notify { reason })
                .await;
        }
        Ok(())
    }

    /// A stored credential was refused: it is counted, and at the limit the
    /// session waits for a person instead of trying again.
    async fn refused(&self, row: &SessionRow, entry: &AuthEntry) -> Result<(), Failure> {
        self.fail(
            row.id,
            failure::credential_refused("the side that checks it"),
        );
        let attempts = row.stored_attempts.saturating_add(1);
        if attempts >= entry.attempt_limit {
            return self
                .want_person(row, PersonReason::CredentialRefused, attempts)
                .await;
        }
        let (to, kind) = if row.state == SessionState::SigningIn {
            (SessionState::Absent, EventKind::Abandoned)
        } else {
            (SessionState::Expired, EventKind::Expired)
        };
        self.advance(
            row,
            to,
            kind,
            "credential_refused",
            |next| next.stored_attempts = attempts,
            Saved::Delete,
        )
        .await
        .map(|_| ())
    }

    /// Records the credentials the home runner now holds, and makes the
    /// session valid at their epoch.
    async fn held(
        &self,
        row: &SessionRow,
        outcome: WorkOutcome,
        kind: EventKind,
        cause: &'static str,
        automatic: bool,
        fresh: bool,
    ) -> Result<(), Failure> {
        let WorkOutcome::Held {
            epoch,
            expires_at,
            saved,
            profile,
            confirmed,
        } = outcome
        else {
            return Ok(());
        };
        let now = self.now();
        let attempts = if confirmed || !automatic {
            0
        } else {
            row.stored_attempts.saturating_add(1)
        };
        let moved = self
            .advance(
                row,
                SessionState::Valid,
                kind,
                cause,
                |next| {
                    next.epoch = epoch;
                    next.expires_at = expires_at;
                    next.profile = profile;
                    next.checked_at = Some(now);
                    next.stored_attempts = attempts;
                    if fresh || next.signed_in_at.is_none() {
                        next.signed_in_at = Some(now);
                    }
                },
                saved.map_or(Saved::Delete, Saved::Put),
            )
            .await?;
        if moved.is_none() {
            // The session moved on while the work ran, so what the work
            // holds was never made the session's. It is dropped by the
            // epoch it was held at, which leaves newer credentials alone.
            // One case keeps it: other work made the session valid at that
            // very epoch meanwhile, and the credentials held there now are
            // the session's own.
            let in_force = self.store.get(row.id).await?.is_some_and(|now| {
                now.epoch == epoch
                    && matches!(
                        now.state,
                        SessionState::Valid | SessionState::Suspect | SessionState::Renewing
                    )
            });
            if !in_force {
                self.forget(row, epoch).await;
            }
        }
        Ok(())
    }

    /// Has the home runner drop the credentials it holds for the session at
    /// `epoch` or an earlier one.
    async fn forget(&self, row: &SessionRow, epoch: u64) {
        let entry = self
            .entry_of(row.id)
            .unwrap_or_else(|| Arc::new(AuthEntry::new(row.provider.clone())));
        let mut ended = row.clone();
        ended.epoch = epoch;
        self.perform(&ended, &entry, Work::Forget).await;
    }

    fn spawn_sign_in(
        self: &Arc<Self>,
        row: SessionRow,
        entry: Arc<AuthEntry>,
        automatic: bool,
        busy: Busy,
    ) {
        let inner = Arc::clone(self);
        tokio::spawn(async move {
            let _busy = busy;
            let outcome = inner.perform(&row, &entry, Work::SignIn).await;
            if matches!(outcome, WorkOutcome::Held { .. }) {
                let mut drafted = locked(&inner.drafted);
                if entry.surfaces.from_published {
                    drafted.remove(&row.id);
                } else {
                    drafted.insert(row.id);
                }
                drop(drafted);
                inner.allow(row.id, &entry);
            }
            if let Err(failure) = inner.signed_in(&row, &entry, automatic, outcome).await {
                inner.fail(row.id, failure);
            }
            inner.wake();
        });
    }

    async fn signed_in(
        &self,
        row: &SessionRow,
        entry: &AuthEntry,
        automatic: bool,
        outcome: WorkOutcome,
    ) -> Result<(), Failure> {
        let abandon = |cause: &'static str| {
            self.advance(
                row,
                SessionState::Absent,
                EventKind::Abandoned,
                cause,
                |_| {},
                Saved::Delete,
            )
        };
        match outcome {
            held @ WorkOutcome::Held { .. } => {
                self.held(row, held, EventKind::SignedIn, "sign_in", automatic, true)
                    .await
            }
            WorkOutcome::Refused if automatic => self.refused(row, entry).await,
            WorkOutcome::Refused => {
                self.fail(
                    row.id,
                    failure::credential_refused("the side that checks it"),
                );
                // The refused credential is counted as `refused` counts it,
                // so that no lease tries it again by itself: a lockout
                // would count that as a second failure.
                let attempts = row.stored_attempts.saturating_add(1);
                self.advance(
                    row,
                    SessionState::Absent,
                    EventKind::Abandoned,
                    "credential_refused",
                    |next| next.stored_attempts = attempts,
                    Saved::Delete,
                )
                .await
                .map(|_| ())
            }
            WorkOutcome::NeedsPerson(reason) if automatic => {
                self.want_person(row, reason, row.stored_attempts).await
            }
            WorkOutcome::NeedsPerson(reason) => {
                self.fail(
                    row.id,
                    match reason {
                        PersonReason::SecondFactor => failure::needs_mfa(),
                        _ => failure::needs_login("the sign-in was not completed"),
                    },
                );
                abandon("not_completed").await.map(|_| ())
            }
            WorkOutcome::Failed(failure) => {
                self.fail(row.id, failure);
                abandon("sign_in_failed").await.map(|_| ())
            }
            WorkOutcome::Ended => abandon("sign_in_failed").await.map(|_| ()),
        }
    }

    fn spawn_renewal(self: &Arc<Self>, row: SessionRow, entry: Arc<AuthEntry>, busy: Busy) {
        let inner = Arc::clone(self);
        tokio::spawn(async move {
            let _busy = busy;
            if let Err(failure) = inner.climb(&row, Some(entry)).await {
                inner.fail(row.id, failure);
            }
            inner.wake();
        });
    }

    /// The one re-check of a suspect session: the target's probe. A pass
    /// makes it valid again; anything else starts the one renewal.
    async fn recheck(&self, row: &SessionRow) {
        let entry = self.entry_of(row.id);
        let outcome = self.work.probe(&session_ref(row)).await;
        let now = self.now();
        let result = if outcome == ProbeOutcome::SignedIn {
            self.advance(
                row,
                SessionState::Valid,
                EventKind::Checked,
                "probe_passed",
                |next| {
                    next.checked_at = Some(now);
                    next.stored_attempts = 0;
                },
                Saved::Keep,
            )
            .await
            .map(|_| ())
        } else {
            let moved = self
                .advance(
                    row,
                    SessionState::Renewing,
                    EventKind::RenewalStarted,
                    "probe_failed",
                    |_| {},
                    Saved::Keep,
                )
                .await;
            match moved {
                Ok(Some(renewing)) => self.climb(&renewing, entry).await,
                Ok(None) => Ok(()),
                Err(failure) => Err(failure),
            }
        };
        if let Err(failure) = result {
            self.fail(row.id, failure);
        }
        self.wake();
    }

    /// The renewal ladder, for a session in `renewing`. Each rung is tried
    /// at most once, in order, and only if the provider has it.
    async fn climb(&self, row: &SessionRow, entry: Option<Arc<AuthEntry>>) -> Result<(), Failure> {
        let provider = entry
            .as_ref()
            .and_then(|entry| self.providers.get(&entry.provider).ok());
        let (Some(entry), Some(provider)) = (entry, provider) else {
            return self.expire(row, "entry_unknown").await;
        };
        // A session signed in on a drafts' word, or one whose entry is a
        // drafts', is never renewed without its person (decision 0248).
        if !entry.surfaces.from_published || locked(&self.drafted).contains(&row.id) {
            self.fail(
                row.id,
                failure::not_published("it would renew without its person"),
            );
            return self
                .want_person(row, PersonReason::SignIn, row.stored_attempts)
                .await;
        }
        let needs = provider.needs();
        let started = self.clock.elapsed();
        let mut reason = PersonReason::SignIn;

        if needs.silent_renewal {
            match self.perform(row, &entry, Work::Renew(Rung::Silent)).await {
                held @ WorkOutcome::Held { .. } => {
                    return self.renewed(row, held, started, true, false).await;
                }
                // The stored credential itself was refused: it is counted
                // and never tried a second time on a higher rung.
                WorkOutcome::Refused if !needs.person => return self.refused(row, &entry).await,
                WorkOutcome::NeedsPerson(wanted) => reason = wanted,
                WorkOutcome::Failed(failure) => {
                    self.fail(row.id, failure);
                    if !needs.person {
                        return self.expire(row, "renewal_failed").await;
                    }
                }
                WorkOutcome::Refused | WorkOutcome::Ended => {}
            }
        }

        if needs.light_prompt {
            let moved = self
                .advance(
                    row,
                    SessionState::NeedsPerson,
                    EventKind::Prompted,
                    "light_prompt",
                    |next| next.wants = Some(reason),
                    Saved::Keep,
                )
                .await?;
            let Some(waiting) = moved else {
                return Ok(());
            };
            let outcome = self
                .perform(&waiting, &entry, Work::Renew(Rung::LightPrompt))
                .await;
            return match outcome {
                held @ WorkOutcome::Held { .. } => {
                    self.renewed(&waiting, held, started, false, false).await
                }
                // The session stays in `needs_person`: the next rung is a
                // full sign-in, which the person starts.
                _ => {
                    self.count(FORCED_SIGN_INS);
                    self.work
                        .ask_person(&session_ref(&waiting), PersonAsk::Notify { reason })
                        .await;
                    Ok(())
                }
            };
        }

        if needs.person || needs.silent_renewal {
            // A full sign-in by the person. The keeper asks; it does not
            // sign in for them.
            return self.want_person(row, reason, row.stored_attempts).await;
        }

        // A stored credential with no silent rung: one full sign-in with
        // it, if the attempt limit still allows one.
        if row.stored_attempts >= entry.attempt_limit {
            self.fail(
                row.id,
                failure::credential_refused("the system it signs in to"),
            );
            return self
                .want_person(row, PersonReason::CredentialRefused, row.stored_attempts)
                .await;
        }
        match self
            .perform(row, &entry, Work::Renew(Rung::FullSignIn))
            .await
        {
            held @ WorkOutcome::Held { .. } => self.renewed(row, held, started, true, true).await,
            WorkOutcome::Refused => self.refused(row, &entry).await,
            WorkOutcome::NeedsPerson(wanted) => {
                self.want_person(row, wanted, row.stored_attempts).await
            }
            WorkOutcome::Failed(failure) => {
                self.fail(row.id, failure);
                self.expire(row, "renewal_failed").await
            }
            WorkOutcome::Ended => self.expire(row, "renewal_failed").await,
        }
    }

    async fn renewed(
        &self,
        row: &SessionRow,
        held: WorkOutcome,
        started: Duration,
        automatic: bool,
        fresh: bool,
    ) -> Result<(), Failure> {
        self.held(row, held, EventKind::Renewed, "renewal", automatic, fresh)
            .await?;
        if let Some(telemetry) = &self.telemetry {
            telemetry.count(RENEWALS, &[], 1);
            telemetry.observe(
                RENEWAL_DURATION,
                &[],
                self.clock.elapsed().saturating_sub(started),
            );
        }
        Ok(())
    }

    /// Records a lease and makes it. `None` when the session was renewed or
    /// ended after `row` was read: no lease is recorded or given for
    /// credentials that are no longer the session's, and the call looks
    /// again.
    async fn grant(
        &self,
        row: &SessionRow,
        req: &LeaseRequest,
    ) -> Result<Option<SessionLease>, Failure> {
        let now = self.now();
        let event = SessionEvent {
            session: row.id,
            at: now,
            kind: EventKind::Leased,
            state: row.state,
            epoch: row.epoch,
            cause: match req.purpose {
                Purpose::Run => "run".to_owned(),
                Purpose::Probe => "probe".to_owned(),
            },
            run: Some(req.run),
            decided: Some(req.decided),
        };
        let used = (req.purpose == Purpose::Run).then_some(now);
        if !self.store.note(used, None, false, event).await? {
            return Ok(None);
        }
        let grant = SessionGrant {
            session: row.id,
            epoch: row.epoch,
            state: row.state,
            home: row.home,
            provider: row.provider.clone(),
            granted_at: now,
            run: req.run,
            decided: req.decided,
            purpose: req.purpose,
            profile: row.profile.clone(),
        };
        Ok(Some(match &self.local {
            Some(worker) => worker.bind(grant),
            None => SessionLease::new(grant, None),
        }))
    }

    async fn lease(self: &Arc<Self>, req: LeaseRequest) -> Result<SessionLease, Failure> {
        owner_only(&req.key, req.acting_for.as_ref())?;
        let provider = self.providers.get(&req.entry.provider)?;
        checked(provider.as_ref(), &req.entry, &req.key.target)?;
        let needs = provider.needs();
        let published = req.entry.surfaces.from_published;
        let started = self.clock.elapsed();
        // Subscribed before the first read, so no change is missed.
        let mut changes = self.changed.subscribe();
        // Whether this call has already started work or waited for some. A
        // call does that once: it never starts a second sign-in or renewal.
        let mut acted = false;
        let mut restored = false;
        loop {
            let row = self.find_or_create(&req.key, &req.entry).await?;
            self.remember_entry(row.id, &req.entry);
            let now = self.now();
            let probing = req.purpose == Purpose::Probe
                && matches!(row.state, SessionState::Suspect | SessionState::Renewing);
            if probing {
                self.may_use(&row, &req.entry)?;
                match self.grant(&row, &req).await? {
                    Some(lease) => return Ok(lease),
                    None => continue,
                }
            }
            match call_rule(row.state) {
                CallRule::Run => {
                    self.may_use(&row, &req.entry)?;
                    if req.purpose == Purpose::Probe {
                        match self.grant(&row, &req).await? {
                            Some(lease) => return Ok(lease),
                            None => continue,
                        }
                    }
                    let max_age = req
                        .entry
                        .absolute
                        .zip(row.signed_in_at)
                        .map(|(absolute, signed)| (absolute, between(signed, now)));
                    if max_age.is_some_and(|(absolute, age)| age >= absolute) {
                        self.expire(&row, "max_age").await?;
                        continue;
                    }
                    let ahead = if acted {
                        Duration::ZERO
                    } else {
                        self.settings.renew_ahead
                    };
                    let due = row
                        .expires_at
                        .is_some_and(|expires| between(now, expires) <= ahead);
                    if due && !published {
                        return Err(failure::not_published(
                            "the session's credentials have run out, and it would renew without its person",
                        ));
                    }
                    if due {
                        if acted {
                            return Err(self.last_failure(row.id).unwrap_or_else(|| {
                                failure::needs_login("its credentials have run out")
                            }));
                        }
                        acted = true;
                        let busy = self.mark_busy(row.id);
                        let moved = self
                            .advance(
                                &row,
                                SessionState::Renewing,
                                EventKind::RenewalStarted,
                                "credential_expiry",
                                |_| {},
                                Saved::Keep,
                            )
                            .await?;
                        if let Some(renewing) = moved {
                            self.spawn_renewal(renewing, Arc::clone(&req.entry), busy);
                        }
                        continue;
                    }
                    if let Some(headroom) = req.headroom {
                        let by_age = max_age.map(|(absolute, age)| absolute.saturating_sub(age));
                        let by_expiry = row
                            .expires_at
                            .filter(|_| !needs.silent_renewal)
                            .map(|expires| between(now, expires));
                        let left = match (by_age, by_expiry) {
                            (Some(a), Some(b)) => Some(a.min(b)),
                            (a, b) => a.or(b),
                        };
                        if left.is_some_and(|left| left < headroom) {
                            return Err(failure::headroom_short());
                        }
                    }
                    if let Some(worker) = &self.local
                        && !worker.holds(row.id, row.epoch)
                    {
                        // The record says valid and this process holds no
                        // credentials: a restart. Saved state is opened
                        // once; without it the session is gone.
                        if restored {
                            return Err(failure::device_required());
                        }
                        restored = true;
                        self.restore(&row, &req.entry).await?;
                        continue;
                    }
                    if let Some(lease) = self.grant(&row, &req).await? {
                        return Ok(lease);
                    }
                }
                CallRule::Wait => {
                    let refused = row.wants == Some(PersonReason::CredentialRefused);
                    if row.state == SessionState::NeedsPerson && refused {
                        // Nobody is on the way: waiting would only delay.
                        return Err(self.last_failure(row.id).unwrap_or_else(|| {
                            failure::credential_refused("the side that checks it")
                        }));
                    }
                    let for_person = row.state == SessionState::NeedsPerson
                        || (row.state == SessionState::SigningIn && needs.person);
                    let limit = match req.wait {
                        Wait::Interactive(limit) => limit,
                        Wait::NoWait if for_person => Duration::ZERO,
                        Wait::NoWait => self.settings.work_limit,
                    };
                    let out_of_time = self.clock.elapsed().saturating_sub(started) >= limit;
                    if out_of_time || self.changed_or_closed(&mut changes).await {
                        return Err(if row.wants == Some(PersonReason::SecondFactor) {
                            failure::needs_mfa()
                        } else if for_person {
                            failure::needs_login("a person must sign in, and none has yet")
                        } else {
                            failure::needs_login("the wait for it ran out")
                        });
                    }
                    acted = true;
                }
                CallRule::NeedsLogin => {
                    if acted {
                        return Err(self
                            .last_failure(row.id)
                            .unwrap_or_else(|| failure::needs_login("it could not be signed in")));
                    }
                    if row.state == SessionState::Revoked {
                        return Err(failure::needs_login("it was ended on purpose"));
                    }
                    if needs.person {
                        return Err(failure::needs_login("nobody is signed in"));
                    }
                    if row.stored_attempts >= req.entry.attempt_limit {
                        return Err(failure::credential_refused("the side that checks it"));
                    }
                    if !published {
                        return Err(failure::not_published(
                            "it would sign in with a stored credential without its person",
                        ));
                    }
                    // A stored credential and no person to wait for: the
                    // keeper signs in, once.
                    acted = true;
                    let busy = self.mark_busy(row.id);
                    let moved = self
                        .advance(
                            &row,
                            SessionState::SigningIn,
                            EventKind::SignInStarted,
                            "stored_credential",
                            |next| next.provider = req.entry.provider.clone(),
                            Saved::Keep,
                        )
                        .await?;
                    if let Some(signing) = moved {
                        self.spawn_sign_in(signing, Arc::clone(&req.entry), true, busy);
                    }
                }
            }
        }
    }

    /// Waits for the next change of any session or the next tick. True when
    /// the keeper is gone and nothing will change again.
    async fn changed_or_closed(&self, changes: &mut watch::Receiver<u64>) -> bool {
        self.waiting.fetch_add(1, Ordering::SeqCst);
        let _waiting = Waiting(&self.waiting);
        changes.changed().await.is_err()
    }

    /// Opens a valid session's saved state on the home runner after a
    /// restart. A session with nothing saved, or with state that no longer
    /// opens, has ended.
    async fn restore(&self, row: &SessionRow, entry: &Arc<AuthEntry>) -> Result<(), Failure> {
        let Some(sealed) = self.store.saved(row.id).await? else {
            return self.expire(row, "state_lost").await;
        };
        match self.perform(row, entry, Work::Restore(sealed)).await {
            WorkOutcome::Held { .. } => {
                let event = SessionEvent {
                    session: row.id,
                    at: self.now(),
                    kind: EventKind::Restored,
                    state: row.state,
                    epoch: row.epoch,
                    cause: "restart".to_owned(),
                    run: None,
                    decided: None,
                };
                // The store orders the restore against every change of the
                // session: it is recorded only while the epoch is the one
                // the state was saved at. When the session was renewed or
                // ended while the restore ran, a revoke above all, what the
                // restore brought back is dropped again.
                if !self.store.note(None, None, false, event).await? {
                    self.forget(row, row.epoch).await;
                }
                Ok(())
            }
            // The key store does not answer: the state may open later.
            WorkOutcome::Failed(failure) if failure.class() == FailureClass::Busy => Err(failure),
            // The home runner knows the session ended or was renewed while
            // the restore was on its way: the call looks at the record again.
            WorkOutcome::Ended => Ok(()),
            _ => self.expire(row, "state_lost").await,
        }
    }

    async fn tick(self: &Arc<Self>) {
        let now = self.now();
        let rows = self.store.all().await.unwrap_or_default();
        for row in rows {
            if self.is_busy(row.id) {
                continue;
            }
            let stale = between(row.state_since, now) >= self.settings.work_limit;
            let result = match row.state {
                SessionState::Valid => self.tick_valid(&row, now).await,
                SessionState::SigningIn if stale => self
                    .advance(
                        &row,
                        SessionState::Absent,
                        EventKind::Abandoned,
                        "interrupted",
                        |_| {},
                        Saved::Delete,
                    )
                    .await
                    .map(|_| ()),
                SessionState::Renewing if stale => self.expire(&row, "interrupted").await,
                SessionState::Suspect if stale => {
                    let moved = self
                        .advance(
                            &row,
                            SessionState::Renewing,
                            EventKind::RenewalStarted,
                            "interrupted",
                            |_| {},
                            Saved::Keep,
                        )
                        .await;
                    match moved {
                        Ok(Some(renewing)) => self.expire(&renewing, "interrupted").await,
                        Ok(None) => Ok(()),
                        Err(failure) => Err(failure),
                    }
                }
                SessionState::NeedsPerson
                    if between(row.state_since, now) >= self.settings.keep_alive =>
                {
                    self.expire(&row, "nobody_answered").await
                }
                _ => Ok(()),
            };
            if let Err(failure) = result {
                tracing::warn!(session = %row.id, code = %failure.code(), "a session's timer could not be served");
            }
        }
        self.wake();
    }

    async fn tick_valid(self: &Arc<Self>, row: &SessionRow, now: Timestamp) -> Result<(), Failure> {
        let Some(entry) = self.entry_of(row.id) else {
            return Ok(());
        };
        let too_old = entry
            .absolute
            .zip(row.signed_in_at)
            .is_some_and(|(absolute, signed)| between(signed, now) >= absolute);
        if too_old {
            return self.expire(row, "max_age").await;
        }
        // Only while wanted: after the keep-alive window the session is no
        // longer checked, and lapses.
        let wanted = row
            .last_used_at
            .is_some_and(|used| between(used, now) < self.settings.keep_alive);
        let every = entry
            .idle
            .map_or(self.settings.probe_every, |idle| idle / 2);
        let due = row
            .checked_at
            .is_none_or(|checked| between(checked, now) >= every);
        if !wanted || !due {
            return Ok(());
        }
        let _busy = self.mark_busy(row.id);
        if self.work.probe(&session_ref(row)).await == ProbeOutcome::SignedIn {
            let event = SessionEvent {
                session: row.id,
                at: now,
                kind: EventKind::Checked,
                state: row.state,
                epoch: row.epoch,
                cause: "probe_passed".to_owned(),
                run: None,
                decided: None,
            };
            // Nothing is noted when the session was renewed or ended while
            // the probe ran: the pass was about credentials that are gone.
            return self
                .store
                .note(None, Some(now), true, event)
                .await
                .map(|_| ());
        }
        self.count(CHECKS_FAILED);
        let moved = self
            .advance(
                row,
                SessionState::Suspect,
                EventKind::Checked,
                "probe_failed",
                |_| {},
                Saved::Keep,
            )
            .await?;
        if let Some(suspect) = moved {
            self.recheck(&suspect).await;
        }
        Ok(())
    }
}
