//! `Control`: everything the engine needs from the control plane while it
//! runs one job, and nothing more.
//!
//! In one process the action registry implements it by calling the
//! scheduler, the keeper, the policy engine, and the store. On a runner it
//! is implemented over the runner link. The engine never learns which.
//!
//! Every call is made for the job the implementation was built for: it
//! attaches the run and the lease number itself, so the engine can neither
//! forge nor forget them.

use std::time::Duration;

use a2m_auth::{ExpirySign, SessionLease};
use a2m_core::{
    Digest, EffectClass, Environment, Failure, FailureClass, FailureCode, Finding, Hazard, Mode,
    OperationName, Plan, RunState, SurfaceName, Values, Version,
};
use a2m_ledger::{
    Acted, ActedToken, BlobKind, BlobRef, IntentToken, LockHeld, PlannedRecord, ReadBack,
    Reconciled, RecordIntent, RecordRef, RetentionClass, Settled,
};
use a2m_provider::{BoxFuture, DiscoveryReport};
use a2m_telemetry::{ChoiceReason, FinishedTrace};
use std::sync::Arc;

/// The question the engine asks at guard: may this call happen. The
/// implementation turns it into a request to the policy engine, which
/// decides and records the decision. The engine never sees a policy type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardQuestion {
    /// The tool that was called.
    pub tool: OperationName,
    /// The environment the run acts in.
    pub environment: Environment,
    /// The tool's effect class.
    pub effect: EffectClass,
    /// The tool's hazard.
    pub hazard: Hazard,
    /// Whether the job may write: false for a preview, a read, a reconcile
    /// that only reads, and everything in the sandbox.
    pub writes: bool,
    /// How many records the run would touch, when the source has been read.
    pub records: Option<u32>,
}

/// A request for a session lease on the surface a binding acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionWant {
    /// The surface.
    pub surface: SurfaceName,
    /// The time the session must still have left, when a pipeline says so.
    pub headroom: Option<Duration>,
    /// True when the engine asks again after it reported a sign of expiry
    /// on the lease it held. A refusal now rests the run at `needs_login`.
    pub again: bool,
}

/// What the preview found: the commit points, and the diff a person
/// approves. Stored in one transaction, while the run is `validating`.
#[derive(Debug, Clone)]
pub struct Preview {
    /// Every commit point, `planned` or `unchanged`.
    pub records: Vec<PlannedRecord>,
    /// The diff's hash. The engine alone computes it.
    pub diff: Digest,
    /// The stored diff, as [`crate::Diff::encode`] writes it.
    pub diff_blob: BlobRef,
}

/// The answer to the approval check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clearance {
    /// The run may write: an approval bound to this diff was used, or a
    /// recorded decision allowed the write without a person, and the run
    /// moved to `running` in the same transaction.
    Cleared,
    /// Nobody has approved this diff yet, and a person must. The run rests
    /// at `needs_approval`.
    Waits,
}

/// A piece of evidence to keep. Its bytes are already redacted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    /// What it holds.
    pub kind: BlobKind,
    /// How long it is kept.
    pub class: RetentionClass,
    /// The content.
    pub bytes: Vec<u8>,
}

/// A binding the engine passed over, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassedOver {
    /// The binding's mode.
    pub mode: Mode,
    /// The class of the reason.
    pub class: FailureClass,
    /// The code of the reason.
    pub code: FailureCode,
}

/// Something the engine tells the control plane while it runs a job.
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// The run moves to `validating`, or, for a run that needs no approval
    /// (a read, a write with nothing to change, a resumed run that was
    /// cleared before and has been reconciled), to `running`. Every other
    /// state is the job's outcome, which the engine's caller applies.
    Moved(RunState),
    /// A pipeline step began.
    StepStarted {
        /// The step's position in the pipeline, from 0. An operation called
        /// alone is step 0.
        step_seq: u32,
        /// The operation the step calls.
        operation: OperationName,
    },
    /// A binding was chosen for an operation call.
    BindingChosen {
        /// The operation.
        operation: OperationName,
        /// The mode of the binding chosen.
        mode: Mode,
        /// Why that binding.
        reason: ChoiceReason,
        /// The provider whose driver carries it out.
        provider: String,
        /// The provider's version.
        provider_version: Version,
        /// The digest of the provider's manifest.
        provider_digest: Digest,
        /// The bindings before it in the plan's order that did not qualify.
        passed_over: Vec<PassedOver>,
    },
    /// Values a step learned before its commit point, which reconcile will
    /// need. Durable before the committing step is sent.
    BeforeCommit {
        /// The commit point.
        record: RecordRef,
        /// The stored values, as [`crate::encode_values`] writes them.
        outputs: BlobRef,
    },
    /// What the tool returns: the outputs of a read, or of the last step.
    Result(Values),
    /// A binding failed in a way that says something about the binding
    /// itself, or worked after it was thought unhealthy.
    BindingHealth {
        /// The operation.
        operation: OperationName,
        /// The binding's mode.
        mode: Mode,
        /// The surface it acts on.
        surface: SurfaceName,
        /// The class and code of the failure; `None` when it worked.
        failure: Option<(FailureClass, FailureCode)>,
    },
    /// The finished step trace of this attempt, with its signature.
    Trace(FinishedTrace),
    /// A record the ledger holds as verified no longer reads as it was
    /// written. Sent by a `VerifyAfterLoss` job; it changes no state.
    VerifiedDiffers {
        /// The commit point.
        record: RecordRef,
        /// The evidence of the read.
        evidence: BlobRef,
    },
    /// What one discoverer found on a surface, through the sandbox.
    Discovered {
        /// The surface observed.
        surface: SurfaceName,
        /// The provider that brought the discoverer.
        provider: String,
        /// The discoverer's name.
        discoverer: String,
        /// Its findings, each already checked against its manifest.
        findings: Vec<Finding>,
        /// How it ended: the coverage, the bounds it hit, what it dropped.
        /// `Err` when it failed; the findings made before stand.
        report: Result<DiscoveryReport, Failure>,
    },
}

impl EngineEvent {
    /// Whether the engine waits for this event to be stored before it goes
    /// on, and stops when it cannot be. The others are sent and, if they
    /// are lost, the run is not harmed.
    #[must_use]
    pub const fn must_be_acknowledged(&self) -> bool {
        matches!(
            self,
            Self::Moved(_) | Self::BeforeCommit { .. } | Self::Result(_) | Self::Discovered { .. }
        )
    }
}

/// Everything the engine needs from the control plane while it runs a job.
///
/// Every method returns a boxed future, so the trait is a trait object and
/// needs no macro; an implementation writes `Box::pin(async move { ... })`.
///
/// What an implementation must hold to:
///
/// - Every call is for the job's run under the job's lease number. A call
///   under a lease that is no longer the run's is refused with
///   `possible_commit`, `runner_lost`.
/// - `intend` returns only after the intents are durable.
/// - A method that waits (a lease, a lock, a rate budget) waits up to its
///   own limit and then fails; the engine holds no timer for it.
/// - A failure it returns has passed the redactor.
pub trait Control: Send + Sync {
    /// The plan by its hash. The engine checks the hash again before it
    /// uses the plan.
    fn plan<'a>(&'a self, hash: &'a Digest) -> BoxFuture<'a, Result<Arc<Plan>, Failure>>;

    /// The policy decision at guard, made and recorded behind this call.
    /// `Ok` means allowed.
    fn decide<'a>(&'a self, question: &'a GuardQuestion) -> BoxFuture<'a, Result<(), Failure>>;

    /// A session lease for the surface. The keeper checks and renews behind
    /// it if it must. A refusal is of class `session_expired`.
    fn session_lease<'a>(
        &'a self,
        want: &'a SessionWant,
    ) -> BoxFuture<'a, Result<SessionLease, Failure>>;

    /// Relays a driver's sign that the session has ended to the keeper. The
    /// engine never renews; it asks for a lease again afterwards.
    fn report_expiry<'a>(&'a self, lease: &'a SessionLease, sign: ExpirySign) -> BoxFuture<'a, ()>;

    /// A rate budget of up to `want` requests on the surface, waiting
    /// through the scheduler up to its limit. Answers how many were
    /// granted, at least one.
    fn rate<'a>(
        &'a self,
        surface: &'a SurfaceName,
        want: u32,
    ) -> BoxFuture<'a, Result<u32, Failure>>;

    /// Stores what the preview found: the commit points and the diff.
    fn store_preview<'a>(&'a self, preview: &'a Preview) -> BoxFuture<'a, Result<(), Failure>>;

    /// The approval check for the run's diff. `Cleared` means the approval
    /// was used, or a recorded decision allowed the write, and the run is
    /// now `running`, all in one transaction.
    ///
    /// A failure is of class `policy`: `approval_stale` when the approval
    /// was given for another diff, `approval_missing` when it was revoked,
    /// expired, or used before.
    fn approval<'a>(&'a self, diff: &'a Digest) -> BoxFuture<'a, Result<Clearance, Failure>>;

    /// Takes the lock on a record, or confirms it under this lease, waiting
    /// through the scheduler up to its limit; then `busy`, `record_locked`.
    fn lock<'a>(&'a self, record: RecordRef) -> BoxFuture<'a, Result<LockHeld, Failure>>;

    /// Stores intents and gives their tokens, in the order asked. Durable
    /// before it returns.
    fn intend<'a>(
        &'a self,
        intents: Vec<RecordIntent>,
    ) -> BoxFuture<'a, Result<Vec<IntentToken>, Failure>>;

    /// Marks a record `acted`: its write was sent. Consumes the token.
    fn acted<'a>(
        &'a self,
        token: IntentToken,
        acted: Acted,
    ) -> BoxFuture<'a, Result<ActedToken, Failure>>;

    /// Marks a record `verified`, with what was read back and its evidence.
    /// The ledger compares the digests itself and releases the lock.
    fn verified<'a>(
        &'a self,
        token: &'a ActedToken,
        read_back: ReadBack,
    ) -> BoxFuture<'a, Result<(), Failure>>;

    /// Marks a record `failed` with the failure that stopped it.
    fn failed<'a>(
        &'a self,
        record: RecordRef,
        why: &'a Failure,
        evidence: Option<BlobRef>,
    ) -> BoxFuture<'a, Result<(), Failure>>;

    /// Settles a record in doubt with what reading it showed. The lock must
    /// have been taken or confirmed under this lease first.
    fn settle<'a>(
        &'a self,
        record: RecordRef,
        seen: Reconciled,
    ) -> BoxFuture<'a, Result<Settled, Failure>>;

    /// Sends one event.
    fn send<'a>(&'a self, event: EngineEvent) -> BoxFuture<'a, Result<(), Failure>>;

    /// Stores a piece of evidence and gives its name.
    fn evidence<'a>(&'a self, blob: Evidence) -> BoxFuture<'a, Result<BlobRef, Failure>>;
}
