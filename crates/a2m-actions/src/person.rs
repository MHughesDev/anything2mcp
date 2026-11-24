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
use a2m_ledger::{AuditEntry, Durability, Fence, RunRow, Store};
use a2m_policy::{ApprovalSurface, Approved, Channel, Declined, PresenceCheck, PrincipalRef};

use crate::acts::{Acts, PersonActAsk, PersonActId};
use crate::codes;
use crate::def::{ActionName, BoxFuture};
use crate::dispatch::Response;
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
    /// The effect class of the operation the write carries out. A prompt
    /// that must refuse an `irreversible` operation reads it here.
    pub effect: a2m_core::EffectClass,
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

/// How long before it is used a proof of presence may have been made, in
/// milliseconds. A prompt is answered and its proof used at once; a proof
/// older than this was kept by someone, and proves nothing about now.
pub const PRESENCE_WINDOW_MS: i64 = 120_000;

/// How far ahead of the kernel's clock a proof's time may be, in
/// milliseconds: the difference between two readings of one clock.
const PRESENCE_AHEAD_MS: i64 = 5_000;

/// Proof that a person was present, made only by a person surface.
///
/// It says which kind of check was passed, who passed it, when, and, where
/// the surface says so, which prompt answered. Whoever uses a proof checks
/// it: the dispatcher against its caller, the approval desk against the
/// approver, both against the kernel's clock.
///
/// Nothing in the type system keeps another crate from making one: the
/// crate that raises the prompt is above this one, so the constructor must
/// be reachable from it. What a proof is worth is decided by which person
/// surface the product crate mounts, and a surface makes a proof only
/// after its own prompt was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresenceProof {
    check: PresenceCheck,
    principal: PrincipalId,
    at: Timestamp,
    prompt: Option<&'static str>,
    act: Option<PersonActId>,
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
            prompt: None,
            act: None,
        }
    }

    /// The same proof, given for one person's act that waits on a page:
    /// the desk carries that act out with it and no other.
    #[must_use]
    pub const fn for_act(mut self, act: PersonActId) -> Self {
        self.act = Some(act);
        self
    }

    /// The person's act the proof was given for, when it was given for one.
    #[must_use]
    pub const fn act(&self) -> Option<PersonActId> {
        self.act
    }

    /// The same proof, naming the prompt that answered, such as the name
    /// of the operating system's own. The name is recorded with an
    /// approval; it is a word of the surface's and is checked as one:
    /// lowercase letters, digits, and underscores, at most 40. A name of
    /// another shape is not kept.
    #[must_use]
    pub const fn naming(mut self, prompt: &'static str) -> Self {
        self.prompt = if is_word(prompt) { Some(prompt) } else { None };
        self
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

    /// The prompt that answered, when the surface named it.
    #[must_use]
    pub const fn prompt(&self) -> Option<&'static str> {
        self.prompt
    }

    /// Checks the proof where it is used: it is `person`'s, and it was made
    /// within [`PRESENCE_WINDOW_MS`] before `now`.
    ///
    /// # Errors
    ///
    /// Why the proof is not taken, as the end of a sentence.
    pub fn checked(&self, person: PrincipalId, now: Timestamp) -> Result<(), &'static str> {
        if self.principal != person {
            return Err("it was given for another person");
        }
        let age = now
            .as_unix_millis()
            .saturating_sub(self.at.as_unix_millis());
        if age > PRESENCE_WINDOW_MS {
            return Err("it is older than a proof of presence may be");
        }
        if age < -PRESENCE_AHEAD_MS {
            return Err("its time is ahead of the engine's clock");
        }
        Ok(())
    }
}

/// Whether `text` is a short word: lowercase letters, digits, underscores.
const fn is_word(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > 40 {
        return false;
    }
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_') {
            return false;
        }
        at += 1;
    }
    true
}

/// What a person surface's presence prompt can do on this machine now, for
/// `doctor.run`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceReport {
    /// The kind of prompt: a word of the surface's, such as the name of the
    /// operating system's own prompt, or `not_available`.
    pub kind: &'static str,
    /// Why it stands as it does, in a word of the surface's; `unknown`
    /// when the surface does not say.
    pub reason: &'static str,
    /// What this means for approvals here, in a sentence.
    pub message: String,
    /// What to do about it; empty when nothing needs doing.
    pub fix: String,
}

impl PresenceReport {
    /// The word for a machine, or a surface, with no presence prompt.
    pub const NOT_AVAILABLE: &'static str = "not_available";

    /// The report of a surface that does not say what its prompt can do.
    #[must_use]
    pub fn unknown() -> Self {
        Self {
            kind: Self::NOT_AVAILABLE,
            reason: "unknown",
            message: "This engine's person surface does not say whether it can ask for proof that a person is present.".to_owned(),
            fix: "Run the engine through `anything2mcp`, whose surface raises the operating system's own prompt.".to_owned(),
        }
    }

    /// Whether a prompt is there that can prove a person present.
    #[must_use]
    pub fn available(&self) -> bool {
        self.kind != Self::NOT_AVAILABLE
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

    /// Shows a person's act that an agent asked for on a page of the
    /// surface, and gives the link that goes into the caller's result. A
    /// press there raises the presence prompt, and only the proof from
    /// that press, given for this act ([`PresenceProof::for_act`]), carries
    /// it out, through [`ApprovalDesk::carry_out`]. A surface with no such
    /// page refuses by name, and the act is not done.
    /// Whether this surface has a page for a person's act. The dispatcher
    /// asks before it pins anything: a surface with none is answered
    /// `person_required` at once, as before decision 0163.
    fn shows_person_acts(&self) -> bool {
        false
    }

    fn ask_person_act(&self, ask: PersonActAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            Err(codes::policy(
                codes::PERSON_REQUIRED,
                format!(
                    "`{}` is a person's act, and this engine's person surface has no page on which a person can carry it out.",
                    ask.action
                ),
                "Ask the person to run it themselves, at a terminal, with no engine running.",
            ))
        })
    }

    /// What this surface's presence prompt can do on this machine now.
    /// `doctor.run` reports it. A surface that does not say reports
    /// [`PresenceReport::unknown`]. Asking raises no prompt.
    fn presence(&self) -> BoxFuture<'_, PresenceReport> {
        Box::pin(async { PresenceReport::unknown() })
    }

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
    /// How many will be written although their binding cannot read what
    /// they hold now (decision 0196): each field of such a record shows
    /// [`NOW_CANNOT_BE_READ`] as its current value.
    pub write: u32,
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

/// One notice a surface carries, as a page shows it (decision 0193): the
/// notice's name and the fixed words for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewNotice {
    /// The notice's name: `SurfaceNotice::as_str`.
    pub code: &'static str,
    /// The fixed words a page shows: `SurfaceNotice::words`.
    pub words: &'static str,
}

/// The notices `surface` carries, as its kind's pack registered them.
pub(crate) fn notices_of(
    set: &a2m_files::CompilerSet,
    surface: &a2m_core::CompiledSurface,
) -> Vec<PreviewNotice> {
    set.surface_notices(surface)
        .into_iter()
        .map(|notice| PreviewNotice {
            code: notice.as_str(),
            words: notice.words(),
        })
        .collect()
}

/// One record of a preview.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PreviewRecord {
    /// The operation that writes it.
    pub operation: String,
    /// The record's key, its parts joined by ` / `.
    pub key: String,
    /// `create`, `update`, `unchanged`, or `write`: a record whose binding
    /// cannot read what it holds now, so that each field's current value is
    /// [`NOW_CANNOT_BE_READ`] (decision 0196).
    pub action: &'static str,
    /// Its fields, in the record's declared order.
    pub fields: Vec<PreviewField>,
    /// What will stand as the evidence that the write landed. `None` when
    /// it is read back, as most writes are. `Some` with the words the page
    /// shows beside the record before a person approves it, for a write
    /// whose only evidence is the system's own acknowledgement: "accepted
    /// by the system, not read back" (decision 0176).
    pub not_read_back: Option<&'static str>,
    /// `Some(BINDING_LATER)` when the binding that writes the record is
    /// chosen only once the record it waits for is written; `bindings` then
    /// names every binding it could take (decision 0185).
    pub binding_later: Option<&'static str>,
    /// The binding the approved diff names for the record, or every binding
    /// it could take, each with the evidence its write would end on. The
    /// engine writes the record through no other.
    pub bindings: Vec<PreviewBinding>,
    /// Every notice of each surface of `bindings`, each once, in order of
    /// the surfaces: what a person must be told of where the write goes,
    /// such as credentials over an unencrypted link (decision 0193).
    pub notices: Vec<PreviewNotice>,
}

/// One binding a record may be written by, as the approval page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewBinding {
    /// Its mode.
    pub mode: String,
    /// What will stand as the evidence that its write landed: `read_back`,
    /// `response`, or `acknowledged`.
    pub evidence: &'static str,
}

/// What the approval page shows beside a record whose write is accepted by
/// the system and never read back.
pub const NOT_READ_BACK: &str = "accepted by the system, not read back";

/// What the approval page shows as the current value of each field of a
/// record whose binding cannot read it: the engine's fixed words.
pub use a2m_engine::NOW_CANNOT_BE_READ;

/// What the approval page shows beside a record whose binding is chosen
/// only when it is written, when one binding it could take is accepted by
/// the system and never read back.
pub const MAY_NOT_BE_READ_BACK: &str =
    "may be accepted by the system and not read back, by a binding chosen when it is written";

/// What the approval page shows beside a record whose binding is chosen
/// only once the record it waits for is written.
pub const BINDING_LATER: &str =
    "written by a binding chosen once the record it waits for is written";

/// One field of one record of a preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewField {
    /// The field's name.
    pub name: String,
    /// The value the record holds now; `None` when the record does not
    /// exist. For a field labelled restricted, the hash of the value. For a
    /// record whose binding cannot read it, the fixed words
    /// [`NOW_CANNOT_BE_READ`], and `now_cannot_be_read` is true.
    pub current: Option<String>,
    /// True when `current` holds the fixed words [`NOW_CANNOT_BE_READ`],
    /// not a value: the record's binding has nothing to read it with, so
    /// what it holds now is not known (decision 0196). A page shows the
    /// words as the product's, never as the system's value.
    pub now_cannot_be_read: bool,
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
    acts: Arc<Acts>,
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
        acts: Arc<Acts>,
    ) -> Self {
        Self {
            surface: Arc::new(surface),
            store,
            workspace,
            signals,
            redact,
            acts,
        }
    }

    /// The person's act that waits under `act`, as its page shows it:
    /// exactly what will be done, every value redacted.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` when nothing waits under `act`: it was carried
    /// out, declined, or its time ended, or a press is carrying it out.
    pub fn person_act(&self, act: PersonActId) -> Result<PersonActAsk, Failure> {
        self.acts.kernel()?.waiting_act(act)
    }

    /// Carries out the person's act that waits under `act`, once, as the
    /// person who proved presence. The proof must have been given for this
    /// act ([`PresenceProof::for_act`]), be the person's the act waits
    /// for, and be of now; the act is then decided again, for the person,
    /// and its handler run with the proof. The response is what the act
    /// came to, redacted: the page shows it, and the same call by the
    /// program that asked gives it back.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` when nothing waits under `act`, or a press
    /// carries it out already; `policy`/`presence_required` for a proof of
    /// another ask, of another person, or not of now. Nothing is done, and
    /// an act that waits goes on waiting.
    pub async fn carry_out(
        &self,
        act: PersonActId,
        proof: PresenceProof,
    ) -> Result<Response, Failure> {
        self.acts.kernel()?.carry_out_act(act, proof, None).await
    }

    /// As [`ApprovalDesk::carry_out`], with what the person typed on the
    /// act's page beside their press: for a sign-in, the credentials the
    /// page asked for, by role (decision 0205). Each value is registered
    /// with the redactor before anything else; only a value for a secret
    /// the page asked for is kept, and it reaches the act's handler and
    /// nothing else. It is never part of the act's input, its result, or
    /// what is kept of it.
    ///
    /// # Errors
    ///
    /// As [`ApprovalDesk::carry_out`].
    pub async fn carry_out_with(
        &self,
        act: PersonActId,
        proof: PresenceProof,
        given: a2m_auth::Returned,
    ) -> Result<Response, Failure> {
        self.acts
            .kernel()?
            .carry_out_act(act, proof, Some(given))
            .await
    }

    /// Records that the person declined the act, or, with `by_person`
    /// false, that its page closed unanswered. Nothing is done for it. An
    /// act that no longer waits is left as it is, so a second press is
    /// harmless.
    pub fn decline_act(&self, act: PersonActId, by_person: bool) {
        if let Ok(kernel) = self.acts.kernel() {
            kernel.decline_act(act, by_person);
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
        // The compiled target the diff was made under, for the notices of the
        // surfaces the records' bindings write to. Read from the store by the
        // diff's own compiled target hash; where it cannot be read, no notice
        // is invented and the page shows the rest.
        let set_and_compiled_target = match self.acts.kernel() {
            Ok(kernel) => {
                let (target, hash) = (diff.target.clone(), diff.compiled_target);
                self.store
                    .read(move |tx| tx.versions().load(&workspace, &target, &hash))
                    .await
                    .ok()
                    .and_then(|encoded| {
                        a2m_core::CompiledTarget::decode(&encoded.bytes, &diff.compiled_target).ok()
                    })
                    .map(|compiled_target| (kernel, compiled_target))
            }
            Err(_) => None,
        };
        let notices_for = |record: &a2m_engine::DiffRecord| -> Vec<PreviewNotice> {
            let Some((kernel, compiled_target)) = set_and_compiled_target.as_ref() else {
                return Vec::new();
            };
            let set = kernel.providers().compiler_set();
            let Some(operation) = compiled_target
                .operations
                .iter()
                .find(|operation| operation.name == record.operation)
            else {
                return Vec::new();
            };
            let mut surfaces: Vec<usize> = record
                .approved_bindings()
                .into_iter()
                .filter_map(|position| operation.bindings.get(position))
                .map(|binding| binding.surface.get())
                .collect();
            surfaces.sort_unstable();
            surfaces.dedup();
            let mut found: Vec<PreviewNotice> = Vec::new();
            for surface in surfaces
                .into_iter()
                .filter_map(|index| compiled_target.surfaces.get(index))
            {
                for notice in notices_of(set, surface) {
                    if !found.contains(&notice) {
                        found.push(notice);
                    }
                }
            }
            found
        };
        let shown = |value: &a2m_core::Value| {
            let text = self.redact.redact(&render::line(value));
            // A value that was masked, here or when the engine stored it,
            // is shown as masked and as nothing else: whatever stands
            // beside the mark is no part of the value a person is asked
            // about, and is never shown as if it were.
            if text.contains(a2m_core::REDACTED) {
                return (a2m_core::REDACTED.to_owned(), false);
            }
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
                        let unread = record.now_cannot_be_read();
                        let current = if unread {
                            Some((NOW_CANNOT_BE_READ.to_owned(), false))
                        } else {
                            field.current.as_ref().map(shown)
                        };
                        let (proposed, cut) = shown(&field.proposed);
                        PreviewField {
                            name: self.redact.redact(&field.name),
                            cut: cut || current.as_ref().is_some_and(|(_, cut)| *cut),
                            now_cannot_be_read: unread,
                            current: current.map(|(text, _)| text),
                            proposed,
                            changes: field.changes,
                        }
                    })
                    .collect(),
                not_read_back: match (record.not_read_back(), record.binding_unknown()) {
                    (false, _) => None,
                    (true, false) => Some(NOT_READ_BACK),
                    (true, true)
                        if record.could_take.iter().all(|binding| {
                            binding.evidence == a2m_core::WriteEvidence::Acknowledged
                        }) =>
                    {
                        Some(NOT_READ_BACK)
                    }
                    (true, true) => Some(MAY_NOT_BE_READ_BACK),
                },
                binding_later: record.binding_unknown().then_some(BINDING_LATER),
                notices: notices_for(record),
                bindings: record
                    .binding
                    .iter()
                    .chain(&record.could_take)
                    .map(|binding| PreviewBinding {
                        mode: self.redact.redact(binding.mode.as_str()),
                        evidence: binding.evidence.as_str(),
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
            write: counts.write,
            total: diff.records.len(),
            offset,
            records,
        })
    }

    /// Records a person's approval of the diff they saw.
    ///
    /// The proof of presence is checked first, by the store's clock, which
    /// is the kernel's: it must be the approver's own and of now. Then the
    /// policy engine decides whether this person may approve this run, and
    /// records that decision whether it allows or refuses. What the proof
    /// says, its kind of check, whose it is, when it was made, and the
    /// prompt that answered, is written to the audit log in the same
    /// transaction as the approval, and so is a refusal.
    ///
    /// # Errors
    ///
    /// `policy`/`approval_refused` when the proof is another person's or
    /// not of now; `policy`/`approval_refused`, `policy`/`approval_stale`,
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
        let surface = Arc::clone(&self.surface);
        let workspace = self.workspace;
        let approved = Approved {
            run,
            diff_hash,
            tool_version: waiting.tool_version,
            approver,
            channel,
            presence: proof.check(),
            proved_at: proof.at(),
            prompt: proof.prompt().map(str::to_owned),
        };
        let approval = self
            .store
            .write(Durability::Synced, move |tx| {
                let entry = |action: &str, detail: String| AuditEntry {
                    workspace,
                    actor: approved.approver.id.to_string(),
                    action: action.to_owned(),
                    subject: run.to_string(),
                    detail,
                };
                let shown = format!(
                    "presence={} of={} at={} prompt={} channel={}",
                    proof.check(),
                    proof.principal(),
                    proof.at().as_unix_millis(),
                    proof.prompt().unwrap_or("unnamed"),
                    approved.channel,
                );
                if let Err(why) = proof.checked(approved.approver.id, tx.now()) {
                    tx.audit()
                        .append(&entry("approval.refused", format!("{shown} refused={why}")))?;
                    return Ok(Err(codes::policy(
                        a2m_policy::APPROVAL_REFUSED,
                        format!("The approval was not recorded: the proof of presence was not taken, because {why}."),
                        "Open the approval link again and answer the prompt yourself.",
                    )));
                }
                // The decision about the approver commits whether the
                // approval is recorded or refused.
                match surface.record_approval(tx, &approved) {
                    Ok(id) => {
                        tx.audit()
                            .append(&entry("approval.recorded", format!("{shown} approval={id}")))?;
                        Ok(Ok(id))
                    }
                    Err(refused) => {
                        tx.audit().append(&entry(
                            "approval.refused",
                            format!("{shown} refused={}", refused.code()),
                        ))?;
                        Ok(Err(refused))
                    }
                }
            })
            .await??;
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
