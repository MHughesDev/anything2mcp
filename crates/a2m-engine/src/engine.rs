//! The engine: one job in, one outcome out.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use a2m_core::{
    Digest, Failure, FailureClass, KeyedDigest, OperationName, Plan, Redact, RunState, SurfaceName,
    ToolRuns, Version,
};
use a2m_provider::{Exclusive, Posture, Registry};
use a2m_sources::Limits;
use a2m_telemetry::{Clock, TraceBuilder, TraceIdentity};

use crate::codes;
use crate::control::{Control, EngineEvent};
use crate::guard::Timed;
use crate::job::{JobKind, JobOutcome, JobSpec, ToolRef};
use crate::loaded::Loaded;
use crate::run::{Run, Stop};
use crate::tool::{self, Done};

/// What the engine is told once, when it is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineSettings {
    /// How many times a `transient` failure of one step is tried again.
    pub transient_retries: u32,
    /// How old a read of a record may be before it is read again ahead of
    /// its write.
    pub freshness_window: Duration,
    /// How long a driver may take to prepare.
    pub prepare_timeout: Duration,
    /// The time limit of a step whose plan states none.
    pub step_timeout: Duration,
    /// How long the engine waits for one call to `Control` to be answered.
    /// An implementation that waits (for a lock, a lease, a rate budget)
    /// keeps its own, shorter limit; this one ends a call that never
    /// answers.
    pub control_timeout: Duration,
    /// What reading a source may use up.
    pub source_limits: Limits,
    /// How many plans are kept loaded.
    pub plans_kept: usize,
    /// The engine's version, for the trace.
    pub version: Version,
}

impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            transient_retries: 2,
            freshness_window: Duration::from_secs(60),
            prepare_timeout: Duration::from_secs(30),
            step_timeout: Duration::from_secs(30),
            control_timeout: Duration::from_secs(120),
            source_limits: Limits {
                bytes: 64 * 1024 * 1024,
                unpacked_bytes: 256 * 1024 * 1024,
                rows: 100_000,
                cells: 5_000_000,
                cell_bytes: 32 * 1024,
            },
            plans_kept: 16,
            version: Version::new(0, 0, 1),
        }
    }
}

/// What the engine is built from. All of it is handed in by the role that
/// starts the process; the engine makes none of it.
#[derive(Clone)]
pub struct EngineDeps {
    /// The registry: the only way to a driver, a discoverer, or a reader.
    pub registry: Arc<Registry>,
    /// The vault's redactor.
    pub redact: Arc<dyn Redact>,
    /// The vault's keyed digest, for what a trace may hold only as one.
    pub keyed: Arc<dyn KeyedDigest>,
    /// Where durations come from. None of them is hashed or compared.
    pub clock: Arc<dyn Clock>,
    /// The settings.
    pub settings: EngineSettings,
}

/// Something a job that writes must hold alone for as long as it is leased:
/// what the mode of one of its bindings declares that one write holds
/// alone, on the surface the binding acts on.
///
/// The engine does not take it. Whoever submits the job names it in the
/// job's requirements, so that the scheduler takes every such key together
/// when it leases the job, before any record lock, and passes what a lost
/// job held to its reconcile job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExclusiveNeed {
    /// What is held alone, as the mode's manifest words it.
    pub kind: Exclusive,
    /// The surface it is held on.
    pub surface: SurfaceName,
}

/// The engine. It holds the plans it has loaded and nothing else between
/// jobs: the ledger, reached through `Control`, is the authority.
pub struct Engine {
    pub(crate) deps: EngineDeps,
    plans: Mutex<Vec<Arc<Loaded>>>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("settings", &self.deps.settings)
            .finish_non_exhaustive()
    }
}

/// The run state a failure ends in, by its class alone (spec section 208),
/// from the state the run is in.
fn failed(state: RunState, failure: Failure) -> JobOutcome {
    if state == RunState::NeedsApproval {
        // A run that waits for approval leaves that state only by an
        // approval, its expiry, or a cancel: it goes on waiting.
        return JobOutcome::Rests {
            state,
            failure: Some(failure),
        };
    }
    match failure.class() {
        FailureClass::Input if matches!(state, RunState::Queued | RunState::Validating) => {
            JobOutcome::Finished {
                state: RunState::Rejected,
                failure: Some(failure),
            }
        }
        // `rejected` is a state a run enters only before anything ran.
        FailureClass::Input | FailureClass::Policy | FailureClass::Stale => JobOutcome::Finished {
            state: RunState::Failed,
            failure: Some(failure),
        },
        FailureClass::SessionExpired => JobOutcome::Rests {
            state: RunState::NeedsLogin,
            failure: Some(failure),
        },
        FailureClass::Transient
        | FailureClass::Busy
        | FailureClass::Mismatch
        | FailureClass::EdgeBlocked
        | FailureClass::UnknownVariant
        | FailureClass::Rejected
        | FailureClass::Drift
        | FailureClass::PossibleCommit => JobOutcome::Stopped(failure),
    }
}

impl Engine {
    /// An engine over the registry and services given.
    #[must_use]
    pub fn new(deps: EngineDeps) -> Self {
        Self {
            deps,
            plans: Mutex::new(Vec::new()),
        }
    }

    /// The plan by its hash, fetched through `Control` and checked against
    /// the hash before it is used.
    pub(crate) async fn plan(
        &self,
        hash: &Digest,
        control: &dyn Control,
    ) -> Result<Arc<Loaded>, Failure> {
        {
            let plans = self.plans.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(found) = plans.iter().find(|loaded| loaded.hash == *hash) {
                return Ok(Arc::clone(found));
            }
        }
        let plan = control.plan(hash).await?;
        let loaded = Arc::new(Loaded::load(plan, hash)?);
        let mut plans = self.plans.lock().unwrap_or_else(PoisonError::into_inner);
        if plans.len() >= self.deps.settings.plans_kept.max(1) {
            plans.remove(0);
        }
        plans.push(Arc::clone(&loaded));
        Ok(loaded)
    }

    /// What a job that carries out `tool` must hold alone, when the tool
    /// writes: for every binding that may write, what its mode declares
    /// one write holds alone, on the binding's surface. In the order of
    /// the kinds and then of the surfaces' names, each once. Empty for a
    /// tool that only reads, and for a tool the plan does not hold.
    ///
    /// A binding's mode is looked up by name in the registry, as data; a
    /// mode no enabled provider declares can never be chosen and adds
    /// nothing.
    #[must_use]
    pub fn exclusive_needs(&self, plan: &Plan, tool: &OperationName) -> Vec<ExclusiveNeed> {
        let operations: Vec<_> = match plan.tool(tool).map(|tool| tool.runs) {
            Some(ToolRuns::Operation(index)) => plan.operation(index).into_iter().collect(),
            Some(ToolRuns::Pipeline(index)) => plan
                .pipeline(index)
                .into_iter()
                .flat_map(|pipeline| &pipeline.steps)
                .filter_map(|step| plan.operation(step.operation))
                .collect(),
            None => Vec::new(),
        };
        let mut needs = Vec::new();
        for binding in operations.iter().flat_map(|operation| &operation.bindings) {
            if !binding.act.iter().any(|step| step.commits)
                || binding.status == a2m_core::BindingStatus::Retired
            {
                continue;
            }
            let Some(defaults) = self.deps.registry.mode_defaults(&binding.mode) else {
                continue;
            };
            let Some(surface) = plan.surface(binding.surface) else {
                continue;
            };
            if defaults.exclusive != Exclusive::None {
                needs.push(ExclusiveNeed {
                    kind: defaults.exclusive,
                    surface: surface.name.clone(),
                });
            }
        }
        needs.sort_by(|left, right| {
            (left.kind.as_str(), &left.surface).cmp(&(right.kind.as_str(), &right.surface))
        });
        needs.dedup();
        needs
    }

    /// The environment a job names, as its position in the plan.
    pub(crate) fn environment(loaded: &Loaded, job: &JobSpec) -> Result<usize, Failure> {
        let plan = &loaded.plan;
        match &job.environment {
            Some(name) => plan
                .environments
                .iter()
                .position(|environment| &environment.name == name)
                .ok_or_else(|| {
                    codes::job_invalid(format!("The plan holds no environment named `{name}`."))
                }),
            None => {
                let position = plan.target.default_environment.get();
                if position < plan.environments.len() {
                    Ok(position)
                } else {
                    Err(codes::job_invalid(
                        "The plan's default environment is not in the plan.",
                    ))
                }
            }
        }
    }

    /// Carries out one job. Returns when the job ends, stops, rests, or its
    /// lease is about to end.
    ///
    /// The future must be driven to its end: the engine never drops the
    /// future of a step that commits, and a caller that drops this one
    /// while such a step is in flight leaves the record in doubt, to be
    /// reconciled.
    pub async fn run_job(&self, job: &JobSpec, control: &dyn Control) -> JobOutcome {
        self.run_job_with(job, control, None).await
    }

    /// Runs an offline test's job: the job as [`Engine::run_job`] runs it,
    /// with every binding handed `replay` in place of its provider's driver,
    /// through a handle that checks each step against the provider's
    /// declared step kinds and codes. No session is asked for and no
    /// provider's driver is prepared, so nothing leaves the machine
    /// (decision 0177). A write commits only where `replay` answers its
    /// committing step, and the control plane decides whether it is cleared
    /// to: a replay is never a reason to clear one.
    pub async fn run_offline(
        &self,
        job: &JobSpec,
        control: &dyn Control,
        replay: Arc<dyn a2m_provider::Driver>,
    ) -> JobOutcome {
        self.run_job_with(job, control, Some(replay)).await
    }

    async fn run_job_with(
        &self,
        job: &JobSpec,
        control: &dyn Control,
        replay: Option<Arc<dyn a2m_provider::Driver>>,
    ) -> JobOutcome {
        let claimed = job
            .snapshot
            .as_ref()
            .map_or(RunState::Queued, |snapshot| snapshot.state);
        let ToolRef::Tool(name) = &job.tool else {
            if replay.is_some() {
                // Discovery observes a live system; there is no replay of it.
                return JobOutcome::Left(codes::job_invalid(
                    "An offline test runs a tool, and this job names a surface to observe.",
                ));
            }
            return self.discover(job, control).await;
        };
        // A call to the control plane that never answers is given up.
        let timed = Timed::new(control, self.deps.settings.control_timeout);
        let control: &dyn Control = &timed;
        let keeps_state = job.kind == JobKind::VerifyAfterLoss;
        let give_up = |state: RunState, failure: Failure| {
            if keeps_state {
                JobOutcome::Left(failure)
            } else {
                failed(state, failure)
            }
        };
        if job.kind == JobKind::Discover {
            return give_up(
                claimed,
                codes::job_invalid("A discovery job names a surface, and this one names a tool."),
            );
        }
        // A new run is `validating` from here on: the plan, the
        // environment, and the tool are checked as part of it, so that a
        // job that names one the plan does not hold ends `rejected`, which
        // a run that is still `queued` cannot.
        let mut claimed = claimed;
        if job.snapshot.is_none() {
            if let Err(failure) = control.send(EngineEvent::Moved(RunState::Validating)).await {
                return give_up(claimed, failure);
            }
            claimed = RunState::Validating;
        }
        let loaded = match self.plan(&job.plan, control).await {
            Ok(loaded) => loaded,
            Err(failure) => return give_up(claimed, failure),
        };
        let position = match Self::environment(&loaded, job) {
            Ok(position) => position,
            Err(failure) => return give_up(claimed, failure),
        };
        let posture = if job.kind == JobKind::Try {
            // Every driver of a try is prepared with its own read-only
            // guard on. The context in which a declared test record is
            // written is the one exception, and it is prepared for that
            // record's write alone (`Run::hold`).
            Posture::Sandbox {
                test_records: false,
            }
        } else {
            Posture::Live
        };
        let trace = TraceBuilder::new(
            TraceIdentity {
                workspace: job.workspace,
                run: job.run,
                attempt: job.attempt,
                operation: name.clone(),
                plan: loaded.hash,
                engine: self.deps.settings.version,
                registry: self.deps.registry.digest(),
            },
            job.capture,
            Arc::clone(&self.deps.redact),
            Arc::clone(&self.deps.keyed),
        );
        let mut run = Run::new(self, job, control, loaded, position, posture, Some(trace));
        run.state = claimed;
        run.replay = replay;
        let result = tool::run_tool(&mut run, name).await;
        run.release().await;
        if let Some(trace) = run.trace.take() {
            // A trace that still holds a registered secret is dropped, not
            // sent: nothing of it leaves the engine.
            if let Ok(finished) = trace.finish() {
                run.tell(EngineEvent::Trace(finished)).await;
            }
        }
        match result {
            Ok(Done::Completed) => JobOutcome::Finished {
                state: RunState::Completed,
                failure: None,
            },
            Ok(Done::NeedsApproval) => JobOutcome::Rests {
                state: RunState::NeedsApproval,
                failure: None,
            },
            Ok(Done::Settled { remaining }) => JobOutcome::Settled { remaining },
            Err(Stop::LeaseEnding) => JobOutcome::LeaseEnding,
            Err(Stop::Cancelled) => JobOutcome::Finished {
                state: RunState::Cancelled,
                failure: None,
            },
            Err(Stop::Fail(failure)) => give_up(run.state, failure),
        }
    }
}
