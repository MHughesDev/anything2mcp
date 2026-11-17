//! What the session keeper is handed: the way to a session's own person,
//! and the probe.
//!
//! The keeper never calls upward. Its requests to a person go through
//! [`PersonBridge`] to the role's person surface, and its probe goes
//! through [`ProbeBridge`] to the engine.

use std::sync::{Arc, OnceLock};

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

/// What runs a session's probe. The kernel sets it once the engine is
/// joined; until then nothing is known about a session, and the keeper
/// treats that as it treats any probe that could not be made.
pub(crate) type Prober =
    Arc<dyn for<'a> Fn(&'a SessionRef) -> BoxFuture<'a, ProbeOutcome> + Send + Sync>;

/// [`RunProbe`] for the keeper of one process.
#[derive(Default)]
pub(crate) struct ProbeBridge {
    prober: OnceLock<Prober>,
}

impl ProbeBridge {
    /// Joins the probe to what runs it. Done once.
    #[allow(dead_code, reason = "joined when the engine runs a declared probe operation")]
    pub(crate) fn join(&self, prober: Prober) {
        let _ = self.prober.set(prober);
    }
}

impl RunProbe for ProbeBridge {
    fn probe<'a>(&'a self, session: &'a SessionRef) -> BoxFuture<'a, ProbeOutcome> {
        match self.prober.get() {
            Some(prober) => prober(session),
            None => Box::pin(async { ProbeOutcome::Unknown }),
        }
    }
}
