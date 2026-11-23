//! The kernel handle: every service below, joined once.
//!
//! Opening takes two calls, because the provider registry needs the
//! redactor and telemetry before it can be built, and the kernel needs the
//! registry. [`Kernel::prepare`] opens the store, the vault, and telemetry;
//! the product crate builds the provider registry with what
//! [`Prepared::host_services`] gives; [`Kernel::open`] loads policy and
//! starts the keeper and the scheduler.
//!
//! Neither call fails for an impaired part: the kernel opens in a lower
//! health state and says why, so that a front door can always answer
//! `engine.status` and `doctor.run`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use a2m_auth::{
    InProcessWork, Keeper, KeeperSettings, KeeperWorker, ProviderSet, SessionStore,
    StoredCredentials,
};
use a2m_core::{
    Digest, EffectClass, Environment, Failure, Hazard, OperationName, Plan, PrincipalId, Redact,
    Secret, TargetName, WorkspaceId,
};
use a2m_ledger::{AuditEntry, Durability, Owner, SqlValue, Store, StoreConfig, Tx};
use a2m_policy::{ActingFor, BuiltIn, Policy, PolicyHealth, PrincipalRef};
use a2m_provider::{HostServices, RegistryProblem, Roots, Supervisor};
use a2m_sched::{SchedSettings, Scheduler};
use a2m_telemetry::{Clock, LogLevel, Telemetry, TelemetrySettings};
use a2m_vault::{KeyStore, RandomSource, Redactor, StoreHealth as KeyStoreHealth, Vault};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::codes;
use crate::dispatch::Principal;
use crate::health::{Health, Impaired, Part};
use crate::keeper::{PersonBridge, ProbeBridge};
use crate::paths::Paths;
use crate::person::{ApprovalDesk, PersonSurface};
use crate::registry::ActionRegistry;
use crate::settings::Settings;
use crate::signals::RunSignals;

/// Where the store is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorePlace {
    /// In the data directory the paths name.
    Disk,
    /// In memory, gone when the kernel closes. For tests and for a command
    /// that must leave nothing behind.
    Memory,
}

/// What the kernel is opened from. The key store, the clock, and the
/// randomness are handed in, so that a test uses ones in memory and the
/// product crate decides which key store a machine has.
pub struct KernelParts {
    /// The loaded settings.
    pub settings: Settings,
    /// The on-disk layout.
    pub paths: Paths,
    /// Where the store is kept.
    pub place: StorePlace,
    /// Where the vault's root key is kept.
    pub key_store: Arc<dyn KeyStore>,
    /// Where time comes from.
    pub clock: Arc<dyn Clock>,
    /// Where random bytes come from.
    pub random: Arc<dyn RandomSource>,
    /// The operating-system user this engine runs as. On one machine, the
    /// only caller the local channel lets in.
    pub os_user: String,
}

impl std::fmt::Debug for KernelParts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KernelParts")
            .field("place", &self.place)
            .finish_non_exhaustive()
    }
}

/// The first half of an open kernel: the store, the vault, the redactor,
/// and telemetry. The provider registry is built from it.
pub struct Prepared {
    settings: Settings,
    paths: Paths,
    clock: Arc<dyn Clock>,
    random: Arc<dyn RandomSource>,
    os_user: String,
    store: Option<Store>,
    vault: Option<Arc<Vault>>,
    redact: Arc<dyn Redact>,
    telemetry: Telemetry,
    impaired: Vec<Impaired>,
}

impl std::fmt::Debug for Prepared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prepared")
            .field("store", &self.store.is_some())
            .field("vault", &self.vault.is_some())
            .finish_non_exhaustive()
    }
}

impl Prepared {
    /// The redactor: the vault's, which knows every secret the vault
    /// released. Hand it to everything that emits text.
    #[must_use]
    pub fn redact(&self) -> Arc<dyn Redact> {
        Arc::clone(&self.redact)
    }

    /// Telemetry.
    #[must_use]
    pub fn telemetry(&self) -> Telemetry {
        self.telemetry.clone()
    }

    /// The settings the kernel was prepared with.
    #[must_use]
    pub const fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The on-disk layout.
    #[must_use]
    pub const fn paths(&self) -> &Paths {
        &self.paths
    }

    /// What the provider registry hands on to packs: the supervisor and the
    /// named roots the product crate made, with this kernel's redactor and
    /// telemetry.
    #[must_use]
    pub fn host_services(&self, supervisor: Supervisor, roots: Roots) -> HostServices {
        HostServices {
            supervisor,
            roots,
            redact: self.redact(),
            telemetry: self.telemetry(),
        }
    }
}

/// The workspace of one machine and the principal of its user. Made at the
/// first open and kept in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Installation {
    pub(crate) workspace: WorkspaceId,
    pub(crate) user: PrincipalId,
}

/// A caller's credential, as a front door took it from its transport. The
/// front door never checks one itself.
pub enum CallerCredential {
    /// The local channel's peer: the operating-system user the channel saw
    /// connect.
    LocalUser(String),
    /// A bearer token.
    Bearer(Secret),
    /// An API key.
    ApiKey(Secret),
}

impl std::fmt::Debug for CallerCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::LocalUser(_) => "CallerCredential::LocalUser",
            Self::Bearer(_) => "CallerCredential::Bearer",
            Self::ApiKey(_) => "CallerCredential::ApiKey",
        })
    }
}

/// Why the kernel is closing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    /// Nothing asked for it for the configured time.
    Idle,
    /// A person or the operating system asked.
    Asked,
    /// A newer client asked an older engine to restart.
    Restart,
}

pub(crate) struct Inner {
    pub(crate) settings: Settings,
    pub(crate) paths: Paths,
    pub(crate) clock: Arc<dyn Clock>,
    os_user: String,
    pub(crate) store: Option<Store>,
    pub(crate) vault: Option<Arc<Vault>>,
    redact: Arc<dyn Redact>,
    pub(crate) telemetry: Telemetry,
    pub(crate) policy: Option<Policy>,
    pub(crate) keeper: Option<Keeper>,
    pub(crate) credentials: Option<Arc<StoredCredentials>>,
    pub(crate) sched: Option<Scheduler>,
    pub(crate) providers: a2m_provider::Registry,
    pub(crate) actions: ActionRegistry,
    pub(crate) person: Arc<dyn PersonSurface>,
    pub(crate) installation: Option<Installation>,
    local: bool,
    health: Mutex<Health>,
    fixed: Vec<Impaired>,
    /// The protected settings whose value is not the one in force. Any
    /// one of them stops the kernel; `config.show` marks each.
    pub(crate) not_in_force: Vec<crate::bounds::NotInForce>,
    /// Where other programs keep their data on this machine: what a
    /// project folder may not be.
    pub(crate) places: crate::project::Places,
    /// The plans read from the store, by target and hash, the newest
    /// [`PLANS_KEPT`] of them.
    pub(crate) plans: Mutex<BTreeMap<(TargetName, Digest), Arc<Plan>>>,
    /// The drafted plans of trials, newest last, the newest [`DRAFTS_KEPT`].
    pub(crate) drafts: Mutex<Vec<KeptDraft>>,
    /// The trials a fixture is being recorded from, by the digest that
    /// makes each the run it is, with what each step observed, in memory
    /// alone until it is scrubbed (decision 0196).
    pub(crate) recordings: Mutex<BTreeMap<Digest, crate::fixture::Recording>>,
    pub(crate) runner: crate::runner::RunnerState,
    pub(crate) signals: Arc<RunSignals>,
    /// The person's acts that wait on a page, and what became of the last
    /// ones (decision 0163).
    pub(crate) acts: Arc<crate::acts::Acts>,
    /// Where the identifiers of those acts come from.
    pub(crate) random: Arc<dyn RandomSource>,
    /// One call at a time for each caller's idempotency key, from the
    /// look at the key to binding its run.
    pub(crate) key_locks: crate::signals::KeyLocks,
    pub(crate) wake: Arc<Notify>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

/// The kernel. Cheap to clone; every clone is the same kernel.
#[derive(Clone)]
pub struct Kernel {
    pub(crate) inner: Arc<Inner>,
}

impl std::fmt::Debug for Kernel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kernel")
            .field("health", &self.health().state)
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Kernel {
    /// Opens the store and runs its migrations, opens the vault over the
    /// key store given and joins it to the store, and starts telemetry.
    /// Never fails: a part that cannot be opened is noted, and the kernel
    /// opens in the health state that part brings.
    pub async fn prepare(parts: KernelParts) -> Prepared {
        let mut impaired = Vec::new();
        let config = match parts.place {
            StorePlace::Disk => StoreConfig::in_dir(parts.paths.store.clone()),
            StorePlace::Memory => StoreConfig::in_memory(),
        }
        .with_clock(Arc::clone(&parts.clock))
        .with_random(Arc::clone(&parts.random));
        let store = match Store::open(config).await {
            Ok(store) => Some(store),
            Err(failure) => {
                impaired.push(Impaired {
                    part: Part::Store,
                    failure,
                });
                None
            }
        };

        // The vault reads nothing until its first use. Joining it to the
        // store makes the digest key, which needs the key store to answer:
        // when it does not, the store stays without a vault, nothing can be
        // sealed, and the kernel is degraded.
        let mut vault = None;
        let mut redact: Arc<dyn Redact> = Arc::new(Redactor::new());
        if let Some(store) = &store {
            let opened = Arc::new(Vault::with_random(
                Arc::clone(&parts.key_store),
                Arc::new(store.key_records()),
                Arc::clone(&parts.random),
            ));
            redact = opened.redactor();
            match store.attach(Arc::clone(&opened)).await {
                Ok(()) => vault = Some(opened),
                Err(failure) => impaired.push(Impaired {
                    part: Part::Vault,
                    failure,
                }),
            }
        }

        let level = match parts.settings.text("logging.level") {
            Some("error") => LogLevel::Error,
            Some("warn") => LogLevel::Warn,
            Some("debug") => LogLevel::Debug,
            _ => LogLevel::Info,
        };
        let in_memory =
            || Telemetry::in_memory(level, Arc::clone(&redact), Arc::clone(&parts.clock));
        let telemetry = match parts.place {
            StorePlace::Memory => in_memory(),
            StorePlace::Disk => a2m_telemetry::start_with_clock(
                &TelemetrySettings {
                    log_level: level,
                    ..TelemetrySettings::default()
                },
                Arc::clone(&redact),
                &parts.paths.logs,
                Arc::clone(&parts.clock),
            )
            .unwrap_or_else(|_| in_memory()),
        };

        Prepared {
            settings: parts.settings,
            paths: parts.paths,
            clock: parts.clock,
            random: parts.random,
            os_user: parts.os_user,
            store,
            vault,
            redact,
            telemetry,
            impaired,
        }
    }

    /// Loads policy, starts the session keeper and the scheduler, builds
    /// the action registry over the provider registry, hands the approval
    /// desk to the person surface, and starts the task that tends them.
    /// Never fails: see the module's words on health.
    ///
    /// `problems` is what building the provider registry found; it is kept
    /// for `doctor.run` and never lowers the health state.
    pub async fn open(
        prepared: Prepared,
        providers: a2m_provider::Registry,
        problems: Vec<RegistryProblem>,
        person: Arc<dyn PersonSurface>,
    ) -> Self {
        let mut fixed = prepared.impaired;
        for problem in problems {
            fixed.push(Impaired {
                part: Part::Providers,
                failure: problem.failure,
            });
        }
        let actions = match ActionRegistry::build(&providers) {
            Ok(actions) => actions,
            Err(failure) => {
                fixed.push(Impaired {
                    part: Part::Providers,
                    failure,
                });
                ActionRegistry::kernel_only()
            }
        };
        let local = matches!(
            prepared.settings.text("profile.name"),
            Some("solo" | "workstation" | "embedded") | None
        );

        // What a scout's compare stage copied into the cache, left by an
        // engine that stopped before it removed it: no scout runs before
        // this one opens, so all of it goes (audit finding A5).
        let scratch = prepared.paths.cache.join(crate::handlers::SCOUT_SCRATCH);
        let _ = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(scratch)).await;
        let signals = Arc::new(RunSignals::default());
        let acts = crate::acts::Acts::new();
        let mut installation = None;
        let mut policy = None;
        let mut keeper = None;
        let mut credentials = None;
        let mut sched = None;
        if let Some(store) = &prepared.store {
            match installation_of(store).await {
                Ok(found) => installation = Some(found),
                Err(failure) => fixed.push(Impaired {
                    part: Part::Store,
                    failure,
                }),
            }
        }
        let mut not_in_force = Vec::new();
        if let (Some(store), Some(found)) = (&prepared.store, installation) {
            // What the policy engine is opened with: the built-in set of
            // the profile, with the installation's policy settings carried
            // into it. The policy engine keeps them in force under any set
            // published later; a value it cannot work by says what is not
            // in force, and the kernel stops with its name (see `bounds`).
            let built_in = if local {
                BuiltIn::local(found.user, found.workspace)
            } else {
                BuiltIn::server()
            };
            let (built_in, refused) = crate::bounds::carried(&prepared.settings, built_in);
            not_in_force = refused;
            for refused in &not_in_force {
                fixed.push(Impaired {
                    part: Part::Policy,
                    failure: refused.failure.clone(),
                });
            }
            match Policy::open(store, built_in).await {
                Ok((opened, surface)) => {
                    // The one handle that can record an approval goes to the
                    // person surface, and nowhere else.
                    person.take_desk(ApprovalDesk::new(
                        surface,
                        store.clone(),
                        found.workspace,
                        Arc::clone(&signals),
                        Arc::clone(&prepared.redact),
                        Arc::clone(&acts),
                    ));
                    policy = Some(opened);
                }
                Err(failure) => fixed.push(Impaired {
                    part: Part::Policy,
                    failure,
                }),
            }

            let settings = sched_settings(&prepared.settings);
            sched = Some(Scheduler::new(
                store.clone(),
                settings,
                Some(prepared.telemetry.clone()),
            ));

            if let Some(vault) = &prepared.vault {
                let mut set = ProviderSet::neutral();
                for provider in providers.auth_providers() {
                    match set.with(provider) {
                        Ok(with) => set = with,
                        Err(failure) => {
                            fixed.push(Impaired {
                                part: Part::Providers,
                                failure,
                            });
                            set = ProviderSet::neutral();
                            break;
                        }
                    }
                }
                let sessions = SessionStore::new(store.clone());
                let stored = Arc::new(StoredCredentials::new(sessions.clone(), Arc::clone(vault)));
                let held = Arc::new(
                    KeeperWorker::new(
                        set.clone(),
                        Arc::clone(vault),
                        Arc::clone(&stored) as Arc<dyn a2m_auth::Credentials>,
                        Arc::clone(&prepared.clock),
                    )
                    .with_random(Arc::clone(&prepared.random)),
                );
                let probe = Arc::new(ProbeBridge);
                let work = Arc::new(InProcessWork::new(
                    Arc::clone(&held),
                    Arc::new(PersonBridge::new(Arc::clone(&person))),
                    Arc::clone(&probe) as Arc<dyn a2m_auth::RunProbe>,
                ));
                let hours = prepared
                    .settings
                    .int("sessions.keep_alive_hours")
                    .unwrap_or(8);
                let minutes = prepared
                    .settings
                    .int("sessions.probe_minutes")
                    .unwrap_or(15);
                let keeper_settings = KeeperSettings {
                    keep_alive: seconds(hours.saturating_mul(3600)),
                    probe_every: seconds(minutes.saturating_mul(60)),
                    ..KeeperSettings::default()
                };
                keeper = Some(
                    Keeper::builder(sessions, set, work, Arc::clone(&prepared.clock))
                        .local_worker(Arc::clone(&held))
                        .settings(keeper_settings)
                        .telemetry(prepared.telemetry.clone())
                        .random(Arc::clone(&prepared.random))
                        .build(),
                );
                credentials = Some(stored);
            }
        }

        let kernel = Self {
            inner: Arc::new(Inner {
                settings: prepared.settings,
                paths: prepared.paths,
                clock: prepared.clock,
                os_user: prepared.os_user,
                store: prepared.store,
                vault: prepared.vault,
                redact: prepared.redact,
                telemetry: prepared.telemetry,
                policy,
                keeper,
                credentials,
                sched,
                providers,
                actions,
                person,
                installation,
                local,
                health: Mutex::new(Health::of(Vec::new())),
                fixed,
                not_in_force,
                places: crate::project::Places::of_process(),
                plans: Mutex::new(BTreeMap::new()),
                drafts: Mutex::new(Vec::new()),
                recordings: Mutex::new(BTreeMap::new()),
                runner: crate::runner::RunnerState::default(),
                signals,
                acts: Arc::clone(&acts),
                random: Arc::clone(&prepared.random),
                key_locks: crate::signals::KeyLocks::default(),
                wake: Arc::new(Notify::new()),
                tasks: Mutex::new(Vec::new()),
            }),
        };
        acts.bind(&kernel.inner);
        kernel.recheck().await;
        kernel.record_start().await;
        kernel.start_tending();
        kernel
    }

    /// The health state, and every impaired part.
    #[must_use]
    pub fn health(&self) -> Health {
        lock(&self.inner.health).clone()
    }

    /// Looks at every part again and sets the health state. A move to a
    /// lower state is written to the audit log.
    pub async fn recheck(&self) -> Health {
        let inner = &self.inner;
        let mut reasons = inner.fixed.clone();
        let has =
            |reasons: &[Impaired], part: Part| reasons.iter().any(|reason| reason.part == part);
        if let Some(store) = &inner.store {
            let health = store.health().await;
            if !health.can_commit && !has(&reasons, Part::Store) {
                reasons.push(Impaired {
                    part: Part::Store,
                    failure: codes::busy(
                        a2m_ledger::codes::STORE_UNAVAILABLE,
                        "The store cannot commit.",
                        "Free disk space or restore the database, then start the engine again.",
                    ),
                });
            }
            if let Some(policy) = &inner.policy {
                let policy = policy.clone();
                let loaded = store
                    .read(move |tx| Ok(policy.health(tx)))
                    .await
                    .unwrap_or(PolicyHealth::NoValidSet);
                if loaded == PolicyHealth::NoValidSet && !has(&reasons, Part::Policy) {
                    reasons.push(Impaired {
                        part: Part::Policy,
                        failure: codes::busy(
                            codes::ENGINE_DEGRADED,
                            "No valid policy set is stored, so every decision is a denial.",
                            "Publish a policy set whose tests pass.",
                        ),
                    });
                }
            } else if !has(&reasons, Part::Policy) && !has(&reasons, Part::Store) {
                reasons.push(Impaired {
                    part: Part::Policy,
                    failure: codes::busy(
                        codes::ENGINE_DEGRADED,
                        "The policy engine is not open.",
                        "Call `doctor.run` for the cause.",
                    ),
                });
            }
        }
        if let Some(vault) = &inner.vault {
            let health = vault.health().await;
            if matches!(health.store_health, KeyStoreHealth::Unreachable)
                && !has(&reasons, Part::Vault)
            {
                // A passphrase route that could not be taken was given no
                // passphrase: there is no key store to unlock (decision
                // 0220).
                let failure = if health.store.strength == a2m_vault::StoreStrength::Passphrase {
                    a2m_vault::PassphraseKeyStore::not_given(crate::paths::PASSPHRASE_VARIABLE)
                } else {
                    codes::busy(
                        a2m_vault::codes::KEY_STORE_UNREACHABLE,
                        format!("The key store `{}` does not answer.", health.store.name),
                        "Unlock or start the key store; the engine notices by itself when it returns.",
                    )
                };
                reasons.push(Impaired {
                    part: Part::Vault,
                    failure,
                });
            }
        }
        let health = Health::of(reasons);
        let before = {
            let mut held = lock(&inner.health);
            std::mem::replace(&mut *held, health.clone())
        };
        if health.state > before.state {
            // See `audit`: nobody is told here when it cannot be written.
            let _ = self
                .audit(
                    "engine",
                    "health.lowered",
                    health.state.as_str(),
                    &health.cause(),
                )
                .await;
        }
        health
    }

    /// Writes one entry to the audit log, when there is a store to write
    /// it to. The detail passes the redactor inside the store.
    ///
    /// An action that changed something tells its caller when its entry
    /// could not be written. The engine's own entries, a lowered health
    /// state, changed settings, a close, have nobody to tell: an entry
    /// that cannot be appended means the store cannot commit, which the
    /// next look at the health state finds and reports.
    ///
    /// # Errors
    ///
    /// The store's failures.
    pub(crate) async fn audit(
        &self,
        actor: &str,
        action: &str,
        subject: &str,
        detail: &str,
    ) -> Result<(), Failure> {
        let (Some(store), Some(installation)) = (&self.inner.store, self.inner.installation) else {
            return Ok(());
        };
        let entry = AuditEntry {
            workspace: installation.workspace,
            actor: actor.to_owned(),
            action: action.to_owned(),
            subject: subject.to_owned(),
            detail: detail.to_owned(),
        };
        store
            .write(Durability::Synced, move |tx| {
                tx.audit().append(&entry).map(|_| ())
            })
            .await
    }

    /// Records the digest of the effective settings and of the catalog at
    /// this start, and writes an audit entry when the settings differ from
    /// the last start's.
    async fn record_start(&self) {
        let Some(store) = &self.inner.store else {
            return;
        };
        let settings = self.inner.settings.digest().to_string();
        let catalog = self.inner.actions.catalog().digest;
        let now = settings.clone();
        let changed = store
            .write(Durability::Synced, move |tx| {
                let before = fact(tx, "settings_digest")?;
                set_fact(tx, "settings_digest", &now)?;
                set_fact(tx, "catalog_digest", &catalog)?;
                Ok(before.is_some_and(|before| before != now))
            })
            .await
            .unwrap_or(false);
        if changed {
            // See `audit`: nobody is told here when it cannot be written.
            let _ = self
                .audit("engine", "settings.changed", "settings", &settings)
                .await;
        }
    }

    /// The one place a caller's credential is checked. On one machine the
    /// only caller is the operating-system user the engine runs as, whom
    /// the local channel saw connect; it holds every role, and an agent it
    /// starts acts for it. Bearer tokens and API keys belong to a server's
    /// front doors, which are not built: they are refused.
    ///
    /// # Errors
    ///
    /// `policy`/`caller_not_verified`.
    pub async fn verify_caller(&self, credential: CallerCredential) -> Result<Principal, Failure> {
        let refused = |why: &str| {
            codes::policy(
                codes::CALLER_NOT_VERIFIED,
                format!("The caller was not let in: {why}."),
                "Call from the account that runs the engine.",
            )
        };
        match credential {
            CallerCredential::LocalUser(user) => {
                if !self.inner.local {
                    return Err(refused("this engine is a server and takes no local caller"));
                }
                if user != self.inner.os_user {
                    return Err(refused("the local channel is this engine's user's alone"));
                }
                let installation = self.inner.installation.ok_or_else(|| {
                    codes::busy(
                        codes::ENGINE_DEGRADED,
                        "The store is not open, so the caller cannot be named.",
                        "Call `doctor.run` for the fix.",
                    )
                })?;
                Ok(Principal {
                    principal: PrincipalRef::person(installation.user),
                    workspace: installation.workspace,
                    acting_for: Some(ActingFor::LocalUser(installation.user)),
                })
            }
            CallerCredential::Bearer(_) | CallerCredential::ApiKey(_) => Err(refused(
                "bearer tokens and API keys are checked on a server, which this build does not serve",
            )),
        }
    }

    /// The caller a stopped engine answers status to, when the store never
    /// opened and so nobody can be named: nobody. It is a principal of no
    /// installation, which holds no role and no grant under any policy
    /// set, so it is served only what a stopped kernel serves without a
    /// decision: the engine's own status. A caller is named, with what it
    /// may do, by [`Kernel::verify_caller`] and by nothing else.
    #[must_use]
    pub fn status_only_caller(&self) -> Principal {
        Principal {
            principal: PrincipalRef::person(PrincipalId::from_bytes([0; 16])),
            workspace: WorkspaceId::from_bytes([0; 16]),
            acting_for: None,
        }
    }

    /// Whether this engine serves one machine: its only caller is the user
    /// it runs as.
    pub(crate) fn is_local(&self) -> bool {
        self.inner.local
    }

    /// The redactor. Everything that leaves a front door passes it.
    #[must_use]
    pub fn redact(&self) -> Arc<dyn Redact> {
        Arc::clone(&self.inner.redact)
    }

    /// Telemetry.
    #[must_use]
    pub fn telemetry(&self) -> &Telemetry {
        &self.inner.telemetry
    }

    /// The registry of actions, for generating faces.
    #[must_use]
    pub fn actions(&self) -> &ActionRegistry {
        &self.inner.actions
    }

    /// The provider registry.
    #[must_use]
    pub fn providers(&self) -> &a2m_provider::Registry {
        &self.inner.providers
    }

    /// The effective settings.
    #[must_use]
    pub fn settings(&self) -> &Settings {
        &self.inner.settings
    }

    /// The on-disk layout.
    #[must_use]
    pub fn paths(&self) -> &Paths {
        &self.inner.paths
    }

    pub(crate) fn person(&self) -> &Arc<dyn PersonSurface> {
        &self.inner.person
    }

    pub(crate) fn policy(&self) -> Option<&Policy> {
        self.inner.policy.as_ref()
    }

    /// The store, or the refusal of a stopped kernel.
    pub(crate) fn store(&self) -> Result<&Store, Failure> {
        self.inner.store.as_ref().ok_or_else(|| {
            codes::busy(
                codes::ENGINE_DEGRADED,
                "The store is not open.",
                "Call `doctor.run` for the fix.",
            )
        })
    }

    pub(crate) fn installation(&self) -> Result<Installation, Failure> {
        self.inner.installation.ok_or_else(|| {
            codes::busy(
                codes::ENGINE_DEGRADED,
                "The store is not open.",
                "Call `doctor.run` for the fix.",
            )
        })
    }

    pub(crate) fn sched(&self) -> Result<&Scheduler, Failure> {
        self.inner.sched.as_ref().ok_or_else(|| {
            codes::busy(
                codes::ENGINE_DEGRADED,
                "The scheduler is not running.",
                "Call `doctor.run` for the fix.",
            )
        })
    }

    /// Registers a secret a person handed over with the redactor, so that
    /// it is masked wherever it would appear. With no vault open nothing
    /// can be sealed either, and the secret is used for nothing.
    pub(crate) fn register_secret(&self, secret: &a2m_core::Secret) {
        if let Some(vault) = &self.inner.vault {
            vault.register(secret);
        }
    }

    pub(crate) fn keeper(&self) -> Result<&Keeper, Failure> {
        self.inner.keeper.as_ref().ok_or_else(|| {
            codes::busy(
                codes::ENGINE_DEGRADED,
                "The session keeper is not running, because the vault is not open.",
                "Call `doctor.run` for the fix.",
            )
        })
    }

    /// The environment an action that names none is decided in.
    /// The toolset a decision names for a call that came through `named`
    /// (decision 0399). A toolset the set in force defines is named as it
    /// is, and narrows. One that the engine's own doors serve (`use`,
    /// `build`, `admin`) and the set does not define is left out: it is a
    /// list of what is shown up front and holds no permit, so a set that
    /// begins to define toolsets does not stop the built-in ones. Any other
    /// name is named as it is, and a set that defines toolsets refuses what
    /// it does not define (unknown means stop). When the set in force
    /// cannot be read, the name is kept, which narrows and never widens.
    pub(crate) async fn toolset_for_decision(
        &self,
        named: Option<&a2m_policy::ToolsetName>,
    ) -> Option<a2m_policy::ToolsetName> {
        let named = named?;
        if !matches!(named.as_str(), "use" | "build" | "admin") {
            return Some(named.clone());
        }
        let (Some(policy), Ok(store)) = (self.policy().cloned(), self.store()) else {
            return Some(named.clone());
        };
        let wanted = named.clone();
        let defined = store
            .read(move |tx| {
                Ok(policy
                    .layers_in_force(tx)
                    .is_none_or(|layers| layers.toolsets.iter().any(|set| set.name == wanted)))
            })
            .await
            .unwrap_or(true);
        defined.then(|| named.clone())
    }

    pub(crate) fn environment(&self) -> Environment {
        Environment::fixed("production")
    }

    /// The effect class and hazard of a published tool, for the decision
    /// about `tool.call`. `None` when no such tool is published; the
    /// handler then answers that it was not found.
    pub(crate) async fn tool_effect(&self, tool: &str) -> Option<(EffectClass, Hazard)> {
        let name = OperationName::new(tool).ok()?;
        let found = self.find_tool(&name).await.ok()?;
        Some(found.effect())
    }

    /// One pass of tending: the scheduler's sweep, the keeper's tick, and a
    /// fresh look at the health state. The kernel's own task calls it
    /// whenever a deadline of the scheduler comes due; a test that moves
    /// its clock by hand calls it itself.
    pub async fn tend(&self) {
        if let Some(sched) = &self.inner.sched {
            let _ = sched.sweep().await;
            // A sweep may have ended a job or stopped a run: whoever waits
            // for one looks again.
            self.run_changed();
        }
        // The watches: ends, due checks, checks whose run ended, and
        // watches waiting for a sign-in (decision 0283).
        self.tend_watches().await;
        if let Some(keeper) = &self.inner.keeper {
            keeper.tick().await;
        }
        self.recheck().await;
    }

    fn start_tending(&self) {
        let Some(sched) = self.inner.sched.clone() else {
            return;
        };
        let weak = Arc::downgrade(&self.inner);
        let clock = Arc::clone(&self.inner.clock);
        let wake = Arc::clone(&self.inner.wake);
        let probe_every = self
            .inner
            .settings
            .int("sessions.probe_minutes")
            .map_or(seconds(900), |minutes| seconds(minutes.saturating_mul(60)));
        let task = tokio::spawn(async move {
            let mut due = sched.due_watch();
            loop {
                // Wait for the scheduler's next deadline, for a deadline to
                // be added or removed, or for the keeper's next round.
                // The earlier of the scheduler's next deadline and the
                // watches' next due time or end (decision 0262).
                let next = match (sched.next_due().await, sched.next_watch_due().await) {
                    (Ok(Some(due)), Ok(Some(watch))) => Ok(Some(due.min(watch))),
                    (Ok(None), Ok(watch)) => Ok(watch),
                    (Ok(due), _) => Ok(due),
                    (Err(failure), _) => Err(failure),
                };
                let wait = match next {
                    Ok(Some(at)) => {
                        let ahead = at
                            .as_unix_millis()
                            .saturating_sub(clock.now().as_unix_millis());
                        std::time::Duration::from_millis(u64::try_from(ahead).unwrap_or(0))
                            .min(probe_every)
                    }
                    _ => probe_every,
                };
                tokio::select! {
                    alive = due.changed() => {
                        if !alive {
                            break;
                        }
                        continue;
                    }
                    () = wake.notified() => {}
                    () = tokio::time::sleep(wait) => {}
                }
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                Self { inner }.tend().await;
            }
        });
        lock(&self.inner.tasks).push(task);
    }

    /// Stops the runner loop and the tending task, stops every subsystem
    /// the supervisor holds, and writes the reason to the audit log.
    /// Sessions' saved state is in the store already: the keeper saves it
    /// whenever credentials change.
    pub async fn close(self, why: CloseReason) {
        self.stop_runner_loop().await;
        let tasks: Vec<JoinHandle<()>> = lock(&self.inner.tasks).drain(..).collect();
        for task in &tasks {
            task.abort();
        }
        for task in tasks {
            let _ = task.await;
        }
        if let Some(keeper) = &self.inner.keeper {
            keeper.idle().await;
        }
        // See `audit`: nobody is told here when it cannot be written.
        let _ = self
            .audit(
                "engine",
                "engine.closed",
                "engine",
                match why {
                    CloseReason::Idle => "idle",
                    CloseReason::Asked => "asked",
                    CloseReason::Restart => "restart",
                },
            )
            .await;
        // The engine is closed: the store's lock on its data folder goes,
        // though a clone of the store may outlive this kernel for a while.
        if let Some(store) = &self.inner.store {
            store.release_folder();
        }
    }
}

/// How many plans read from the store are kept in memory.
pub(crate) const PLANS_KEPT: usize = 64;
/// A drafted plan kept for a trial: its target and hash, and the plan.
pub(crate) type KeptDraft = ((TargetName, Digest), Arc<Plan>);
/// How many drafted plans of trials are kept in memory. A trial whose plan
/// was let go since cannot be resumed; it is tried again.
pub(crate) const DRAFTS_KEPT: usize = 16;

/// The shortest lease the in-process runner works under. The engine sends
/// a save only when the lease leaves time for the whole binding, each step
/// at its time limit, and a step that states none counts as the engine's
/// default of thirty seconds. Ten minutes leaves room, between two
/// heartbeats, for a binding of nineteen such steps, or for one step at the
/// longest limit a target may declare by default. A tool that needs more
/// stops with `busy`/`lease_too_short`, which names the setting to raise.
const IN_PROCESS_LEASE_FLOOR: std::time::Duration = std::time::Duration::from_secs(600);

fn seconds(count: i64) -> std::time::Duration {
    std::time::Duration::from_secs(u64::try_from(count).unwrap_or(0))
}

fn sched_settings(settings: &Settings) -> SchedSettings {
    let mut out = SchedSettings::default();
    if let Some(length) = settings.int("scheduler.queue_length_per_target") {
        out.queue_length = u32::try_from(length).unwrap_or(out.queue_length);
    }
    if let Some(lease) = settings.int("runners.lease_seconds") {
        out.lease_length = seconds(lease);
    }
    // The engine sends a committing step only when the lease leaves time
    // for the whole binding: its act, its confirm, and its read-back. A
    // lease shorter than that would hand the job out for ever. In one
    // process a lost runner is a lost process, found at the next start, so
    // a longer lease costs nothing: the setting is a floor here.
    out.lease_length = out.lease_length.max(IN_PROCESS_LEASE_FLOOR);
    if let Some(heartbeat) = settings.int("runners.heartbeat_seconds") {
        out.heartbeat_interval = seconds(heartbeat);
    }
    if let Some(quarantine) = settings.int("runners.quarantine_seconds") {
        out.quarantine = seconds(quarantine);
    }
    // The limits on watches (decision 0283). Each key is protected, and the
    // scheduler and the ledger hold their own floor and ceiling whatever
    // the setting says.
    let count = |key: &str| {
        settings
            .int(key)
            .and_then(|value| u32::try_from(value).ok())
    };
    if let Some(shortest) = settings.int("watch.shortest_interval_seconds") {
        out.watch.shortest_interval = seconds(shortest);
    }
    if let Some(days) = settings.int("watch.longest_days") {
        out.watch.longest = seconds(days.saturating_mul(86_400));
    }
    if let Some(most) = count("watch.most") {
        out.watch.most_per_person = most;
    }
    if let Some(share) = count("watch.budget_share_percent") {
        out.watch.budget_share_percent = share;
    }
    if let Some(reserve) = count("watch.reserve_percent") {
        out.watch.reserve_percent = reserve;
    }
    out
}

/// One fact of the installation.
pub(crate) fn fact(tx: &mut Tx<'_>, name: &str) -> Result<Option<String>, Failure> {
    let row = tx.tables(Owner::Actions).query_first(
        "SELECT value FROM actions_installation WHERE name = ?1",
        &[SqlValue::from(name)],
    )?;
    row.map(|row| row.text(0).map(str::to_owned)).transpose()
}

pub(crate) fn set_fact(tx: &mut Tx<'_>, name: &str, value: &str) -> Result<(), Failure> {
    let now = tx.now();
    tx.tables(Owner::Actions).execute(
        "INSERT INTO actions_installation (name, value, set_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (name) DO UPDATE SET value = excluded.value, set_at = excluded.set_at",
        &[
            SqlValue::from(name),
            SqlValue::from(value),
            SqlValue::from(now),
        ],
    )?;
    Ok(())
}

/// The workspace and the user of this installation: read, or made at the
/// first open.
async fn installation_of(store: &Store) -> Result<Installation, Failure> {
    store
        .write(Durability::Synced, |tx| {
            let workspace = match fact(tx, "workspace")? {
                Some(text) => text.parse::<WorkspaceId>()?,
                None => {
                    let made = WorkspaceId::from_parts(tx.now(), tx.random()?)?;
                    set_fact(tx, "workspace", &made.to_string())?;
                    made
                }
            };
            let user = match fact(tx, "local_user")? {
                Some(text) => text.parse::<PrincipalId>()?,
                None => {
                    let made = PrincipalId::from_parts(tx.now(), tx.random()?)?;
                    set_fact(tx, "local_user", &made.to_string())?;
                    made
                }
            };
            Ok(Installation { workspace, user })
        })
        .await
}

/// The targets of a workspace that have a published version.
pub(crate) fn published_targets(
    tx: &mut Tx<'_>,
    workspace: &WorkspaceId,
) -> Result<Vec<TargetName>, Failure> {
    let rows = tx.tables(Owner::Actions).query(
        "SELECT target FROM actions_targets WHERE workspace_id = ?1 ORDER BY target",
        &[SqlValue::from(workspace.to_string())],
    )?;
    rows.iter()
        .map(|row| Ok(TargetName::new(row.text(0)?)?))
        .collect()
}

pub(crate) fn note_published(
    tx: &mut Tx<'_>,
    workspace: &WorkspaceId,
    target: &TargetName,
) -> Result<(), Failure> {
    let now = tx.now();
    tx.tables(Owner::Actions).execute(
        "INSERT INTO actions_targets (workspace_id, target, first_published_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (workspace_id, target) DO NOTHING",
        &[
            SqlValue::from(workspace.to_string()),
            SqlValue::from(target.as_str()),
            SqlValue::from(now),
        ],
    )?;
    Ok(())
}
