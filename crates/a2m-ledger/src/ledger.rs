//! Runs, their events, the ledger of commit points with its nine rules, and
//! record locks (spec section 208).
//!
//! Every change appends an event and updates the current row in the same
//! transaction. The rules are enforced twice: here, with a failure that says
//! what was refused, and by the triggers of the migrations, which refuse the
//! same things whoever sends the statement.

use std::collections::{BTreeMap, BTreeSet};

use a2m_core::{
    ApprovalId, DecisionId, Digest, DigestPurpose, EffectClass, Environment, Epoch, Failure,
    FailureClass, FailureCode, Identity, Mode, OperationName, PrincipalId, RecordKey, RecordState,
    RunId, RunState, TargetName, Timestamp, WorkspaceId, WriteEvidence,
};
use serde_json::json;

use crate::blob::{BlobRef, RetentionClass, words};
use crate::codec::{count, digest_of, moment, parse, parse_opt, small, stored};
use crate::failure;
use crate::store::Store;
use crate::tx::{Durability, Row, SqlValue, Tx};

words! {
    /// Whether a run is one operation or a pipeline of them.
    ToolKind {
        /// One operation.
        Operation => "operation",
        /// A pipeline.
        Pipeline => "pipeline",
    }
}

words! {
    /// What kind of thing happened in a run (ledger schema, `run_events`).
    EventKind {
        /// The run moved to another state.
        StateChanged => "state_changed",
        /// A pipeline step began.
        StepStarted => "step_started",
        /// A binding was chosen for a step.
        BindingChosen => "binding_chosen",
        /// An intent was stored for a record.
        Intent => "intent",
        /// A record's write was sent.
        Acted => "acted",
        /// A record was read back and matched.
        Verified => "verified",
        /// A record stopped with a failure class.
        Failed => "failed",
        /// A record in doubt was read and settled.
        ReconcileRead => "reconcile_read",
        /// The run's clearance to write was bound to it.
        ApprovalBound => "approval_bound",
        /// A lease was granted.
        LeaseGranted => "lease_granted",
        /// A lease was lost or given up.
        LeaseLost => "lease_lost",
        /// The engine asked the control plane for something.
        Need => "need",
        /// Evidence that arrived after its lease ended.
        LateEvidence => "late_evidence",
        /// Records were planned from the source.
        Planned => "planned",
        /// The diff of the preview was stored.
        DiffStored => "diff_stored",
        /// A record lock was taken.
        Lock => "lock",
    }
}

/// The right to change a run: the run and the lease number its holder was
/// given. A call that carries another number than the run's current one is
/// refused (rule 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fence {
    /// The run.
    pub run: RunId,
    /// The lease number.
    pub epoch: Epoch,
}

/// A run to open.
#[derive(Debug, Clone)]
pub struct NewRun {
    /// The workspace.
    pub workspace: WorkspaceId,
    /// The caller.
    pub principal: PrincipalId,
    /// Which client made the call. A label, never a decision.
    pub client_label: String,
    /// The tool that was called.
    pub tool_name: OperationName,
    /// Whether it is an operation or a pipeline.
    pub tool_kind: ToolKind,
    /// The target.
    pub target: TargetName,
    /// The published version that runs: its plan's hash.
    pub target_version: Digest,
    /// The environment.
    pub environment: Environment,
    /// Who the system will see.
    pub identity: Option<Identity>,
    /// The tool's effect class.
    pub effect: EffectClass,
    /// The digest of the inputs. For a file input it covers the content.
    pub input_hash: Digest,
    /// The stored inputs.
    pub input: Option<BlobRef>,
    /// How long the run is kept.
    pub retention: RetentionClass,
}

/// What opening a run gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    /// A new run, with the right to change it until a lease is granted.
    New(Fence),
    /// The unfinished run the same caller already has for the same tool
    /// version and inputs (rule 6). Not an error.
    Existing(RunId),
}

impl Opened {
    /// The run, new or not.
    #[must_use]
    pub fn run(&self) -> RunId {
        match self {
            Self::New(fence) => fence.run,
            Self::Existing(run) => *run,
        }
    }
}

/// A run, as the store holds it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    /// The run.
    pub id: RunId,
    /// The workspace.
    pub workspace: WorkspaceId,
    /// The caller.
    pub principal: PrincipalId,
    /// Which client made the call.
    pub client_label: String,
    /// The tool.
    pub tool_name: OperationName,
    /// Operation or pipeline.
    pub tool_kind: ToolKind,
    /// The target.
    pub target: TargetName,
    /// The published version that runs.
    pub target_version: Digest,
    /// The environment.
    pub environment: Environment,
    /// The effect class.
    pub effect: EffectClass,
    /// The digest of the inputs.
    pub input_hash: Digest,
    /// The state: what a caller sees as `status`.
    pub state: RunState,
    /// When stopped or failed, the class and code of the failure.
    pub failure: Option<(FailureClass, FailureCode)>,
    /// The attempt number. It grows on each resume.
    pub attempt: u32,
    /// The current lease number.
    pub epoch: Epoch,
    /// The hash of the diff, once the preview is built.
    pub diff_hash: Option<Digest>,
    /// The stored diff.
    pub diff: Option<BlobRef>,
    /// The approval the run used.
    pub approval: Option<ApprovalId>,
    /// The decision that let the run write without a person.
    pub decision: Option<DecisionId>,
    /// The stored result.
    pub result: Option<BlobRef>,
    /// The stored trace.
    pub trace: Option<BlobRef>,
    /// When the run was opened.
    pub created_at: Timestamp,
    /// When it first ran.
    pub started_at: Option<Timestamp>,
    /// When it reached a final state.
    pub finished_at: Option<Timestamp>,
}

impl RunRow {
    /// Whether the run has been cleared to write (rule 3).
    #[must_use]
    pub fn cleared_to_write(&self) -> bool {
        self.approval.is_some() || self.decision.is_some()
    }
}

/// One thing that happened in a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunEvent {
    /// Its number within the run, from 1, with no gap.
    pub seq: u64,
    /// The lease number of its writer.
    pub epoch: Epoch,
    /// When it was recorded.
    pub at: Timestamp,
    /// What kind of thing happened.
    pub kind: EventKind,
    /// Small JSON, redacted.
    pub payload: String,
    /// Evidence, if any.
    pub evidence: Option<BlobRef>,
}

/// What lets a write run enter `running` for the first time (rule 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteClearance {
    /// An approval a person gave for this run's diff, which the policy
    /// engine marked used in this same transaction.
    Approval(ApprovalId),
    /// A recorded decision that this run may write without a person.
    PreApproved(DecisionId),
    /// The clearance of an offline test's own run, whose every step is
    /// answered by a replay of its fixture: taken only by a store made by
    /// [`StoreConfig::throwaway`](crate::StoreConfig::throwaway), and refused
    /// by every other (decision 0180).
    OfflineTest,
}

/// One commit point of a run: its step and its place within the step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordRef {
    /// The run.
    pub run: RunId,
    /// The pipeline step, from 0.
    pub step_seq: u32,
    /// The record within the step, from 0.
    pub record_seq: u32,
}

/// A commit point known from the source at the preview.
#[derive(Debug, Clone)]
pub struct PlannedRecord {
    /// The pipeline step.
    pub step_seq: u32,
    /// The record within the step.
    pub record_seq: u32,
    /// The record's key.
    pub key: RecordKey,
    /// The operation that writes this commit point.
    pub operation: OperationName,
    /// The digest of the intended values, after normalization. The ledger
    /// compares digests it is given and never the values themselves.
    pub intended: Digest,
    /// The stored intended values.
    pub intended_values: Option<BlobRef>,
    /// The stored values seen before the write.
    pub before: Option<BlobRef>,
    /// True when the record already equals the intended values, so no write
    /// is needed.
    pub unchanged: bool,
}

/// A record of the ledger, as the store holds it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordRow {
    /// Which commit point.
    pub record: RecordRef,
    /// The record's key.
    pub key: RecordKey,
    /// The workspace's keyed digest of the key: what the lock is taken on
    /// and what a trace names the record by.
    pub key_hash: Digest,
    /// The operation that writes it.
    pub operation: OperationName,
    /// How far it got.
    pub state: RecordState,
    /// The digest of the intended values.
    pub intended: Digest,
    /// The stored values seen before the write.
    pub before: Option<BlobRef>,
    /// The stored values read back.
    pub after: Option<BlobRef>,
    /// The record's idempotency token.
    pub idempotency_token: String,
    /// The key the system assigned, once known.
    pub assigned_key: Option<String>,
    /// The record's stored outputs, for steps that depend on it.
    pub outputs: Option<BlobRef>,
    /// The lease number its intent was stored under.
    pub intent_epoch: Option<Epoch>,
    /// How many times an intent was stored for it.
    pub attempts: u32,
    /// When failed, the class and code.
    pub failure: Option<(FailureClass, FailureCode)>,
    /// The evidence of its read-back or of its failure.
    pub evidence: Option<BlobRef>,
    /// The mode of the binding its last intent was stored for: after a
    /// restart, a record in doubt is read through the binding it was
    /// written by.
    pub mode: Option<Mode>,
    /// When its write was sent, in its current attempt: a record that
    /// reached `acted` stays in doubt, whatever it failed with, until it is
    /// read back.
    pub acted_at: Option<Timestamp>,
    /// When it was last read back by reconcile.
    pub reconciled_at: Option<Timestamp>,
    /// When it ended on the system's acknowledgement: its write was sent and
    /// answered, its binding's only evidence is that answer, and nothing was
    /// read back (decision 0172). The record stays `acted`, is never called
    /// verified, and is settled, not in doubt.
    pub acknowledged_at: Option<Timestamp>,
}

impl RecordRow {
    /// Whether the record must be read back before its run writes again
    /// (rule 5): it is in `intent`, or `acted` and did not end on an
    /// acknowledgement; or it `failed` and was not read since, and either
    /// its write was sent or the failure's class allows another try.
    #[must_use]
    pub fn in_doubt(&self) -> bool {
        match self.state {
            RecordState::Intent => true,
            RecordState::Acted => self.acknowledged_at.is_none(),
            RecordState::Failed => {
                self.reconciled_at.is_none()
                    && (self.acted_at.is_some()
                        || self
                            .failure
                            .as_ref()
                            .is_some_and(|(class, _)| !SETTLED_CLASSES.contains(class)))
            }
            _ => false,
        }
    }

    /// Whether the record ended on the system's acknowledgement: accepted
    /// by the system and not read back. Such a record is settled; it is
    /// never verified, and a result that counts it says it was not read
    /// back.
    #[must_use]
    pub fn acknowledged(&self) -> bool {
        self.state == RecordState::Acted && self.acknowledged_at.is_some()
    }

    /// The evidence its write ended on, where the ledger knows it: only
    /// `acknowledged`. A verified record was read back or compared with the
    /// save's answer, and the ledger keeps the digest it compared, not which
    /// of the two gave it.
    #[must_use]
    pub fn ended_on(&self) -> Option<WriteEvidence> {
        self.acknowledged().then_some(WriteEvidence::Acknowledged)
    }
}

/// How a run's records stand, counted: what a result says of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecordTally {
    /// Read back and matched.
    pub verified: u32,
    /// Already held the intended values.
    pub unchanged: u32,
    /// Accepted by the system and not read back (decision 0172).
    pub acknowledged: u32,
    /// Every other record: planned, in doubt, or failed.
    pub open: u32,
}

impl RecordTally {
    /// Whether the run may complete: every record is verified, unchanged,
    /// or acknowledged.
    #[must_use]
    pub fn settled(&self) -> bool {
        self.open == 0
    }
}

/// What the run's own stored diff says of one record: that its write ends
/// on the system's acknowledgement. Made only by
/// [`Store::acknowledged_in_diff`], which opens the diff and reads it; asked
/// for by [`Ledger::mark_acknowledged`], so that the ledger takes the
/// evidence a binding offers from the diff a person approved, never from the
/// caller's word.
#[derive(Debug)]
pub struct AcknowledgedInDiff {
    record: RecordRef,
    diff: BlobRef,
}

impl AcknowledgedInDiff {
    /// Which commit point.
    #[must_use]
    pub fn record(&self) -> RecordRef {
        self.record
    }
}

/// A record about to be written.
#[derive(Debug, Clone)]
pub struct RecordIntent {
    /// Which commit point.
    pub record: RecordRef,
    /// The digest of the intended values. It must be the one that was
    /// planned and approved.
    pub intended: Digest,
    /// The mode of the binding that will write it.
    pub mode: Option<Mode>,
}

/// What the stored intent of one record is, before a token is made of it.
pub(crate) struct StoredIntent {
    record: RecordRef,
    key: RecordKey,
    epoch: Epoch,
    intended: Digest,
    idempotency_token: String,
    intent_seq: u64,
}

/// Proof that one record's intent is durably stored under the current lease
/// (spec section 208, "The intent token").
///
/// Only this crate makes one, and only after the transaction that stored the
/// intent has committed with a sync to disk. It has no public constructor,
/// is not `Clone`, and does not serialize. A driver's handle asks for it
/// before a step that commits; marking the record `acted` consumes it.
///
/// It cannot be built by hand:
///
/// ```compile_fail,E0451
/// fn forge(token: a2m_ledger::IntentToken) -> a2m_ledger::IntentToken {
///     // The one field is private to the crate.
///     a2m_ledger::IntentToken { stored: token.stored }
/// }
/// ```
///
/// and it cannot be copied:
///
/// ```compile_fail,E0599
/// fn twice(token: a2m_ledger::IntentToken) -> (a2m_ledger::IntentToken, a2m_ledger::IntentToken) {
///     (token.clone(), token)
/// }
/// ```
#[must_use = "an intent token is the right to send one write; marking the record acted consumes it"]
pub struct IntentToken {
    stored: StoredIntent,
}

impl IntentToken {
    pub(crate) fn issue(stored: StoredIntent) -> Self {
        Self { stored }
    }

    /// The run.
    #[must_use]
    pub fn run(&self) -> RunId {
        self.stored.record.run
    }

    /// Which commit point.
    #[must_use]
    pub fn record(&self) -> RecordRef {
        self.stored.record
    }

    /// The record's key.
    #[must_use]
    pub fn record_key(&self) -> &RecordKey {
        &self.stored.key
    }

    /// The lease number the intent was stored under.
    #[must_use]
    pub fn epoch(&self) -> Epoch {
        self.stored.epoch
    }

    /// The digest of the intended values.
    #[must_use]
    pub fn intended(&self) -> &Digest {
        &self.stored.intended
    }

    /// The record's idempotency token: stable for the run and the commit
    /// point. A binding sends it where the system takes an idempotency key,
    /// and it is the stamp for `stamped` identity.
    #[must_use]
    pub fn idempotency_token(&self) -> &str {
        &self.stored.idempotency_token
    }
}

impl std::fmt::Debug for IntentToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IntentToken")
            .field("record", &self.stored.record)
            .field("epoch", &self.stored.epoch)
            .finish_non_exhaustive()
    }
}

/// Proof that a record was marked `acted`: what marking it `verified` asks
/// for. Made only by [`Ledger::mark_acted`].
#[must_use = "a record that was acted must be read back and marked verified or failed"]
pub struct ActedToken {
    record: RecordRef,
    epoch: Epoch,
}

impl ActedToken {
    /// Which commit point.
    #[must_use]
    pub fn record(&self) -> RecordRef {
        self.record
    }

    /// The lease number it was acted under.
    #[must_use]
    pub fn epoch(&self) -> Epoch {
        self.epoch
    }
}

impl std::fmt::Debug for ActedToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActedToken")
            .field("record", &self.record)
            .field("epoch", &self.epoch)
            .finish()
    }
}

/// What is known when a write has been sent.
#[derive(Debug, Clone, Default)]
pub struct Acted {
    /// The key the system assigned, when its answer carried one.
    pub assigned_key: Option<String>,
    /// The record's stored outputs.
    pub outputs: Option<BlobRef>,
}

/// What was read back from the system.
#[derive(Debug, Clone)]
pub struct ReadBack {
    /// The digest of the values read, after normalization.
    pub digest: Digest,
    /// The stored values read.
    pub after: Option<BlobRef>,
    /// The evidence of the read.
    pub evidence: BlobRef,
}

/// What reading a record in doubt showed (spec section 208, "Resume,
/// exactly").
#[derive(Debug, Clone)]
pub enum Reconciled {
    /// The record holds the intended values: the write landed.
    Landed {
        /// What was read.
        read: ReadBack,
        /// What the read found the system assigned and the outputs a
        /// dependent record needs, kept as `mark_acted` keeps them, so that
        /// a resume need not read the record once more.
        acted: Acted,
    },
    /// The record holds the values seen before the write: it did not land.
    /// The evidence of the read is required: a record goes back to
    /// `planned`, and may be written again, only on a read that was made.
    NotLanded {
        /// The evidence of the read.
        evidence: BlobRef,
    },
    /// The record holds neither.
    Neither {
        /// The evidence of the read.
        evidence: BlobRef,
    },
    /// Nothing was sent for the record: it `failed` and no intent was
    /// stored for it since it was last planned, which the ledger checks
    /// from its own row. It goes back to `planned` without a read; a record
    /// that had an intent is refused this way (decision 0145).
    NeverSent,
}

/// Where a settled record ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settled {
    /// Marked `verified`; nothing is written.
    Verified,
    /// Returned to `planned`; it continues through the normal write path.
    Planned,
    /// Left `failed` as `stale`, `changed_since_preview`: a new preview and
    /// approval are needed for it.
    Stale,
}

/// Proof that a run holds the lock on a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockHeld {
    /// Which commit point the lock was taken for.
    pub record: RecordRef,
    /// The lease number it is held under.
    pub epoch: Epoch,
}

/// A record lock, as the store holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockRow {
    /// The keyed digest of the record key.
    pub key_hash: Digest,
    /// The run that holds it.
    pub run: RunId,
    /// The lease number it was taken under.
    pub epoch: Epoch,
    /// True when the lock is quarantined: it passes to reconcile, and no
    /// other run can write the record before it has been read back.
    pub quarantined: bool,
}

/// What the events of a run say its state is: the state rebuilt from history
/// alone, to compare with the current rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replayed {
    /// The run's state.
    pub state: RunState,
    /// The lease number.
    pub epoch: Epoch,
    /// Each record's state, by step and record number.
    pub records: BTreeMap<(u32, u32), RecordState>,
    /// The records that ended `acted` on the system's acknowledgement.
    pub acknowledged: BTreeSet<(u32, u32)>,
}

/// Rebuilds a run's state from its events, in order.
///
/// # Errors
///
/// `busy`/`store_corrupt` when an event's payload is not what this crate
/// writes.
pub fn replay(events: &[RunEvent]) -> Result<Replayed, Failure> {
    let unreadable = || failure::corrupt("an event whose payload does not read");
    let mut replayed = Replayed {
        state: RunState::Queued,
        epoch: Epoch::new(0),
        records: BTreeMap::new(),
        acknowledged: BTreeSet::new(),
    };
    for event in events {
        let payload: serde_json::Value =
            serde_json::from_str(&event.payload).map_err(|_| unreadable())?;
        let text = |name: &str| payload.get(name).and_then(serde_json::Value::as_str);
        let record = || -> Result<(u32, u32), Failure> {
            let part = |name: &str| {
                payload
                    .get(name)
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|number| u32::try_from(number).ok())
                    .ok_or_else(unreadable)
            };
            Ok((part("step")?, part("record")?))
        };
        match event.kind {
            EventKind::StateChanged => {
                replayed.state = text("to")
                    .and_then(|name| name.parse().ok())
                    .ok_or_else(unreadable)?;
            }
            EventKind::LeaseGranted => replayed.epoch = event.epoch,
            EventKind::Planned => {
                for entry in payload
                    .get("records")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(unreadable)?
                {
                    let number = |at: usize| {
                        entry
                            .get(at)
                            .and_then(serde_json::Value::as_u64)
                            .and_then(|number| u32::try_from(number).ok())
                            .ok_or_else(unreadable)
                    };
                    let state = entry
                        .get(2)
                        .and_then(serde_json::Value::as_str)
                        .and_then(|name| name.parse().ok())
                        .ok_or_else(unreadable)?;
                    replayed.records.insert((number(0)?, number(1)?), state);
                }
            }
            EventKind::Intent => {
                replayed.records.insert(record()?, RecordState::Intent);
            }
            EventKind::Acted => {
                // A record that ended on an acknowledgement stays `acted`;
                // its second `acted` event says so.
                let place = record()?;
                if text("evidence") == Some(WriteEvidence::Acknowledged.as_str()) {
                    replayed.acknowledged.insert(place);
                }
                replayed.records.insert(place, RecordState::Acted);
            }
            EventKind::Verified => {
                replayed.records.insert(record()?, RecordState::Verified);
            }
            EventKind::Failed => {
                replayed.records.insert(record()?, RecordState::Failed);
            }
            EventKind::ReconcileRead => {
                let state = text("state")
                    .and_then(|name| name.parse().ok())
                    .ok_or_else(unreadable)?;
                replayed.records.insert(record()?, state);
            }
            _ => {}
        }
    }
    Ok(replayed)
}

/// The moves a run may make. `needs_approval` to `running` is here, and is
/// made only by [`Ledger::begin_writes`].
fn may_move(from: RunState, to: RunState) -> bool {
    use RunState::{
        Cancelled, Completed, Expired, Failed, NeedsApproval, NeedsInput, NeedsLogin, Queued,
        Rejected, Running, Stopped, Validating,
    };
    match from {
        Queued => matches!(to, Validating | Stopped | Failed | Cancelled),
        Validating => matches!(
            to,
            Rejected | NeedsApproval | Running | NeedsLogin | Stopped | Failed | Cancelled
        ),
        NeedsApproval => matches!(to, Running | Expired | Cancelled),
        Running => matches!(
            to,
            NeedsInput | NeedsLogin | Stopped | Completed | Failed | Cancelled
        ),
        NeedsInput => matches!(to, Running | Stopped | Failed | Cancelled),
        NeedsLogin => matches!(to, Running | Validating | Stopped | Failed | Cancelled),
        Stopped => matches!(to, Running | Failed | Cancelled),
        Rejected | Expired | Completed | Failed | Cancelled => false,
    }
}

/// The presence checks that show a person was present, as the policy engine
/// writes them in `approvals.presence_check`. `not_available` is the one
/// other word: a machine with no such check.
const PRESENCE_PROVED: [&str; 5] = [
    "os_prompt",
    "polkit",
    "local_authentication",
    "windows_hello",
    "signed_in",
];

/// The failure classes after which nothing is in doubt when the write was
/// never sent: the record was read, or nothing was sent. A record that
/// reached `acted` is in doubt whatever its class.
const SETTLED_CLASSES: [FailureClass; 3] = [
    FailureClass::Policy,
    FailureClass::Stale,
    FailureClass::Input,
];

fn record_ref_params(record: &RecordRef) -> [SqlValue; 3] {
    [
        record.run.to_string().into(),
        record.step_seq.into(),
        record.record_seq.into(),
    ]
}

fn idempotency_token(record: &RecordRef, key: &RecordKey) -> String {
    let digest = digest_of(
        "idempotency token 1",
        &[
            ("run", record.run.as_bytes()),
            ("step", &record.step_seq.to_le_bytes()),
            ("record", &record.record_seq.to_le_bytes()),
            ("key", &key.canonical_bytes()),
        ],
    )
    .to_string();
    digest.chars().take(32).collect()
}

fn failure_of(
    class: Option<&str>,
    code: Option<&str>,
) -> Result<Option<(FailureClass, FailureCode)>, Failure> {
    match (class, code) {
        (Some(class), Some(code)) => Ok(Some((parse(class)?, parse(code)?))),
        _ => Ok(None),
    }
}

/// A failure rebuilt from the payload of the event that stored it, when the
/// payload carries the class and code the row holds. What the system said
/// is handed back apart, through [`Failure::with_said`], which bounds it
/// again; it is never joined into the sentence.
fn stored_failure(payload: &str, class: FailureClass, code: &FailureCode) -> Option<Failure> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    let text = |name: &str| value.get(name).and_then(serde_json::Value::as_str);
    if text("class") != Some(class.as_str()) || text("code") != Some(code.as_str()) {
        return None;
    }
    let failure = Failure::new(
        class,
        code.clone(),
        text("message").unwrap_or_default(),
        text("next_step").unwrap_or_default(),
    );
    Some(match text("said") {
        Some(said) => failure.with_said(said),
        None => failure,
    })
}

fn epoch_of(number: i64) -> Result<Epoch, Failure> {
    count(number).map(Epoch::new)
}

pub(crate) fn run_row(row: &Row) -> Result<RunRow, Failure> {
    Ok(RunRow {
        id: parse(row.text(0)?)?,
        workspace: parse(row.text(1)?)?,
        principal: parse(row.text(2)?)?,
        client_label: row.text(3)?.to_owned(),
        tool_name: parse(row.text(4)?)?,
        tool_kind: parse(row.text(5)?)?,
        target: parse(row.text(6)?)?,
        target_version: parse(row.text(7)?)?,
        environment: parse(row.text(8)?)?,
        effect: parse(row.text(9)?)?,
        input_hash: parse(row.text(10)?)?,
        state: parse(row.text(11)?)?,
        failure: failure_of(row.opt_text(12)?, row.opt_text(13)?)?,
        attempt: small(row.int(14)?)?,
        epoch: epoch_of(row.int(15)?)?,
        diff_hash: parse_opt(row.opt_text(16)?)?,
        diff: parse_opt(row.opt_text(17)?)?,
        approval: parse_opt(row.opt_text(18)?)?,
        decision: parse_opt(row.opt_text(19)?)?,
        result: parse_opt(row.opt_text(20)?)?,
        trace: parse_opt(row.opt_text(21)?)?,
        created_at: moment(row.int(22)?),
        started_at: row.opt_int(23)?.map(moment),
        finished_at: row.opt_int(24)?.map(moment),
    })
}

macro_rules! select_records {
    ($rest:literal) => {
        concat!(
            "SELECT run_id, step_seq, record_seq, record_key, record_key_hash, operation, state,
                    intended_hash, before_ref, after_ref, idempotency_token, assigned_key,
                    outputs_ref, intent_epoch, attempts, failure_class, failure_code,
                    evidence_ref, intent_seq, binding_mode, acted_at, reconciled_at,
                    acknowledged_at
             FROM run_records ",
            $rest
        )
    };
}

fn record_row(row: &Row) -> Result<(RecordRow, Option<u64>), Failure> {
    let key: RecordKey = serde_json::from_str(row.text(3)?)
        .map_err(|_| failure::corrupt("a stored record key that no longer reads"))?;
    Ok((
        RecordRow {
            record: RecordRef {
                run: parse(row.text(0)?)?,
                step_seq: small(row.int(1)?)?,
                record_seq: small(row.int(2)?)?,
            },
            key,
            key_hash: parse(row.text(4)?)?,
            operation: parse(row.text(5)?)?,
            state: parse(row.text(6)?)?,
            intended: parse(row.text(7)?)?,
            before: parse_opt(row.opt_text(8)?)?,
            after: parse_opt(row.opt_text(9)?)?,
            idempotency_token: row.text(10)?.to_owned(),
            assigned_key: row.opt_text(11)?.map(str::to_owned),
            outputs: parse_opt(row.opt_text(12)?)?,
            intent_epoch: row.opt_int(13)?.map(epoch_of).transpose()?,
            attempts: small(row.int(14)?)?,
            failure: failure_of(row.opt_text(15)?, row.opt_text(16)?)?,
            evidence: parse_opt(row.opt_text(17)?)?,
            mode: parse_opt(row.opt_text(19)?)?,
            acted_at: row.opt_int(20)?.map(moment),
            reconciled_at: row.opt_int(21)?.map(moment),
            acknowledged_at: row.opt_int(22)?.map(moment),
        },
        row.opt_int(18)?.map(count).transpose()?,
    ))
}

/// The run store and the ledger, inside one transaction.
pub struct Ledger<'t, 'a> {
    pub(crate) tx: &'t mut Tx<'a>,
}

impl<'a> Tx<'a> {
    /// The run store and the ledger.
    pub fn ledger(&mut self) -> Ledger<'_, 'a> {
        Ledger { tx: self }
    }
}

impl Store {
    /// Stores the intents of the records about to be written, commits them
    /// with a sync to disk, and only then gives a token for each (rule 1).
    /// Up to the mode's intent window at once.
    ///
    /// # Errors
    ///
    /// `possible_commit`/`runner_lost` when the fence is not the run's
    /// current lease (rule 4). `possible_commit`/`reconcile_required` while
    /// any record of the run is in doubt from an earlier lease (rule 5).
    /// `policy`/`approval_missing` when the run was never cleared to write
    /// (rule 3). `busy`/`record_locked` when another run holds a record's
    /// lock (rule 7). `unknown_variant`/`ledger_state_unexpected` when the
    /// run is not `running`, a record is not `planned`, its lock was not
    /// taken, or the digest is not the planned one. Nothing is stored and no
    /// token is given when any record is refused.
    pub async fn store_intents(
        &self,
        fence: &Fence,
        intents: Vec<RecordIntent>,
    ) -> Result<Vec<IntentToken>, Failure> {
        let fence = *fence;
        let stored = self
            .write(Durability::Synced, move |tx| {
                tx.ledger().intend(&fence, &intents)
            })
            .await?;
        // The commit has returned: the intents are on disk.
        Ok(stored.into_iter().map(IntentToken::issue).collect())
    }

    /// Marks a record `verified` in a transaction of its own, committed
    /// with a sync to disk: [`Ledger::mark_verified`], for a caller that
    /// holds the proof of `acted` by reference. A transaction runs on the
    /// store's own thread and takes only what it can own, so the proof's
    /// two facts are carried in and the same proof is made there.
    ///
    /// # Errors
    ///
    /// What [`Ledger::mark_verified`] answers.
    pub async fn store_verified(
        &self,
        token: &ActedToken,
        read_back: ReadBack,
    ) -> Result<(), Failure> {
        let (record, epoch) = (token.record, token.epoch);
        self.write(Durability::Synced, move |tx| {
            tx.ledger()
                .mark_verified(&ActedToken { record, epoch }, read_back)
        })
        .await
    }

    /// Reads the run's own stored diff, and answers what it says of one
    /// record when it says the record's write ends on the system's
    /// acknowledgement (`acknowledged`, decision 0161). The diff is a sealed
    /// blob, opened outside any transaction; [`Ledger::mark_acknowledged`]
    /// checks, inside its own, that the run's diff is still this one.
    ///
    /// The diff is read as data, by the names the engine's diff writes:
    /// `records`, and in each `step_seq`, `record_seq`, `operation`, and
    /// `evidence`, which is absent where it is `read_back`.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` for no such run or record.
    /// `unknown_variant`/`ledger_state_unexpected` when the run has no
    /// stored diff, the diff does not hold the record, or holds it with
    /// another operation or with evidence other than `acknowledged`.
    /// `busy`/`store_corrupt` when the stored diff does not read. What
    /// [`crate::Blobs::get`] answers.
    pub async fn acknowledged_in_diff(
        &self,
        record: RecordRef,
    ) -> Result<AcknowledgedInDiff, Failure> {
        let (diff, operation) = self
            .read(move |tx| {
                let mut ledger = tx.ledger();
                let run = ledger.run(record.run)?;
                let row = ledger.record(record)?;
                Ok((run.diff, row.operation))
            })
            .await?;
        let diff = diff.ok_or_else(|| {
            failure::unexpected(
                "the run has no stored diff, so nothing says its write ends on an acknowledgement",
            )
        })?;
        let opened = self.blobs().get(&diff).await?;
        says_acknowledged(opened.expose(), record, &operation)?;
        Ok(AcknowledgedInDiff { record, diff })
    }

    /// Ends a record on the system's acknowledgement, in a transaction of
    /// its own committed with a sync to disk: [`Store::acknowledged_in_diff`],
    /// then [`Ledger::mark_acknowledged`], for a caller that holds the proof
    /// of `acted` by reference.
    ///
    /// # Errors
    ///
    /// What the two calls answer.
    pub async fn store_acknowledged(
        &self,
        token: &ActedToken,
        evidence: BlobRef,
    ) -> Result<(), Failure> {
        let (record, epoch) = (token.record, token.epoch);
        let diff = self.acknowledged_in_diff(record).await?;
        self.write(Durability::Synced, move |tx| {
            tx.ledger()
                .mark_acknowledged(&ActedToken { record, epoch }, evidence, &diff)
        })
        .await
    }
}

/// Whether a stored diff says the record's write ends on the system's
/// acknowledgement. The diff is the engine's; the ledger reads only the
/// names it needs, as data.
fn says_acknowledged(
    plain: &[u8],
    record: RecordRef,
    operation: &OperationName,
) -> Result<(), Failure> {
    let unreadable = || failure::corrupt("a stored diff that does not read");
    let diff: serde_json::Value = serde_json::from_slice(plain).map_err(|_| unreadable())?;
    let number =
        |entry: &serde_json::Value, name: &str| entry.get(name).and_then(serde_json::Value::as_u64);
    let mut found = diff
        .get("records")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(unreadable)?
        .iter()
        .filter(|entry| {
            number(entry, "step_seq") == Some(u64::from(record.step_seq))
                && number(entry, "record_seq") == Some(u64::from(record.record_seq))
        });
    let entry = found
        .next()
        .ok_or_else(|| failure::unexpected("the run's diff does not hold the record"))?;
    if found.next().is_some() {
        return Err(failure::corrupt(
            "a stored diff that holds one record twice",
        ));
    }
    if entry.get("operation").and_then(serde_json::Value::as_str) != Some(operation.as_str()) {
        return Err(failure::unexpected(
            "the run's diff holds the record under another operation",
        ));
    }
    if entry.get("evidence").and_then(serde_json::Value::as_str)
        != Some(WriteEvidence::Acknowledged.as_str())
    {
        return Err(failure::unexpected(
            "the run's diff does not say the record's write ends on an acknowledgement; it must be read back",
        ));
    }
    Ok(())
}

impl Ledger<'_, '_> {
    // ---- reading -------------------------------------------------------

    /// The run.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` when there is no such run.
    pub fn run(&mut self, run: RunId) -> Result<RunRow, Failure> {
        let row = self
            .tx
            .row(
                "SELECT id, workspace_id, principal_id, client_label, tool_name, tool_kind, target,
                        target_version_hash, environment, effect, input_hash, state, state_class,
                        state_code, attempt, lease_epoch, diff_hash, diff_ref, approval_id,
                        decision_id, result_ref, trace_ref, created_at, started_at, finished_at
                 FROM runs WHERE id = ?1",
                &[run.to_string().into()],
            )?
            .ok_or_else(|| failure::not_found("run"))?;
        run_row(&row)
    }

    /// How many runs of `workspace` are in `state` now. It reads the index
    /// on a run's workspace and state, so it is cheap enough to ask often.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn count_runs(&mut self, workspace: &WorkspaceId, state: RunState) -> Result<u64, Failure> {
        let found = self
            .tx
            .row(
                "SELECT COUNT(*) FROM runs WHERE workspace_id = ?1 AND state = ?2",
                &[workspace.to_string().into(), state.as_str().into()],
            )?
            .ok_or_else(|| failure::corrupt("a count that gave no row"))?
            .int(0)?;
        count(found)
    }

    /// The run's events with a number above `after`, in order, at most
    /// `limit` of them.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn events(&mut self, run: RunId, after: u64, limit: u32) -> Result<Vec<RunEvent>, Failure> {
        let rows = self.tx.rows(
            "SELECT seq, epoch, at, kind, payload, evidence_ref FROM run_events
             WHERE run_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
            &[run.to_string().into(), stored(after)?.into(), limit.into()],
        )?;
        rows.iter()
            .map(|row| {
                Ok(RunEvent {
                    seq: count(row.int(0)?)?,
                    epoch: epoch_of(row.int(1)?)?,
                    at: moment(row.int(2)?),
                    kind: parse(row.text(3)?)?,
                    payload: row.text(4)?.to_owned(),
                    evidence: parse_opt(row.opt_text(5)?)?,
                })
            })
            .collect()
    }

    /// Every record of the run, in step and record order.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn records(&mut self, run: RunId) -> Result<Vec<RecordRow>, Failure> {
        let rows = self.tx.rows(
            select_records!("WHERE run_id = ?1 ORDER BY step_seq, record_seq"),
            &[run.to_string().into()],
        )?;
        rows.iter()
            .map(|row| record_row(row).map(|(record, _)| record))
            .collect()
    }

    /// One record.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` when there is no such record.
    pub fn record(&mut self, record: RecordRef) -> Result<RecordRow, Failure> {
        self.record_with_seq(record).map(|(row, _)| row)
    }

    /// Rule 5's question: the records that must be read back before the run
    /// writes again, in key order: those of which [`RecordRow::in_doubt`]
    /// holds. A record that reached `acted` is among them whatever class it
    /// failed with: the ledger does not take a failure's class as proof that
    /// a write that was sent did not land.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn in_doubt(&mut self, run: RunId) -> Result<Vec<RecordRow>, Failure> {
        let rows = self.tx.rows(
            select_records!(
                "WHERE run_id = ?1
                   AND (state = 'intent'
                        OR (state = 'acted' AND acknowledged_at IS NULL)
                        OR (state = 'failed' AND reconciled_at IS NULL
                            AND (acted_at IS NOT NULL
                                 OR failure_class NOT IN ('policy', 'stale', 'input'))))
                 ORDER BY record_key, step_seq, record_seq"
            ),
            &[run.to_string().into()],
        )?;
        rows.iter()
            .map(|row| record_row(row).map(|(record, _)| record))
            .collect()
    }

    /// The locks the run holds, held or quarantined.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn locks(&mut self, run: RunId) -> Result<Vec<LockRow>, Failure> {
        let rows = self.tx.rows(
            "SELECT record_key_hash, run_id, epoch, state FROM record_locks
             WHERE run_id = ?1 ORDER BY record_key_hash",
            &[run.to_string().into()],
        )?;
        rows.iter()
            .map(|row| {
                Ok(LockRow {
                    key_hash: parse(row.text(0)?)?,
                    run: parse(row.text(1)?)?,
                    epoch: epoch_of(row.int(2)?)?,
                    quarantined: row.text(3)? == "quarantined",
                })
            })
            .collect()
    }

    /// How the run's records stand, counted: verified, unchanged,
    /// acknowledged and not read back, and the rest. What a result says of
    /// them, and what decides whether the run may complete.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn tally(&mut self, run: RunId) -> Result<RecordTally, Failure> {
        let row = self
            .tx
            .row(
                "SELECT COALESCE(SUM(state = 'verified'), 0),
                        COALESCE(SUM(state = 'unchanged'), 0),
                        COALESCE(SUM(state = 'acted' AND acknowledged_at IS NOT NULL), 0),
                        COUNT(*)
                 FROM run_records WHERE run_id = ?1",
                &[run.to_string().into()],
            )?
            .ok_or_else(|| failure::corrupt("a count that gave no row"))?;
        let number = |at: usize| -> Result<u32, Failure> { small(row.int(at)?) };
        let (verified, unchanged, acknowledged) = (number(0)?, number(1)?, number(2)?);
        let open = number(3)?
            .checked_sub(verified + unchanged + acknowledged)
            .ok_or_else(|| failure::corrupt("a count of records that does not add up"))?;
        Ok(RecordTally {
            verified,
            unchanged,
            acknowledged,
            open,
        })
    }

    /// The failure that stopped or ended the run, whole, as it was stored:
    /// its class and code from the run's row; its sentence, its next step,
    /// and what the system itself said from the event that moved the run.
    /// What the system said is handed back apart, in [`Failure::said`], and
    /// is untrusted text: it was bounded and redacted when it was stored,
    /// and is never joined into the sentence. `None` when the run carries no
    /// failure, or when the event that stored it is not found.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` for no such run; `busy` failures of the database.
    pub fn run_failure(&mut self, run: RunId) -> Result<Option<Failure>, Failure> {
        let row = self.run(run)?;
        let Some((class, code)) = row.failure else {
            return Ok(None);
        };
        let found = self.tx.row(
            "SELECT payload FROM run_events
             WHERE run_id = ?1 AND kind = 'state_changed' ORDER BY seq DESC LIMIT 1",
            &[run.to_string().into()],
        )?;
        let payload = match found {
            Some(found) => found.text(0)?.to_owned(),
            None => return Ok(None),
        };
        Ok(stored_failure(&payload, class, &code))
    }

    /// The failure a record was marked `failed` with, whole, as it was
    /// stored, with what the system said kept apart as [`Failure::said`]:
    /// as [`Ledger::run_failure`], for one record. `None` when the record
    /// carries no failure, or when the event that stored it is not found.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` for no such record; `busy` failures of the
    /// database.
    pub fn record_failure(&mut self, record: RecordRef) -> Result<Option<Failure>, Failure> {
        let row = self.record(record)?;
        let Some((class, code)) = row.failure else {
            return Ok(None);
        };
        if code == failure::CHANGED_SINCE_PREVIEW && class == FailureClass::Stale {
            // `settle` with `Neither` leaves the ledger's own failure.
            return Ok(Some(failure::changed_since_preview()));
        }
        let rows = self.tx.rows(
            "SELECT payload FROM run_events
             WHERE run_id = ?1 AND kind = 'failed' ORDER BY seq DESC",
            &[record.run.to_string().into()],
        )?;
        for found in &rows {
            let payload = found.text(0)?;
            let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
                continue;
            };
            let at = |name: &str| value.get(name).and_then(serde_json::Value::as_u64);
            if at("step") == Some(u64::from(record.step_seq))
                && at("record") == Some(u64::from(record.record_seq))
            {
                return Ok(stored_failure(payload, class, &code));
            }
        }
        Ok(None)
    }

    // ---- runs ----------------------------------------------------------

    /// Opens a run, in `queued`. When the same caller already has an
    /// unfinished run of the same tool version with the same inputs, that
    /// run is returned and nothing is written (rule 6).
    ///
    /// # Errors
    ///
    /// `busy` failures of the database, and of the random source.
    pub fn open_run(&mut self, new: NewRun) -> Result<Opened, Failure> {
        let dedupe_key = digest_of(
            "run identity 1",
            &[
                ("workspace", new.workspace.as_bytes()),
                ("principal", new.principal.as_bytes()),
                ("tool", new.tool_name.as_str().as_bytes()),
                ("target", new.target.as_str().as_bytes()),
                ("version", new.target_version.as_bytes()),
                ("environment", new.environment.as_str().as_bytes()),
                ("input", new.input_hash.as_bytes()),
            ],
        )
        .to_string();
        if let Some(row) = self.tx.row(
            "SELECT id FROM runs WHERE dedupe_key = ?1
               AND state NOT IN ('rejected', 'expired', 'completed', 'failed', 'cancelled')",
            &[dedupe_key.clone().into()],
        )? {
            return Ok(Opened::Existing(parse(row.text(0)?)?));
        }
        let random = self.tx.random()?;
        let run = RunId::from_parts(self.tx.now(), random)?;
        let client_label = self.tx.shared.attached()?.redact.redact(&new.client_label);
        self.tx.exec(
            "INSERT INTO runs
                (id, workspace_id, principal_id, client_label, tool_name, tool_kind, target,
                 target_version_hash, environment, identity_ref, effect, input_hash, input_ref,
                 dedupe_key, state, attempt, lease_epoch, created_at, retention_class)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 'queued', 1, 0,
                     ?15, ?16)",
            &[
                run.to_string().into(),
                new.workspace.to_string().into(),
                new.principal.to_string().into(),
                client_label.into(),
                new.tool_name.as_str().into(),
                new.tool_kind.as_str().into(),
                new.target.as_str().into(),
                new.target_version.to_string().into(),
                new.environment.as_str().into(),
                new.identity.map(|identity| identity.to_string()).into(),
                new.effect.as_str().into(),
                new.input_hash.to_string().into(),
                new.input.into(),
                dedupe_key.into(),
                self.tx.now().into(),
                new.retention.as_str().into(),
            ],
        )?;
        let epoch = Epoch::new(0);
        self.event(
            run,
            epoch,
            EventKind::StateChanged,
            &json!({ "to": "queued" }),
            None,
        )?;
        Ok(Opened::New(Fence { run, epoch }))
    }

    /// Grants a new lease on the run: the lease number grows by one, and the
    /// new fence is the only one that may change the run from now on. Every
    /// lease after the first is a resume, so the attempt number grows too,
    /// and the locks of the lease that ended are sorted: those of records in
    /// doubt are quarantined, the rest released.
    ///
    /// # Errors
    ///
    /// `unknown_variant`/`ledger_state_unexpected` for a run in a final
    /// state.
    pub fn new_epoch(&mut self, run: RunId) -> Result<Fence, Failure> {
        let row = self.run(run)?;
        if row.state.is_final() {
            return Err(failure::unexpected(
                "a run in a final state takes no new lease",
            ));
        }
        let next = row
            .epoch
            .next()
            .ok_or_else(|| failure::unexpected("the lease numbers have run out"))?;
        let resumed = row.epoch.get() > 0;
        if resumed {
            self.event(run, row.epoch, EventKind::LeaseLost, &json!({}), None)?;
            self.sort_locks(run)?;
        }
        self.tx.exec(
            "UPDATE runs SET lease_epoch = ?2, attempt = attempt + ?3 WHERE id = ?1",
            &[
                run.to_string().into(),
                stored(next.get())?.into(),
                resumed.into(),
            ],
        )?;
        self.event(run, next, EventKind::LeaseGranted, &json!({}), None)?;
        Ok(Fence { run, epoch: next })
    }

    /// Moves the run to another state. `stopped` and `failed` carry the
    /// failure that ended the work.
    ///
    /// Entering `running` is refused for a write run that has records to
    /// write and no clearance (rule 3; use [`Ledger::begin_writes`]), and
    /// while a record is in doubt from an earlier lease (rule 5). Entering
    /// `completed` is refused while a record is neither verified nor
    /// unchanged. Leaving `running` for `stopped`, `needs_login`, or a final
    /// state sorts the run's locks: those of records in doubt are
    /// quarantined, the rest released.
    ///
    /// # Errors
    ///
    /// `possible_commit`/`runner_lost` (rule 4);
    /// `unknown_variant`/`ledger_state_unexpected` for a move no rule allows,
    /// above all any move out of a final state (rule 8);
    /// `policy`/`approval_missing`; `possible_commit`/`reconcile_required`.
    pub fn set_state(
        &mut self,
        fence: &Fence,
        to: RunState,
        why: Option<&Failure>,
    ) -> Result<(), Failure> {
        let run = self.fenced(fence)?;
        if !may_move(run.state, to) {
            return Err(failure::unexpected(format!(
                "a run cannot move from `{}` to `{to}`",
                run.state
            )));
        }
        if matches!(to, RunState::Stopped | RunState::Failed) && why.is_none() {
            return Err(failure::unexpected(
                "a run that stops or fails carries the failure that ended it",
            ));
        }
        if to == RunState::Running {
            if run.state == RunState::NeedsApproval {
                return Err(failure::approval_missing(
                    "a run that waits for approval starts through `begin_writes`",
                ));
            }
            self.may_run(&run)?;
        }
        if to == RunState::Completed && !self.tally(run.id)?.settled() {
            return Err(failure::unexpected(
                "a run is completed only when every record is verified, unchanged, or acknowledged",
            ));
        }
        let why = why.map(|why| self.redacted(why)).transpose()?;
        let payload = match &why {
            Some(why) => json!({
                "from": run.state.as_str(),
                "to": to.as_str(),
                "class": why.class().as_str(),
                "code": why.code().as_str(),
                "message": why.message(),
                "next_step": why.next_step(),
                "said": why.said(),
            }),
            None => json!({ "from": run.state.as_str(), "to": to.as_str() }),
        };
        self.event(run.id, fence.epoch, EventKind::StateChanged, &payload, None)?;
        if to.is_final() || matches!(to, RunState::Stopped | RunState::NeedsLogin) {
            self.sort_locks(run.id)?;
        }
        let now = self.tx.now();
        self.tx.exec(
            "UPDATE runs
             SET state = ?2, state_class = ?3, state_code = ?4,
                 started_at = COALESCE(started_at, ?5),
                 finished_at = ?6
             WHERE id = ?1",
            &[
                run.id.to_string().into(),
                to.as_str().into(),
                why.as_ref().map(|why| why.class().as_str()).into(),
                why.as_ref().map(|why| why.code().as_str()).into(),
                (to == RunState::Running).then_some(now).into(),
                to.is_final().then_some(now).into(),
            ],
        )?;
        Ok(())
    }

    /// Stores the diff the preview built: its hash, which the engine alone
    /// computes, and the blob that holds it. Only while the run is
    /// `validating`.
    ///
    /// # Errors
    ///
    /// `possible_commit`/`runner_lost`;
    /// `unknown_variant`/`ledger_state_unexpected` in any other state.
    pub fn store_diff(
        &mut self,
        fence: &Fence,
        hash: Digest,
        diff: Option<BlobRef>,
    ) -> Result<(), Failure> {
        let run = self.fenced(fence)?;
        if run.state != RunState::Validating {
            return Err(failure::unexpected(
                "a diff is stored while the run is `validating`",
            ));
        }
        self.event(
            run.id,
            fence.epoch,
            EventKind::DiffStored,
            &json!({ "diff_hash": hash.to_string() }),
            diff,
        )?;
        self.tx.exec(
            "UPDATE runs SET diff_hash = ?2, diff_ref = ?3 WHERE id = ?1",
            &[
                run.id.to_string().into(),
                hash.to_string().into(),
                diff.into(),
            ],
        )?;
        Ok(())
    }

    /// Rule 3: the first entry of a write run into `running`. The clearance
    /// is checked here, in the same transaction that changes the run's
    /// state. For an approval, the policy engine must have marked it used in
    /// this same transaction; the ledger never marks one used.
    ///
    /// # Errors
    ///
    /// `policy`/`approval_missing` when the clearance does not exist, is for
    /// another run, was revoked, is not marked used, or was used in an
    /// earlier transaction. `policy`/`approval_stale` when it was given for
    /// another diff. `possible_commit`/`runner_lost`.
    /// `unknown_variant`/`ledger_state_unexpected` when the run is a read,
    /// is not waiting to start, or was cleared before.
    pub fn begin_writes(
        &mut self,
        fence: &Fence,
        clearance: WriteClearance,
    ) -> Result<(), Failure> {
        let run = self.fenced(fence)?;
        if run.effect == EffectClass::Read {
            return Err(failure::unexpected("a read needs no clearance to write"));
        }
        if !matches!(run.state, RunState::NeedsApproval | RunState::Validating) {
            return Err(failure::unexpected(
                "writes begin from `needs_approval` or `validating`",
            ));
        }
        if run.cleared_to_write() {
            return Err(failure::unexpected("the run was cleared to write before"));
        }
        let diff_hash = run
            .diff_hash
            .ok_or_else(|| failure::approval_missing("the run has no diff to approve"))?
            .to_string();
        let mut offline = None;
        let payload = match clearance {
            WriteClearance::Approval(approval) => {
                self.check_approval(&run, approval, &diff_hash)?;
                json!({ "approval": approval.to_string() })
            }
            WriteClearance::PreApproved(decision) => {
                self.check_pre_approval(&run, decision, &diff_hash)?;
                json!({ "decision": decision.to_string() })
            }
            WriteClearance::OfflineTest => {
                if !self.tx.shared.throwaway {
                    return Err(failure::approval_missing(
                        "an offline test's clearance is taken only by the store that test opened for itself",
                    ));
                }
                // The test's one rule, recorded as a decision of this store
                // alone, so that rule 3's triggers hold here as anywhere: an
                // allowed decision to write without a person, for this
                // run's diff, under the offline test's rule.
                let decision = DecisionId::from_parts(self.tx.now(), self.tx.random()?)?;
                // The store took its own policy key when it was opened, so
                // no policy engine can be opened over it.
                let key = crate::tx::PolicyKey::new(self.tx.shared.id);
                let now = self.tx.now();
                self.tx.policy_tables(&key)?.execute(
                    "INSERT INTO decisions
                        (id, workspace_id, run_id, principal_id, question, allowed, diff_hash,
                         reasons, policy_version, decided_at)
                     VALUES (?1, ?2, ?3, ?4, 'write_without_person', 1, ?5,
                             '[\"offline_test\"]', 'offline_test', ?6)",
                    &[
                        decision.to_string().into(),
                        run.workspace.to_string().into(),
                        run.id.to_string().into(),
                        run.principal.to_string().into(),
                        diff_hash.clone().into(),
                        now.into(),
                    ],
                )?;
                offline = Some(decision);
                json!({ "offline_test": true, "decision": decision.to_string() })
            }
        };
        self.event(
            run.id,
            fence.epoch,
            EventKind::ApprovalBound,
            &payload,
            None,
        )?;
        self.event(
            run.id,
            fence.epoch,
            EventKind::StateChanged,
            &json!({ "from": run.state.as_str(), "to": "running" }),
            None,
        )?;
        let (approval, decision) = match clearance {
            WriteClearance::Approval(approval) => (Some(approval.to_string()), None),
            WriteClearance::PreApproved(decision) => (None, Some(decision.to_string())),
            WriteClearance::OfflineTest => (None, offline.map(|decision| decision.to_string())),
        };
        self.tx.exec(
            "UPDATE runs
             SET state = 'running', state_class = NULL, state_code = NULL,
                 approval_id = ?2, decision_id = ?3, started_at = COALESCE(started_at, ?4)
             WHERE id = ?1",
            &[
                run.id.to_string().into(),
                approval.into(),
                decision.into(),
                self.tx.now().into(),
            ],
        )?;
        Ok(())
    }

    /// Stores the run's result. Before the run reaches a final state.
    ///
    /// # Errors
    ///
    /// `possible_commit`/`runner_lost`;
    /// `unknown_variant`/`ledger_state_unexpected` for a final run or a
    /// reference that names no blob.
    pub fn store_result(&mut self, fence: &Fence, result: BlobRef) -> Result<(), Failure> {
        let run = self.fenced(fence)?;
        self.tx.exec(
            "UPDATE runs SET result_ref = ?2 WHERE id = ?1",
            &[run.id.to_string().into(), result.into()],
        )?;
        Ok(())
    }

    /// Stores the run's finished trace. A trace may arrive after the run's
    /// final state, so this takes no fence and changes nothing else.
    ///
    /// # Errors
    ///
    /// `input`/`not_found`; `unknown_variant`/`ledger_state_unexpected` for a
    /// reference that names no blob.
    pub fn store_trace(&mut self, run: RunId, trace: BlobRef) -> Result<(), Failure> {
        let changed = self.tx.exec(
            "UPDATE runs SET trace_ref = ?2 WHERE id = ?1",
            &[run.to_string().into(), trace.into()],
        )?;
        if changed == 1 {
            Ok(())
        } else {
            Err(failure::not_found("run"))
        }
    }

    /// Appends one of the engine's own events: a step that began, a binding
    /// that was chosen, a need. The payload is small JSON and passes the
    /// redactor. The kinds the ledger writes itself are refused, so history
    /// cannot be forged through this call.
    ///
    /// # Errors
    ///
    /// `possible_commit`/`runner_lost`;
    /// `unknown_variant`/`ledger_state_unexpected` for a final run or a kind
    /// that is the ledger's own.
    pub fn append_event(
        &mut self,
        fence: &Fence,
        kind: EventKind,
        payload: &serde_json::Value,
        evidence: Option<BlobRef>,
    ) -> Result<u64, Failure> {
        if !matches!(
            kind,
            EventKind::StepStarted | EventKind::BindingChosen | EventKind::Need
        ) {
            return Err(failure::unexpected(
                "that kind of event is written only by the ledger's own calls",
            ));
        }
        let run = self.fenced(fence)?;
        self.event(run.id, fence.epoch, kind, payload, evidence)
    }

    /// Records evidence that arrived after its lease ended. It changes no
    /// state, and is accepted whatever the run's state or lease.
    ///
    /// # Errors
    ///
    /// `input`/`not_found`; `unknown_variant`/`ledger_state_unexpected` for a
    /// reference that names no blob.
    pub fn late_evidence(&mut self, run: RunId, evidence: BlobRef) -> Result<u64, Failure> {
        let row = self.run(run)?;
        self.event(
            run,
            row.epoch,
            EventKind::LateEvidence,
            &json!({}),
            Some(evidence),
        )
    }

    // ---- records -------------------------------------------------------

    /// Records the commit points the preview found, each `planned`, or
    /// `unchanged` when the record already equals the intended values. Only
    /// while the run is `validating`. Planning the same commit point again
    /// with the same key, operation, and digest changes nothing, so a
    /// preview that is repeated cannot plan a write twice.
    ///
    /// # Errors
    ///
    /// `possible_commit`/`runner_lost`;
    /// `unknown_variant`/`ledger_state_unexpected` for a read, a run that is
    /// not `validating`, or a commit point planned before with other content.
    pub fn plan_records(
        &mut self,
        fence: &Fence,
        records: &[PlannedRecord],
    ) -> Result<(), Failure> {
        let run = self.fenced(fence)?;
        if run.effect == EffectClass::Read {
            return Err(failure::unexpected("a read has no records"));
        }
        if run.state != RunState::Validating {
            return Err(failure::unexpected(
                "records are planned while the run is `validating`",
            ));
        }
        let digest = std::sync::Arc::clone(&self.tx.shared.attached()?.digest);
        let mut planned = Vec::new();
        for record in records {
            let place = RecordRef {
                run: run.id,
                step_seq: record.step_seq,
                record_seq: record.record_seq,
            };
            let key_hash = digest
                .keyed_digest(
                    &run.workspace,
                    DigestPurpose::RecordKey,
                    &record.key.canonical_bytes(),
                )
                .to_string();
            if let Some(row) = self.tx.row(
                "SELECT record_key_hash, operation, intended_hash FROM run_records
                 WHERE run_id = ?1 AND step_seq = ?2 AND record_seq = ?3",
                &record_ref_params(&place),
            )? {
                if row.text(0)? == key_hash
                    && row.text(1)? == record.operation.as_str()
                    && row.text(2)? == record.intended.to_string()
                {
                    continue;
                }
                return Err(failure::unexpected(
                    "a commit point was planned before with another key, operation, or digest",
                ));
            }
            let state = if record.unchanged {
                RecordState::Unchanged
            } else {
                RecordState::Planned
            };
            let key_text = serde_json::to_string(&record.key)
                .map_err(|_| failure::invalid("a record key that cannot be written"))?;
            self.tx.exec(
                "INSERT INTO run_records
                    (run_id, step_seq, record_seq, record_key, record_key_hash, operation, state,
                     intended_hash, intended_ref, before_ref, idempotency_token)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                &[
                    run.id.to_string().into(),
                    record.step_seq.into(),
                    record.record_seq.into(),
                    key_text.into(),
                    key_hash.into(),
                    record.operation.as_str().into(),
                    state.as_str().into(),
                    record.intended.to_string().into(),
                    record.intended_values.into(),
                    record.before.into(),
                    idempotency_token(&place, &record.key).into(),
                ],
            )?;
            planned.push(json!([record.step_seq, record.record_seq, state.as_str()]));
        }
        if !planned.is_empty() {
            self.event(
                run.id,
                fence.epoch,
                EventKind::Planned,
                &json!({ "records": planned }),
                None,
            )?;
        }
        Ok(())
    }

    /// Takes the lock on a record for the run, or confirms it under a new
    /// lease when this run already holds it, held or quarantined (rule 7).
    /// Waiting for a lock another run holds is the scheduler's work; this
    /// call answers at once.
    ///
    /// # Errors
    ///
    /// `busy`/`record_locked` when another run holds the lock.
    /// `possible_commit`/`runner_lost`.
    /// `unknown_variant`/`ledger_state_unexpected` for a record that is
    /// already verified or unchanged, or a final run.
    pub fn take_lock(&mut self, fence: &Fence, record: RecordRef) -> Result<LockHeld, Failure> {
        let run = self.fenced_for(fence, record)?;
        let row = self.record(record)?;
        if matches!(row.state, RecordState::Verified | RecordState::Unchanged) || row.acknowledged()
        {
            return Err(failure::unexpected(
                "a record that is verified, unchanged, or acknowledged needs no lock",
            ));
        }
        let place = [
            SqlValue::from(run.workspace.to_string()),
            run.target.as_str().into(),
            run.environment.as_str().into(),
            row.key_hash.to_string().into(),
        ];
        let mut insert = place.to_vec();
        insert.extend([
            SqlValue::from(run.id.to_string()),
            stored(fence.epoch.get())?.into(),
            self.tx.now().into(),
        ]);
        let taken = self.tx.exec(
            "INSERT INTO record_locks
                (workspace_id, target, environment, record_key_hash, run_id, epoch, state, acquired_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'held', ?7)
             ON CONFLICT (workspace_id, target, environment, record_key_hash) DO NOTHING",
            &insert,
        )?;
        if taken == 0 {
            let mut confirm = place.to_vec();
            confirm.extend([
                SqlValue::from(run.id.to_string()),
                stored(fence.epoch.get())?.into(),
            ]);
            let confirmed = self.tx.exec(
                "UPDATE record_locks SET state = 'held', epoch = ?6
                 WHERE workspace_id = ?1 AND target = ?2 AND environment = ?3
                   AND record_key_hash = ?4 AND run_id = ?5",
                &confirm,
            )?;
            if confirmed == 0 {
                return Err(failure::locked());
            }
        }
        self.event(
            run.id,
            fence.epoch,
            EventKind::Lock,
            &json!({ "step": record.step_seq, "record": record.record_seq }),
            None,
        )?;
        Ok(LockHeld {
            record,
            epoch: fence.epoch,
        })
    }

    /// Stores the intents. Tokens are made of the result only after the
    /// commit, by [`Store::store_intents`].
    pub(crate) fn intend(
        &mut self,
        fence: &Fence,
        intents: &[RecordIntent],
    ) -> Result<Vec<StoredIntent>, Failure> {
        let run = self.fenced(fence)?;
        if run.state != RunState::Running {
            return Err(failure::unexpected(
                "an intent is stored while the run is `running`",
            ));
        }
        if !run.cleared_to_write() {
            return Err(failure::approval_missing(
                "no approval or decision is bound to it",
            ));
        }
        self.nothing_in_doubt(&run)?;
        let mut stored_intents = Vec::new();
        for intent in intents {
            if intent.record.run != run.id {
                return Err(failure::unexpected(
                    "an intent names a record of another run",
                ));
            }
            let row = self.record(intent.record)?;
            if row.state != RecordState::Planned {
                return Err(failure::unexpected(format!(
                    "an intent is stored for a `planned` record, and this one is `{}`",
                    row.state
                )));
            }
            if row.intended != intent.intended {
                return Err(failure::unexpected(
                    "the digest of the intent is not the one that was planned",
                ));
            }
            self.holds_lock(&run, &row, fence.epoch)?;
            let seq = self.event(
                run.id,
                fence.epoch,
                EventKind::Intent,
                &json!({ "step": intent.record.step_seq, "record": intent.record.record_seq }),
                None,
            )?;
            let mut params = record_ref_params(&intent.record).to_vec();
            params.extend([
                SqlValue::from(stored(fence.epoch.get())?),
                stored(seq)?.into(),
                self.tx.now().into(),
                intent.mode.as_ref().map(Mode::as_str).into(),
            ]);
            self.tx.exec(
                "UPDATE run_records
                 SET state = 'intent', intent_epoch = ?4, intent_seq = ?5, intent_at = ?6,
                     binding_mode = ?7, attempts = attempts + 1,
                     failure_class = NULL, failure_code = NULL,
                     acted_at = NULL, reconciled_at = NULL
                 WHERE run_id = ?1 AND step_seq = ?2 AND record_seq = ?3",
                &params,
            )?;
            stored_intents.push(StoredIntent {
                record: intent.record,
                key: row.key,
                epoch: fence.epoch,
                intended: row.intended,
                idempotency_token: row.idempotency_token,
                intent_seq: seq,
            });
        }
        Ok(stored_intents)
    }

    /// Marks a record `acted`: its write was sent. The token is consumed. A
    /// record whose row is not in `intent` under the token's lease and
    /// intent event is refused, so a token changes nothing twice (rule 1).
    ///
    /// # Errors
    ///
    /// `possible_commit`/`runner_lost` when the token's lease is not the
    /// run's current one. `unknown_variant`/`ledger_state_unexpected` when
    /// the run is not `running` or the record is not in `intent` under this
    /// token.
    pub fn mark_acted(&mut self, token: IntentToken, acted: Acted) -> Result<ActedToken, Failure> {
        let intent = token.stored;
        let fence = Fence {
            run: intent.record.run,
            epoch: intent.epoch,
        };
        let run = self.fenced(&fence)?;
        if run.state != RunState::Running {
            return Err(failure::unexpected(
                "a record is acted while the run is `running`",
            ));
        }
        let (row, intent_seq) = self.record_with_seq(intent.record)?;
        if row.state != RecordState::Intent
            || row.intent_epoch != Some(intent.epoch)
            || intent_seq != Some(intent.intent_seq)
        {
            return Err(failure::unexpected(
                "the record is not in `intent` under this token's lease and intent",
            ));
        }
        self.event(
            run.id,
            fence.epoch,
            EventKind::Acted,
            &json!({ "step": intent.record.step_seq, "record": intent.record.record_seq }),
            None,
        )?;
        let mut params = record_ref_params(&intent.record).to_vec();
        params.extend([
            SqlValue::from(self.tx.now()),
            acted.assigned_key.into(),
            acted.outputs.into(),
        ]);
        self.tx.exec(
            "UPDATE run_records
             SET state = 'acted', acted_at = ?4, assigned_key = ?5, outputs_ref = ?6
             WHERE run_id = ?1 AND step_seq = ?2 AND record_seq = ?3",
            &params,
        )?;
        Ok(ActedToken {
            record: intent.record,
            epoch: intent.epoch,
        })
    }

    /// Marks a record `verified` (rule 2): only when the digest read back
    /// equals the intended digest, and with the evidence of the read. The
    /// record's lock is released in the same transaction.
    ///
    /// # Errors
    ///
    /// `mismatch`/`read_back_differs` when the digests differ; nothing
    /// changes, and the record may be read again or marked failed.
    /// `possible_commit`/`runner_lost`.
    /// `unknown_variant`/`ledger_state_unexpected` when the record is not
    /// `acted`, or the evidence names no blob.
    pub fn mark_verified(
        &mut self,
        token: &ActedToken,
        read_back: ReadBack,
    ) -> Result<(), Failure> {
        let fence = Fence {
            run: token.record.run,
            epoch: token.epoch,
        };
        let run = self.fenced(&fence)?;
        let row = self.record(token.record)?;
        if row.state != RecordState::Acted {
            return Err(failure::unexpected(format!(
                "a record is verified from `acted`, and this one is `{}`",
                row.state
            )));
        }
        if row.acknowledged() {
            return Err(failure::unexpected(
                "a record that ended on the system's acknowledgement is settled and is never verified",
            ));
        }
        self.verify(&run, &row, fence.epoch, &read_back, EventKind::Verified)
    }

    /// Ends a record on the system's acknowledgement (decisions 0161 and
    /// 0172): its intent was stored, its write was sent and answered, and
    /// the binding's only evidence is that answer, as the run's own stored
    /// diff says. The record stays `acted`, is never called verified, and
    /// never counts as read back; it is settled, not in doubt. The evidence
    /// of the answer is kept, the lock is released, and an `acted` event
    /// that says `acknowledged` is written, in the same transaction.
    ///
    /// # Errors
    ///
    /// `possible_commit`/`runner_lost` when the token's lease is not the
    /// run's current one. `policy`/`approval_missing` when the run was never
    /// cleared to write (rule 3). `unknown_variant`/`ledger_state_unexpected`
    /// when the run is not `running` or not irreversible, `diff` is of
    /// another record or another diff than the run's, the record is not
    /// `acted` under this lease, already ended, or holds no lock under this
    /// lease, or the evidence names no blob.
    pub fn mark_acknowledged(
        &mut self,
        token: &ActedToken,
        evidence: BlobRef,
        diff: &AcknowledgedInDiff,
    ) -> Result<(), Failure> {
        let fence = Fence {
            run: token.record.run,
            epoch: token.epoch,
        };
        let run = self.fenced(&fence)?;
        if run.state != RunState::Running {
            return Err(failure::unexpected(
                "a record ends on an acknowledgement while the run is `running`",
            ));
        }
        if !run.cleared_to_write() {
            return Err(failure::approval_missing(
                "no approval or decision is bound to it",
            ));
        }
        if run.effect != EffectClass::Irreversible {
            return Err(failure::unexpected(
                "only an irreversible run has records whose only evidence is an acknowledgement",
            ));
        }
        if diff.record != token.record || run.diff != Some(diff.diff) {
            return Err(failure::unexpected(
                "what the diff says was read for another record or another diff",
            ));
        }
        let row = self.record(token.record)?;
        if row.acknowledged() {
            return Err(failure::unexpected(
                "the record ended on an acknowledgement before",
            ));
        }
        if row.state != RecordState::Acted
            || row.intent_epoch != Some(token.epoch)
            || row.acted_at.is_none()
        {
            return Err(failure::unexpected(format!(
                "a record ends on an acknowledgement from `acted` under this lease, and this one is `{}`",
                row.state
            )));
        }
        self.holds_lock(&run, &row, fence.epoch)?;
        self.event(
            run.id,
            fence.epoch,
            EventKind::Acted,
            &json!({
                "step": row.record.step_seq,
                "record": row.record.record_seq,
                "evidence": WriteEvidence::Acknowledged.as_str(),
            }),
            Some(evidence),
        )?;
        let mut params = record_ref_params(&row.record).to_vec();
        params.extend([SqlValue::from(evidence), self.tx.now().into()]);
        self.tx.exec(
            "UPDATE run_records SET evidence_ref = ?4, acknowledged_at = ?5
             WHERE run_id = ?1 AND step_seq = ?2 AND record_seq = ?3",
            &params,
        )?;
        self.end_lock(&run, &row, false)
    }

    /// Marks a record `failed` with the failure that stopped it. Its lock is
    /// quarantined when a write may have been sent and the failure leaves
    /// that in doubt; otherwise it is released. A record that reached
    /// `acted` keeps its lock, quarantined, whatever the class: the class is
    /// the caller's word, and a sent write is settled only by a read.
    ///
    /// # Errors
    ///
    /// `possible_commit`/`runner_lost`;
    /// `unknown_variant`/`ledger_state_unexpected` when the record is
    /// already verified, unchanged, or failed.
    pub fn mark_failed(
        &mut self,
        fence: &Fence,
        record: RecordRef,
        why: &Failure,
        evidence: Option<BlobRef>,
    ) -> Result<(), Failure> {
        let run = self.fenced_for(fence, record)?;
        let row = self.record(record)?;
        if !matches!(
            row.state,
            RecordState::Planned | RecordState::Intent | RecordState::Acted
        ) {
            return Err(failure::unexpected(format!(
                "a record fails from `planned`, `intent`, or `acted`, and this one is `{}`",
                row.state
            )));
        }
        if row.acknowledged() {
            return Err(failure::unexpected(
                "a record that ended on the system's acknowledgement is settled and does not fail",
            ));
        }
        let why = self.redacted(why)?;
        self.event(
            run.id,
            fence.epoch,
            EventKind::Failed,
            &json!({
                "step": record.step_seq,
                "record": record.record_seq,
                "class": why.class().as_str(),
                "code": why.code().as_str(),
                "message": why.message(),
                "next_step": why.next_step(),
                "said": why.said(),
            }),
            evidence,
        )?;
        let mut params = record_ref_params(&record).to_vec();
        params.extend([
            SqlValue::from(why.class().as_str()),
            why.code().as_str().into(),
            evidence.into(),
        ]);
        self.tx.exec(
            "UPDATE run_records
             SET state = 'failed', failure_class = ?4, failure_code = ?5,
                 evidence_ref = COALESCE(?6, evidence_ref)
             WHERE run_id = ?1 AND step_seq = ?2 AND record_seq = ?3",
            &params,
        )?;
        // A write that was sent stays in doubt whatever the failure's class
        // says: only a read settles it.
        let in_doubt = row.state == RecordState::Acted
            || (row.state != RecordState::Planned && !SETTLED_CLASSES.contains(&why.class()));
        self.end_lock(&run, &row, in_doubt)
    }

    /// Settles a record in doubt with what reading it showed (spec section
    /// 208, "Resume, exactly"). The run must hold the record's lock under
    /// this lease: take or confirm it first.
    ///
    /// # Errors
    ///
    /// `mismatch`/`read_back_differs` when `Landed` carries a digest other
    /// than the intended one (rule 2). `possible_commit`/`runner_lost`.
    /// `unknown_variant`/`ledger_state_unexpected` when the record is not in
    /// doubt or its lock is not held under this lease.
    pub fn settle(
        &mut self,
        fence: &Fence,
        record: RecordRef,
        seen: Reconciled,
    ) -> Result<Settled, Failure> {
        let run = self.fenced_for(fence, record)?;
        let row = self.record(record)?;
        if matches!(seen, Reconciled::NeverSent)
            && (row.state != RecordState::Failed
                || row.intent_epoch.is_some()
                || row.acted_at.is_some())
        {
            return Err(failure::unexpected(
                "a record goes back to `planned` unread only when no intent was stored for it; this one must be read",
            ));
        }
        if !matches!(seen, Reconciled::NeverSent) && !row.in_doubt() {
            return Err(failure::unexpected(format!(
                "only a record in doubt is settled, and this one is `{}`",
                row.state
            )));
        }
        self.holds_lock(&run, &row, fence.epoch)?;
        let place = record_ref_params(&record);
        match seen {
            Reconciled::Landed { read, acted } => {
                // What the read found the system assigned, and the outputs a
                // dependent record needs, are kept with the record, as
                // `mark_acted` keeps them on the normal path.
                let mut params = place.to_vec();
                params.extend([
                    SqlValue::from(acted.assigned_key),
                    acted.outputs.into(),
                    self.tx.now().into(),
                ]);
                self.tx.exec(
                    "UPDATE run_records
                     SET assigned_key = COALESCE(?4, assigned_key),
                         outputs_ref = COALESCE(?5, outputs_ref), reconciled_at = ?6
                     WHERE run_id = ?1 AND step_seq = ?2 AND record_seq = ?3",
                    &params,
                )?;
                self.verify(&run, &row, fence.epoch, &read, EventKind::ReconcileRead)?;
                Ok(Settled::Verified)
            }
            Reconciled::NotLanded { evidence } => {
                self.event(
                    run.id,
                    fence.epoch,
                    EventKind::ReconcileRead,
                    &json!({ "step": record.step_seq, "record": record.record_seq, "state": "planned" }),
                    Some(evidence),
                )?;
                let mut params = place.to_vec();
                params.extend([SqlValue::from(evidence), self.tx.now().into()]);
                self.tx.exec(
                    "UPDATE run_records
                     SET state = 'planned', intent_epoch = NULL, intent_seq = NULL,
                         failure_class = NULL, failure_code = NULL, evidence_ref = ?4,
                         acted_at = NULL, reconciled_at = ?5
                     WHERE run_id = ?1 AND step_seq = ?2 AND record_seq = ?3",
                    &params,
                )?;
                Ok(Settled::Planned)
            }
            Reconciled::NeverSent => {
                self.event(
                    run.id,
                    fence.epoch,
                    EventKind::ReconcileRead,
                    &json!({
                        "step": record.step_seq,
                        "record": record.record_seq,
                        "state": "planned",
                        "read": false,
                    }),
                    None,
                )?;
                self.tx.exec(
                    "UPDATE run_records
                     SET state = 'planned', intent_epoch = NULL, intent_seq = NULL,
                         failure_class = NULL, failure_code = NULL
                     WHERE run_id = ?1 AND step_seq = ?2 AND record_seq = ?3",
                    &place,
                )?;
                Ok(Settled::Planned)
            }
            Reconciled::Neither { evidence } => {
                let why = failure::changed_since_preview();
                self.event(
                    run.id,
                    fence.epoch,
                    EventKind::ReconcileRead,
                    &json!({ "step": record.step_seq, "record": record.record_seq, "state": "failed" }),
                    Some(evidence),
                )?;
                let mut params = place.to_vec();
                params.extend([
                    SqlValue::from(why.class().as_str()),
                    why.code().as_str().into(),
                    evidence.into(),
                ]);
                params.push(self.tx.now().into());
                self.tx.exec(
                    "UPDATE run_records
                     SET state = 'failed', failure_class = ?4, failure_code = ?5, evidence_ref = ?6,
                         reconciled_at = ?7
                     WHERE run_id = ?1 AND step_seq = ?2 AND record_seq = ?3",
                    &params,
                )?;
                self.end_lock(&run, &row, false)?;
                Ok(Settled::Stale)
            }
        }
    }

    /// Sorts the run's locks after its lease was lost: the locks of records
    /// in doubt are quarantined, so no other run can write those records
    /// before they have been read back, and the rest are released. The same
    /// happens by itself when a new lease is granted or the run stops.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn quarantine_locks(&mut self, run: RunId) -> Result<(), Failure> {
        self.sort_locks(run)
    }

    // ---- inside --------------------------------------------------------

    fn record_with_seq(&mut self, record: RecordRef) -> Result<(RecordRow, Option<u64>), Failure> {
        let row = self
            .tx
            .row(
                select_records!("WHERE run_id = ?1 AND step_seq = ?2 AND record_seq = ?3"),
                &record_ref_params(&record),
            )?
            .ok_or_else(|| failure::not_found("record"))?;
        record_row(&row)
    }

    /// The run, when the fence is its current lease and it is not final.
    fn fenced(&mut self, fence: &Fence) -> Result<RunRow, Failure> {
        let run = self.run(fence.run)?;
        if run.epoch != fence.epoch {
            return Err(failure::runner_lost());
        }
        if run.state.is_final() {
            return Err(failure::unexpected("a run in a final state never changes"));
        }
        Ok(run)
    }

    fn fenced_for(&mut self, fence: &Fence, record: RecordRef) -> Result<RunRow, Failure> {
        if record.run != fence.run {
            return Err(failure::unexpected("the record belongs to another run"));
        }
        self.fenced(fence)
    }

    fn count_records(&mut self, run: RunId, sql: &'static str) -> Result<i64, Failure> {
        self.tx
            .row(sql, &[run.to_string().into()])?
            .ok_or_else(|| failure::corrupt("a count that gave no row"))?
            .int(0)
    }

    /// Rule 5: nothing of the run is in doubt from an earlier lease.
    fn nothing_in_doubt(&mut self, run: &RunRow) -> Result<(), Failure> {
        let doubtful = self
            .tx
            .row(
                "SELECT COUNT(*) FROM run_records
                 WHERE run_id = ?1 AND intent_epoch < ?2
                   AND (state = 'intent'
                        OR (state = 'acted' AND acknowledged_at IS NULL)
                        OR (state = 'failed' AND reconciled_at IS NULL
                            AND (acted_at IS NOT NULL
                                 OR failure_class NOT IN ('policy', 'stale', 'input'))))",
                &[run.id.to_string().into(), stored(run.epoch.get())?.into()],
            )?
            .ok_or_else(|| failure::corrupt("a count that gave no row"))?
            .int(0)?;
        if doubtful > 0 {
            return Err(failure::reconcile_required());
        }
        Ok(())
    }

    /// Rule 3 for an approval: it is this run's, for its diff and its tool
    /// version, in its workspace, given by a principal, unexpired, not
    /// revoked, marked used by the policy engine in this very transaction,
    /// and backed by a presence check that proves presence wherever the
    /// channel is not the engine's own prompt on one machine.
    fn check_approval(
        &mut self,
        run: &RunRow,
        approval: ApprovalId,
        diff_hash: &str,
    ) -> Result<(), Failure> {
        let row = self
            .tx
            .row(
                "SELECT rowid, scope, run_id, diff_hash, used_at, revoked_at, expires_at,
                        tool_version_hash, workspace_id, approver_id, channel, presence_check
                 FROM approvals WHERE id = ?1",
                &[approval.to_string().into()],
            )?
            .ok_or_else(|| failure::approval_missing("there is no such approval"))?;
        if row.text(1)? != "run" || row.opt_text(2)? != Some(run.id.to_string().as_str()) {
            return Err(failure::approval_missing(
                "the approval is not for this run",
            ));
        }
        if row.text(8)? != run.workspace.to_string() {
            return Err(failure::approval_missing(
                "the approval is of another workspace",
            ));
        }
        if row.opt_text(3)? != Some(diff_hash) {
            return Err(failure::approval_stale());
        }
        if row.opt_text(7)? != Some(run.target_version.to_string().as_str()) {
            return Err(failure::approval_stale());
        }
        if row.opt_int(5)?.is_some() {
            return Err(failure::approval_missing("the approval was revoked"));
        }
        match row.opt_int(6)? {
            Some(expires) if expires > self.tx.now().as_unix_millis() => {}
            Some(_) => return Err(failure::approval_missing("the approval has expired")),
            None => {
                return Err(failure::approval_missing("the approval carries no expiry"));
            }
        }
        if row.text(9)?.parse::<PrincipalId>().is_err() {
            return Err(failure::approval_missing(
                "the approval names no principal who gave it",
            ));
        }
        let own_prompt = row.text(10)? == "local_prompt";
        match row.text(11)? {
            check if PRESENCE_PROVED.contains(&check) => {}
            "not_available" if own_prompt => {}
            "not_available" => {
                return Err(failure::approval_missing(
                    "an approval that did not come by the engine's own prompt must prove a person was present",
                ));
            }
            _ => {
                return Err(failure::approval_missing(
                    "the approval names a presence check the ledger does not know",
                ));
            }
        }
        if row.opt_int(4)?.is_none() {
            return Err(failure::approval_missing(
                "the policy engine has not marked the approval used",
            ));
        }
        if !self
            .tx
            .db
            .changed_in_this_transaction("approvals", row.int(0)?)
        {
            return Err(failure::approval_missing(
                "the approval was used before; an approval is used once, in the transaction that starts the writes",
            ));
        }
        Ok(())
    }

    /// Rule 3 for a decision that lets a run write without a person: it is
    /// this run's, in its workspace, for its diff, an allowed answer to
    /// `write_without_person` made under a policy version, and recorded in
    /// this very transaction, so that no kill switch or newer policy set
    /// came between the decision and the writes.
    fn check_pre_approval(
        &mut self,
        run: &RunRow,
        decision: DecisionId,
        diff_hash: &str,
    ) -> Result<(), Failure> {
        let row = self
            .tx
            .row(
                "SELECT rowid, run_id, question, allowed, diff_hash, workspace_id, policy_version
                 FROM decisions WHERE id = ?1",
                &[decision.to_string().into()],
            )?
            .ok_or_else(|| failure::approval_missing("there is no such decision"))?;
        if row.opt_text(1)? != Some(run.id.to_string().as_str())
            || row.text(2)? != "write_without_person"
            || row.int(3)? != 1
            || row.text(5)? != run.workspace.to_string()
        {
            return Err(failure::approval_missing(
                "the decision did not allow this run to write without a person",
            ));
        }
        if row.opt_text(4)? != Some(diff_hash) {
            return Err(failure::approval_stale());
        }
        if row.opt_text(6)?.is_none() {
            return Err(failure::approval_missing(
                "the decision was made with no policy set in force",
            ));
        }
        if !self
            .tx
            .db
            .changed_in_this_transaction("decisions", row.int(0)?)
        {
            return Err(failure::approval_missing(
                "the decision was recorded in an earlier transaction; a write begins on a decision made in its own",
            ));
        }
        Ok(())
    }

    /// Whether the run may enter `running`.
    fn may_run(&mut self, run: &RunRow) -> Result<(), Failure> {
        if run.effect != EffectClass::Read && !run.cleared_to_write() {
            let planned = self.count_records(
                run.id,
                "SELECT COUNT(*) FROM run_records WHERE run_id = ?1 AND state = 'planned'",
            )?;
            if planned > 0 {
                return Err(failure::approval_missing(
                    "it has records to write and its diff was not approved",
                ));
            }
        }
        self.nothing_in_doubt(run)
    }

    /// Refuses unless the run holds the record's lock, held, under `epoch`.
    fn holds_lock(
        &mut self,
        run: &RunRow,
        record: &RecordRow,
        epoch: Epoch,
    ) -> Result<(), Failure> {
        let row = self.tx.row(
            "SELECT run_id, epoch, state FROM record_locks
             WHERE workspace_id = ?1 AND target = ?2 AND environment = ?3 AND record_key_hash = ?4",
            &[
                run.workspace.to_string().into(),
                run.target.as_str().into(),
                run.environment.as_str().into(),
                record.key_hash.to_string().into(),
            ],
        )?;
        match row {
            Some(row) if row.text(0)? != run.id.to_string() => Err(failure::locked()),
            Some(row) if row.text(2)? == "held" && epoch_of(row.int(1)?)? == epoch => Ok(()),
            _ => Err(failure::unexpected(
                "the record's lock is not held under this lease; take it first",
            )),
        }
    }

    /// Marks the record verified and releases its lock.
    fn verify(
        &mut self,
        run: &RunRow,
        record: &RecordRow,
        epoch: Epoch,
        read_back: &ReadBack,
        kind: EventKind,
    ) -> Result<(), Failure> {
        if read_back.digest != record.intended {
            return Err(failure::read_back_differs());
        }
        self.event(
            run.id,
            epoch,
            kind,
            &json!({
                "step": record.record.step_seq,
                "record": record.record.record_seq,
                "state": "verified",
            }),
            Some(read_back.evidence),
        )?;
        let mut params = record_ref_params(&record.record).to_vec();
        params.extend([
            SqlValue::from(read_back.digest.to_string()),
            read_back.after.into(),
            read_back.evidence.into(),
            self.tx.now().into(),
        ]);
        self.tx.exec(
            "UPDATE run_records
             SET state = 'verified', read_back_hash = ?4, after_ref = ?5, evidence_ref = ?6,
                 verified_at = ?7, failure_class = NULL, failure_code = NULL
             WHERE run_id = ?1 AND step_seq = ?2 AND record_seq = ?3",
            &params,
        )?;
        self.end_lock(run, record, false)
    }

    /// Releases the run's lock on the record, or quarantines it.
    fn end_lock(
        &mut self,
        run: &RunRow,
        record: &RecordRow,
        quarantine: bool,
    ) -> Result<(), Failure> {
        let params = [
            SqlValue::from(run.workspace.to_string()),
            run.target.as_str().into(),
            run.environment.as_str().into(),
            record.key_hash.to_string().into(),
            run.id.to_string().into(),
        ];
        if quarantine {
            self.tx.exec(
                "UPDATE record_locks SET state = 'quarantined'
                 WHERE workspace_id = ?1 AND target = ?2 AND environment = ?3
                   AND record_key_hash = ?4 AND run_id = ?5",
                &params,
            )?;
        } else {
            self.tx.exec(
                "DELETE FROM record_locks
                 WHERE workspace_id = ?1 AND target = ?2 AND environment = ?3
                   AND record_key_hash = ?4 AND run_id = ?5",
                &params,
            )?;
        }
        Ok(())
    }

    /// Quarantines the locks of the run's records in doubt and releases its
    /// other locks.
    fn sort_locks(&mut self, run: RunId) -> Result<(), Failure> {
        let run = [SqlValue::from(run.to_string())];
        self.tx.exec(
            "UPDATE record_locks SET state = 'quarantined'
             WHERE run_id = ?1 AND record_key_hash IN (
                 SELECT record_key_hash FROM run_records
                 WHERE run_id = ?1
                   AND (state = 'intent'
                        OR (state = 'acted' AND acknowledged_at IS NULL)
                        OR (state = 'failed' AND intent_epoch IS NOT NULL AND reconciled_at IS NULL
                            AND (acted_at IS NOT NULL
                                 OR failure_class NOT IN ('policy', 'stale', 'input')))))",
            &run,
        )?;
        self.tx.exec(
            "DELETE FROM record_locks WHERE run_id = ?1 AND state = 'held'",
            &run,
        )?;
        Ok(())
    }

    fn redacted(&mut self, why: &Failure) -> Result<Failure, Failure> {
        Ok(self.tx.shared.attached()?.redact.redact_failure(why))
    }

    /// Appends one event with the next number of its run. The payload passes
    /// the redactor.
    fn event(
        &mut self,
        run: RunId,
        epoch: Epoch,
        kind: EventKind,
        payload: &serde_json::Value,
        evidence: Option<BlobRef>,
    ) -> Result<u64, Failure> {
        let text = self
            .tx
            .shared
            .attached()?
            .redact
            .redact(&payload.to_string());
        let seq = self
            .tx
            .row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM run_events WHERE run_id = ?1",
                &[run.to_string().into()],
            )?
            .ok_or_else(|| failure::corrupt("a count that gave no row"))?
            .int(0)?;
        self.tx.exec(
            "INSERT INTO run_events (run_id, seq, epoch, at, kind, payload, evidence_ref)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            &[
                run.to_string().into(),
                seq.into(),
                stored(epoch.get())?.into(),
                self.tx.now().into(),
                kind.as_str().into(),
                text.into(),
                evidence.into(),
            ],
        )?;
        count(seq)
    }
}
