//! The in-process runner loop: claims jobs from the scheduler and hands
//! them to the engine with a `Control` that answers in this process.
//!
//! The lease rules of the runner protocol apply unchanged on one machine:
//! the lease is counted on a steady clock, a task per held job sends its
//! heartbeats, and the engine stops acting before the lease runs out.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use a2m_core::{
    DecisionId, Failure, Identity, Mode, PrincipalId, RunId, RunState, RunnerId, Values,
};
use a2m_engine::{
    Conditions, Diff, Directive, Engine, EngineDeps, EngineSettings, JobKind, JobOutcome, JobSpec,
    KernelHealth, LeaseClock, Parallelism, RecordSnapshot, RunSnapshot, ToolRef, decode_values,
};
use a2m_ledger::{BlobRef, Durability, EventKind, Owner, RunRow, SqlValue, Tx};
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

/// How long one claim waits for work before it asks again. After a claim
/// the store could not answer, it is also the longest the loop waits before
/// it asks once more: the store gives no notice that it can be asked again,
/// so this is an interval between tries and not a wait for a condition. The
/// kernel's wake notice ends it early.
const CLAIM_WAIT: Duration = Duration::from_secs(30);

/// The most rate budgets noted as set. Past it the notes are dropped, and a
/// budget is set once more the next time its surface is used.
const BUDGETS_NOTED: usize = 4096;

/// The field of a discovery run's stored call that names its surface.
pub(crate) const DISCOVERY_SURFACE: &str = "surface";
/// The field of a discovery run's stored call that holds the focus its
/// discoverers are given.
pub(crate) const DISCOVERY_FOCUS: &str = "focus";
/// The field of a discovery run's stored call that holds the files of the
/// project's `sources` folder the call handed its discoverers, each name to
/// its path as the call wrote it (decision 0263). Left out when none were.
pub(crate) const DISCOVERY_FILES: &str = "files";

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
    /// The project folder of the request that started the run, as the
    /// dispatcher checked it: kept with the run's stored call, so that every
    /// lease of the run, a resume's too, hands the engine the same folder
    /// (decision 0176). `None` when the request stated none, or one whose
    /// path is not text and so cannot be kept.
    pub(crate) project: Option<std::path::PathBuf>,
    /// For a trial: the binding of the operation it runs, by place, mode,
    /// and digest ([`a2m_engine::BindingPin::to_json`]), so that every lease
    /// runs that one, and the trial is evidence for it alone (audit finding
    /// AC-6). `None` for a call, which the engine chooses for.
    pub(crate) binding: Option<Json>,
}

impl CallerFacts {
    pub(crate) fn of(caller: &Caller, decision: DecisionId) -> Self {
        Self {
            principal: caller.principal.principal.clone(),
            is_agent: caller.is_agent,
            acting_for: caller.principal.acting_for.clone(),
            toolset: caller.toolset.clone(),
            decision,
            project: None,
            binding: None,
        }
    }

    /// The same facts, with the binding a trial runs.
    pub(crate) fn with_binding(mut self, binding: Option<&a2m_engine::BindingPin>) -> Self {
        self.binding = binding.map(a2m_engine::BindingPin::to_json);
        self
    }

    /// The same facts, with the request's checked project folder.
    pub(crate) fn with_project(mut self, project: Option<std::path::PathBuf>) -> Self {
        self.project = project;
        self
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
            "project": self.project.as_deref().and_then(std::path::Path::to_str),
            "binding": self.binding,
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
            project: json["project"].as_str().map(std::path::PathBuf::from),
            binding: Some(json["binding"].clone()).filter(|binding| !binding.is_null()),
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

/// What a run was called with, as the store holds it. `None` when the row
/// is missing or its caller cannot be read: such a run is never run.
///
/// The caller's facts are written once, when the run is made, and nothing
/// changes them afterwards: a resume never replaces who a run belongs to or
/// what its caller was.
pub(crate) fn run_call(tx: &mut Tx<'_>, run: &RunRow) -> Result<Option<RunCall>, Failure> {
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
}

impl Default for RunnerState {
    fn default() -> Self {
        Self {
            task: Mutex::new(None),
            engine: OnceLock::new(),
            budgets: Mutex::new(BTreeSet::new()),
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
        self.engine().await?;
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

    /// The engine, made once from the provider registry, the redactor, the
    /// vault's keyed digest, and the settings. The runner loop runs jobs
    /// through it, and whoever submits a job asks it what the job's tool
    /// must hold alone.
    ///
    /// # Errors
    ///
    /// `busy`/`engine_degraded` when the vault is not open.
    pub(crate) async fn engine(&self) -> Result<Arc<Engine>, Failure> {
        if let Some(engine) = self.inner.runner.engine.get() {
            return Ok(Arc::clone(engine));
        }
        let vault = self.inner.vault.clone().ok_or_else(|| {
            codes::busy(
                codes::ENGINE_DEGRADED,
                "The vault is not open, so nothing can be sealed or verified.",
                "Call `doctor.run` for the fix.",
            )
        })?;
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
                shadow_limit: Duration::from_millis(
                    self.settings()
                        .int("engine.shadow.time_limit_ms")
                        .and_then(|millis| u64::try_from(millis).ok())
                        .unwrap_or(2_000),
                ),
                ..EngineSettings::default()
            },
        });
        Ok(Arc::clone(
            self.inner.runner.engine.get_or_init(|| Arc::new(engine)),
        ))
    }

    /// What a job of `tool` must hold alone for as long as it is leased:
    /// one key for every surface a write of the tool may be made on, of
    /// the kind the write's mode declares (one run at a time on one file,
    /// one link, one surface). A read holds nothing. The scheduler takes
    /// all of a job's keys in the transaction that leases it, or none, so
    /// two jobs never wait for each other; the keys are sorted all the
    /// same, which is the scheduler's order.
    ///
    /// # Errors
    ///
    /// `busy`/`engine_degraded` when the engine cannot be made; the
    /// scheduler's refusal of a key.
    pub(crate) async fn held_alone(
        &self,
        row_workspace: a2m_core::WorkspaceId,
        plan: &a2m_core::Plan,
        tool: &a2m_core::OperationName,
        environment: &a2m_core::Environment,
        writes: bool,
    ) -> Result<Vec<a2m_sched::ResourceKey>, Failure> {
        if !writes {
            return Ok(Vec::new());
        }
        let engine = self.engine().await?;
        let mut keys = Vec::new();
        for need in engine.exclusive_needs(plan, tool) {
            keys.push(a2m_sched::ResourceKey::held_alone(
                need.kind.as_str(),
                row_workspace,
                &plan.header.target,
                environment,
                need.surface.as_str(),
            )?);
        }
        keys.sort();
        keys.dedup();
        Ok(keys)
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
        let mut noted = lock(&self.inner.runner.budgets);
        if noted.len() >= BUDGETS_NOTED {
            noted.clear();
        }
        noted.insert(key.as_str().to_owned());
    }

    /// Whether a record of the run may have been written without being
    /// read back. When the ledger cannot be asked, the answer is yes: a
    /// write whose state cannot be read is a write in doubt.
    pub(crate) async fn run_in_doubt(&self, run: RunId) -> bool {
        match self.store() {
            Ok(store) => store
                .read(move |tx| tx.ledger().in_doubt(run))
                .await
                .map_or(true, |records| !records.is_empty()),
            Err(_) => true,
        }
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
            Err(failure) => {
                // The job could not be carried through, perhaps after the
                // engine had sent a write: the run is stopped with the
                // failure, and whether a record is in doubt is read from
                // the ledger, never assumed.
                let in_doubt = match lease.fence() {
                    Some(fence) => {
                        self.stop_run(fence, &failure).await;
                        self.run_in_doubt(fence.run).await
                    }
                    None => false,
                };
                JobEnd::Stopped {
                    class: failure.class(),
                    code: failure.code().clone(),
                    in_doubt,
                }
            }
        };
        // The job is unfinished in the scheduler's own table until this
        // commits, and that table is what a caller waiting for the run
        // reads. It is told only afterwards. When it cannot be recorded
        // the store cannot commit: the job stays leased until its lease
        // ends, the scheduler's sweep then stops the run and queues its
        // reconcile, and the health state is looked at again now.
        if sched.finish(&lease_ref, end).await.is_err() {
            self.recheck().await;
        }
        self.run_changed();
    }

    /// Stops a run with a failure, under the job's lease, when the job
    /// could not be carried through. A run that has ended stays as it
    /// ended.
    async fn stop_run(&self, fence: a2m_ledger::Fence, why: &Failure) {
        let Ok(store) = self.store() else {
            return;
        };
        let why = self.redact().redact_failure(why);
        let run = fence.run;
        // Best effort: when this cannot be written either, the lease ends
        // and the scheduler's sweep stops the run.
        let _ = store
            .write(Durability::Synced, move |tx| {
                let mut ledger = tx.ledger();
                let state = ledger.run(run)?.state;
                if state.is_final() || state == RunState::Stopped {
                    return Ok(());
                }
                ledger.set_state(&fence, RunState::Stopped, Some(&why))
            })
            .await;
    }

    /// Ends a run whose stored call is missing or cannot be read. Nothing
    /// is known of its input or of its caller, so it is not run with an
    /// empty input and a caller made up for it (rule 7: unknown means
    /// stop). A run with a record in doubt stops, so that the doubt stays
    /// in view; any other run ends as `failed`, or as `cancelled` where the
    /// ledger allows no other end from the state it rests in.
    async fn end_unreadable(&self, fence: a2m_ledger::Fence) -> Result<JobEnd, Failure> {
        let why = codes::busy(
            codes::RUN_CALL_UNREADABLE,
            "What this run was called with is missing from the store or cannot be read, so the run was not carried out.",
            "Call the tool again; that is a new run. Call `trace.show` with this run's identifier to see what it did before.",
        );
        let run = fence.run;
        let (class, code) = (why.class(), why.code().clone());
        self.store()?
            .write(Durability::Synced, move |tx| {
                let mut ledger = tx.ledger();
                let state = ledger.run(run)?.state;
                if state.is_final() {
                    return Ok(());
                }
                let ends = if ledger.in_doubt(run)?.is_empty() {
                    [RunState::Failed, RunState::Cancelled]
                } else {
                    [RunState::Stopped, RunState::Stopped]
                };
                for to in ends {
                    if state == to || ledger.set_state(&fence, to, Some(&why)).is_ok() {
                        return Ok(());
                    }
                }
                Ok(())
            })
            .await?;
        self.run_changed();
        // No reconcile is queued: it would be claimed, and end, the same
        // way.
        Ok(JobEnd::Stopped {
            class,
            code,
            in_doubt: false,
        })
    }

    async fn run_claimed(&self, lease: &JobLease, terms: RunnerTerms) -> Result<JobEnd, Failure> {
        let Some(fence) = lease.fence() else {
            // A system job. In one process nothing queues one: the keeper's
            // work, a sign-out and a forget included, is a call on the
            // keeper's own worker that is waited for to its answer, so
            // none can be lost on the way. A system job that reaches this
            // loop all the same is work it cannot carry out, and it is
            // never answered as done.
            let failure = codes::busy(
                codes::SYSTEM_JOB_NOT_CARRIED,
                "This runner carries out jobs of runs only, and was handed a job of another kind.",
                "Report this; it is a fault in the engine.",
            );
            return Ok(JobEnd::Stopped {
                class: failure.class(),
                code: failure.code().clone(),
                in_doubt: false,
            });
        };
        let store = self.store()?;
        let sched = self.sched()?.clone();
        let engine = self.engine().await?;
        let run = fence.run;
        let (row, call) = store
            .read(move |tx| {
                let row = tx.ledger().run(run)?;
                let call = run_call(tx, &row)?;
                Ok((row, call))
            })
            .await?;
        let plan = self.plan_by_hash(&row.target, &row.target_version).await?;
        let Some(call) = call else {
            return self.end_unreadable(fence).await;
        };
        let opened = store.blobs().get(&call.input).await?;
        // The source is the stored bytes the run was called with: every
        // lease of the run reads the same ones, whatever the file in the
        // project holds by now.
        let source = match &call.source {
            Some(blob) => Some(Arc::new(store.blobs().get(blob).await?.expose().to_vec())),
            None => None,
        };
        let (input, caller) = (decode_values(opened.expose())?, call.caller);
        // A trial runs the binding it was made for, or nothing: a stored
        // binding that cannot be read is never replaced by the engine's own
        // choice (audit finding AC-6).
        let binding = match &caller.binding {
            Some(stored) => match a2m_engine::BindingPin::from_json(&row.tool_name, stored) {
                Some(pin) => Some(pin),
                None => return self.end_unreadable(fence).await,
            },
            None => None,
        };
        // The run's project folder was checked at the request. It is checked
        // again at every lease, a resume's days later too: a folder that no
        // longer passes, or that now leads elsewhere, is not handed to the
        // engine (audit finding A-4), and a pack that needs it stops by name.
        let caller = match caller.project.clone() {
            Some(folder) => match self.checked_project(&folder).await {
                Ok(checked) if checked == folder => caller,
                _ => caller.with_project(None),
            },
            None => caller,
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
        // A discovery run's stored call names its surface, the focus
        // handed to its discoverers, and the files of the project's
        // `sources` folder the call handed them (`discover.run`): the job
        // names the surface, the discoverers are given the focus alone,
        // and the files reach the engine through the control, which checks
        // each again (decision 0263). Files stored in a form this build
        // cannot read are never dropped: the run cannot be carried out.
        let mut handed = std::collections::BTreeMap::new();
        let (tool, input) = if kind == JobKind::Discover {
            let mut input = input;
            let surface = match input.remove(DISCOVERY_SURFACE) {
                Some(a2m_core::Value::Text(surface)) => a2m_core::SurfaceName::new(&surface)?,
                _ => return self.end_unreadable(fence).await,
            };
            let focus = match input.remove(DISCOVERY_FOCUS) {
                Some(a2m_core::Value::Record(focus)) => focus,
                _ => Values::new(),
            };
            match input.remove(DISCOVERY_FILES) {
                None => {}
                Some(a2m_core::Value::Record(files)) => {
                    for (name, path) in files.iter() {
                        let a2m_core::Value::Text(path) = path else {
                            return self.end_unreadable(fence).await;
                        };
                        handed.insert(name.to_owned(), path.clone());
                    }
                }
                Some(_) => return self.end_unreadable(fence).await,
            }
            (ToolRef::Surface(surface), focus)
        } else {
            (ToolRef::Tool(row.tool_name.clone()), input)
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
            tool,
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
            binding,
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
            InProcessControl::new(self.clone(), fence, lease.lease_ref(), row, plan, caller)
                .with_discovery_files(handed);
        let outcome = engine.run_job(&job, &control).await;
        beat.abort();
        let _ = beat.await;

        let end = match outcome {
            JobOutcome::Finished { state, failure } => {
                control.apply(state, failure).await?;
                JobEnd::Done
            }
            JobOutcome::Rests { state, failure } => {
                control.apply(state, failure).await?;
                JobEnd::Rests
            }
            // A record in doubt that reconcile read and could not settle
            // waits for a person: reading it again at once would answer
            // the same, and a reconcile queued on every stop would read the
            // system without end. The run rests `stopped`; a resume asks
            // again (decision 0185).
            JobOutcome::Stopped(failure)
                if failure.class() == a2m_core::FailureClass::PossibleCommit
                    && *failure.code() == a2m_engine::codes::CANNOT_RECONCILE =>
            {
                control.apply(RunState::Stopped, Some(failure)).await?;
                JobEnd::Rests
            }
            JobOutcome::Stopped(failure) => {
                let (class, code) = (failure.class(), failure.code().clone());
                control.apply(RunState::Stopped, Some(failure)).await?;
                JobEnd::Stopped {
                    class,
                    code,
                    in_doubt: may_have_written(class) || self.run_in_doubt(run).await,
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
                        in_doubt: self.run_in_doubt(run).await,
                    }
                } else {
                    JobEnd::LeaseEnding
                }
            }
            // A reconcile or a second look ended: the run's state stays.
            JobOutcome::Settled { .. } => JobEnd::Done,
            // A discovery that could not be carried out wrote nothing and
            // is a run of its own: it ends as failed, with the failure,
            // and is not left running.
            JobOutcome::Left(failure) if kind == JobKind::Discover => {
                control.apply(RunState::Failed, Some(failure)).await?;
                JobEnd::Done
            }
            JobOutcome::Left(failure) => JobEnd::Stopped {
                class: failure.class(),
                code: failure.code().clone(),
                in_doubt: self.run_in_doubt(run).await,
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
        // run resumed after a restart has them as well. Only those recorded
        // after the record's current intent belong to the attempt whose
        // send may be in doubt: an earlier attempt's values name what that
        // attempt sent, and a reconcile is never given them (decision 0185).
        let (rows, events) = store
            .read(move |tx| {
                let mut ledger = tx.ledger();
                let rows = ledger.records(run)?;
                let events = ledger.events(run, 0, u32::MAX)?;
                Ok((rows, events))
            })
            .await?;
        let place_of = |payload: &Json| {
            let place = |name: &str| {
                payload[name]
                    .as_u64()
                    .and_then(|number| u32::try_from(number).ok())
            };
            place("step").zip(place("record"))
        };
        let mut records = Vec::new();
        for record in rows {
            let place = (record.record.step_seq, record.record.record_seq);
            let mut before_commit_blob = None;
            let mut sent_by: Option<a2m_core::Timestamp> = record.acted_at;
            if let Some(intent_seq) = record.intent_seq {
                for event in events.iter().filter(|event| event.seq > intent_seq) {
                    let Ok(payload) = serde_json::from_str::<Json>(&event.payload) else {
                        continue;
                    };
                    if place_of(&payload) != Some(place) {
                        continue;
                    }
                    match event.kind {
                        EventKind::Need if payload["before_commit"] == true => {
                            // A later event of the same attempt replaces an
                            // earlier one.
                            if let Some(blob) = event.evidence {
                                before_commit_blob = Some(blob);
                            }
                        }
                        // What followed the send was recorded at this
                        // moment: the send was made before it.
                        EventKind::Acted | EventKind::Failed
                            if sent_by.is_none_or(|at| {
                                at.as_unix_millis() < event.at.as_unix_millis()
                            }) =>
                        {
                            sent_by = Some(event.at);
                        }
                        _ => {}
                    }
                }
            }
            let before = self.stored_values(record.before).await?;
            let outputs = self.stored_values(record.outputs).await?;
            let before_commit = self.stored_values(before_commit_blob).await?;
            let acknowledged = record.acknowledged();
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
                // The mode of the binding its intent was stored for, as
                // the ledger keeps it with the record: after a restart, a
                // record in doubt is read through the binding that wrote it.
                mode: record.mode,
                failure: record.failure,
                acknowledged,
                intent_at: record.intent_at,
                sent_by,
            });
        }
        // The diff the run was previewed with, which a person approved: the
        // engine writes each record only through the binding it names.
        let approved = match row.diff {
            Some(blob) => {
                let opened = store.blobs().get(&blob).await?;
                Some(Diff::decode(opened.expose())?)
            }
            None => None,
        };
        Ok(Some(RunSnapshot {
            state: row.state,
            diff: row.diff_hash,
            cleared: row.cleared_to_write(),
            records,
            approved,
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
