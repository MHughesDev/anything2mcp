//! `ControlPort`: everything the engine needs from the control plane while it
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
    CompiledTarget, Coverage, Digest, EffectClass, Environment, Failure, FailureClass, FailureCode,
    Finding, Hazard, Mode, OperationName, RunState, SurfaceName, Values, Version,
};
use a2m_ledger::{
    Acted, ActedToken, BlobKind, BlobRef, IntentToken, LockHeld, PlannedRecord, ReadBack,
    Reconciled, RecordIntent, RecordRef, RetentionClass, Settled, ShadowOutcome,
};
use a2m_logs::{ChoiceReason, FinishedTrace, Phase};
use a2m_provider::{BoxFuture, DiscoveryReport};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::codes;

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
    /// that only reads, and discovery. A try of a write says true: it may
    /// write a declared test record.
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

/// A step's capture, to keep as evidence of the run: tied to the run (the
/// implementation's own), to the record when the step belongs to one, and
/// to the step by its pass, phase, and position. Its bytes have passed the
/// redactor and the run's capture level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptCapture {
    /// The commit point the step belongs to, when it belongs to one.
    pub record: Option<RecordRef>,
    /// The pass of the trace the step was carried out in.
    pub pass: u32,
    /// The step's phase.
    pub phase: Phase,
    /// The step's position in its phase, from 0.
    pub step: u32,
    /// The capture's name, as the driver gave it, redacted.
    pub name: String,
    /// Its media type.
    pub media_type: String,
    /// Its bytes, redacted.
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

/// How a binding in shadow compared with the binding that did the real work
/// of a live read (decision 0207). The implementation keeps it in the
/// ledger with the job's run and lease, through `Shadows::record`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowCompared {
    /// The operation called.
    pub operation: OperationName,
    /// The shadow binding: its place, its mode, and its digest.
    pub shadow: crate::BindingPin,
    /// The binding that did the real work, whose outputs the caller was
    /// given.
    pub verified: crate::BindingPin,
    /// Whether the operation writes. Always false today: the shadow of a
    /// write is not built.
    pub writes: bool,
    /// How it compared: the names of the outputs that differed and how,
    /// never a value.
    pub outcome: ShadowOutcome,
    /// The inputs of the call it was made beside. The ledger keeps only
    /// the workspace's keyed digest of them, which is never shown: what
    /// lets it count distinct inputs (decision 0251).
    pub input: Values,
    /// What the shadow read cost the system, in the rate budget's units:
    /// one per step it was granted (decision 0251).
    pub requests: u32,
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
        /// The bindings before it in the compiled target's order that did not
        /// qualify.
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
    Result {
        /// The outputs.
        values: Values,
        /// Whether they hold every record the system has that matches the
        /// input (decision 0472): what the steps of a tool that is one read,
        /// called once, stated; `unknown` for any other tool.
        coverage: Coverage,
    },
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
        /// After a drift, the knowledge it was seen in; after a canary's
        /// pass, everything the pass relied on, which it vouches for again
        /// (decision 0380). Empty when nothing can be named.
        knowledge: Vec<crate::DriftKnowledge>,
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
            Self::Moved(_)
                | Self::BeforeCommit { .. }
                | Self::Result { .. }
                | Self::Discovered { .. }
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
///   own limit and then fails. The engine gives any call up after the
///   longer time its settings name, and treats it as failed: the future is
///   dropped, so what the call had begun must stand or fall as one
///   transaction.
/// - A failure it returns has passed the redactor.
pub trait ControlPort: Send + Sync {
    /// The compiled target by its hash. The engine checks the hash again
    /// before it uses the compiled target.
    fn compiled_target<'a>(
        &'a self,
        hash: &'a Digest,
    ) -> BoxFuture<'a, Result<Arc<CompiledTarget>, Failure>>;

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

    /// Keeps a step's capture as evidence of the run, tied to the run, the
    /// record, and the step, and gives its name. The trace record of the
    /// step names it. A capture taken before a committing step is kept
    /// before that step is sent; one that cannot be kept is dropped, and
    /// the run goes on: a capture is evidence, never what a write depends
    /// on.
    ///
    /// By default the control plane keeps no captures, and says so with
    /// `busy`, `capture_not_kept`.
    fn keep_capture<'a>(&'a self, capture: KeptCapture) -> BoxFuture<'a, Result<BlobRef, Failure>> {
        let _ = capture;
        Box::pin(async move {
            Err(codes::fail(
                &codes::row::CAPTURE_NOT_KEPT,
                "The control plane keeps no captures, so the step's capture was dropped.",
                "Nothing to do; the run goes on without it.",
            ))
        })
    }

    /// Ends a record whose binding's only evidence is the system's own
    /// acknowledgement (`acknowledged`): the save was sent and answered, and
    /// nothing can be read back or compared. The record stays `acted`, is
    /// never called verified, and its lock is released; a result and a
    /// diff that show it say "accepted by the system, not read back".
    /// `evidence` names what the save's answer showed, when it was kept.
    ///
    /// By default the control plane cannot, and says so with `busy`,
    /// `acknowledgement_not_kept`: the record stays `acted`, in doubt, and
    /// the run stops for reconcile.
    fn acknowledged<'a>(
        &'a self,
        token: &'a ActedToken,
        evidence: Option<BlobRef>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        let _ = (token, evidence);
        Box::pin(async move {
            Err(codes::fail(
                &codes::row::ACKNOWLEDGEMENT_NOT_KEPT,
                "The system accepted the write, and the control plane cannot end a record on an acknowledgement alone, so it stays in doubt.",
                "Look at the system; the run stops until the record is settled.",
            ))
        })
    }

    /// The project folder of the request that started the run, as the
    /// control plane checked it: a full path to a folder that exists and is
    /// none of the engine's own. A pack resolves a file of the project
    /// against it, never against the folder the engine was started in.
    /// `None` when the run has none, and by default.
    fn project_folder(&self) -> Option<PathBuf> {
        None
    }

    /// The files of the project's `inputs` folder that the call which
    /// started a discovery hands its discoverers: each name a discoverer
    /// declares it reads, to the path the call gave, `inputs/` and the
    /// file below it, with forward slashes (decision 0263). The engine
    /// checks each path again under a read-only root of that folder before
    /// any discoverer runs, and hands nothing else. Asked only by a
    /// discovery job; empty when the call named none, and by default.
    fn discovery_files(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    /// The answerer of whether another record of `operation` in the
    /// workspace holds a value a step of `asking` recorded, which the
    /// engine hands that one step (decision 0193). It compares digests of
    /// the values recorded before each other record's save, never hands a
    /// value back, and cannot tell where a value it would compare was
    /// masked. `None` by default: every question answers `CannotTell`.
    fn other_records(
        &self,
        _operation: &OperationName,
        _asking: RecordRef,
    ) -> Option<Arc<dyn a2m_provider::OtherRecords>> {
        None
    }

    /// What one step of a `Try` job that only reads observed: its values,
    /// or `None` for a record it found absent. Told as each such step
    /// ends, in order, so that a control plane recording a fixture from a
    /// trial has every answer in the form a replay gives it back (decision
    /// 0196). The values are the system's: a control plane keeps them in
    /// memory only, scrubs them, and stores none of them as they are.
    /// Nothing by default.
    fn step_observed(&self, _step: &a2m_core::Step, _observed: Option<&Values>) {}

    /// Whether the control plane keeps shadow comparisons. Only then does
    /// the engine run a binding in shadow beside a live read: a shadow that
    /// could not be counted would cost the system for nothing. False by
    /// default.
    fn keeps_shadow(&self) -> bool {
        false
    }

    /// A rate budget of `want` requests on the surface for a binding in
    /// shadow: all of them at once, or none. It never waits, and it never
    /// takes a budget below the share it leaves for callers' own reads, as
    /// a watch's check does not (decisions 0247 and 0251): a shadow is a
    /// second read of a person's system, and never takes what a caller's
    /// read needs. Answers whether it was granted.
    ///
    /// By default nothing is granted, so a binding in shadow on a surface
    /// that declares a rate is not compared.
    fn shadow_rate<'a>(
        &'a self,
        surface: &'a SurfaceName,
        want: u32,
    ) -> BoxFuture<'a, Result<bool, Failure>> {
        let _ = (surface, want);
        Box::pin(async { Ok(false) })
    }

    /// Keeps how a binding in shadow compared, in the ledger, under the
    /// job's run and lease. A failure is the engine's to ignore: a shadow
    /// never changes what the caller is given. The engine calls it only
    /// after the call's result was sent, and within what is left of the
    /// job's shadow budget (decision 0251).
    ///
    /// By default the control plane keeps none, and says so with `busy`,
    /// `shadow_not_kept`.
    fn shadow<'a>(&'a self, compared: ShadowCompared) -> BoxFuture<'a, Result<(), Failure>> {
        let _ = compared;
        Box::pin(async move {
            Err(codes::fail(
                &codes::row::SHADOW_NOT_KEPT,
                "The control plane keeps no shadow comparisons.",
                "Nothing to do; the call's result is the verified binding's.",
            ))
        })
    }
}
