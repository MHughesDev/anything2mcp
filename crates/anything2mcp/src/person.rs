//! The person surface of a one-shot command: a terminal, or nobody.
//!
//! The local engine's surface (a notification, the approval page, and the
//! operating system's presence prompt) belongs to `a2m-local`, which this
//! build does not have. A command that runs the engine in its own process
//! can reach a person only through the terminal it was started at, so:
//!
//! - **Approval.** There is none. The approval desk the kernel hands over
//!   is dropped, so a one-shot command cannot record an approval by any
//!   path. A write rests at `needs_approval` and says where approval will
//!   be possible.
//! - **Presence.** When the command was started at a terminal, the person
//!   is asked there to type the word `yes`. This machine then offers no
//!   presence check the operating system owns, and the proof says exactly
//!   that (`not_available`): it stops a mistake, and is not a security
//!   boundary (spec section 222). With no terminal, nobody is asked and
//!   the act is refused.

use std::io::{BufRead, IsTerminal, Write};
use std::sync::Arc;

use a2m_actions::{
    ApprovalAsk, ApprovalDesk, BoxFuture, PersonSurface, PresenceAsk, PresenceProof, SessionAsk,
    SignInAsk, SurfaceLink,
};
use a2m_auth::PersonAnswer;
use a2m_core::{Failure, FailureClass};
use a2m_policy::PresenceCheck;
use a2m_telemetry::Clock;

/// The word a person types to say they are there.
const YES: &str = "yes";

/// A terminal a person may be at.
pub trait Terminal: Send + Sync {
    /// Whether a person can be asked: both the input and the place the
    /// question is shown are a terminal.
    fn interactive(&self) -> bool;

    /// Shows `question` and returns the line the person typed, without its
    /// end. `None` when nothing could be read.
    fn ask(&self, question: &str) -> Option<String>;
}

/// The terminal the process was started at: the question goes to standard
/// error, so that standard output stays the result alone.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessTerminal;

impl Terminal for ProcessTerminal {
    fn interactive(&self) -> bool {
        std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
    }

    fn ask(&self, question: &str) -> Option<String> {
        let mut shown = std::io::stderr().lock();
        write!(shown, "{question} ").ok()?;
        shown.flush().ok()?;
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).ok()?;
        Some(line.trim().to_owned())
    }
}

/// The person surface of a one-shot command.
pub struct TerminalSurface {
    terminal: Arc<dyn Terminal>,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for TerminalSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalSurface").finish_non_exhaustive()
    }
}

impl TerminalSurface {
    /// A surface over `terminal`.
    #[must_use]
    pub fn new(terminal: Arc<dyn Terminal>, clock: Arc<dyn Clock>) -> Self {
        Self { terminal, clock }
    }
}

fn nobody(message: &str, fix: &str) -> Failure {
    Failure::new(
        FailureClass::Policy,
        a2m_policy::PRESENCE_REQUIRED,
        message,
        fix,
    )
}

impl PersonSurface for TerminalSurface {
    fn take_desk(&self, desk: ApprovalDesk) {
        // Dropped on purpose: nothing in a one-shot command can approve.
        drop(desk);
    }

    fn ask_approval(&self, _ask: ApprovalAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async {
            Ok(SurfaceLink {
                url: None,
                text: "A person approves a write on the approval page of the local engine, \
                       which this build does not have yet. Nothing was written."
                    .to_owned(),
            })
        })
    }

    fn ask_sign_in(&self, _ask: SignInAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async {
            Ok(SurfaceLink {
                url: None,
                text: "A person signs in at the local engine's own window, which this build \
                       does not have yet."
                    .to_owned(),
            })
        })
    }

    fn confirm_presence(&self, ask: PresenceAsk) -> BoxFuture<'_, Result<PresenceProof, Failure>> {
        Box::pin(async move {
            if !self.terminal.interactive() {
                return Err(nobody(
                    "This is a person's act, and the command was not started at a terminal.",
                    "Run the command yourself, at a terminal.",
                ));
            }
            let terminal = Arc::clone(&self.terminal);
            let question = format!(
                "`{}` is a person's act: {}\nThis machine has no presence check of its own here, \
                 so this question only guards against a mistake.\nType `{YES}` to go on:",
                ask.action, ask.summary
            );
            // Reading a terminal blocks, so it is done on a thread that may.
            let answer = tokio::task::spawn_blocking(move || terminal.ask(&question))
                .await
                .ok()
                .flatten();
            if answer.as_deref() == Some(YES) {
                Ok(PresenceProof::new(
                    PresenceCheck::NotAvailable,
                    ask.principal,
                    self.clock.now(),
                ))
            } else {
                Err(nobody(
                    "The person at the terminal did not say yes.",
                    "Run the command again and type `yes` when asked.",
                ))
            }
        })
    }

    fn ask_session(&self, _ask: SessionAsk) -> BoxFuture<'_, PersonAnswer> {
        Box::pin(async { PersonAnswer::NoAnswer })
    }
}
