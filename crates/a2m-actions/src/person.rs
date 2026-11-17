//! Reaching a person, and the approval desk.
//!
//! A role supplies a [`PersonSurface`]: `a2m-local` with a notification, the
//! loopback page, and the operating system's prompt; `a2m-api` with the
//! console pages behind the identity provider's sign-in. The kernel asks
//! through it and never opens a window itself.
//!
//! Recording an approval is not an action. The one handle that can record
//! one comes out of `Policy::open` and is moved into the [`ApprovalDesk`],
//! which [`Kernel::open`](crate::Kernel::open) hands to the person surface
//! and to nothing else. The kernel keeps no copy, so no handler, and so no
//! command, tool, or route, can reach it.

use a2m_auth::{PersonAnswer, PersonAsk, PersonReason};
use a2m_core::{
    ApprovalId, Digest, EffectClass, Environment, Failure, Identity, PrincipalId, RunId, RunState,
    SessionId, TargetName, Timestamp, WorkspaceId,
};
use a2m_ledger::Store;
use a2m_policy::{ApprovalSurface, Approved, Channel, Declined, PresenceCheck, PrincipalRef};

use crate::codes;
use crate::def::{ActionName, BoxFuture};

/// Where a person goes to answer: a link they open themselves, and words to
/// show beside it. Following a link is never an approval by itself, so the
/// link may be given to an agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceLink {
    /// The address to open, when the surface has a page for it.
    pub url: Option<String>,
    /// What the person is told to do.
    pub text: String,
}

/// A run rests at `needs_approval`: tell its person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalAsk {
    /// The run.
    pub run: RunId,
    /// The workspace.
    pub workspace: WorkspaceId,
    /// Who started it.
    pub started_by: PrincipalId,
    /// The tool it runs.
    pub tool: String,
    /// The hash of the diff the person is asked about.
    pub diff_hash: Digest,
    /// What the write would do, in a sentence.
    pub summary: String,
}

/// A session needs its person: tell them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignInAsk {
    /// The session, when one exists already.
    pub session: Option<SessionId>,
    /// The workspace.
    pub workspace: WorkspaceId,
    /// The target.
    pub target: TargetName,
    /// The environment.
    pub environment: Environment,
    /// Whose session it is. Only that person is asked.
    pub identity: Identity,
    /// Why they are wanted.
    pub reason: PersonReason,
}

/// A request from the session keeper to a session's own person: a link to
/// open in their own browser, a code to enter, or a notice. Nothing drives
/// that browser; the surface shows the link or the prompt and waits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionAsk {
    /// The session.
    pub session: SessionId,
    /// The workspace.
    pub workspace: WorkspaceId,
    /// The target.
    pub target: TargetName,
    /// The environment.
    pub environment: Environment,
    /// Whose session it is. Only that person is asked.
    pub identity: Identity,
    /// What is asked.
    pub ask: PersonAsk,
}

/// An act that is a person's: ask for proof that one is present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceAsk {
    /// The person the caller says they are.
    pub principal: PrincipalId,
    /// The action.
    pub action: ActionName,
    /// Its effect.
    pub effect: EffectClass,
    /// What it would do, in a sentence.
    pub summary: String,
}

/// Proof that a person was present, made only by a person surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresenceProof {
    check: PresenceCheck,
    principal: PrincipalId,
    at: Timestamp,
}

impl PresenceProof {
    /// A proof that `principal` passed `check` at `at`. A surface makes one
    /// after its prompt was answered, and at no other time.
    #[must_use]
    pub const fn new(check: PresenceCheck, principal: PrincipalId, at: Timestamp) -> Self {
        Self {
            check,
            principal,
            at,
        }
    }

    /// The check that was passed.
    #[must_use]
    pub const fn check(&self) -> PresenceCheck {
        self.check
    }

    /// Who passed it.
    #[must_use]
    pub const fn principal(&self) -> PrincipalId {
        self.principal
    }

    /// When.
    #[must_use]
    pub const fn at(&self) -> Timestamp {
        self.at
    }
}

/// What a role supplies so that a person can be reached.
///
/// Every method returns a boxed future, so that the trait can be used as a
/// trait object; an implementation writes `Box::pin(async move { ... })`.
pub trait PersonSurface: Send + Sync {
    /// Takes the approval desk. Called once, by `Kernel::open`. The surface
    /// keeps it for its own pages and prompts and gives it to nothing a
    /// caller can reach.
    fn take_desk(&self, desk: ApprovalDesk);

    /// Tells the person that a run waits for their approval, and gives the
    /// link that goes into the run's result.
    fn ask_approval(&self, ask: ApprovalAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>>;

    /// Tells the person that a session needs them, and gives the link that
    /// goes into the result.
    fn ask_sign_in(&self, ask: SignInAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>>;

    /// Asks for proof that a person is present. On one machine the proof
    /// comes only from the operating system's prompt.
    fn confirm_presence(&self, ask: PresenceAsk) -> BoxFuture<'_, Result<PresenceProof, Failure>>;

    /// Carries a request of the session keeper to the session's own person
    /// and waits for the answer.
    fn ask_session(&self, ask: SessionAsk) -> BoxFuture<'_, PersonAnswer>;

    /// The address a person's browser is sent back to after a link, where
    /// the surface listens: a loopback address on one machine, the
    /// console's address on a server. `None` when the surface cannot
    /// receive one.
    fn return_address(&self) -> Option<String> {
        None
    }
}

/// A run that waits for a person's approval, as the desk shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingApproval {
    /// The run.
    pub run: RunId,
    /// Who started it.
    pub started_by: PrincipalId,
    /// The tool it runs.
    pub tool: String,
    /// The hash of the diff a person is asked about.
    pub diff_hash: Digest,
    /// The hash of the tool version the diff was made by.
    pub tool_version: Digest,
}

/// The desk approvals are recorded at. It records an answer through the
/// policy engine and does nothing else. It is not `Clone`, has no public
/// constructor, and is reached by no action.
pub struct ApprovalDesk {
    surface: ApprovalSurface,
    store: Store,
    workspace: WorkspaceId,
}

impl std::fmt::Debug for ApprovalDesk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApprovalDesk").finish_non_exhaustive()
    }
}

impl ApprovalDesk {
    pub(crate) const fn new(surface: ApprovalSurface, store: Store, workspace: WorkspaceId) -> Self {
        Self {
            surface,
            store,
            workspace,
        }
    }

    /// One run that waits at `needs_approval`, with the diff a person is
    /// asked about.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` when there is no such run or it does not wait
    /// for an approval; the store's failures.
    pub async fn waiting(&self, run: RunId) -> Result<WaitingApproval, Failure> {
        let workspace = self.workspace;
        self.store
            .read(move |tx| {
                let row = tx.ledger().run(run)?;
                match (row.state, row.diff_hash) {
                    (RunState::NeedsApproval, Some(diff_hash)) if row.workspace == workspace => {
                        Ok(WaitingApproval {
                            run,
                            started_by: row.principal,
                            tool: row.tool_name.to_string(),
                            diff_hash,
                            tool_version: row.target_version,
                        })
                    }
                    _ => Err(codes::input(
                        codes::NOT_FOUND,
                        "That run does not wait for an approval.",
                        "Call `run.get` to see where the run stands.",
                    )),
                }
            })
            .await
    }

    /// Records a person's approval of the diff they saw. The policy engine
    /// decides first whether this person may approve this run, and records
    /// that decision whether it allows or refuses.
    ///
    /// # Errors
    ///
    /// `policy`/`approval_refused`, `policy`/`approval_stale`,
    /// `input`/`not_found`, and the store's failures, as the policy engine
    /// answers them.
    pub async fn approve(
        &self,
        run: RunId,
        diff_hash: Digest,
        approver: PrincipalRef,
        channel: Channel,
        proof: PresenceProof,
    ) -> Result<ApprovalId, Failure> {
        let waiting = self.waiting(run).await?;
        self.surface
            .approve(Approved {
                run,
                diff_hash,
                tool_version: waiting.tool_version,
                approver,
                channel,
                presence: proof.check(),
            })
            .await
    }

    /// Records a person's refusal, or an approval window that ended in
    /// silence.
    ///
    /// # Errors
    ///
    /// The policy engine's and the store's failures.
    pub async fn decline(
        &self,
        run: RunId,
        by: Option<PrincipalRef>,
        channel: Channel,
    ) -> Result<(), Failure> {
        self.surface.decline(Declined { run, by, channel }).await
    }
}
