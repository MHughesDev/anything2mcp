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
use std::sync::Arc;

use a2m_core::{
    ApprovalId, Digest, EffectClass, Environment, Failure, Identity, PrincipalId, Redact, RunId,
    RunState, SessionId, TargetName, Timestamp, WorkspaceId,
};
use a2m_engine::Diff;
use a2m_ledger::{Durability, Fence, RunRow, Store};
use a2m_policy::{ApprovalSurface, Approved, Channel, Declined, PresenceCheck, PrincipalRef};

use crate::codes;
use crate::def::{ActionName, BoxFuture};
use crate::render;
use crate::signals::RunSignals;

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

/// The most records one call of [`ApprovalDesk::preview`] gives.
pub const PREVIEW_RECORDS: usize = 100;
/// The most characters of one value a preview shows. A longer value is cut
/// and marked.
pub const PREVIEW_VALUE_CHARS: usize = 400;

/// The change a waiting run would make, as an approval page shows it: the
/// content of the diff whose hash the desk reports for the run, and of no
/// other. Every text in it passed the redactor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalPreview {
    /// The run.
    pub run: RunId,
    /// The hash of the diff shown. An approval is recorded for this hash.
    pub diff_hash: Digest,
    /// The tool.
    pub tool: String,
    /// The target the write goes to.
    pub target: String,
    /// The environment.
    pub environment: String,
    /// How many records the write would create.
    pub create: u32,
    /// How many it would change.
    pub update: u32,
    /// How many already hold the intended values.
    pub unchanged: u32,
    /// How many records the diff holds in all.
    pub total: usize,
    /// How many records were skipped before the first one shown.
    pub offset: usize,
    /// The records shown, in the order the run would write them.
    pub records: Vec<PreviewRecord>,
}

impl ApprovalPreview {
    /// Whether every record of the diff from `offset` on is shown. When
    /// not, ask again with `offset` moved on by the number shown.
    #[must_use]
    pub const fn complete(&self) -> bool {
        self.offset + self.records.len() >= self.total
    }
}

/// One record of a preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRecord {
    /// The operation that writes it.
    pub operation: String,
    /// The record's key, its parts joined by ` / `.
    pub key: String,
    /// `create`, `update`, or `unchanged`.
    pub action: &'static str,
    /// Its fields, in the record's declared order.
    pub fields: Vec<PreviewField>,
}

/// One field of one record of a preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewField {
    /// The field's name.
    pub name: String,
    /// The value the record holds now; `None` when the record does not
    /// exist. For a field labelled restricted, the hash of the value.
    pub current: Option<String>,
    /// The value the write would leave, or its hash when restricted.
    pub proposed: String,
    /// Whether the write would change the field.
    pub changes: bool,
    /// Whether a value was longer than [`PREVIEW_VALUE_CHARS`] and was cut.
    pub cut: bool,
}

/// The desk approvals are recorded at. It records an answer through the
/// policy engine, ends a declined run, shows what a waiting run would
/// change, and does nothing else. It is not `Clone`, has no public
/// constructor, and is reached by no action.
pub struct ApprovalDesk {
    surface: Arc<ApprovalSurface>,
    store: Store,
    workspace: WorkspaceId,
    signals: Arc<RunSignals>,
    redact: Arc<dyn Redact>,
}

impl std::fmt::Debug for ApprovalDesk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApprovalDesk").finish_non_exhaustive()
    }
}

fn not_waiting() -> Failure {
    codes::input(
        codes::NOT_FOUND,
        "That run does not wait for an approval.",
        "Call `run.get` to see where the run stands.",
    )
}

/// The run and the hash of its diff, when it waits for an approval in
/// `workspace`.
fn waiting_row(row: RunRow, workspace: WorkspaceId) -> Result<(RunRow, Digest), Failure> {
    match (row.state, row.diff_hash) {
        (RunState::NeedsApproval, Some(diff_hash)) if row.workspace == workspace => {
            Ok((row, diff_hash))
        }
        _ => Err(not_waiting()),
    }
}

impl ApprovalDesk {
    pub(crate) fn new(
        surface: ApprovalSurface,
        store: Store,
        workspace: WorkspaceId,
        signals: Arc<RunSignals>,
        redact: Arc<dyn Redact>,
    ) -> Self {
        Self {
            surface: Arc::new(surface),
            store,
            workspace,
            signals,
            redact,
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
                let (row, diff_hash) = waiting_row(tx.ledger().run(run)?, workspace)?;
                Ok(WaitingApproval {
                    run,
                    started_by: row.principal,
                    tool: row.tool_name.to_string(),
                    diff_hash,
                    tool_version: row.target_version,
                })
            })
            .await
    }

    /// What the waiting run would change: which records, which fields, from
    /// what to what. At most [`PREVIEW_RECORDS`] records from `offset` on,
    /// each value cut to [`PREVIEW_VALUE_CHARS`] characters, every text
    /// redacted. The page shows this, and an approval is then recorded for
    /// [`ApprovalPreview::diff_hash`].
    ///
    /// The stored diff is read back and its hash computed again: a diff
    /// that is not the one the run waits on is never shown.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` when there is no such run or it does not wait
    /// for an approval; `policy`/`approval_stale` when the stored diff is
    /// not the one whose hash the run carries; the store's failures.
    pub async fn preview(&self, run: RunId, offset: usize) -> Result<ApprovalPreview, Failure> {
        let workspace = self.workspace;
        let (row, diff_hash) = self
            .store
            .read(move |tx| waiting_row(tx.ledger().run(run)?, workspace))
            .await?;
        let stale = || {
            codes::policy(
                a2m_policy::APPROVAL_STALE,
                "The stored diff is not the one the run waits on, so it is not shown.",
                "Call the tool again to build a new diff.",
            )
        };
        let blob = row.diff.ok_or_else(stale)?;
        let opened = self.store.blobs().get(&blob).await?;
        let diff = Diff::decode(opened.expose())?;
        if diff.hash() != diff_hash {
            return Err(stale());
        }
        let counts = diff.counts();
        let shown = |value: &a2m_core::Value| {
            let text = self.redact.redact(&render::line(value));
            let cut = text.chars().count() > PREVIEW_VALUE_CHARS;
            let text = if cut {
                text.chars().take(PREVIEW_VALUE_CHARS).collect()
            } else {
                text
            };
            (text, cut)
        };
        let records = diff
            .records
            .iter()
            .skip(offset)
            .take(PREVIEW_RECORDS)
            .map(|record| PreviewRecord {
                operation: record.operation.to_string(),
                key: record
                    .key
                    .parts()
                    .iter()
                    .map(|part| shown(part).0)
                    .collect::<Vec<_>>()
                    .join(" / "),
                action: record.action.as_str(),
                fields: record
                    .fields
                    .iter()
                    .map(|field| {
                        let current = field.current.as_ref().map(shown);
                        let (proposed, cut) = shown(&field.proposed);
                        PreviewField {
                            name: self.redact.redact(&field.name),
                            cut: cut || current.as_ref().is_some_and(|(_, cut)| *cut),
                            current: current.map(|(text, _)| text),
                            proposed,
                            changes: field.changes,
                        }
                    })
                    .collect(),
            })
            .collect();
        Ok(ApprovalPreview {
            run,
            diff_hash,
            tool: diff.tool.to_string(),
            target: diff.target.to_string(),
            environment: diff.environment.to_string(),
            create: counts.create,
            update: counts.update,
            unchanged: counts.unchanged,
            total: diff.records.len(),
            offset,
            records,
        })
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
        let approval = self
            .surface
            .approve(Approved {
                run,
                diff_hash,
                tool_version: waiting.tool_version,
                approver,
                channel,
                presence: proof.check(),
            })
            .await?;
        // A reader of the run is told that it is approved and waits only
        // to be resumed. This decides nothing: the approval itself is in
        // the store.
        self.signals.note_approved(run, diff_hash);
        self.signals.changed();
        Ok(approval)
    }

    /// Records a person's refusal, or an approval window that ended in
    /// silence (`by` is then `None`), and ends the run as `expired` in the
    /// same transaction (spec section 212). The run's failure is `policy`,
    /// `approval_refused`, which `run.get` reports from then on. An unused
    /// approval of the run is revoked.
    ///
    /// A run that has already ended is left as it is and nothing is
    /// recorded, so a second press, or a window that ends after a press,
    /// is harmless.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` when there is no such run in this workspace or
    /// it is being worked on and no longer waits; the policy engine's and
    /// the store's failures.
    pub async fn decline(
        &self,
        run: RunId,
        by: Option<PrincipalRef>,
        channel: Channel,
    ) -> Result<(), Failure> {
        let workspace = self.workspace;
        let surface = Arc::clone(&self.surface);
        let why = codes::policy(
            a2m_policy::APPROVAL_REFUSED,
            if by.is_some() {
                "A person declined the write. Nothing was written."
            } else {
                "The approval window ended with no answer, which is a decline. Nothing was written."
            },
            "Call the tool again if the write is still wanted; a person must approve the new diff.",
        );
        let declined = Declined { run, by, channel };
        let ended = self
            .store
            .write(Durability::Synced, move |tx| {
                let row = tx.ledger().run(run)?;
                if row.workspace != workspace {
                    return Err(not_waiting());
                }
                if row.state.is_final() {
                    return Ok(false);
                }
                if row.state != RunState::NeedsApproval {
                    return Err(not_waiting());
                }
                surface.record_decline(tx, &declined)?;
                // Nobody writes under the run's lease number while it
                // waits; a job that holds it finds the run ended at its
                // next call and stops.
                let fence = Fence {
                    run,
                    epoch: row.epoch,
                };
                tx.ledger()
                    .set_state(&fence, RunState::Expired, Some(&why))?;
                Ok(true)
            })
            .await?;
        if ended {
            self.signals.forget(run);
            self.signals.changed();
        }
        Ok(())
    }
}
