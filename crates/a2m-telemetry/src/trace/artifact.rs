//! Artifacts: the large pieces of evidence a driver returns, and the rule
//! that says which of them a capture level keeps.

use a2m_core::Digest;
use serde::{Deserialize, Serialize};

use super::CaptureLevel;

/// What kind of artifact a piece of evidence is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    /// A compact snapshot of what a surface showed, as text.
    Snapshot,
    /// A request and its answer, as text.
    Exchange,
    /// An image of what was on the screen.
    Screenshot,
    /// The tree of windows and elements a desktop surface showed, as text.
    WindowTree,
}

impl ArtifactKind {
    /// Whether the artifact is text, which can be redacted and checked. An
    /// image cannot; it is masked by the pack that took it.
    #[must_use]
    pub const fn is_text(self) -> bool {
        !matches!(self, Self::Screenshot)
    }
}

/// Where in a run an artifact was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapturePoint {
    /// At an ordinary step.
    Step,
    /// At a checkpoint the pipeline declares.
    Checkpoint,
    /// At a failure.
    Failure,
}

/// What the pack that took an image painted over. Only the pack knows where
/// the password inputs and labelled fields were on its screen; this crate
/// never paints over an image, and keeps none that arrives without a report.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MaskReport {
    /// The names of the fields that were painted over. Empty when the pack
    /// looked and found nothing to mask.
    pub fields: Vec<String>,
}

/// An artifact a driver returned, offered to the trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferedArtifact {
    /// What kind it is.
    pub kind: ArtifactKind,
    /// Where it was taken.
    pub at: CapturePoint,
    /// Its content.
    pub bytes: Vec<u8>,
    /// For an image, what the pack masked.
    pub masked: Option<MaskReport>,
}

/// What became of an offered artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactOutcome {
    /// It is kept: here are its redacted bytes, for the store.
    Kept(KeptArtifact),
    /// It is not kept.
    Dropped(DropReason),
}

/// An artifact that is kept, ready for the store to seal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptArtifact {
    /// What kind it is.
    pub kind: ArtifactKind,
    /// Where it was taken.
    pub at: CapturePoint,
    /// Its content, redacted and checked.
    pub bytes: Vec<u8>,
    /// For an image, what the pack masked.
    pub masked: Option<MaskReport>,
}

/// Why an artifact was not kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DropReason {
    /// The run's capture level does not keep this kind at this point. This
    /// is the ordinary case and records no limitation.
    Level,
    /// An image arrived without a report of what was masked.
    Unmasked,
    /// The artifact still held a registered secret after redaction.
    HeldSecret,
    /// An artifact of a text kind was not UTF-8 text, so it could not be
    /// redacted or checked.
    NotText,
}

/// The reference a step record holds to an artifact the store kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    /// What kind it is.
    pub kind: ArtifactKind,
    /// Where it was taken.
    pub at: CapturePoint,
    /// The name the store gave the blob.
    pub blob: Digest,
}

/// Whether `level` keeps an artifact of `kind` taken at `at` (section 217,
/// "Capture levels").
pub(crate) const fn level_keeps(level: CaptureLevel, kind: ArtifactKind, at: CapturePoint) -> bool {
    match level {
        CaptureLevel::Minimal => false,
        CaptureLevel::Standard => match kind {
            ArtifactKind::Snapshot | ArtifactKind::WindowTree => {
                matches!(at, CapturePoint::Checkpoint | CapturePoint::Failure)
            }
            // Outside `full`, a screenshot is kept only at a failure.
            ArtifactKind::Screenshot => matches!(at, CapturePoint::Failure),
            ArtifactKind::Exchange => false,
        },
        CaptureLevel::Full => true,
    }
}
