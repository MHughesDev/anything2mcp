//! The driver contract: what a pack implements to carry out the steps of
//! its mode, and the handle through which alone the engine reaches it.
//!
//! Every async method of every trait in this crate returns a
//! [`BoxFuture`](crate::BoxFuture), so that each trait can be used as a
//! trait object. An implementation writes `Box::pin(async move { ... })`.

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use a2m_auth::SessionLease;
use a2m_core::{
    Archetype, CompiledSurface, CompiledTarget, Digest, Environment, Epoch, Failure, FailureClass,
    FailureCode, Mode, RecordKey, Redact, RunId, ScopedHost, Step, StepKind, TargetName, Value,
    Values, Version,
};
use a2m_ledger::{IntentToken, RecordRef};
use a2m_telemetry::Acting;

use crate::BoxFuture;
use crate::codes;
use crate::name::ProviderName;
use crate::path::CheckedPaths;
use crate::place::{MachineState, NetworkPlace, Placed};
use crate::supervisor::{Cleanup, ScopedSupervisor};

/// A moment by which something must be done. Nothing waits without one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Deadline(tokio::time::Instant);

impl Deadline {
    /// The moment `after` from now.
    #[must_use]
    pub fn after(after: Duration) -> Self {
        Self(tokio::time::Instant::now() + after)
    }

    /// The moment itself.
    #[must_use]
    pub fn at(self) -> tokio::time::Instant {
        self.0
    }

    /// How long is left; zero when it has passed.
    #[must_use]
    pub fn remaining(self) -> Duration {
        self.0
            .saturating_duration_since(tokio::time::Instant::now())
    }

    /// Whether it has passed.
    #[must_use]
    pub fn passed(self) -> bool {
        self.remaining().is_zero()
    }
}

/// The surface a driver acts on: the compiled target's surface, with the
/// address of the run's environment already applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceRef {
    /// The target.
    pub target: TargetName,
    /// The environment of the run.
    pub environment: Environment,
    /// The surface, as the compiled target holds it, at this environment's
    /// address.
    pub surface: CompiledSurface,
}

/// What a run may do to the system. Outside `Live` a driver applies its own
/// kind's read-only guard as well; the handle refuses every committing step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Posture {
    /// A real run.
    Live,
    /// The engine's sandbox. Writes are possible only to declared test
    /// records.
    Sandbox {
        /// Whether declared test records exist that may be written.
        test_records: bool,
    },
    /// Discovery: nothing is written.
    Discovery,
}

/// The addresses a run may reach: the target's declared surfaces, and what
/// its operation's permission manifest adds. A driver sends nothing to an
/// address that is not here.
///
/// Each address carries its scope, read from the address itself when the
/// set is made ([`ScopedHost::of_destination`], decision 0240). Nothing else
/// sets it: there is no other constructor and no setter, so no input can
/// say where a destination is. The scope decides nothing yet; the
/// allow-list compares addresses as written, as before.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Destinations {
    allowed: BTreeMap<String, ScopedHost>,
}

impl Destinations {
    /// The addresses given, each with its scope read from it.
    #[must_use]
    pub fn new(addresses: impl IntoIterator<Item = String>) -> Self {
        Self {
            allowed: addresses
                .into_iter()
                .map(|address| {
                    let scope = ScopedHost::of_destination(&address);
                    (address, scope)
                })
                .collect(),
        }
    }

    /// Whether `address` is one of them, compared as written.
    #[must_use]
    pub fn contains(&self, address: &str) -> bool {
        self.allowed.contains_key(address)
    }

    /// Every address, in order.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.allowed.keys().map(String::as_str)
    }

    /// The scope of `address`, read from it when the set was made; nothing
    /// when it is not one of them.
    #[must_use]
    pub fn scope_of(&self, address: &str) -> Option<&ScopedHost> {
        self.allowed.get(address)
    }

    /// The network place of `address` given what the machine says of
    /// itself: derived from the scope read when the set was made and from
    /// `state`, never set by anyone (decision 0223). Nothing when it is not
    /// one of the destinations. The allow-list itself still compares
    /// addresses as written.
    #[must_use]
    pub fn place_of(&self, address: &str, state: &MachineState) -> Option<Placed> {
        self.scope_of(address)
            .map(|host| NetworkPlace::of(host, state))
    }
}

/// What `prepare` is given.
pub struct PrepareContext<'a> {
    /// The run.
    pub run: RunId,
    /// The run's lease number. A token stored under another is refused.
    pub epoch: Epoch,
    /// The surface.
    pub surface: &'a SurfaceRef,
    /// The published compiled target, as data. A step's `refers_to` names an
    /// entry of one of its pack sections.
    pub compiled_target: &'a CompiledTarget,
    /// What the run may do.
    pub posture: Posture,
    /// When preparing must be done.
    pub deadline: Deadline,
    /// The redactor. Every capture passes it before it leaves `act`.
    pub redact: &'a dyn Redact,
    /// The addresses the run may reach.
    pub destinations: &'a Destinations,
    /// Every file the run may touch, already checked.
    pub paths: &'a CheckedPaths,
    /// Starts only what the manifest and the published compiled target
    /// declare.
    pub supervisor: &'a ScopedSupervisor,
}

impl fmt::Debug for PrepareContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrepareContext")
            .field("run", &self.run)
            .field("surface", &self.surface.surface.name)
            .field("posture", &self.posture)
            .finish_non_exhaustive()
    }
}

/// What `prepare` acquired: the driver's own state, opaque to the kernel.
pub struct Prepared {
    state: Box<dyn Any + Send>,
    guard: Option<Guard>,
    /// What the record's earlier steps recorded, for the step being acted.
    recorded: Values,
    /// Who answers whether another record holds a value, and the record
    /// of the step being acted, for that step alone.
    others: Option<(Arc<dyn OtherRecords>, Option<RecordRef>)>,
}

/// The engine's answer to a driver that asks whether another record holds
/// a value it recorded ([`Prepared::recorded_elsewhere`]). It names the
/// record and never gives back a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordedElsewhere {
    /// Another record of the operation holds the value under the name: the
    /// run and the record. Never the record that asks.
    Held(RecordRef),
    /// No other record of the operation, in any run of the workspace, holds
    /// it, as the ledger's stored values say.
    NotHeld,
    /// The engine cannot tell: it was not asked through a handle that
    /// answers, the question was malformed, its time ran out, or a value it
    /// would compare was masked. A driver treats this as "may be held".
    CannotTell,
}

/// What answers [`Prepared::recorded_elsewhere`]: the engine, for one step
/// of one record, from its `Control` (the action registry, from the
/// ledger's stored values). It is bound to the workspace, the operation,
/// and the record that asks, so the driver names only the value.
pub trait OtherRecords: Send + Sync {
    /// Whether any other record of the asking record's operation, in any
    /// run of the workspace, holds a recorded value under `name` whose
    /// [`recorded_digest`] is `value`. Compared by digest; the values are
    /// never handed out.
    fn holding<'a>(&'a self, name: &'a str, value: Digest) -> BoxFuture<'a, RecordedElsewhere>;
}

/// The digest a recorded value is compared by when a driver asks whether
/// another record holds it: of the value's canonical bytes. The answerer
/// digests each stored value the same way.
#[must_use]
pub fn recorded_digest(value: &Value) -> Digest {
    Digest::of(&value.canonical_bytes())
}

/// The longest name a driver may ask about.
const LONGEST_RECORDED_NAME: usize = 64;

/// What the handle remembers of the context a `Prepared` was made in.
#[derive(Debug, Clone, Copy)]
struct Guard {
    run: RunId,
    epoch: Epoch,
    posture: Posture,
}

impl Prepared {
    /// Wraps the driver's state.
    #[must_use]
    pub fn new<T: Any + Send>(state: T) -> Self {
        Self {
            state: Box::new(state),
            guard: None,
            recorded: Values::new(),
            others: None,
        }
    }

    /// Asks the engine, within a step, whether any other record of this
    /// operation, in any run of this workspace, holds `value` under `name`
    /// among the values it recorded before its save: a message a take
    /// already holds, a sequence another record recorded. Bounded: one
    /// name of lowercase letters, digits, and underscores, at most 64
    /// characters; one plain value, compared by its [`recorded_digest`];
    /// an answer by `deadline`. The answer is [`RecordedElsewhere::CannotTell`]
    /// outside `act`, through a handle the engine called without an
    /// answerer ([`DriverHandle::act`], [`DriverHandle::act_recorded`]), for
    /// a malformed name, a null, a list, a record, or a masked value, and
    /// when the time runs out. A driver never takes it for "not held".
    ///
    /// The future holds nothing of the `Prepared`, so a driver may await it
    /// while it holds its own state.
    pub fn recorded_elsewhere(
        &self,
        name: &str,
        value: &Value,
        deadline: Deadline,
    ) -> BoxFuture<'static, RecordedElsewhere> {
        let named = !name.is_empty()
            && name.len() <= LONGEST_RECORDED_NAME
            && name.starts_with(|c: char| c.is_ascii_lowercase())
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        let plain = match value {
            Value::Null | Value::List(_) | Value::Record(_) => false,
            Value::Text(text) => !text.contains(a2m_core::REDACTED),
            _ => true,
        };
        let asked = match &self.others {
            Some((others, asking)) if named && plain => Some((
                Arc::clone(others),
                *asking,
                name.to_owned(),
                recorded_digest(value),
            )),
            _ => None,
        };
        Box::pin(async move {
            let Some((others, asking, name, digest)) = asked else {
                return RecordedElsewhere::CannotTell;
            };
            match tokio::time::timeout_at(deadline.at(), others.holding(&name, digest)).await {
                // The asking record is never "another" record.
                Ok(RecordedElsewhere::Held(record)) if Some(record) == asking => {
                    RecordedElsewhere::CannotTell
                }
                Ok(answer) => answer,
                Err(_) => RecordedElsewhere::CannotTell,
            }
        })
    }

    /// What the earlier steps of the record this step belongs to returned
    /// in [`Observed::for_reconcile`], by name: within a pass, and on a
    /// resume what the engine stored before the record's commit point. A
    /// read-back, a lookup, or a reconcile read uses it to find the record
    /// by a message id, a log position, a file's digest, or a frame built
    /// before the save. Empty for a step of no record, and for a record
    /// whose steps recorded nothing.
    ///
    /// The handle sets it for each step, just before the driver's `act` is
    /// called, and empties it after; outside `act` it is empty. A driver
    /// that holds its state borrowed reads this first.
    #[must_use]
    pub fn recorded(&self) -> &Values {
        &self.recorded
    }

    /// The driver's state, when it is a `T`.
    #[must_use]
    pub fn state<T: Any>(&mut self) -> Option<&mut T> {
        self.state.downcast_mut::<T>()
    }

    /// Takes the driver's state back, for `release`.
    ///
    /// # Errors
    ///
    /// The `Prepared` itself, when its state is not a `T`.
    pub fn into_state<T: Any>(self) -> Result<Box<T>, Self> {
        let guard = self.guard;
        self.state.downcast::<T>().map_err(|state| Self {
            state,
            guard,
            recorded: Values::new(),
            others: None,
        })
    }
}

impl fmt::Debug for Prepared {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Prepared").finish_non_exhaustive()
    }
}

/// What one step is given. The values are already evaluated.
#[derive(Debug, Clone, Copy)]
pub struct StepInput<'a> {
    /// The step's arguments, by name.
    pub args: &'a Values,
    /// The commit point the step belongs to, when it belongs to one.
    pub record: Option<RecordRef>,
    /// The record's key in the system: from its key inputs, or the key the
    /// system assigned. `None` while the record has none; never the
    /// engine's own name for the record in a run (decision 0161).
    pub key: Option<&'a RecordKey>,
    /// The record's idempotency token: sent where the system takes an
    /// idempotency key, and the stamp for `stamped` identity.
    pub stamp: Option<&'a str>,
    /// When the step must be done.
    pub deadline: Deadline,
}

impl<'a> StepInput<'a> {
    /// The input of a step of no record, with these arguments, to be done
    /// by `deadline`. A caller that makes a `StepInput` makes it here, so
    /// that a field the contract adds later does not break it.
    #[must_use]
    pub const fn new(args: &'a Values, deadline: Deadline) -> Self {
        Self {
            args,
            record: None,
            key: None,
            stamp: None,
            deadline,
        }
    }

    /// The same input, for a step of this record, with its key in the
    /// system and its stamp, when known.
    #[must_use]
    pub const fn for_record(
        mut self,
        record: RecordRef,
        key: Option<&'a RecordKey>,
        stamp: Option<&'a str>,
    ) -> Self {
        self.record = Some(record);
        self.key = key;
        self.stamp = stamp;
        self
    }
}

/// Something a step kept as evidence: a response, a screenshot, a file. It
/// has passed the redactor before it left `act`.
///
/// One name is the contract's own: a capture named [`Capture::ACTING`], of
/// the media type `application/json`, is how the step was done, an
/// [`Acting`], which the engine puts in the step's trace record and does not
/// keep as evidence. Make it with [`Capture::acting`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    /// A short name, such as `response`.
    pub name: String,
    /// Its media type.
    pub media_type: String,
    /// Its bytes, already redacted.
    pub bytes: Vec<u8>,
}

impl Capture {
    /// The name of the capture that says how a step was done.
    pub const ACTING: &'static str = "acting";

    /// The capture that says how the step was done: `means`, which the
    /// trace's signature covers, so two runs that did the same thing give
    /// the same signature; and `measured`, which it does not. Each detail's
    /// name is the pack's own fixed word.
    #[must_use]
    pub fn acting(acting: &Acting) -> Self {
        Self {
            name: Self::ACTING.to_owned(),
            media_type: "application/json".to_owned(),
            // Two lists of names and texts always encode.
            bytes: serde_json::to_vec(acting).unwrap_or_default(),
        }
    }

    /// Whether this is the capture that says how the step was done.
    #[must_use]
    pub fn is_acting(&self) -> bool {
        self.name == Self::ACTING && self.media_type == "application/json"
    }
}

/// What a step observed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Observed {
    /// The values it read, by name.
    pub values: Values,
    /// The key the system assigned, when the step learned one.
    pub assigned_key: Option<RecordKey>,
    /// What reconcile will need, learned before the commit point. The engine
    /// records it durably before the committing step is sent. The handle
    /// refuses it on the committing step itself.
    pub for_reconcile: Values,
    /// Evidence, already redacted.
    pub captures: Vec<Capture>,
}

impl Observed {
    /// How the step was done, from its [`Capture::ACTING`] capture; `None`
    /// when it has none, or when that capture is not an [`Acting`] once the
    /// redactor has passed over it.
    #[must_use]
    pub fn acting(&self) -> Option<Acting> {
        self.captures
            .iter()
            .find(|capture| capture.is_acting())
            .and_then(|capture| serde_json::from_slice(&capture.bytes).ok())
    }
}

/// How a step ended when it did not fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// It was done.
    Done(Observed),
    /// The record it looked for does not exist. Allowed only for a step the
    /// compiled target marks `lookup`.
    Absent(Observed),
}

/// Whether a request left, and whether its fate is known. This decides
/// whether a write may have happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sent {
    /// Nothing was sent to the system.
    NotSent,
    /// The system replied, so the outcome is known.
    Answered,
    /// It may have been sent, and nothing says what became of it.
    Unknown,
}

/// The failure of a step: what went wrong, and whether anything was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepFailure {
    /// What went wrong.
    pub failure: Failure,
    /// Whether the request left, and whether its fate is known.
    pub sent: Sent,
}

impl StepFailure {
    /// A failure before anything was sent.
    #[must_use]
    pub fn not_sent(failure: Failure) -> Self {
        Self {
            failure,
            sent: Sent::NotSent,
        }
    }

    /// A failure the system itself answered with.
    #[must_use]
    pub fn answered(failure: Failure) -> Self {
        Self {
            failure,
            sent: Sent::Answered,
        }
    }

    /// A failure after which the request's fate is not known.
    #[must_use]
    pub fn unknown(failure: Failure) -> Self {
        Self {
            failure,
            sent: Sent::Unknown,
        }
    }

    /// The same failure with what the system itself said: its validation
    /// message, the name of the constraint it broke. Bounded, redacted by
    /// the handle, and carried to the run's failure as untrusted data, kept
    /// apart from the sentence (see [`Failure::with_said`]).
    #[must_use]
    pub fn with_said(mut self, said: impl AsRef<str>) -> Self {
        self.failure = self.failure.with_said(said);
        self
    }
}

impl fmt::Display for StepFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.failure.fmt(f)
    }
}

impl std::error::Error for StepFailure {}

/// Whether a driver may be chosen for a surface now, from memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverHealth {
    /// It may.
    Ready,
    /// It works, with the problem named.
    Degraded(FailureCode),
    /// It may not, with the problem named.
    Unavailable(FailureCode),
}

/// What a pack implements for each mode it drives. A public contract.
///
/// What an implementation must hold to:
///
/// - `act` carries out exactly one step, as written, and never retries. A
///   step it cannot do is a failure, never a guess.
/// - Every failure says whether anything was sent. When in doubt it is
///   [`Sent::Unknown`].
/// - It must be safe to drop the future of `prepare`, and of `act` for any
///   step not marked `commits`. The engine never drops the future of a
///   committing step.
/// - Credentials reach it only through the lease's `apply`. No secret
///   appears in a value, a capture, or a failure.
/// - It resolves no path, starts no process except through the context's
///   supervisor, and reaches no address outside the context's destinations.
pub trait Driver: Send + Sync {
    /// Acquires what the binding needs: a tab, a client, a window, an open
    /// file. `lease` is `None` for a surface with no session.
    fn prepare<'a>(
        &'a self,
        cx: &'a PrepareContext<'a>,
        lease: Option<&'a SessionLease>,
    ) -> BoxFuture<'a, Result<Prepared, StepFailure>>;

    /// Carries out one step of a kind this mode declared.
    fn act<'a>(
        &'a self,
        prepared: &'a mut Prepared,
        step: &'a Step,
        input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>>;

    /// From memory, without input or output: may this driver be chosen for
    /// this surface now.
    fn health(&self, surface: &SurfaceRef) -> DriverHealth;

    /// Gives back what `prepare` acquired. `poisoned` after a failure:
    /// recycle, never reuse.
    fn release<'a>(&'a self, prepared: Prepared, poisoned: bool) -> BoxFuture<'a, Cleanup>;
}

/// Why a provider drives a mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChoiceReason {
    /// It is the only ready provider that drives the mode.
    OnlyProvider,
    /// A setting names it for the mode.
    NamedBySetting,
}

impl ChoiceReason {
    /// The reason as a trace writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OnlyProvider => "only_provider",
            Self::NamedBySetting => "named_by_setting",
        }
    }
}

/// Which provider drives a mode, and why. Recorded in the trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderChoice {
    /// The provider.
    pub provider: ProviderName,
    /// Its version.
    pub version: Version,
    /// The digest of its manifest.
    pub digest: Digest,
    /// Why it was chosen.
    pub reason: ChoiceReason,
}

/// What a step that commits must carry.
#[derive(Debug, Clone, Copy)]
pub enum Authority<'a> {
    /// The right to read. A step marked `commits` is refused.
    Read,
    /// The right to send one write: the token the ledger issued for this
    /// record's stored intent.
    Write(&'a IntentToken),
}

/// What the handle knows of the mode and its provider, from the manifest.
pub(crate) struct HandleFacts {
    pub mode: Mode,
    pub archetype: Archetype,
    pub steps: BTreeSet<StepKind>,
    /// The kinds of step that always save.
    pub commit_steps: BTreeSet<StepKind>,
    /// The kinds of step whose "absent" is authoritative.
    pub absence_steps: BTreeSet<StepKind>,
    /// The longest one `prepare` may take, as the manifest states it.
    pub prepare_within: Option<Duration>,
    pub codes: BTreeMap<FailureCode, FailureClass>,
    pub choice: ProviderChoice,
    pub supervisor: ScopedSupervisor,
}

/// The only way the engine reaches a driver. It cannot be made outside this
/// crate: [`Registry::driver`](crate::Registry::driver) gives one.
///
/// It is the enforcement point. Before the pack is called it checks that the
/// step's kind is one the mode declared, and that a step marked `commits`
/// carries the intent token of this run, this lease number, and this record,
/// in a posture that may write. After the pack returns it checks what came
/// back.
#[derive(Clone)]
pub struct DriverHandle {
    driver: Arc<dyn Driver>,
    facts: Arc<HandleFacts>,
    redact: Arc<dyn Redact>,
}

impl fmt::Debug for DriverHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DriverHandle")
            .field("mode", &self.facts.mode)
            .field("provider", &self.facts.choice.provider)
            .finish_non_exhaustive()
    }
}

fn refused(failure: Failure) -> StepFailure {
    StepFailure::not_sent(failure)
}

impl DriverHandle {
    pub(crate) fn new(
        driver: Arc<dyn Driver>,
        facts: Arc<HandleFacts>,
        redact: Arc<dyn Redact>,
    ) -> Self {
        Self {
            driver,
            facts,
            redact,
        }
    }

    /// The mode it drives.
    #[must_use]
    pub fn mode(&self) -> &Mode {
        &self.facts.mode
    }

    /// The mode's archetype.
    #[must_use]
    pub fn archetype(&self) -> Archetype {
        self.facts.archetype
    }

    /// Which provider drives the mode, and why.
    #[must_use]
    pub fn choice(&self) -> &ProviderChoice {
        &self.facts.choice
    }

    /// The longest one `prepare` of this mode's driver may take, as the
    /// provider's manifest states it (`prepare_within_ms`), never more than
    /// [`crate::PREPARE_CEILING`]; `None` where the manifest states none,
    /// and the engine then gives its own default (decision 0999).
    #[must_use]
    pub fn prepare_within(&self) -> Option<Duration> {
        self.facts.prepare_within
    }

    /// Whether "absent" from these steps proves that a record is not there:
    /// true only when there is at least one step and the mode's manifest
    /// states every one of their kinds in `absence_steps` (decision 0185).
    /// A lookup of other steps, or none, proves nothing by finding nothing.
    #[must_use]
    pub fn proves_absence(&self, steps: &[Step]) -> bool {
        !steps.is_empty()
            && steps
                .iter()
                .all(|step| !step.commits && self.facts.absence_steps.contains(&step.kind))
    }

    /// The supervisor to put in the [`PrepareContext`]: it starts only the
    /// programs the provider's manifest declares, and, when the manifest says
    /// `compiled_target_programs`, those of `compiled_target_programs`, which
    /// the engine takes from the published compiled target.
    #[must_use]
    pub fn supervisor(
        &self,
        compiled_target_programs: Vec<crate::supervisor::ProgramRef>,
    ) -> ScopedSupervisor {
        self.facts
            .supervisor
            .with_compiled_target_programs(compiled_target_programs)
    }

    /// Calls the driver's `prepare`, and remembers the run, the lease
    /// number, and the posture for the checks of [`DriverHandle::act`].
    ///
    /// # Errors
    ///
    /// The driver's failure, redacted, with its code checked.
    pub async fn prepare(
        &self,
        cx: &PrepareContext<'_>,
        lease: Option<&SessionLease>,
    ) -> Result<Prepared, StepFailure> {
        let mut prepared = self
            .driver
            .prepare(cx, lease)
            .await
            .map_err(|failure| self.checked_failure(failure, None))?;
        prepared.guard = Some(Guard {
            run: cx.run,
            epoch: cx.epoch,
            posture: cx.posture,
        });
        Ok(prepared)
    }

    /// Carries out one step through the driver.
    ///
    /// # Errors
    ///
    /// Refused unsent, before the driver is called:
    /// `unknown_variant`/`step_kind_unknown` for a step of a kind the mode
    /// did not declare; `policy`/`intent_missing` for a step marked
    /// `commits` without the token of this run, lease number, and record;
    /// `policy`/`read_only_posture` for a committing step outside a posture
    /// that may write.
    ///
    /// After the driver returns: `unknown_variant`/`unexpected_absent` for
    /// `Absent` from a step that is no lookup;
    /// `unknown_variant`/`reconcile_too_late` for `for_reconcile` from a
    /// committing step; `unknown_variant`/`undeclared_code` for a failure
    /// code nothing declares; and a failure of a committing step whose fate
    /// is unknown is always `possible_commit`/`no_confirmation`.
    pub async fn act(
        &self,
        prepared: &mut Prepared,
        step: &Step,
        input: &StepInput<'_>,
        authority: Authority<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        self.act_recorded(prepared, step, input, authority, &Values::new())
            .await
    }

    /// As [`DriverHandle::act`], with what the record's earlier steps
    /// recorded, which the driver reads through [`Prepared::recorded`]
    /// while it acts.
    ///
    /// # Errors
    ///
    /// As [`DriverHandle::act`].
    pub async fn act_recorded(
        &self,
        prepared: &mut Prepared,
        step: &Step,
        input: &StepInput<'_>,
        authority: Authority<'_>,
        recorded: &Values,
    ) -> Result<StepOutcome, StepFailure> {
        prepared.recorded = recorded.clone();
        let result = self.act_checked(prepared, step, input, authority).await;
        prepared.recorded = Values::new();
        result
    }

    /// As [`DriverHandle::act_recorded`], with the engine's answerer for
    /// the question whether another record holds a value, which the driver
    /// asks through [`Prepared::recorded_elsewhere`] while it acts, for this
    /// step and its record alone (decision 0193).
    ///
    /// # Errors
    ///
    /// As [`DriverHandle::act`].
    pub async fn act_asking(
        &self,
        prepared: &mut Prepared,
        step: &Step,
        input: &StepInput<'_>,
        authority: Authority<'_>,
        recorded: &Values,
        others: Arc<dyn OtherRecords>,
    ) -> Result<StepOutcome, StepFailure> {
        prepared.others = Some((others, input.record));
        let result = self
            .act_recorded(prepared, step, input, authority, recorded)
            .await;
        prepared.others = None;
        result
    }

    async fn act_checked(
        &self,
        prepared: &mut Prepared,
        step: &Step,
        input: &StepInput<'_>,
        authority: Authority<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        if !self.facts.steps.contains(&step.kind) {
            return Err(refused(codes::failure(
                &codes::row::STEP_KIND_UNKNOWN,
                format!(
                    "The mode `{}` declares no step of the kind `{}`.",
                    self.facts.mode, step.kind
                ),
                "Compile the target again with the provider that is enabled now.",
            )));
        }
        if step.commits {
            self.may_commit(prepared, input, authority)?;
        } else if self.facts.commit_steps.contains(&step.kind) {
            // The mode says a step of this kind always saves. Sent as a
            // step that commits nothing, it would be a write with no intent
            // stored and no token, on the word of whoever built the step.
            return Err(refused(codes::failure(
                &codes::row::INTENT_MISSING,
                format!(
                    "A `{}` step always commits in the mode `{}`, and was sent as a step that commits nothing.",
                    step.kind, self.facts.mode
                ),
                "Mark the step as the binding's commit point, so that its intent is stored first.",
            )));
        }
        let result = self.driver.act(prepared, step, input).await;
        match result {
            Err(failure) => Err(self.checked_failure(failure, Some(step))),
            Ok(StepOutcome::Absent(_)) if !step.lookup => {
                Err(StepFailure::answered(codes::failure(
                    &codes::row::UNEXPECTED_ABSENT,
                    format!(
                        "A `{}` step that is not a lookup reported its record absent.",
                        step.kind
                    ),
                    "Report this to the provider's author; only a lookup may find nothing.",
                )))
            }
            Ok(StepOutcome::Done(observed) | StepOutcome::Absent(observed))
                if step.commits && !observed.for_reconcile.is_empty() =>
            {
                Err(StepFailure::answered(codes::failure(
                    &codes::row::RECONCILE_TOO_LATE,
                    format!(
                        "The committing `{}` step returned values for reconcile, which must be known before the write is sent.",
                        step.kind
                    ),
                    "Read the record back; then report this to the provider's author.",
                )))
            }
            Ok(outcome) => Ok(self.redacted(outcome)),
        }
    }

    /// The backstop of rule 4. A driver redacts what it returns; whatever
    /// it missed is redacted here, with the same redactor, before the
    /// outcome reaches the engine: every text in the values, in the values
    /// for reconcile, in a capture's name, and in a capture whose bytes are
    /// text. Bytes that are not text cannot be searched and pass as the
    /// driver left them.
    fn redacted(&self, outcome: StepOutcome) -> StepOutcome {
        let clean = |mut observed: Observed| {
            observed.values = self.redact.redact_values(&observed.values);
            observed.for_reconcile = self.redact.redact_values(&observed.for_reconcile);
            for capture in &mut observed.captures {
                capture.name = self.redact.redact(&capture.name);
                if let Ok(text) = std::str::from_utf8(&capture.bytes)
                    && self.redact.contains_secret(text)
                {
                    capture.bytes = self.redact.redact(text).into_bytes();
                }
            }
            observed
        };
        match outcome {
            StepOutcome::Done(observed) => StepOutcome::Done(clean(observed)),
            StepOutcome::Absent(observed) => StepOutcome::Absent(clean(observed)),
        }
    }

    /// From memory: may the driver be chosen for this surface now.
    #[must_use]
    pub fn health(&self, surface: &SurfaceRef) -> DriverHealth {
        self.driver.health(surface)
    }

    /// Gives back what `prepare` acquired.
    pub async fn release(&self, prepared: Prepared, poisoned: bool) -> Cleanup {
        self.driver.release(prepared, poisoned).await
    }

    fn may_commit(
        &self,
        prepared: &Prepared,
        input: &StepInput<'_>,
        authority: Authority<'_>,
    ) -> Result<(), StepFailure> {
        let missing = |why: &str| {
            refused(codes::failure(
                &codes::row::INTENT_MISSING,
                format!("A step that commits was refused before it was sent: {why}."),
                "Store the record's intent in the ledger, and pass the token it gives for this record.",
            ))
        };
        let Some(guard) = prepared.guard else {
            return Err(missing("its driver was not prepared through the handle"));
        };
        if !matches!(
            guard.posture,
            Posture::Live | Posture::Sandbox { test_records: true }
        ) {
            return Err(refused(codes::failure(
                &codes::row::READ_ONLY_POSTURE,
                "A step that commits was asked for where nothing may be written.",
                "Run the operation live, or against a declared test record.",
            )));
        }
        let Authority::Write(token) = authority else {
            return Err(missing("it carried no intent token"));
        };
        if token.run() != guard.run {
            return Err(missing("its token belongs to another run"));
        }
        if token.epoch() != guard.epoch {
            return Err(missing("its token was stored under another lease number"));
        }
        if input.record != Some(token.record()) {
            return Err(missing("its token is for another record"));
        }
        if input.key.is_some_and(|key| key != token.record_key()) {
            return Err(missing("its token is for a record with another key"));
        }
        Ok(())
    }

    /// Redacts a driver's failure, refuses a code nothing declares, and
    /// fixes the class of a write that was sent and is not settled.
    fn checked_failure(&self, failure: StepFailure, step: Option<&Step>) -> StepFailure {
        let StepFailure { failure, mut sent } = failure;
        let mut failure = self.redact.redact_failure(&failure);
        let declared = !codes::is_kernel_only(failure.code())
            && (self.facts.codes.get(failure.code()) == Some(&failure.class())
                || step.is_some_and(|step| {
                    step.fails_as.class == failure.class() && step.fails_as.code == *failure.code()
                })
                || is_contract_code(failure.class(), failure.code()));
        if !declared {
            failure = codes::failure(
                &codes::row::UNDECLARED_CODE,
                format!(
                    "The driver of the mode `{}` failed with `{}/{}`, which its manifest does not declare.",
                    self.facts.mode,
                    failure.class(),
                    failure.code()
                ),
                "Report this to the provider's author; every failure code is declared in the manifest.",
            );
        }
        // A write that was sent is settled only by the system's own
        // verdict. Where nothing says what became of it, it is in doubt.
        // `policy` and `input` say that nothing was sent: the kernel
        // releases a record's lock on them without reading the record. On a
        // committing step that did leave they are not a verdict of the
        // system, so the write is in doubt as well. `stale` stays: there
        // the system answered that the record had changed and nothing was
        // written.
        let in_doubt = step.is_some_and(|step| step.commits)
            && match sent {
                Sent::NotSent => false,
                Sent::Unknown => true,
                Sent::Answered => {
                    matches!(failure.class(), FailureClass::Policy | FailureClass::Input)
                }
            };
        if in_doubt {
            sent = Sent::Unknown;
            failure = codes::row::NO_CONFIRMATION
                .failure(
                    failure.message().to_owned(),
                    "Reconcile the record: read it back before anything is written again.",
                )
                .carrying_said(&failure);
        }
        StepFailure { failure, sent }
    }
}

/// The codes any driver may return without declaring them: this crate's
/// own, and those of the session keeper, which reach a driver through the
/// lease.
fn is_contract_code(class: FailureClass, code: &FailureCode) -> bool {
    use a2m_auth::codes as auth;
    if codes::ALL
        .iter()
        .any(|(known, text)| *known == class && *text == code.as_str())
    {
        return true;
    }
    let session = [
        auth::NEEDS_LOGIN,
        auth::NEEDS_MFA,
        auth::CREDENTIAL_REFUSED,
        auth::LEASE_STALE,
        auth::DEVICE_REQUIRED,
    ];
    class == FailureClass::SessionExpired && session.contains(code)
}

/// A port over one prepared driver that can only read: every step goes
/// through the handle with [`Authority::Read`], so a step marked `commits`
/// is refused before the driver sees it. The engine's sandbox gives a
/// discoverer one of these, or its own implementation of
/// [`SandboxPort`](crate::SandboxPort) built on it.
pub struct ReadOnlyPort {
    handle: DriverHandle,
    prepared: tokio::sync::Mutex<Prepared>,
}

impl ReadOnlyPort {
    /// A port over what `handle` prepared.
    #[must_use]
    pub fn new(handle: DriverHandle, prepared: Prepared) -> Self {
        Self {
            handle,
            prepared: tokio::sync::Mutex::new(prepared),
        }
    }

    /// Gives back the handle and what was prepared, for release.
    #[must_use]
    pub fn into_parts(self) -> (DriverHandle, Prepared) {
        (self.handle, self.prepared.into_inner())
    }
}

impl crate::discover::SandboxPort for ReadOnlyPort {
    fn act<'a>(
        &'a self,
        step: &'a Step,
        input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async move {
            let mut prepared = self.prepared.lock().await;
            self.handle
                .act(&mut prepared, step, input, Authority::Read)
                .await
        })
    }
}
