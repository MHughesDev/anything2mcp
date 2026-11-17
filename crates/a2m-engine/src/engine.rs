//! The engine: one job in, one outcome out.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use a2m_core::{Digest, Failure, FailureClass, KeyedDigest, Redact, RunState, Version};
use a2m_provider::{Posture, Registry};
use a2m_sources::Limits;
use a2m_telemetry::{Clock, TraceBuilder, TraceIdentity};

use crate::codes;
use crate::control::{Control, EngineEvent};
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
        let claimed = job
            .snapshot
            .as_ref()
            .map_or(RunState::Queued, |snapshot| snapshot.state);
        let ToolRef::Tool(name) = &job.tool else {
            return self.discover(job, control).await;
        };
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
        let loaded = match self.plan(&job.plan, control).await {
            Ok(loaded) => loaded,
            Err(failure) => return give_up(claimed, failure),
        };
        let position = match Self::environment(&loaded, job) {
            Ok(position) => position,
            Err(failure) => return give_up(claimed, failure),
        };
        let posture = if job.kind == JobKind::Try {
            Posture::Sandbox {
                test_records: !loaded.plan.environments[position].test_records.is_empty(),
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
