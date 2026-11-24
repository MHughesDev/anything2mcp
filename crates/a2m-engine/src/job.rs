//! One job in, one outcome out: the data a job is, and how it can end.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use a2m_core::{
    Digest, Environment, Epoch, Failure, FailureCode, Identity, Mode, OperationName, RecordKey,
    RecordState, RunId, RunState, SessionState, SurfaceName, Timestamp, Values, WorkspaceId,
};
use a2m_ledger::RecordRef;

use crate::diff::Diff;
use a2m_telemetry::CaptureLevel;
use tokio::time::Instant;

/// What a job is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobKind {
    /// Build the diff of a write and stop before anything is written.
    Preview,
    /// Carry the tool out: a read at once; a write through its preview, the
    /// approval check, and the write path.
    Execute,
    /// A trial of a draft in the sandbox, where only declared test records
    /// may be written.
    Try,
    /// Read the records in doubt and settle them; write nothing.
    Reconcile,
    /// Read the records a lost run verified once more; change nothing.
    VerifyAfterLoss,
    /// A read of a binding, to see whether it still works.
    Canary,
    /// Observe a surface with the registry's discoverers.
    Discover,
}

/// What a job runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolRef {
    /// A tool of the compiled target: an operation or a pipeline.
    Tool(OperationName),
    /// A surface of the compiled target, for a `Discover` job.
    Surface(SurfaceName),
}

/// The kernel's health when the job was claimed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum KernelHealth {
    /// Everything works.
    #[default]
    Ok,
    /// Reads only. The code says what is wrong.
    Degraded(FailureCode),
}

/// What the control plane knew, when the job was claimed, about the things
/// choosing a binding depends on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Conditions {
    /// The kernel's health. A write needs `Ok`.
    pub health: KernelHealth,
    /// The bindings marked as drifted: the operation and the mode.
    pub drifted: Vec<(OperationName, Mode)>,
    /// The surfaces blocked at the edge.
    pub edge_blocked: Vec<SurfaceName>,
    /// The state of the session of each surface that needs one. A surface
    /// that is not listed is taken as usable; the lease decides.
    pub sessions: Vec<(SurfaceName, SessionState)>,
    /// True when policy asks for the freshness check before every write,
    /// whatever the age of the preview.
    pub strict_freshness: bool,
}

/// How many records a job may work on at once. The engine carries records
/// out one after another today, so any grant is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parallelism(pub u32);

impl Default for Parallelism {
    fn default() -> Self {
        Self(1)
    }
}

/// What the latest heartbeat told the runner to do with the job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Directive {
    /// Carry on.
    #[default]
    Continue,
    /// Finish the commit point in hand, record it, and end as cancelled.
    Cancel,
    /// Stop before the next commit point and leave the run resumable.
    Pause,
}

#[derive(Debug)]
struct LeaseState {
    ends: Option<Instant>,
    directive: Directive,
}

/// The job's lease as the engine sees it: the time it has left, on a steady
/// clock, and the latest directive. The runner keeps a clone and updates it
/// from its heartbeats; the engine reads it before every commit point.
#[derive(Debug, Clone)]
pub struct LeaseClock {
    state: Arc<Mutex<LeaseState>>,
}

impl LeaseClock {
    /// A lease that ends at `ends`.
    #[must_use]
    pub fn until(ends: Instant) -> Self {
        Self {
            state: Arc::new(Mutex::new(LeaseState {
                ends: Some(ends),
                directive: Directive::Continue,
            })),
        }
    }

    /// A lease with no end, for a caller that holds the run itself.
    #[must_use]
    pub fn unbounded() -> Self {
        Self {
            state: Arc::new(Mutex::new(LeaseState {
                ends: None,
                directive: Directive::Continue,
            })),
        }
    }

    fn with<T>(&self, f: impl FnOnce(&mut LeaseState) -> T) -> T {
        // The state is two plain values; a holder that panicked cannot have
        // left them half written.
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut state)
    }

    /// Moves the lease's end, after a heartbeat renewed it.
    pub fn renew(&self, ends: Instant) {
        self.with(|state| state.ends = Some(ends));
    }

    /// Sets the directive of the latest heartbeat.
    pub fn direct(&self, directive: Directive) {
        self.with(|state| state.directive = directive);
    }

    /// The latest directive.
    #[must_use]
    pub fn directive(&self) -> Directive {
        self.with(|state| state.directive)
    }

    /// How long the lease has left; `None` when it has no end.
    #[must_use]
    pub fn remaining(&self) -> Option<Duration> {
        self.with(|state| state.ends)
            .map(|ends| ends.saturating_duration_since(Instant::now()))
    }

    /// Whether the lease has more than `needed` left.
    #[must_use]
    pub fn leaves(&self, needed: Duration) -> bool {
        self.remaining().is_none_or(|left| left > needed)
    }
}

/// One record of a run as the ledger held it when the job was claimed, with
/// its stored values read back into data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordSnapshot {
    /// Which commit point.
    pub record: RecordRef,
    /// The record's key.
    pub key: RecordKey,
    /// The operation that writes it.
    pub operation: OperationName,
    /// How far it got.
    pub state: RecordState,
    /// The digest of the intended values.
    pub intended: Digest,
    /// The intended values, as [`crate::decode_values`] reads the stored
    /// blob.
    pub intended_values: Values,
    /// The values seen before the write; `None` when the record did not
    /// exist.
    pub before: Option<Values>,
    /// The record's idempotency token.
    pub idempotency_token: String,
    /// The key the system assigned, once known.
    pub assigned_key: Option<String>,
    /// The record's outputs, stored when it was marked `acted`.
    pub outputs: Option<Values>,
    /// Values recorded before the commit point, for reconcile.
    pub before_commit: Option<Values>,
    /// The mode of the binding its intent was stored for, when one was.
    pub mode: Option<Mode>,
    /// When `failed`, the class and code.
    pub failure: Option<(a2m_core::FailureClass, FailureCode)>,
    /// True when the record ended `acted` on the system's acknowledgement:
    /// settled, never read back, and skipped on a resume (decision 0172).
    pub acknowledged: bool,
    /// When the ledger stored the intent of the record's current attempt.
    pub intent_at: Option<Timestamp>,
    /// When the ledger last recorded what followed the current attempt's
    /// send: its `acted` mark, or the failure that ended the attempt. The
    /// send was made before it. `None` when neither was recorded; then the
    /// send was made, if at all, within the time the binding's act steps
    /// may take after `intent_at`.
    pub sent_by: Option<Timestamp>,
}

/// A run that has been worked on before, as the ledger holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSnapshot {
    /// The run's state when the job was claimed.
    pub state: RunState,
    /// The hash of the stored diff, once a preview has stored one.
    pub diff: Option<Digest>,
    /// Whether the run was cleared to write before: it entered `running`
    /// through the approval check on an earlier attempt.
    pub cleared: bool,
    /// Every record of the run, in the ledger's order.
    pub records: Vec<RecordSnapshot>,
    /// The stored diff the run was previewed with, read back: what a
    /// person approved, binding by binding. `None` before a preview stored
    /// one; a run that writes without it stops as a stale approval.
    pub approved: Option<Diff>,
}

/// Everything about a job that is data. The runner fills it when it claims
/// the job.
#[derive(Debug, Clone)]
pub struct JobSpec {
    /// The run.
    pub run: RunId,
    /// The workspace the run belongs to.
    pub workspace: WorkspaceId,
    /// Which attempt of the run this is, from 1.
    pub attempt: u32,
    /// The lease number the job runs under. Intent tokens carry the same.
    pub epoch: Epoch,
    /// What the job is for.
    pub kind: JobKind,
    /// The compiled target's hash. The compiled target is fetched through
    /// `Control` and checked.
    pub compiled_target: Digest,
    /// What the job runs.
    pub tool: ToolRef,
    /// The environment; `None` for the target's default.
    pub environment: Option<Environment>,
    /// Who the system sees, for the diff.
    pub identity: Option<Identity>,
    /// The stored input.
    pub input: Values,
    /// The bytes of the source input, read once and stored by the caller.
    pub source: Option<Arc<Vec<u8>>>,
    /// On resume: what the ledger holds of the run. `None` for a run that
    /// is new: nothing was planned and nothing stored.
    pub snapshot: Option<RunSnapshot>,
    /// What choosing depends on, read at claim.
    pub conditions: Conditions,
    /// A caller or a setting pinned one mode. Compared by name only.
    pub pin: Option<Mode>,
    /// A test, a trial, or a recorded fixture runs this one binding of its
    /// operation and no other: nothing falls back to another binding, and
    /// a binding that is no longer the one named stops the job as
    /// `stale`/`binding_changed` (audit finding AC-6).
    pub binding: Option<crate::binding::BindingPin>,
    /// How many records may be worked on at once.
    pub parallelism: Parallelism,
    /// The lease's remaining time and the latest directive.
    pub lease: LeaseClock,
    /// How much evidence the run keeps.
    pub capture: CaptureLevel,
}

/// How a job ended.
///
/// The engine's caller applies the outcome: it moves the run to
/// [`JobOutcome::run_state`] when that differs from the state the run is
/// in, with the failure the outcome carries. The engine itself moves a run
/// only to `validating` and `running`, through `Control`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobOutcome {
    /// The run reached a final state: `completed`, `failed`, `rejected`, or
    /// `cancelled`. `failed` and `rejected` carry the failure.
    Finished {
        /// The final state.
        state: RunState,
        /// The failure that ended it, when one did.
        failure: Option<Failure>,
    },
    /// The run waits for a person: `needs_approval`, `needs_input`, or
    /// `needs_login`.
    Rests {
        /// The state it rests in.
        state: RunState,
        /// Why, when a failure brought it there.
        failure: Option<Failure>,
    },
    /// The run stopped with a failure class and can be resumed.
    Stopped(Failure),
    /// The engine stopped acting before the lease ran out, or was told to
    /// pause. Nothing was left half sent; the run is as the ledger has it.
    LeaseEnding,
    /// A `Reconcile` or `VerifyAfterLoss` job ended: every record in doubt
    /// was read and settled, and nothing was written. The run's state is
    /// not changed.
    Settled {
        /// How many records are still to be written.
        remaining: u32,
    },
    /// The job could not be carried out, and the run's state is not to be
    /// changed: a `VerifyAfterLoss` or `Discover` job that failed.
    Left(Failure),
}

impl JobOutcome {
    /// The state the run is left in; `None` when it stays as it was.
    #[must_use]
    pub const fn run_state(&self) -> Option<RunState> {
        match self {
            Self::Finished { state, .. } | Self::Rests { state, .. } => Some(*state),
            Self::Stopped(_) => Some(RunState::Stopped),
            Self::LeaseEnding | Self::Settled { .. } | Self::Left(_) => None,
        }
    }

    /// The failure the outcome carries, when it carries one.
    #[must_use]
    pub const fn failure(&self) -> Option<&Failure> {
        match self {
            Self::Finished { failure, .. } | Self::Rests { failure, .. } => failure.as_ref(),
            Self::Stopped(failure) | Self::Left(failure) => Some(failure),
            Self::LeaseEnding | Self::Settled { .. } => None,
        }
    }
}
