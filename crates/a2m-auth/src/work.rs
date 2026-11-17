//! The keeper's work: what must run on a session's home runner, and the
//! half of the keeper that runs it there.
//!
//! The keeper never calls upward. A probe, a renewal, or a sign-in goes
//! through [`KeeperWork`], which the crate above implements by submitting a
//! job to the session's home runner. The runner hands the job to
//! [`KeeperWorker`], the only caller of an auth provider's `sign_in`,
//! `refresh`, and `sign_out`. In one process, [`InProcessWork`] joins the
//! two directly.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use a2m_core::{Failure, SessionId, Timestamp};
use a2m_telemetry::Clock;
use a2m_vault::{
    BoxFuture, ItemId, OsRandom, OwnerRef, RandomSource, SealContext, Sealed, SealedKind, Vault,
};

use crate::credentials::Credentials;
use crate::failure;
use crate::key::{AuthEntry, SessionKey, SessionRef};
use crate::lease::{Bound, SessionGrant, SessionLease};
use crate::material::Material;
use crate::name::ProfileRef;
use crate::person::{AskPerson, PersonAnswer, PersonAsk, PersonReason};
use crate::provider::{AuthProvider, ProviderSet, Rung, SignInContext};

/// One piece of work on a session's home runner.
#[derive(Debug, Clone)]
pub enum Work {
    /// Sign in, and hold the credentials at the next epoch.
    SignIn,
    /// Renew at one rung of the ladder, and hold the new credentials at the
    /// next epoch. [`Rung::FullSignIn`] signs in afresh.
    Renew(Rung),
    /// Sign out where the provider can, and drop the credentials held.
    SignOut,
    /// Drop the credentials held, without telling the other side.
    Forget,
    /// Open saved state and hold it at the present epoch.
    Restore(Sealed),
}

/// A job for the keeper's worker: the work, the session, and the session's
/// auth file entry. It holds names and sealed bytes, never a credential in
/// the clear.
#[derive(Debug, Clone)]
pub struct KeeperJob {
    /// The session, at the epoch of the credentials now held.
    pub session: SessionRef,
    /// The session's auth file entry.
    pub entry: Arc<AuthEntry>,
    /// What to do.
    pub work: Work,
}

/// How a piece of work ended.
#[derive(Debug)]
pub enum WorkOutcome {
    /// The home runner now holds credentials for the session.
    Held {
        /// The epoch they are held at.
        epoch: u64,
        /// When the credentials themselves run out, if they say.
        expires_at: Option<Timestamp>,
        /// The credentials, sealed under the owner's key, when the session's
        /// state may be saved.
        saved: Option<Sealed>,
        /// The engine-owned profile, when the live state is a directory.
        profile: Option<ProfileRef>,
        /// Whether the other side accepted the credential in this work.
        confirmed: bool,
    },
    /// A person is wanted, and none answered.
    NeedsPerson(PersonReason),
    /// A stored credential was refused.
    Refused,
    /// The credentials were dropped.
    Ended,
    /// The work could not be done.
    Failed(Failure),
}

/// What the target's probe showed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// The cheap signed-in read worked.
    SignedIn,
    /// The read was refused as not signed in.
    SignedOut,
    /// The read could not be made, so nothing is known.
    Unknown,
}

/// The keeper's work, run on the session's home runner. `a2m-actions`
/// implements it by submitting a job to the scheduler with the home runner
/// as a requirement, and waiting for the outcome. The keeper is handed one
/// and has no other way out.
pub trait KeeperWork: Send + Sync {
    /// Runs the target's declared probe as an ordinary read through the
    /// engine, with a lease of purpose `Probe`.
    fn probe<'a>(&'a self, session: &'a SessionRef) -> BoxFuture<'a, ProbeOutcome>;

    /// Runs a sign-in, a renewal, a sign-out, or a restore, through
    /// [`KeeperWorker::run`] on the home runner.
    fn perform(&self, job: KeeperJob) -> BoxFuture<'_, WorkOutcome>;

    /// A request to the session's own person, forwarded to the role's
    /// person surface: a link to open, a code to enter, a notice.
    fn ask_person<'a>(
        &'a self,
        session: &'a SessionRef,
        ask: PersonAsk,
    ) -> BoxFuture<'a, PersonAnswer>;
}

/// The credentials one session has on this runner.
#[derive(Clone)]
pub(crate) struct Held {
    pub(crate) epoch: u64,
    pub(crate) provider: Arc<dyn AuthProvider>,
    pub(crate) material: Arc<Material>,
}

/// The live sessions of one runner: credentials in memory, by session.
#[derive(Default)]
pub(crate) struct Live {
    table: Mutex<BTreeMap<SessionId, Held>>,
}

impl Live {
    fn table(&self) -> MutexGuard<'_, BTreeMap<SessionId, Held>> {
        self.table.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn held(&self, session: SessionId) -> Option<Held> {
        self.table().get(&session).cloned()
    }
}

/// The other half of the keeper, on the home runner. The runner calls
/// [`KeeperWorker::run`] for a job of kind `session_keepalive`. It holds the
/// credentials of the runner's live sessions, in memory only.
pub struct KeeperWorker {
    providers: ProviderSet,
    vault: Arc<Vault>,
    credentials: Arc<dyn Credentials>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn RandomSource>,
    live: Arc<Live>,
}

impl KeeperWorker {
    /// A worker over the providers, the vault, the stored credentials, and
    /// the clock.
    #[must_use]
    pub fn new(
        providers: ProviderSet,
        vault: Arc<Vault>,
        credentials: Arc<dyn Credentials>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            providers,
            vault,
            credentials,
            clock,
            random: Arc::new(OsRandom),
            live: Arc::new(Live::default()),
        }
    }

    /// The same worker, taking random bytes from `random`. For tests that
    /// need a sign-in to be the same on every run.
    #[must_use]
    pub fn with_random(mut self, random: Arc<dyn RandomSource>) -> Self {
        self.random = random;
        self
    }

    /// Whether this runner holds the session's credentials at `epoch`.
    #[must_use]
    pub fn holds(&self, session: SessionId, epoch: u64) -> bool {
        self.live
            .held(session)
            .is_some_and(|held| held.epoch == epoch)
    }

    /// Turns a grant into a lease that reaches the credentials this runner
    /// holds. The lease's `apply` fails with `lease_stale` once the
    /// credentials are replaced or dropped.
    #[must_use]
    pub fn bind(&self, grant: SessionGrant) -> SessionLease {
        SessionLease::new(
            grant,
            Some(Bound {
                live: Arc::clone(&self.live),
                vault: Arc::clone(&self.vault),
            }),
        )
    }

    /// Runs one job. `person` is the way to the session's own person, which
    /// the runner supplies.
    pub async fn run(&self, job: KeeperJob, person: &dyn AskPerson) -> WorkOutcome {
        let provider = match self.providers.get(&job.entry.provider) {
            Ok(provider) => provider,
            Err(failure) => return WorkOutcome::Failed(failure),
        };
        let session = job.session.session;
        let rung = match &job.work {
            Work::Renew(rung) => *rung,
            _ => Rung::FullSignIn,
        };
        let cx = SignInContext {
            session: &job.session,
            entry: &job.entry,
            credentials: self.credentials.as_ref(),
            vault: &self.vault,
            person,
            random: self.random.as_ref(),
            now: self.clock.now(),
            rung,
        };
        let next = job.session.epoch.saturating_add(1);
        match &job.work {
            Work::SignIn | Work::Renew(Rung::FullSignIn) => {
                let made = match provider.check(&job.entry) {
                    Ok(()) => provider.sign_in(&cx).await,
                    Err(failure) => Err(failure),
                };
                self.hold(&job, &provider, made, next).await
            }
            Work::Renew(_) => {
                let Some(held) = self.live.held(session) else {
                    return WorkOutcome::NeedsPerson(PersonReason::SignIn);
                };
                let made = provider.refresh(&held.material, &cx).await;
                self.hold(&job, &provider, made, next).await
            }
            Work::SignOut => {
                let held = self.live.table().remove(&session);
                match held {
                    Some(held) => match provider.sign_out(&held.material, &cx).await {
                        Ok(()) => WorkOutcome::Ended,
                        Err(failure) => WorkOutcome::Failed(failure),
                    },
                    None => WorkOutcome::Ended,
                }
            }
            Work::Forget => {
                self.live.table().remove(&session);
                WorkOutcome::Ended
            }
            Work::Restore(sealed) => self.restore(&job, &provider, sealed).await,
        }
    }

    /// Holds what a sign-in or a renewal made, or says why there is nothing
    /// to hold.
    async fn hold(
        &self,
        job: &KeeperJob,
        provider: &Arc<dyn AuthProvider>,
        made: Result<Material, Failure>,
        epoch: u64,
    ) -> WorkOutcome {
        let material = match made {
            Ok(material) => material,
            Err(failure) => return classify(failure),
        };
        for secret in material.secrets() {
            self.vault.register(secret);
        }
        let needs = provider.needs();
        let saved = if job.entry.movable && needs.movable {
            let sealed = match seal_context(&job.session.key, job.session.session) {
                Ok(cx) => self.vault.seal(&cx, &material.to_sealable()).await,
                Err(failure) => Err(failure),
            };
            match sealed {
                Ok(sealed) => Some(sealed),
                // The key store does not answer: nothing is held that could
                // not be saved (spec section 224).
                Err(failure) => return WorkOutcome::Failed(failure),
            }
        } else {
            None
        };
        let outcome = WorkOutcome::Held {
            epoch,
            expires_at: material.expires_at(),
            saved,
            profile: material.profile().cloned(),
            confirmed: needs.confirms,
        };
        self.live.table().insert(
            job.session.session,
            Held {
                epoch,
                provider: Arc::clone(provider),
                material: Arc::new(material),
            },
        );
        outcome
    }

    async fn restore(
        &self,
        job: &KeeperJob,
        provider: &Arc<dyn AuthProvider>,
        sealed: &Sealed,
    ) -> WorkOutcome {
        let cx = match seal_context(&job.session.key, job.session.session) {
            Ok(cx) => cx,
            Err(failure) => return WorkOutcome::Failed(failure),
        };
        let opened = match self.vault.open(&cx, sealed).await {
            Ok(opened) => opened,
            Err(failure) => return WorkOutcome::Failed(failure),
        };
        let material = match Material::from_sealable(opened.expose()) {
            Ok(material) if material.provider() == &job.entry.provider => material,
            Ok(_) => return WorkOutcome::NeedsPerson(PersonReason::SignIn),
            Err(failure) => return WorkOutcome::Failed(failure),
        };
        for secret in material.secrets() {
            self.vault.register(secret);
        }
        let outcome = WorkOutcome::Held {
            epoch: job.session.epoch,
            expires_at: material.expires_at(),
            saved: None,
            profile: material.profile().cloned(),
            confirmed: false,
        };
        self.live.table().insert(
            job.session.session,
            Held {
                epoch: job.session.epoch,
                provider: Arc::clone(provider),
                material: Arc::new(material),
            },
        );
        outcome
    }
}

impl fmt::Debug for KeeperWorker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeeperWorker")
            .field("providers", &self.providers)
            .field("live", &self.live.table().len())
            .finish_non_exhaustive()
    }
}

/// Reads a provider's failure as the keeper's ladder needs it.
fn classify(failure: Failure) -> WorkOutcome {
    let code = failure.code();
    if code == &failure::CREDENTIAL_REFUSED {
        WorkOutcome::Refused
    } else if code == &failure::NEEDS_MFA {
        WorkOutcome::NeedsPerson(PersonReason::SecondFactor)
    } else if code == &failure::NEEDS_LOGIN {
        WorkOutcome::NeedsPerson(PersonReason::SignIn)
    } else {
        WorkOutcome::Failed(failure)
    }
}

/// What a session's saved state is sealed as: the owner is the session's
/// identity, so it opens for nobody else.
fn seal_context(key: &SessionKey, session: SessionId) -> Result<SealContext, Failure> {
    Ok(SealContext {
        workspace: key.workspace,
        owner: OwnerRef::of(&key.identity),
        kind: SealedKind::SessionState,
        item: ItemId::new(session.to_string())?,
    })
}

/// Runs the target's probe for [`InProcessWork`]. The crate above supplies
/// one that runs the declared read through the engine.
pub trait RunProbe: Send + Sync {
    /// Whether the cheap signed-in read works for the session.
    fn probe<'a>(&'a self, session: &'a SessionRef) -> BoxFuture<'a, ProbeOutcome>;
}

/// [`KeeperWork`] for one process: the job goes straight to the worker, with
/// no scheduler between. For tests of this crate and of the crates above,
/// and the simplest shape of a single-machine engine.
pub struct InProcessWork {
    worker: Arc<KeeperWorker>,
    person: Arc<dyn AskPerson>,
    probe: Arc<dyn RunProbe>,
}

impl InProcessWork {
    /// Work that runs on `worker`, asks through `person`, and probes
    /// through `probe`.
    #[must_use]
    pub fn new(
        worker: Arc<KeeperWorker>,
        person: Arc<dyn AskPerson>,
        probe: Arc<dyn RunProbe>,
    ) -> Self {
        Self {
            worker,
            person,
            probe,
        }
    }
}

impl KeeperWork for InProcessWork {
    fn probe<'a>(&'a self, session: &'a SessionRef) -> BoxFuture<'a, ProbeOutcome> {
        self.probe.probe(session)
    }

    fn perform(&self, job: KeeperJob) -> BoxFuture<'_, WorkOutcome> {
        Box::pin(async move { self.worker.run(job, self.person.as_ref()).await })
    }

    fn ask_person<'a>(
        &'a self,
        session: &'a SessionRef,
        ask: PersonAsk,
    ) -> BoxFuture<'a, PersonAnswer> {
        self.person.ask(session, ask)
    }
}

impl fmt::Debug for InProcessWork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InProcessWork").finish_non_exhaustive()
    }
}
