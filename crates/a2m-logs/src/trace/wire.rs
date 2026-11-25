//! The form a trace takes as bytes: JSON Lines, schema 1.
//!
//! A trace is text in UTF-8, one JSON object per line, each line ended by a
//! line feed. The first line is the header, then one line per step, then one
//! end line. Within a line the entries are written in the order the structs
//! below declare them, with no spaces, so the same records give the same
//! bytes. An entry that has no value is left out.
//!
//! The schema's version is the header's `schema`. A change that a reader of
//! version 1 would misread raises it; an added optional entry does not.
//! `docs/architecture/crates/a2m-logs.md` describes every entry.

use a2m_core::{
    Digest, Failure, FailureClass, FailureCode, Limitation, Mode, OperationName, RunId,
    Sensitivity, StepKind, Value, Version,
};
use serde::{Deserialize, Serialize};

use super::signature::Chain;
use super::{Acting, ArtifactRef, CaptureLevel, ChoiceReason, Locating, Phase, TraceOutcome};

/// The version of the trace schema this crate writes.
pub const TRACE_SCHEMA: u32 = 1;

/// One line of a trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TraceLine {
    /// The first line: what the trace is the trace of.
    Header(HeaderLine),
    /// One step.
    Step(Box<StepLine>),
    /// The last line: the count, the signature, and what is weak about the
    /// trace.
    End(EndLine),
}

/// The first line of a trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderLine {
    /// The schema's version.
    pub schema: u32,
    /// The run.
    pub run: RunId,
    /// Which attempt of the run, from 1.
    pub attempt: u32,
    /// The tool that was called.
    pub operation: OperationName,
    /// The hash of the compiled target that was run.
    /// Stored under the compiled target's old name, `plan`, so that what an
    /// older build wrote still reads (decision 0402).
    #[serde(rename = "plan")]
    pub compiled_target: Digest,
    /// The engine's version.
    pub engine: Version,
    /// The digest of the registry of providers.
    pub registry: Digest,
    /// The capture level the run was traced at.
    pub level: CaptureLevel,
}

/// One step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepLine {
    /// The record's number in the trace, from 0, counting every record
    /// including any that were cut.
    pub seq: u32,
    /// Which pass of the engine loop, from 0.
    pub pass: u32,
    /// The part of the loop.
    pub phase: Phase,
    /// The step's position in its list in the compiled target, from 0.
    pub step: u32,
    /// The kind of step.
    pub kind: StepKind,
    /// The record's key, as the workspace's keyed digest of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<Digest>,
    /// On the first step of a pass: what was chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub begins: Option<PassLine>,
    /// How long the step took, in microseconds.
    pub took_us: u64,
    /// Which locator matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locating: Option<Locating>,
    /// What the driver did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acting: Option<Acting>,
    /// The values the step set, sent, or read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<ValueLine>,
    /// How the step ended.
    pub outcome: TraceOutcome,
    /// The artifacts taken at this step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactRef>,
    /// The digest of this record's part of the signature. Two traces whose
    /// signatures differ first differ at the first record whose `sig`
    /// differs.
    pub sig: Digest,
}

/// The start of a pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassLine {
    /// The operation the pass carries out.
    pub operation: OperationName,
    /// The mode of the binding chosen.
    pub mode: Mode,
    /// Why that binding.
    pub reason: ChoiceReason,
    /// The digest of the provider whose driver was chosen.
    pub driver: Digest,
    /// How long the pass waited, in microseconds.
    pub queue_us: u64,
    /// How long preparing took, in microseconds.
    pub prepare_us: u64,
}

/// One field's values at one step, each held as its label allows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueLine {
    /// The field's name.
    pub field: String,
    /// The field's sensitivity label.
    pub sensitivity: Sensitivity,
    /// True when the value is left out of the signature.
    #[serde(default, skip_serializing_if = "is_false")]
    pub volatile: bool,
    /// The value the step meant to set or send.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intended: Option<Held>,
    /// The value read from the system.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_back: Option<Held>,
    /// The intended value after normalization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_intended: Option<Held>,
    /// The value read after normalization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_read_back: Option<Held>,
    /// Whether the two normalized values were equal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched: Option<bool>,
}

/// For serde, which asks with a reference.
const fn is_false(flag: &bool) -> bool {
    !*flag
}

/// A value as a trace holds it: as it is, or as the workspace's keyed digest
/// of its canonical bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Held {
    /// The value, redacted, with free text cleaned and limited in length.
    Value(Value),
    /// The keyed digest of the value. Equal values in one workspace have
    /// equal digests; the value cannot be confirmed by guessing.
    Keyed(Digest),
}

/// The last line of a trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndLine {
    /// How many steps the run recorded.
    pub records: u32,
    /// How many of them are in this trace. Fewer than `records` when the
    /// trace passed its size limit and was cut.
    pub kept: u32,
    /// The trace signature, over every record, kept or cut.
    pub signature: Digest,
    /// What is weak about this trace.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limitations: Vec<Limitation>,
}

/// Reads a trace's bytes back into its lines.
///
/// # Errors
///
/// Class `input`, code `schema_invalid`, naming the first line that is not a
/// line of this schema. The line's content is never in the message.
pub fn read_trace(bytes: &[u8]) -> Result<Vec<TraceLine>, Failure> {
    bytes
        .split(|byte| *byte == b'\n')
        .enumerate()
        .filter(|(_, line)| !line.is_empty())
        .map(|(index, line)| {
            serde_json::from_slice(line).map_err(|_| {
                Failure::new(
                    FailureClass::Input,
                    FailureCode::SCHEMA_INVALID,
                    format!("Line {} is not a line of a step trace.", index + 1),
                    "Read the trace with the version of the engine that wrote it.",
                )
            })
        })
        .collect()
}

/// What looking at a trace's own chain found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TraceCheck {
    /// One header, the step lines numbered from 0 with no gap, one end
    /// line, as many step lines as the end line counts, and the chain over
    /// their digests gives the signature the end line states.
    Whole,
    /// The trace passed its size limit and was cut: its step lines are the
    /// first `kept` ones, numbered with no gap, and fewer than the
    /// signature covers, so the signature cannot be worked out from them.
    Cut,
    /// A line is missing, added, moved, or repeated, or the end line states
    /// another signature than the lines give.
    Broken,
}

impl TraceCheck {
    /// The word, for a result.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Whole => "whole",
            Self::Cut => "cut",
            Self::Broken => "broken",
        }
    }
}

/// Checks a trace against its own chain: that no step line was removed,
/// added, moved, or repeated since the end line was written.
///
/// What this is not. The signature is not keyed, so whoever can rewrite the
/// end line as well passes this check: that a stored trace is the one the
/// engine wrote rests on the store, which seals it. And a step line's own
/// digest is taken as written: it covers values that are held only as keyed
/// digests, which a reader without the key cannot work out again.
#[must_use]
pub fn check_trace(lines: &[TraceLine]) -> TraceCheck {
    let (Some(TraceLine::Header(_)), Some(TraceLine::End(end))) = (lines.first(), lines.last())
    else {
        return TraceCheck::Broken;
    };
    let mut chain = Chain::new();
    for (index, line) in lines[1..lines.len() - 1].iter().enumerate() {
        match line {
            TraceLine::Step(step) if u64::from(step.seq) == index as u64 => {
                chain.push(&step.sig);
            }
            _ => return TraceCheck::Broken,
        }
    }
    if chain.records() != end.kept || end.kept > end.records {
        return TraceCheck::Broken;
    }
    if end.kept < end.records {
        return TraceCheck::Cut;
    }
    if chain.signature() == end.signature {
        TraceCheck::Whole
    } else {
        TraceCheck::Broken
    }
}
