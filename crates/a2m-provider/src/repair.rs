//! What a pack implements to propose a repair of the knowledge a drift was
//! seen in (decision 0999): a pure rule over two findings of what a surface
//! showed, before and now, and the draft files that hold the knowledge. It
//! proposes a change to one file, which a person accepts as a draft; it
//! applies nothing, and nothing in a run calls it (rule 1).

use a2m_core::{FailureCode, Finding};

/// One draft file a proposer may change, as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairFile {
    /// Its path inside the target folder.
    pub path: String,
    /// Its text, as the drafts hold it.
    pub text: String,
}

/// What a proposer is asked about: one drift, the files that may hold its
/// knowledge, and two findings of the surface.
#[derive(Debug, Clone, Copy)]
pub struct RepairAsk<'a> {
    /// The drifted entry's name, as the drift mark holds it: in the web
    /// pack a screen.
    pub entry: &'a str,
    /// The pack's code the drift stopped on, such as `locator_not_found`.
    pub code: &'a FailureCode,
    /// Every draft file of the file kind whose compiled section holds the
    /// drifted entry, in order of path.
    pub files: &'a [RepairFile],
    /// A finding of what the surface showed while the knowledge still fit
    /// it, when one was given.
    pub before: Option<&'a Finding>,
    /// A finding of what the surface shows now.
    pub now: &'a Finding,
}

/// What a proposer gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairProposal {
    /// A change to one file, whole: its path, its new text, and one line for
    /// each change saying what the findings showed. Every break the rule
    /// sees is repaired, or none is.
    Proposed {
        /// The path of the file, one of [`RepairAsk::files`].
        path: String,
        /// The file's text after the change.
        text: String,
        /// Why each change is made, in the pack's words.
        because: Vec<String>,
    },
    /// No sure repair, and why.
    None {
        /// Why nothing is proposed, in the pack's words.
        reason: String,
    },
}

impl RepairProposal {
    /// No proposal, for `reason`.
    #[must_use]
    pub fn none(reason: impl Into<String>) -> Self {
        Self::None {
            reason: reason.into(),
        }
    }
}

/// A pack's repair proposer for the findings of one kind it owns. Pure: no
/// input or output, and the same ask gives the same proposal. A public
/// contract.
pub trait RepairProposer: Send + Sync {
    /// Proposes a repair, or says why there is none sure enough to propose.
    /// Unknown means no proposal (rule 7).
    fn propose(&self, ask: &RepairAsk<'_>) -> RepairProposal;
}
