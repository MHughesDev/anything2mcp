//! The presence prompt: the one thing on a machine that an agent with a
//! shell cannot answer.
//!
//! The person surface asks through [`PresencePrompt`] and never decides
//! what an answer is worth beyond the table on [`PromptAnswer`]. This
//! crate ships one prompt, [`NoPrompt`], which says that none is
//! available: no operating system's own prompt is built yet. With it, no
//! approval is recorded on this machine and no protected setting is
//! loosened.

use std::sync::Arc;

use a2m_actions::BoxFuture;

/// Which kind of presence check a machine has. `doctor.run` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceKind {
    /// A prompt the operating system owns: a fingerprint, a face, a PIN,
    /// or a password.
    OsPrompt,
    /// A native confirmation dialog alone. It stops mistakes; it is not a
    /// security boundary.
    DialogOnly,
    /// Nothing that can be raised.
    NotAvailable,
}

impl PresenceKind {
    /// The kind as a report writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OsPrompt => "os_prompt",
            Self::DialogOnly => "dialog_only",
            Self::NotAvailable => "not_available",
        }
    }
}

/// What a prompt shows: words the engine made, never a caller's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptText {
    /// What is asked, in a line.
    pub title: String,
    /// What it is about: the tool, the run, the hash of the change.
    pub detail: String,
}

/// How a prompt ended.
///
/// | Answer | An approval | A protected setting |
/// | --- | --- | --- |
/// | `Present` | recorded, with the check `os_prompt` | may be loosened |
/// | `Confirmed` | recorded, with the check `not_available` | refused |
/// | `Dismissed` | not recorded | refused |
/// | `NotAvailable` | not recorded | refused |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptAnswer {
    /// The operating system's own prompt was passed.
    Present,
    /// The person pressed yes in a native dialog. Nothing proves a person.
    Confirmed,
    /// The person dismissed or failed the prompt.
    Dismissed,
    /// No prompt could be raised.
    NotAvailable,
}

/// A way to ask whether a person is at the machine.
pub trait PresencePrompt: Send + Sync {
    /// Which kind of check this is.
    fn kind(&self) -> PresenceKind;

    /// Raises the prompt and waits for its end.
    fn ask(&self, text: PromptText) -> BoxFuture<'_, PromptAnswer>;
}

/// The prompt of a machine with none: every ask ends `NotAvailable`.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoPrompt;

impl PresencePrompt for NoPrompt {
    fn kind(&self) -> PresenceKind {
        PresenceKind::NotAvailable
    }

    fn ask(&self, _text: PromptText) -> BoxFuture<'_, PromptAnswer> {
        Box::pin(async { PromptAnswer::NotAvailable })
    }
}

/// The kind of presence check this build can raise on this machine. No
/// operating system's prompt and no fallback dialog is built yet, so the
/// answer is `NotAvailable` everywhere.
#[must_use]
pub const fn presence_available() -> PresenceKind {
    PresenceKind::NotAvailable
}

/// The prompt this build has for this machine: [`NoPrompt`].
#[must_use]
pub fn system_prompt() -> Arc<dyn PresencePrompt> {
    Arc::new(NoPrompt)
}
