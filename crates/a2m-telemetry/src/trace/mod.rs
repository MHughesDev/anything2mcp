//! The step trace: one structured record per step of a run, the capture of
//! artifacts at the level a run asks for, and the trace signature (spec
//! section 217).
//!
//! The engine makes one [`TraceBuilder`] per run attempt, gives it a
//! [`StepRecord`] for every step a driver carried out, offers it the
//! artifacts drivers returned, and ends it with [`TraceBuilder::finish`].
//! What comes back is redacted bytes for the store, and the signature.
//! Nothing here is written anywhere by this crate.

mod artifact;
mod builder;
mod signature;
mod wire;

use std::time::Duration;

use a2m_core::{
    Digest, FailureClass, FailureCode, Mode, OperationName, RecordKey, RunId, Sensitivity,
    StepKind, Value, Version, WorkspaceId,
};
use serde::{Deserialize, Serialize};

pub use artifact::{
    ArtifactKind, ArtifactOutcome, ArtifactRef, CapturePoint, DropReason, KeptArtifact, MaskReport,
    OfferedArtifact,
};
pub use builder::{DEFAULT_TRACE_LIMIT, FinishedTrace, TraceBuilder, TraceRefused};
pub use wire::{
    EndLine, HeaderLine, Held, PassLine, StepLine, TRACE_SCHEMA, TraceLine, ValueLine, read_trace,
};

/// How much evidence a run keeps. The plan and policy decide a run's level;
/// this crate applies it. Each level includes the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureLevel {
    /// The step trace and nothing else.
    Minimal,
    /// Plus a snapshot at each checkpoint and at any failure.
    Standard,
    /// Plus every exchange and a screenshot per step.
    Full,
}

impl CaptureLevel {
    /// The level's name, as section 217 and a trace's header write it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Standard => "standard",
            Self::Full => "full",
        }
    }
}

/// What one trace is the trace of. Written once, in the trace's first line.
/// None of it enters the signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceIdentity {
    /// The workspace the run belongs to. Its key is the one restricted
    /// values are digested under; the workspace itself is not written.
    pub workspace: WorkspaceId,
    /// The run.
    pub run: RunId,
    /// Which attempt of the run this is, from 1.
    pub attempt: u32,
    /// The tool that was called.
    pub operation: OperationName,
    /// The hash of the plan that was run.
    pub plan: Digest,
    /// The engine's version.
    pub engine: Version,
    /// The digest of the registry: which providers, at which versions, were
    /// there to choose from.
    pub registry: Digest,
}

/// The part of the engine loop a step belongs to. A binding is compiled into
/// three lists of steps, and a step is traced the same way whichever list it
/// came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// The steps that do the work.
    Act,
    /// The steps that wait for the declared confirmation.
    Confirm,
    /// The steps that read the value again.
    ReadBack,
}

impl Phase {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Act => "act",
            Self::Confirm => "confirm",
            Self::ReadBack => "read_back",
        }
    }
}

/// One step, as the engine saw it. One record is one driver instruction: one
/// call of a driver's `act`. A step that is retried is two records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRecord {
    /// Which pass of the engine loop this step belongs to, from 0. A pass is
    /// one turn through the loop: one operation call, for one record, by one
    /// binding.
    pub pass: u32,
    /// The part of the loop the step belongs to.
    pub phase: Phase,
    /// The step's position in its list in the plan, from 0.
    pub step: u32,
    /// The kind of step.
    pub kind: StepKind,
    /// The record being read or written, when there is one.
    pub record: Option<RecordKey>,
    /// On the first step of a pass: what was chosen, and how long the pass
    /// waited and prepared before it.
    pub begins: Option<PassStart>,
    /// How long the step took. Not in the signature.
    pub took: Duration,
    /// For a step that located something: which locator matched.
    pub locating: Option<Locating>,
    /// What the driver did, in the driver's own fixed words.
    pub acting: Option<Acting>,
    /// The values the step set, sent, or read.
    pub values: Vec<TracedValue>,
    /// How the step ended.
    pub outcome: TraceOutcome,
    /// The artifacts taken at this step, by the names the store gave them.
    pub artifacts: Vec<ArtifactRef>,
}

/// The start of a pass: the binding the engine chose and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassStart {
    /// The operation the pass carries out. In a pipeline this differs from
    /// the tool that was called.
    pub operation: OperationName,
    /// The mode of the binding that was chosen.
    pub mode: Mode,
    /// Why that binding.
    pub reason: ChoiceReason,
    /// The digest of the provider whose driver was chosen for the mode.
    pub driver: Digest,
    /// How long the pass waited before it could start: for a rate budget, a
    /// lock, a lease.
    pub queue: Duration,
    /// How long preparing took.
    pub prepare: Duration,
}

/// Why the engine chose the binding it chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChoiceReason {
    /// It is the first binding in the plan's order, and it qualified.
    FirstInOrder,
    /// Bindings before it in the plan's order did not qualify.
    EarlierDidNotQualify,
    /// The caller pinned the mode.
    Pinned,
    /// A binding before it failed during this call, and the engine fell back.
    FellBack,
}

/// Which locator matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Locating {
    /// The locator that matched, as its file writes it.
    pub locator: String,
    /// Its position in the list of locators that were tried, from 0.
    pub position: u32,
    /// The fingerprint score, as a whole number on the scale of the pack
    /// that computed it. Not in the signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<u32>,
}

/// What a driver did. The two lists are the pack's to fill, with its own
/// fixed names: this crate knows no kind of system.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Acting {
    /// How the step was done: what must be the same in two runs that did the
    /// same thing. In the signature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub means: Vec<Detail>,
    /// What was measured while doing it and may differ between two such
    /// runs, such as a size. Not in the signature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub measured: Vec<Detail>,
}

/// One named detail of what a driver did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detail {
    /// The detail's name, fixed by the pack.
    pub name: String,
    /// Its value, as text.
    pub value: String,
}

impl Detail {
    /// A detail from its name and its value.
    #[must_use]
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// One field's values at one step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracedValue {
    /// The field's name.
    pub field: String,
    /// The field's sensitivity label. `public` is kept as it is at every
    /// level; `internal` as it is at `standard` and above, and as a keyed
    /// digest at `minimal`; `restricted` only ever as a keyed digest.
    pub sensitivity: Sensitivity,
    /// True when the plan declares the field's value volatile: it is kept in
    /// the trace and left out of the signature.
    pub volatile: bool,
    /// The value the step meant to set or send.
    pub intended: Option<Value>,
    /// The value read from the system.
    pub read_back: Option<Value>,
    /// The intended value after normalization.
    pub normalized_intended: Option<Value>,
    /// The value read after normalization.
    pub normalized_read_back: Option<Value>,
    /// Whether the two normalized values were equal, when both exist.
    pub matched: Option<bool>,
}

impl TracedValue {
    /// A field that was read: the value as the system gave it, and its
    /// normalized form.
    #[must_use]
    pub fn read(
        field: impl Into<String>,
        sensitivity: Sensitivity,
        read_back: Value,
        normalized: Value,
    ) -> Self {
        Self {
            field: field.into(),
            sensitivity,
            volatile: false,
            intended: None,
            read_back: Some(read_back),
            normalized_intended: None,
            normalized_read_back: Some(normalized),
            matched: None,
        }
    }

    /// Marks the field's value as volatile.
    #[must_use]
    pub fn volatile(mut self) -> Self {
        self.volatile = true;
        self
    }
}

/// How a step ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TraceOutcome {
    /// The step's values were read back and matched.
    Verified,
    /// The step was carried out, and had nothing to verify.
    Done,
    /// The step failed.
    Failed {
        /// The failure's class.
        class: FailureClass,
        /// The failure's code.
        code: FailureCode,
    },
}
