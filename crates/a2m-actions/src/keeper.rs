//! What the session keeper is handed: the way to a session's own person,
//! and the probe.
//!
//! The keeper never calls upward. Its requests to a person go through
//! [`PersonBridge`] to the role's person surface, and its probe goes
//! through [`ProbeBridge`] to the engine.

use std::sync::Arc;

use a2m_auth::{AskPerson, BoxFuture, PersonAnswer, PersonAsk, ProbeOutcome, RunProbe, SessionRef};

use crate::person::{PersonSurface, SessionAsk};

/// [`AskPerson`] over the role's person surface.
pub(crate) struct PersonBridge {
    surface: Arc<dyn PersonSurface>,
}

impl PersonBridge {
    pub(crate) fn new(surface: Arc<dyn PersonSurface>) -> Self {
        Self { surface }
    }
}

impl AskPerson for PersonBridge {
    fn ask<'a>(&'a self, session: &'a SessionRef, ask: PersonAsk) -> BoxFuture<'a, PersonAnswer> {
        self.surface.ask_session(SessionAsk {
            session: session.session,
            workspace: session.key.workspace,
            target: session.key.target.clone(),
            environment: session.key.environment.clone(),
            identity: session.key.identity.clone(),
            ask,
        })
    }

    fn return_address(&self) -> Option<String> {
        self.surface.return_address()
    }
}

/// [`RunProbe`] for the keeper of one process.
///
/// Running a target's declared probe, an operation through the engine or
/// an entry handed to the surface's pack (decision 0121), is not built: the
/// answer is that nothing is known, which the keeper treats as it treats
/// any probe that could not be made. A session is then checked by the
/// signs of expiry its driver reports.
#[derive(Default)]
pub(crate) struct ProbeBridge;

impl RunProbe for ProbeBridge {
    fn probe<'a>(&'a self, _session: &'a SessionRef) -> BoxFuture<'a, ProbeOutcome> {
        Box::pin(async { ProbeOutcome::Unknown })
    }
}
