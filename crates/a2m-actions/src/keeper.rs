//! What the session keeper is handed: the way to a session's own person,
//! and the session check.
//!
//! The keeper never calls upward. Its requests to a person go through
//! [`PersonBridge`] to the role's person surface, and its session check goes
//! through [`SessionCheckBridge`] to the engine.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};
use std::time::Duration;

use a2m_auth::{
    AskPerson, BoxFuture, PersonAnswer, PersonAsk, RunSessionCheck, SessionCheckOutcome, SessionRef,
};
use a2m_core::{
    CompiledSessionCheck, Digest, EffectClass, FailureClass, FieldHasher, Identity, RunState,
    SessionId, TargetName, ToolRuns,
};
use a2m_engine::encode_values;
use a2m_ledger::{BlobKind, Durability, NewBlob, NewRun, RetentionClass, ToolKind};
use a2m_policy::PrincipalRef;
use a2m_sched::{Admitted, JobKind, JobRequirements, Priority, SessionLimits, Submission};
use a2m_vault::OwnerRef;

use crate::kernel::{Inner, Kernel};
use crate::person::{PersonSurface, SessionAsk};
use crate::runner::{CallerFacts, keep_run_call};

/// How long a session check's run may take, from its admission to its end, before
/// the session check counts as one that could not be made.
const SESSION_CHECK_WAIT: Duration = Duration::from_secs(120);

/// What makes a session check's run another run than the person's own call of the
/// same read with the same input (ledger rule 6).
const PROBE_PURPOSE: &[u8] = b"anything2mcp session probe 1";

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

/// The published version a session was last leased under by a run, whose
/// auth file entry's session check checks it.
#[derive(Debug, Clone)]
pub(crate) struct SessionCheckSource {
    pub(crate) target: TargetName,
    pub(crate) compiled_target: Digest,
    pub(crate) probe: CompiledSessionCheck,
}

/// [`RunSessionCheck`] for the keeper of one process: the entry's `probe`
/// operation, run as an ordinary read through the engine, under a lease of
/// purpose `Probe` (spec section 215, decision 0121).
///
/// The session check of a session is the one of the published version a run last
/// leased it under, noted in memory as the lease is given. A session no run
/// has leased since the engine started, a session check that is an entry of a pack's
/// file kind, an operation that no tool of the compiled target runs, and a
/// run that cannot be made or does not end in time, each answer that nothing
/// is known: the keeper changes no state for it.
#[derive(Default)]
pub(crate) struct SessionCheckBridge {
    kernel: OnceLock<Weak<Inner>>,
    sources: Mutex<BTreeMap<SessionId, SessionCheckSource>>,
}

impl SessionCheckBridge {
    /// Joins the bridge to the kernel it runs session checks through. It holds the
    /// kernel weakly: the keeper's work must not keep a closed engine alive.
    pub(crate) fn bind(&self, kernel: &Arc<Inner>) {
        let _ = self.kernel.set(Arc::downgrade(kernel));
    }

    /// Notes the version a run leased `session` under, and its session check.
    pub(crate) fn note(&self, session: SessionId, source: SessionCheckSource) {
        self.sources
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(session, source);
    }

    fn source(&self, session: SessionId) -> Option<SessionCheckSource> {
        self.sources
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&session)
            .cloned()
    }
}

impl RunSessionCheck for SessionCheckBridge {
    fn check_session<'a>(&'a self, session: &'a SessionRef) -> BoxFuture<'a, SessionCheckOutcome> {
        Box::pin(async move {
            let Some(inner) = self.kernel.get().and_then(Weak::upgrade) else {
                return SessionCheckOutcome::Unknown;
            };
            let Some(source) = self.source(session.session) else {
                return SessionCheckOutcome::Unknown;
            };
            Kernel { inner }.run_session_check(session, &source).await
        })
    }
}

impl Kernel {
    /// Runs a session's session check through the engine and reads how its run
    /// ended: completed is signed in; stopped because the session has ended
    /// is signed out; anything else says nothing.
    async fn run_session_check(
        &self,
        session: &SessionRef,
        source: &SessionCheckSource,
    ) -> SessionCheckOutcome {
        let Identity::Person(person) = session.key.identity else {
            return SessionCheckOutcome::Unknown;
        };
        if !self.runner_running() {
            return SessionCheckOutcome::Unknown;
        }
        let CompiledSessionCheck::Operation { operation, inputs } = &source.probe else {
            return SessionCheckOutcome::Unknown;
        };
        let Ok(compiled_target) = self
            .compiled_target_by_hash(&source.target, &source.compiled_target)
            .await
        else {
            return SessionCheckOutcome::Unknown;
        };
        let Some(tool) = compiled_target
            .tools
            .iter()
            .find(|tool| tool.runs == ToolRuns::Operation(*operation))
        else {
            return SessionCheckOutcome::Unknown;
        };
        let (Ok(store), Ok(sched), Ok(bytes)) = (self.store(), self.sched(), encode_values(inputs))
        else {
            return SessionCheckOutcome::Unknown;
        };
        let workspace = session.key.workspace;
        let Ok(stored) = store
            .blobs()
            .put(NewBlob {
                workspace,
                owner: OwnerRef::of(&Identity::Person(person)),
                kind: BlobKind::Input,
                class: RetentionClass::Short,
                plain: bytes,
            })
            .await
        else {
            return SessionCheckOutcome::Unknown;
        };
        let mut hasher = FieldHasher::new();
        hasher.field("purpose", PROBE_PURPOSE);
        hasher.field("input", inputs.digest().to_string().as_bytes());
        let run = NewRun {
            workspace,
            principal: person,
            client_label: "session keeper".to_owned(),
            tool_name: tool.name.clone(),
            tool_kind: ToolKind::Operation,
            target: source.target.clone(),
            target_version: source.compiled_target,
            environment: session.key.environment.clone(),
            identity: Some(Identity::Person(person)),
            effect: EffectClass::Read,
            input_hash: hasher.finish(),
            input: Some(stored),
            retention: RetentionClass::Short,
        };
        // The run is the person's own read: the engine's guard asks policy
        // about the person, as for a call they make themself. No decision
        // let it in, because no call did: the keeper started it.
        let facts = CallerFacts {
            principal: PrincipalRef::person(person),
            is_agent: false,
            acting_for: None,
            toolset: None,
            decision: a2m_core::DecisionId::from_bytes([0; 16]),
            project: None,
            binding: None,
        };
        let kept = run.clone();
        if store
            .write(Durability::Synced, move |tx| {
                keep_run_call(tx, &kept, stored, &facts)
            })
            .await
            .is_err()
        {
            return SessionCheckOutcome::Unknown;
        }
        let admitted = sched
            .submit(Submission {
                job: JobRequirements {
                    kind: JobKind::SessionKeepalive,
                    target: run.target.clone(),
                    environment: run.environment.clone(),
                    modes: Vec::new(),
                    os: None,
                    acts_as: Some(person),
                    session: None,
                    session_limits: SessionLimits {
                        reads: 4,
                        writes: 1,
                    },
                    exclusive: Vec::new(),
                    max_step: Duration::from_secs(30),
                    writes: false,
                    verify_after_loss: false,
                },
                run,
                priority: Priority::Interactive,
                health: crate::handlers::tools::sched_health(self),
                queue_wait: Some(SESSION_CHECK_WAIT),
                budgets: Vec::new(),
            })
            .await;
        let run = match admitted {
            Ok(Admitted::New { run, .. } | Admitted::Existing { run }) => run,
            Err(_) => return SessionCheckOutcome::Unknown,
        };
        let Ok(row) = self.wait_for_rest(run, SESSION_CHECK_WAIT).await else {
            return SessionCheckOutcome::Unknown;
        };
        // A session check's run never waits for anyone: one that rests, or is still
        // under way, is ended, so that the next session check is a run of its own.
        if !row.state.is_final() {
            let _ = sched.cancel(run).await;
        }
        outcome_of(row.state, row.failure.as_ref().map(|(class, _)| *class))
    }
}

/// What a session check's run says of the session, from how it ended.
fn outcome_of(state: RunState, class: Option<FailureClass>) -> SessionCheckOutcome {
    match (state, class) {
        (RunState::Completed, _) => SessionCheckOutcome::SignedIn,
        (RunState::NeedsLogin, _) | (_, Some(FailureClass::SessionExpired)) => {
            SessionCheckOutcome::SignedOut
        }
        _ => SessionCheckOutcome::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_probes_run_says_signed_in_only_when_it_completed_and_signed_out_only_for_an_ended_session()
    {
        assert_eq!(
            outcome_of(RunState::Completed, None),
            SessionCheckOutcome::SignedIn
        );
        assert_eq!(
            outcome_of(RunState::NeedsLogin, Some(FailureClass::SessionExpired)),
            SessionCheckOutcome::SignedOut
        );
        assert_eq!(
            outcome_of(RunState::Failed, Some(FailureClass::SessionExpired)),
            SessionCheckOutcome::SignedOut
        );
        for (state, class) in [
            (RunState::Failed, Some(FailureClass::Policy)),
            (RunState::Failed, Some(FailureClass::Transient)),
            (RunState::Cancelled, None),
            (RunState::Stopped, Some(FailureClass::Busy)),
        ] {
            assert_eq!(
                outcome_of(state, class),
                SessionCheckOutcome::Unknown,
                "{state:?}"
            );
        }
    }
}
