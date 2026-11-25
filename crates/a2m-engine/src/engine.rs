//! The engine: one job in, one outcome out.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use a2m_core::{
    CompiledTarget, Digest, Failure, FailureClass, KeyedDigest, OperationName, Redact, RunState,
    SurfaceName, ToolRuns, Version,
};
use a2m_logs::{Clock, TraceBuilder, TraceIdentity};
use a2m_provider::{Exclusive, Posture, Registry};
use a2m_sources::Limits;

use crate::codes;
use crate::control::{ControlPort, EngineEvent};
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
    /// How long a driver may take to prepare when its mode's manifest
    /// states no `prepare_within_ms` of its own. A mode that states one is
    /// given that, up to the kernel's ceiling (decision 0427).
    pub prepare_timeout: Duration,
    /// The time limit of a step whose compiled target states none.
    pub step_timeout: Duration,
    /// How long the engine waits for one call to `ControlPort` to be answered.
    /// An implementation that waits (for a lock, a lease, a rate budget)
    /// keeps its own, shorter limit; this one ends a call that never
    /// answers.
    pub control_timeout: Duration,
    /// What reading a source may use up.
    pub source_limits: Limits,
    /// How many compiled targets are kept loaded.
    pub compiled_targets_kept: usize,
    /// The engine's version, for the trace.
    pub version: Version,
    /// The one budget of time every binding in shadow of a job shares,
    /// their reads and the keeping of their comparisons together
    /// (decisions 0207 and 0251). The call is never held longer by its
    /// shadows, and less when its own time limit or lease ends first: a
    /// shadow that passes it is cancelled and counted as not compared, and
    /// a comparison that cannot be kept within it is dropped and counted.
    pub shadow_limit: Duration,
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
            compiled_targets_kept: 16,
            version: Version::new(0, 0, 1),
            shadow_limit: Duration::from_secs(2),
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

/// The engine. It holds the compiled targets it has loaded and nothing else
/// between jobs: the ledger, reached through `ControlPort`, is the authority.
pub struct Engine {
    pub(crate) deps: EngineDeps,
    compiled_targets: Mutex<Vec<Arc<Loaded>>>,
    /// Shadow comparisons that could not be kept within the job's shadow
    /// budget, or whose keeping failed (decision 0251).
    pub(crate) shadows_lost: AtomicU64,
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
            compiled_targets: Mutex::new(Vec::new()),
            shadows_lost: AtomicU64::new(0),
        }
    }

    /// How many shadow comparisons this engine made and could not keep:
    /// their keeping failed, or did not end within what was left of the
    /// job's shadow budget. Each was dropped; the call it was made beside
    /// was not changed (decision 0251).
    #[must_use]
    pub fn shadow_comparisons_lost(&self) -> u64 {
        self.shadows_lost.load(Ordering::Relaxed)
    }

    /// The compiled target by its hash, fetched through `ControlPort` and checked
    /// against the hash before it is used.
    pub(crate) async fn compiled_target(
        &self,
        hash: &Digest,
        control: &dyn ControlPort,
    ) -> Result<Arc<Loaded>, Failure> {
        {
            let compiled_targets = self
                .compiled_targets
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(found) = compiled_targets.iter().find(|loaded| loaded.hash == *hash) {
                return Ok(Arc::clone(found));
            }
        }
        let compiled_target = control.compiled_target(hash).await?;
        let loaded = Arc::new(Loaded::load(compiled_target, hash)?);
        let mut compiled_targets = self
            .compiled_targets
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if compiled_targets.len() >= self.deps.settings.compiled_targets_kept.max(1) {
            compiled_targets.remove(0);
        }
        compiled_targets.push(Arc::clone(&loaded));
        Ok(loaded)
    }

    /// What a job that carries out `tool` must hold alone, when the tool
    /// writes: for every binding that may write, what its mode declares one
    /// write holds alone, on the binding's surface. In the order of the kinds
    /// and then of the surfaces' names, each once. Empty for a tool that only
    /// reads, and for a tool the compiled target does not hold.
    ///
    /// A binding's mode is looked up by name in the registry, as data; a
    /// mode no enabled provider declares can never be chosen and adds
    /// nothing.
    #[must_use]
    pub fn exclusive_needs(
        &self,
        compiled_target: &CompiledTarget,
        tool: &OperationName,
    ) -> Vec<ExclusiveNeed> {
        let operations: Vec<_> = match compiled_target.tool(tool).map(|tool| tool.runs) {
            Some(ToolRuns::Operation(index)) => {
                compiled_target.operation(index).into_iter().collect()
            }
            Some(ToolRuns::Pipeline(index)) => compiled_target
                .pipeline(index)
                .into_iter()
                .flat_map(|pipeline| &pipeline.steps)
                .filter_map(|step| compiled_target.operation(step.operation))
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
            let Some(surface) = compiled_target.surface(binding.surface) else {
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

    /// The environment a job names, as its position in the compiled target.
    pub(crate) fn environment(loaded: &Loaded, job: &JobSpec) -> Result<usize, Failure> {
        let compiled_target = &loaded.compiled_target;
        match &job.environment {
            Some(name) => compiled_target
                .environments
                .iter()
                .position(|environment| &environment.name == name)
                .ok_or_else(|| {
                    codes::job_invalid(format!(
                        "The compiled target holds no environment named `{name}`."
                    ))
                }),
            None => {
                let position = compiled_target.target.default_environment.get();
                if position < compiled_target.environments.len() {
                    Ok(position)
                } else {
                    Err(codes::job_invalid(
                        "The compiled target's default environment is not in the compiled target.",
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
    pub async fn run_job(&self, job: &JobSpec, control: &dyn ControlPort) -> JobOutcome {
        self.run_job_with(job, control, None).await
    }

    /// Runs an offline test's job: the job as [`Engine::run_job`] runs it,
    /// with every binding handed `replay`, a [`a2m_provider::Replay`] of
    /// recorded answers and nothing else, in place of its provider's driver,
    /// through a handle that checks each step against the provider's
    /// declared step kinds and codes. No session is asked for and no
    /// provider's driver is prepared, so nothing leaves the machine
    /// (decision 0177). A write commits only where `replay` answers its
    /// committing step, and the control plane decides whether it is cleared
    /// to: a replay is never a reason to clear one.
    pub async fn run_offline(
        &self,
        job: &JobSpec,
        control: &dyn ControlPort,
        replay: Arc<a2m_provider::Replay>,
    ) -> JobOutcome {
        // Typed as the replay itself, never any driver: a pack's driver
        // handed here would let a binding not yet verified act on a live
        // system (audit finding EN-1).
        self.run_job_with(job, control, Some(replay as Arc<dyn a2m_provider::Driver>))
            .await
    }

    async fn run_job_with(
        &self,
        job: &JobSpec,
        control: &dyn ControlPort,
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
        let control: &dyn ControlPort = &timed;
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
        // A new run is `validating` from here on: the compiled target, the
        // environment, and the tool are checked as part of it, so that a job
        // that names one the compiled target does not hold ends `rejected`,
        // which a run that is still `queued` cannot.
        let mut claimed = claimed;
        // So is a run stopped before anything was planned, such as a write
        // a drift or an edge block stopped at its preview: resumed, it is
        // validated again, and its preview stores its records (decision
        // 0381, the state rule).
        let unplanned = job.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.state == RunState::Stopped
                && snapshot.diff.is_none()
                && snapshot.records.is_empty()
        });
        if job.snapshot.is_none() || unplanned {
            if let Err(failure) = control.send(EngineEvent::Moved(RunState::Validating)).await {
                return give_up(claimed, failure);
            }
            claimed = RunState::Validating;
        }
        let loaded = match self.compiled_target(&job.compiled_target, control).await {
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
                compiled_target: loaded.hash,
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
