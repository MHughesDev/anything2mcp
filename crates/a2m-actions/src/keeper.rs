//! What the session keeper is handed: the way to a session's own person,
//! and the probe.
//!
//! The keeper never calls upward. Its requests to a person go through
//! [`PersonBridge`] to the role's person surface, and its probe goes
//! through [`ProbeBridge`] to the engine.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};
use std::time::Duration;

use a2m_auth::{AskPerson, BoxFuture, PersonAnswer, PersonAsk, ProbeOutcome, RunProbe, SessionRef};
use a2m_core::{
    Digest, EffectClass, FailureClass, FieldHasher, Identity, ProbePlan, RunState, SessionId,
    TargetName, ToolRuns,
};
use a2m_engine::encode_values;
use a2m_ledger::{BlobKind, Durability, NewBlob, NewRun, RetentionClass, ToolKind};
use a2m_policy::PrincipalRef;
use a2m_sched::{Admitted, JobKind, JobRequirements, Priority, SessionLimits, Submission};
use a2m_vault::OwnerRef;

use crate::kernel::{Inner, Kernel};
use crate::person::{PersonSurface, SessionAsk};
use crate::runner::{CallerFacts, keep_run_call};

/// How long a probe's run may take, from its admission to its end, before
/// the probe counts as one that could not be made.
const PROBE_WAIT: Duration = Duration::from_secs(120);

/// What makes a probe's run another run than the person's own call of the
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
/// auth file entry's probe checks it.
#[derive(Debug, Clone)]
pub(crate) struct ProbeSource {
    pub(crate) target: TargetName,
    pub(crate) plan: Digest,
    pub(crate) probe: ProbePlan,
}

/// [`RunProbe`] for the keeper of one process: the entry's `probe`
/// operation, run as an ordinary read through the engine, under a lease of
/// purpose `Probe` (spec section 215, decision 0121).
///
/// The probe of a session is the one of the published version a run last
/// leased it under, noted in memory as the lease is given. A session no run
/// has leased since the engine started, a probe that is an entry of a
/// pack's file kind, an operation that no tool of the plan runs, and a run
/// that cannot be made or does not end in time, each answer that nothing is
/// known: the keeper changes no state for it.
#[derive(Default)]
pub(crate) struct ProbeBridge {
    kernel: OnceLock<Weak<Inner>>,
    sources: Mutex<BTreeMap<SessionId, ProbeSource>>,
}

impl ProbeBridge {
    /// Joins the bridge to the kernel it runs probes through. It holds the
    /// kernel weakly: the keeper's work must not keep a closed engine alive.
    pub(crate) fn bind(&self, kernel: &Arc<Inner>) {
        let _ = self.kernel.set(Arc::downgrade(kernel));
    }

    /// Notes the version a run leased `session` under, and its probe.
    pub(crate) fn note(&self, session: SessionId, source: ProbeSource) {
        self.sources
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(session, source);
    }

    fn source(&self, session: SessionId) -> Option<ProbeSource> {
        self.sources
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&session)
            .cloned()
    }
}

impl RunProbe for ProbeBridge {
    fn probe<'a>(&'a self, session: &'a SessionRef) -> BoxFuture<'a, ProbeOutcome> {
        Box::pin(async move {
            let Some(inner) = self.kernel.get().and_then(Weak::upgrade) else {
                return ProbeOutcome::Unknown;
            };
            let Some(source) = self.source(session.session) else {
                return ProbeOutcome::Unknown;
            };
            Kernel { inner }.run_probe(session, &source).await
        })
    }
}

impl Kernel {
    /// Runs a session's probe through the engine and reads how its run
    /// ended: completed is signed in; stopped because the session has ended
    /// is signed out; anything else says nothing.
    async fn run_probe(&self, session: &SessionRef, source: &ProbeSource) -> ProbeOutcome {
        let Identity::Person(person) = session.key.identity else {
            return ProbeOutcome::Unknown;
        };
        if !self.runner_running() {
            return ProbeOutcome::Unknown;
        }
        let ProbePlan::Operation { operation, inputs } = &source.probe else {
            return ProbeOutcome::Unknown;
        };
        let Ok(plan) = self.plan_by_hash(&source.target, &source.plan).await else {
            return ProbeOutcome::Unknown;
        };
        let Some(tool) = plan
            .tools
            .iter()
            .find(|tool| tool.runs == ToolRuns::Operation(*operation))
        else {
            return ProbeOutcome::Unknown;
        };
        let (Ok(store), Ok(sched), Ok(bytes)) = (self.store(), self.sched(), encode_values(inputs))
        else {
            return ProbeOutcome::Unknown;
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
            return ProbeOutcome::Unknown;
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
            target_version: source.plan,
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
            return ProbeOutcome::Unknown;
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
                queue_wait: Some(PROBE_WAIT),
                budgets: Vec::new(),
            })
            .await;
        let run = match admitted {
            Ok(Admitted::New { run, .. } | Admitted::Existing { run }) => run,
            Err(_) => return ProbeOutcome::Unknown,
        };
        let Ok(row) = self.wait_for_rest(run, PROBE_WAIT).await else {
            return ProbeOutcome::Unknown;
        };
        // A probe's run never waits for anyone: one that rests, or is still
        // under way, is ended, so that the next probe is a run of its own.
        if !row.state.is_final() {
            let _ = sched.cancel(run).await;
        }
        outcome_of(row.state, row.failure.as_ref().map(|(class, _)| *class))
    }
}

/// What a probe's run says of the session, from how it ended.
fn outcome_of(state: RunState, class: Option<FailureClass>) -> ProbeOutcome {
    match (state, class) {
        (RunState::Completed, _) => ProbeOutcome::SignedIn,
        (RunState::NeedsLogin, _) | (_, Some(FailureClass::SessionExpired)) => {
            ProbeOutcome::SignedOut
        }
        _ => ProbeOutcome::Unknown,
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
            ProbeOutcome::SignedIn
        );
        assert_eq!(
            outcome_of(RunState::NeedsLogin, Some(FailureClass::SessionExpired)),
            ProbeOutcome::SignedOut
        );
        assert_eq!(
            outcome_of(RunState::Failed, Some(FailureClass::SessionExpired)),
            ProbeOutcome::SignedOut
        );
        for (state, class) in [
            (RunState::Failed, Some(FailureClass::Policy)),
            (RunState::Failed, Some(FailureClass::Transient)),
            (RunState::Cancelled, None),
            (RunState::Stopped, Some(FailureClass::Busy)),
        ] {
            assert_eq!(outcome_of(state, class), ProbeOutcome::Unknown, "{state:?}");
        }
    }
}
