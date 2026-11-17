//! The in-process runner loop: claims jobs from the scheduler and hands
//! them to the engine with a `Control` that answers in this process.
//!
//! The lease rules of the runner protocol apply unchanged on one machine:
//! the lease is counted on a steady clock, a task per held job sends its
//! heartbeats, and the engine stops acting before the lease runs out.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use a2m_core::{
    DecisionId, Failure, Identity, Mode, PrincipalId, RunId, RunState, RunnerId, Values,
};
use a2m_engine::{
    Conditions, Directive, Engine, EngineDeps, EngineSettings, JobKind, JobOutcome, JobSpec,
    KernelHealth, LeaseClock, Parallelism, RecordSnapshot, RunSnapshot, ToolRef, decode_values,
};
use a2m_ledger::{BlobRef, Durability, EventKind, Owner, RecordRef, RunRow, SqlValue, Tx};
use a2m_policy::{ActingFor, GroupName, PrincipalKind, PrincipalRef, ToolsetName};
use a2m_sched::{BudgetKey, JobEnd, JobLease, RunnerOffer, RunnerTerms, Slots};
use a2m_telemetry::CaptureLevel;
use serde_json::{Value as Json, json};
use tokio::sync::{Semaphore, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::codes;
use crate::control::{InProcessControl, may_have_written};
use crate::dispatch::Caller;
use crate::health::HealthState;
use crate::kernel::{Kernel, fact, set_fact};

/// How long one claim waits for work before it asks again.
const CLAIM_WAIT: Duration = Duration::from_secs(30);

/// The verified facts about the caller of a run, kept so that the guard's
/// question to policy is asked about the caller that started the run. Names
/// and flags; never a credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CallerFacts {
    pub(crate) principal: PrincipalRef,
    pub(crate) is_agent: bool,
    pub(crate) acting_for: Option<ActingFor>,
    pub(crate) toolset: Option<ToolsetName>,
    /// The decision that let the call in.
    pub(crate) decision: DecisionId,
}

impl CallerFacts {
    pub(crate) fn of(caller: &Caller, decision: DecisionId) -> Self {
        Self {
            principal: caller.principal.principal.clone(),
            is_agent: caller.is_agent,
            acting_for: caller.principal.acting_for.clone(),
            toolset: caller.toolset.clone(),
            decision,
        }
    }

    /// The person the run acts for: the caller themself, or the person the
    /// calling agent was proved to act for.
    pub(crate) fn person(&self) -> Option<PrincipalId> {
        match &self.acting_for {
            Some(ActingFor::LocalUser(person) | ActingFor::TokenSubject(person)) => Some(*person),
            Some(ActingFor::Delegation(_)) => None,
            None => (self.principal.kind == PrincipalKind::Person).then_some(self.principal.id),
        }
    }

    pub(crate) fn to_json(&self) -> Json {
        let (how, person) = match &self.acting_for {
            Some(ActingFor::LocalUser(person)) => ("local_user", Some(person.to_string())),
            Some(ActingFor::TokenSubject(person)) => ("token_subject", Some(person.to_string())),
            Some(ActingFor::Delegation(person)) => ("delegation", Some(person.to_string())),
            None => ("", None),
        };
        json!({
            "principal": self.principal.id.to_string(),
            "kind": self.principal.kind.as_str(),
            "groups": self.principal.groups.iter().map(GroupName::as_str).collect::<Vec<_>>(),
            "is_agent": self.is_agent,
            "acting_how": how,
            "acting_for": person,
            "toolset": self.toolset.as_ref().map(ToolsetName::as_str),
            "decision": self.decision.to_string(),
        })
    }

    pub(crate) fn from_json(text: &str) -> Option<Self> {
        let json: Json = serde_json::from_str(text).ok()?;
        let person = json["acting_for"]
            .as_str()
            .and_then(|text| text.parse().ok());
        let acting_for = match (json["acting_how"].as_str()?, person) {
            ("local_user", Some(person)) => Some(ActingFor::LocalUser(person)),
            ("token_subject", Some(person)) => Some(ActingFor::TokenSubject(person)),
            ("delegation", Some(person)) => Some(ActingFor::Delegation(person)),
            _ => None,
        };
        Some(Self {
            principal: PrincipalRef {
                id: json["principal"].as_str()?.parse().ok()?,
                kind: PrincipalKind::from_text(json["kind"].as_str()?)?,
                groups: json["groups"]
                    .as_array()?
                    .iter()
                    .filter_map(|group| GroupName::new(group.as_str()?).ok())
                    .collect(),
            },
            is_agent: json["is_agent"].as_bool()?,
            acting_for,
            toolset: json["toolset"]
                .as_str()
                .and_then(|name| ToolsetName::new(name).ok()),
            decision: json["decision"].as_str()?.parse().ok()?,
        })
    }
}

/// What a run was called with.
pub(crate) struct RunCall {
    pub(crate) input: BlobRef,
    pub(crate) caller: CallerFacts,
    /// The stored bytes of the source the run reads, when it reads one.
    pub(crate) source: Option<BlobRef>,
}

/// How the source of a run was handed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceKind {
    /// A file of the project, read once by the dispatcher.
    File,
    /// Rows the caller gave inline.
    Rows,
}

impl SourceKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Rows => "rows",
        }
    }
}

/// The source a run is called with: its stored bytes, and where they came
/// from.
#[derive(Debug, Clone)]
pub(crate) struct KeptSource {
    pub(crate) blob: BlobRef,
    pub(crate) kind: SourceKind,
    /// The path inside the project, as the caller wrote it; empty for rows.
    pub(crate) name: String,
    pub(crate) size: u64,
}

/// Keeps the source a run is called with, before the run is admitted.
pub(crate) fn keep_run_source(
    tx: &mut Tx<'_>,
    run: &a2m_ledger::NewRun,
    source: &KeptSource,
) -> Result<(), Failure> {
    tx.tables(Owner::Actions).execute(
        "INSERT INTO actions_run_sources
            (workspace_id, principal_id, tool_name, target_version, input_hash,
             source_ref, kind, name, size_bytes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT (workspace_id, principal_id, tool_name, target_version, input_hash)
         DO UPDATE SET source_ref = excluded.source_ref, kind = excluded.kind,
             name = excluded.name, size_bytes = excluded.size_bytes",
        &[
            SqlValue::from(run.workspace.to_string()),
            SqlValue::from(run.principal.to_string()),
            SqlValue::from(run.tool_name.as_str()),
            SqlValue::from(run.target_version.to_string()),
            SqlValue::from(run.input_hash.to_string()),
            SqlValue::from(source.blob.to_string()),
            SqlValue::from(source.kind.as_str()),
            SqlValue::from(source.name.as_str()),
            SqlValue::from(i64::try_from(source.size).unwrap_or(i64::MAX)),
        ],
    )?;
    Ok(())
}

/// Keeps what a run is called with, before the run is admitted.
pub(crate) fn keep_run_call(
    tx: &mut Tx<'_>,
    run: &a2m_ledger::NewRun,
    input: BlobRef,
    caller: &CallerFacts,
) -> Result<(), Failure> {
    tx.tables(Owner::Actions).execute(
        "INSERT INTO actions_run_calls
            (workspace_id, principal_id, tool_name, target_version, input_hash, input_ref, caller)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (workspace_id, principal_id, tool_name, target_version, input_hash)
         DO UPDATE SET input_ref = excluded.input_ref, caller = excluded.caller",
        &[
            SqlValue::from(run.workspace.to_string()),
            SqlValue::from(run.principal.to_string()),
            SqlValue::from(run.tool_name.as_str()),
            SqlValue::from(run.target_version.to_string()),
            SqlValue::from(run.input_hash.to_string()),
            SqlValue::from(input.to_string()),
            SqlValue::from(caller.to_json().to_string()),
        ],
    )?;
    Ok(())
}

/// Keeps the facts of whoever resumes a run, in place of those of whoever
/// started it.
pub(crate) fn rekeep_caller(
    tx: &mut Tx<'_>,
    run: &RunRow,
    caller: &CallerFacts,
) -> Result<(), Failure> {
    tx.tables(Owner::Actions).execute(
        "UPDATE actions_run_calls SET caller = ?6
         WHERE workspace_id = ?1 AND principal_id = ?2 AND tool_name = ?3
           AND target_version = ?4 AND input_hash = ?5",
        &[
            SqlValue::from(run.workspace.to_string()),
            SqlValue::from(run.principal.to_string()),
            SqlValue::from(run.tool_name.as_str()),
            SqlValue::from(run.target_version.to_string()),
            SqlValue::from(run.input_hash.to_string()),
            SqlValue::from(caller.to_json().to_string()),
        ],
    )?;
    Ok(())
}

fn run_call(tx: &mut Tx<'_>, run: &RunRow) -> Result<Option<RunCall>, Failure> {
    let row = tx.tables(Owner::Actions).query_first(
        "SELECT input_ref, caller FROM actions_run_calls
         WHERE workspace_id = ?1 AND principal_id = ?2 AND tool_name = ?3
           AND target_version = ?4 AND input_hash = ?5",
        &[
            SqlValue::from(run.workspace.to_string()),
            SqlValue::from(run.principal.to_string()),
            SqlValue::from(run.tool_name.as_str()),
            SqlValue::from(run.target_version.to_string()),
            SqlValue::from(run.input_hash.to_string()),
        ],
    )?;
    let Some(row) = row else {
        return Ok(None);
    };
    let input: BlobRef = row.text(0)?.parse()?;
    let Some(caller) = CallerFacts::from_json(row.text(1)?) else {
        return Ok(None);
    };
    let source = tx
        .tables(Owner::Actions)
        .query_first(
            "SELECT source_ref FROM actions_run_sources
             WHERE workspace_id = ?1 AND principal_id = ?2 AND tool_name = ?3
               AND target_version = ?4 AND input_hash = ?5",
            &[
                SqlValue::from(run.workspace.to_string()),
                SqlValue::from(run.principal.to_string()),
                SqlValue::from(run.tool_name.as_str()),
                SqlValue::from(run.target_version.to_string()),
                SqlValue::from(run.input_hash.to_string()),
            ],
        )?
        .map(|row| row.text(0)?.parse::<BlobRef>())
        .transpose()?;
    Ok(Some(RunCall {
        input,
        caller,
        source,
    }))
}

/// What the kernel keeps about its runner loop.
pub(crate) struct RunnerState {
    pub(crate) task: Mutex<Option<JoinHandle<()>>>,
    engine: OnceLock<Arc<Engine>>,
    budgets: Mutex<BTreeSet<String>>,
    modes: Mutex<BTreeMap<RunId, Vec<(RecordRef, Mode)>>>,
}

impl Default for RunnerState {
    fn default() -> Self {
        Self {
            task: Mutex::new(None),
            engine: OnceLock::new(),
            budgets: Mutex::new(BTreeSet::new()),
            modes: Mutex::new(BTreeMap::new()),
        }
    }
}

/// A handle on the running loop.
#[derive(Debug, Clone)]
pub struct RunnerLoop {
    kernel: Kernel,
    runner: RunnerId,
}

impl RunnerLoop {
    /// The runner this loop claims as.
    #[must_use]
    pub const fn runner(&self) -> RunnerId {
        self.runner
    }

    /// Stops the loop. A job in hand is dropped between the engine's own
    /// waits; its lease runs out and the run is reconciled before anyone
    /// else gets the job.
    pub async fn stop(self) {
        self.kernel.stop_runner_loop().await;
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Kernel {
    /// Starts the loop that claims jobs from the scheduler and runs them
    /// through the engine, `slots` at a time. It says `hello` to the
    /// scheduler first, as a runner on another machine would.
    ///
    /// # Errors
    ///
    /// `busy`/`engine_degraded` when the store, the vault, or the scheduler
    /// is not open; what the scheduler answers to `hello`.
    pub async fn start_runner_loop(&self, slots: u32) -> Result<RunnerLoop, Failure> {
        let store = self.store()?.clone();
        let sched = self.sched()?.clone();
        let installation = self.installation()?;
        let vault = self.inner.vault.clone().ok_or_else(|| {
            codes::busy(
                codes::ENGINE_DEGRADED,
                "The vault is not open, so nothing can be sealed or verified.",
                "Call `doctor.run` for the fix.",
            )
        })?;
        if self.inner.runner.engine.get().is_none() {
            let engine = Engine::new(EngineDeps {
                registry: Arc::new(self.providers().clone()),
                redact: self.redact(),
                keyed: vault.keyed_digest().await?,
                clock: Arc::clone(&self.inner.clock),
                settings: EngineSettings {
                    transient_retries: self
                        .settings()
                        .int("engine.retries.transient")
                        .and_then(|count| u32::try_from(count).ok())
                        .unwrap_or(2),
                    freshness_window: Duration::from_secs(
                        self.settings()
                            .int("policy.preview_freshness_seconds")
                            .and_then(|seconds| u64::try_from(seconds).ok())
                            .unwrap_or(60),
                    ),
                    ..EngineSettings::default()
                },
            });
            let _ = self.inner.runner.engine.set(Arc::new(engine));
        }
        let runner = store
            .write(Durability::Synced, |tx| match fact(tx, "runner")? {
                Some(text) => Ok(text.parse::<RunnerId>()?),
                None => {
                    let made = RunnerId::from_parts(tx.now(), tx.random()?)?;
                    set_fact(tx, "runner", &made.to_string())?;
                    Ok(made)
                }
            })
            .await?;
        let modes: Vec<Mode> = self.providers().modes().cloned().collect();
        let terms = sched
            .hello(RunnerOffer {
                runner,
                workspace: installation.workspace,
                targets: Vec::new(),
                modes: modes.clone(),
                os: None,
                owner: None,
                protocol: a2m_sched::PROTOCOL_VERSION,
            })
            .await?;

        self.stop_runner_loop().await;
        let kernel = self.clone();
        let permits = Arc::new(Semaphore::new(slots.max(1) as usize));
        let task = tokio::spawn(async move {
            loop {
                // A slot must be free before work is asked for.
                let Ok(permit) = Arc::clone(&permits).acquire_owned().await else {
                    break;
                };
                drop(permit);
                let free = u32::try_from(permits.available_permits())
                    .unwrap_or(1)
                    .max(1);
                let offer = Slots {
                    total: free,
                    per_mode: modes.iter().map(|mode| (mode.clone(), free)).collect(),
                };
                let leases = match sched.claim(runner, offer, CLAIM_WAIT).await {
                    Ok(leases) => leases,
                    Err(_) => {
                        // The store cannot be asked. Look at the health and
                        // wait to be woken, or for the wait of one claim.
                        kernel.recheck().await;
                        tokio::select! {
                            () = kernel.inner.wake.notified() => {}
                            () = tokio::time::sleep(CLAIM_WAIT) => {}
                        }
                        continue;
                    }
                };
                for lease in leases {
                    let Ok(permit) = Arc::clone(&permits).acquire_owned().await else {
                        return;
                    };
                    let kernel = kernel.clone();
                    // The engine's future is not `Send`: a driver's prepared
                    // state is held across its waits and may not be shared
                    // between threads. So a job runs on a thread of its
                    // own, driven by this same runtime, whose timers and
                    // connections it goes on using.
                    let runtime = tokio::runtime::Handle::current();
                    tokio::task::spawn_blocking(move || {
                        runtime.block_on(kernel.work(lease, terms));
                        drop(permit);
                    });
                }
            }
        });
        *lock(&self.inner.runner.task) = Some(task);
        Ok(RunnerLoop {
            kernel: self.clone(),
            runner,
        })
    }

    pub(crate) async fn stop_runner_loop(&self) {
        let task = lock(&self.inner.runner.task).take();
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
    }

    /// Whether the runner loop runs, as `engine.status` shows it.
    pub(crate) fn runner_status(&self) -> Json {
        let running = lock(&self.inner.runner.task)
            .as_ref()
            .is_some_and(|task| !task.is_finished());
        json!({"in_process": true, "running": running})
    }

    /// Tells everyone who waits for a run that something about a run
    /// changed.
    pub(crate) fn run_changed(&self) {
        self.inner.signals.changed();
    }

    /// A receiver that is told whenever something about a run changed.
    pub(crate) fn run_watch(&self) -> watch::Receiver<u64> {
        self.inner.signals.watch()
    }

    /// Whether the budget was set in this process already.
    pub(crate) fn budget_noted(&self, key: &BudgetKey) -> bool {
        lock(&self.inner.runner.budgets).contains(key.as_str())
    }

    /// Notes that the budget is set.
    pub(crate) fn note_budget(&self, key: &BudgetKey) {
        lock(&self.inner.runner.budgets).insert(key.as_str().to_owned());
    }

    /// Carries out one claimed job: fills it from the ledger, runs it
    /// through the engine while a task sends its heartbeats, applies the
    /// outcome to the run under the job's lease, and tells the scheduler
    /// how the job ended.
    async fn work(&self, lease: JobLease, terms: RunnerTerms) {
        let Ok(sched) = self.sched().cloned() else {
            return;
        };
        let lease_ref = lease.lease_ref();
        // The claim wrote the run's new lease number: whoever follows the
        // run is told.
        self.run_changed();
        let end = match self.run_claimed(&lease, terms).await {
            Ok(end) => end,
            Err(failure) => JobEnd::Stopped {
                class: failure.class(),
                code: failure.code().clone(),
                in_doubt: false,
            },
        };
        // The job is unfinished in the scheduler's own table until this
        // commits, and that table is what a caller waiting for the run
        // reads. It is told only afterwards.
        let _ = sched.finish(&lease_ref, end).await;
        self.run_changed();
    }

    async fn run_claimed(&self, lease: &JobLease, terms: RunnerTerms) -> Result<JobEnd, Failure> {
        let Some(fence) = lease.fence() else {
            // A system job. The keeper's work runs on its own worker in one
            // process, so nothing is queued for it here.
            return Ok(JobEnd::Done);
        };
        let store = self.store()?;
        let sched = self.sched()?.clone();
        let engine = self.inner.runner.engine.get().cloned().ok_or_else(|| {
            codes::busy(
                codes::ENGINE_DEGRADED,
                "The engine is not started.",
                "Start the runner loop.",
            )
        })?;
        let run = fence.run;
        let (row, call) = store
            .read(move |tx| {
                let row = tx.ledger().run(run)?;
                let call = run_call(tx, &row)?;
                Ok((row, call))
            })
            .await?;
        let plan = self.plan_by_hash(&row.target, &row.target_version).await?;
        let (input, caller, source) = match call {
            Some(call) => {
                let opened = store.blobs().get(&call.input).await?;
                // The source is the stored bytes the run was called with:
                // every lease of the run reads the same ones, whatever the
                // file in the project holds by now.
                let source = match &call.source {
                    Some(blob) => Some(Arc::new(store.blobs().get(blob).await?.expose().to_vec())),
                    None => None,
                };
                (decode_values(opened.expose())?, call.caller, source)
            }
            // A run this installation holds no call for: nothing is known
            // of its input, and its caller is the person who started it.
            None => (
                Values::new(),
                CallerFacts {
                    principal: PrincipalRef::person(row.principal),
                    is_agent: false,
                    acting_for: None,
                    toolset: None,
                    decision: row
                        .decision
                        .unwrap_or_else(|| DecisionId::from_bytes([0; 16])),
                },
                None,
            ),
        };

        let kind = match lease.requirements.kind {
            a2m_sched::JobKind::Preview => JobKind::Preview,
            a2m_sched::JobKind::Try => JobKind::Try,
            a2m_sched::JobKind::Discover => JobKind::Discover,
            a2m_sched::JobKind::Reconcile => JobKind::Reconcile,
            a2m_sched::JobKind::VerifyAfterLoss => JobKind::VerifyAfterLoss,
            a2m_sched::JobKind::Canary => JobKind::Canary,
            _ => JobKind::Execute,
        };
        let health = self.health();
        let capture = |key: &str| match self.settings().text(key) {
            Some("full") => CaptureLevel::Full,
            Some("standard") => CaptureLevel::Standard,
            _ => CaptureLevel::Minimal,
        };
        let clock = LeaseClock::until(Instant::now() + lease.length);
        let job = JobSpec {
            run,
            workspace: row.workspace,
            attempt: lease.attempt,
            epoch: fence.epoch,
            kind,
            plan: row.target_version,
            tool: ToolRef::Tool(row.tool_name.clone()),
            environment: Some(row.environment.clone()),
            identity: Some(Identity::Person(row.principal)),
            input,
            source,
            snapshot: self.snapshot(&row).await?,
            conditions: Conditions {
                health: match health.state {
                    HealthState::Ok => KernelHealth::Ok,
                    _ => KernelHealth::Degraded(
                        health
                            .reasons
                            .first()
                            .map_or(codes::ENGINE_DEGRADED, |reason| {
                                reason.failure.code().clone()
                            }),
                    ),
                },
                ..Conditions::default()
            },
            pin: None,
            parallelism: Parallelism::default(),
            lease: clock.clone(),
            capture: if row.effect == a2m_core::EffectClass::Read {
                capture("evidence.capture")
            } else {
                capture("evidence.capture_on_writes")
            },
        };

        // One task per held job renews its lease and passes on what the
        // scheduler directs.
        let beat = {
            let sched = sched.clone();
            let lease_ref = lease.lease_ref();
            let every = terms.heartbeat;
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(every).await;
                    match sched.heartbeat(&lease_ref).await {
                        Ok(renewal) => {
                            clock.renew(Instant::now() + renewal.remaining);
                            clock.direct(match renewal.directive {
                                a2m_sched::Directive::Continue => Directive::Continue,
                                a2m_sched::Directive::Cancel => Directive::Cancel,
                                _ => Directive::Pause,
                            });
                        }
                        // The lease is no longer this job's: the engine
                        // must stop before its next commit point.
                        Err(_) => {
                            clock.renew(Instant::now());
                            break;
                        }
                    }
                }
            })
        };

        let control =
            InProcessControl::new(self.clone(), fence, lease.lease_ref(), row, plan, caller);
        let outcome = engine.run_job(&job, &control).await;
        beat.abort();
        let _ = beat.await;
        lock(&self.inner.runner.modes)
            .entry(run)
            .or_default()
            .extend(lock(&control.modes).drain(..));

        let end = match outcome {
            JobOutcome::Finished { state, failure } => {
                control.apply(state, failure).await?;
                JobEnd::Done
            }
            JobOutcome::Rests { state, failure } => {
                control.apply(state, failure).await?;
                JobEnd::Rests
            }
            JobOutcome::Stopped(failure) => {
                let (class, code) = (failure.class(), failure.code().clone());
                control.apply(RunState::Stopped, Some(failure)).await?;
                JobEnd::Stopped {
                    class,
                    code,
                    in_doubt: may_have_written(class) || control.in_doubt().await,
                }
            }
            JobOutcome::LeaseEnding => {
                // The engine gave the job back with most of a full lease
                // left and nobody told it to pause: its next commit point
                // needs more time than a lease lasts, and handing the job
                // out again would end the same way for ever.
                let asked_to_stop = job.lease.directive() != Directive::Continue;
                if !asked_to_stop && job.lease.leaves(lease.length / 2) {
                    let failure = codes::busy(
                        codes::LEASE_TOO_SHORT,
                        "A step of this tool may take longer than a job's lease lasts, so it was not started.",
                        "Raise `runners.lease_seconds` above the tool's longest step time limits, then resume the run.",
                    );
                    let (class, code) = (failure.class(), failure.code().clone());
                    control.apply(RunState::Stopped, Some(failure)).await?;
                    JobEnd::Stopped {
                        class,
                        code,
                        in_doubt: control.in_doubt().await,
                    }
                } else {
                    JobEnd::LeaseEnding
                }
            }
            // A reconcile or a second look ended: the run's state stays.
            JobOutcome::Settled { .. } => JobEnd::Done,
            JobOutcome::Left(failure) => JobEnd::Stopped {
                class: failure.class(),
                code: failure.code().clone(),
                in_doubt: false,
            },
        };
        Ok(end)
    }

    /// What the ledger holds of a run that has been worked on before, with
    /// its stored values read back into data. `None` for a new run.
    async fn snapshot(&self, row: &RunRow) -> Result<Option<RunSnapshot>, Failure> {
        if row.state == RunState::Queued {
            return Ok(None);
        }
        let store = self.store()?;
        let run = row.id;
        // The values recorded before a commit point are the evidence of a
        // `need` event of the run. They are read from the ledger, so a
        // run resumed after a restart has them as well.
        let (rows, recorded) = store
            .read(move |tx| {
                let mut ledger = tx.ledger();
                let rows = ledger.records(run)?;
                let mut recorded: BTreeMap<(u32, u32), BlobRef> = BTreeMap::new();
                for event in ledger.events(run, 0, u32::MAX)? {
                    if event.kind != EventKind::Need {
                        continue;
                    }
                    let (Some(blob), Ok(payload)) =
                        (event.evidence, serde_json::from_str::<Json>(&event.payload))
                    else {
                        continue;
                    };
                    if payload["before_commit"] != true {
                        continue;
                    }
                    let place = |name: &str| {
                        payload[name]
                            .as_u64()
                            .and_then(|number| u32::try_from(number).ok())
                    };
                    if let (Some(step), Some(record)) = (place("step"), place("record")) {
                        // A later event of the same record replaces an
                        // earlier one.
                        recorded.insert((step, record), blob);
                    }
                }
                Ok((rows, recorded))
            })
            .await?;
        let modes = lock(&self.inner.runner.modes)
            .get(&run)
            .cloned()
            .unwrap_or_default();
        let mut records = Vec::new();
        for record in rows {
            let before = self.stored_values(record.before).await?;
            let outputs = self.stored_values(record.outputs).await?;
            let before_commit = self
                .stored_values(
                    recorded
                        .get(&(record.record.step_seq, record.record.record_seq))
                        .copied(),
                )
                .await?;
            records.push(RecordSnapshot {
                record: record.record,
                key: record.key,
                operation: record.operation,
                state: record.state,
                intended: record.intended,
                intended_values: Values::new(),
                before,
                idempotency_token: record.idempotency_token,
                assigned_key: record.assigned_key,
                outputs,
                before_commit,
                mode: modes
                    .iter()
                    .rev()
                    .find(|(held, _)| *held == record.record)
                    .map(|(_, mode)| mode.clone()),
                failure: record.failure,
            });
        }
        Ok(Some(RunSnapshot {
            state: row.state,
            diff: row.diff_hash,
            cleared: row.cleared_to_write(),
            records,
        }))
    }

    pub(crate) async fn stored_values(
        &self,
        blob: Option<BlobRef>,
    ) -> Result<Option<Values>, Failure> {
        let Some(blob) = blob else {
            return Ok(None);
        };
        let opened = self.store()?.blobs().get(&blob).await?;
        Ok(Some(decode_values(opened.expose())?))
    }
}
