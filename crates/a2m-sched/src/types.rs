//! What the scheduler is given and what it answers with.
//!
//! Nothing here names a mode, a pack, or a kind of system. A limit that
//! differs by mode reaches the scheduler as a number, and a thing only one
//! job may use at a time reaches it as a key.

use std::collections::BTreeMap;
use std::time::Duration;

use a2m_core::{
    Digest, Environment, Epoch, Failure, FailureClass, FailureCode, JobId, Mode, PrincipalId,
    RunId, RunState, RunnerId, SessionId, TargetName, Timestamp, WorkspaceId,
};
use a2m_ledger::{Fence, NewRun};
use serde::{Deserialize, Serialize};

use crate::failure;

/// The protocol version this scheduler speaks with runners. It also accepts
/// the one before.
pub const PROTOCOL_VERSION: u32 = 1;

/// Defines a closed list of words with the text the store keeps for each.
macro_rules! words {
    (
        $(#[$meta:meta])*
        $name:ident { $( $(#[$value_meta:meta])* $value:ident => $text:literal ),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $( $(#[$value_meta])* $value ),+
        }

        impl $name {
            /// The word, as the store and messages write it.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $( Self::$value => $text ),+ }
            }

            /// Every word of the list, in order.
            pub const ALL: &'static [Self] = &[ $( Self::$value ),+ ];
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

words! {
    /// What a job is for (spec section 223 and the ledger schema).
    JobKind {
        /// Build the diff of a write.
        Preview => "preview",
        /// Carry out the steps.
        Execute => "execute",
        /// A trial of an unpublished tool against a test stand.
        Try => "try",
        /// Discovery of a system.
        Discover => "discover",
        /// Read back the records in doubt after a lease was lost or a run stopped.
        Reconcile => "reconcile",
        /// Read verified records once more, later, after a lost lease.
        VerifyAfterLoss => "verify_after_loss",
        /// A canary run of a binding.
        Canary => "canary",
        /// Keep a session alive.
        SessionKeepalive => "session_keepalive",
    }
}

words! {
    /// Where a job stands.
    JobState {
        /// Waiting for a runner.
        Pending => "pending",
        /// Held by one runner under a lease.
        Leased => "leased",
        /// Ended with a result.
        Done => "done",
        /// Its lease ended without a result.
        Lost => "lost",
        /// Ended without ever being worked on to an end: cancelled, or never
        /// taken within its queue limit.
        Dead => "dead",
    }
}

impl JobState {
    pub(crate) fn parse(text: &str) -> Result<Self, Failure> {
        Self::ALL
            .iter()
            .copied()
            .find(|state| state.as_str() == text)
            .ok_or_else(|| failure::corrupt("a job state that is not in the list"))
    }
}

/// Which jobs go first. Interactive work is dispatched before batch work
/// that was queued earlier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// A person or an agent is waiting for the answer.
    Interactive,
    /// Nobody is waiting: a nightly pipeline.
    Batch,
}

impl Priority {
    pub(crate) const fn stored(self) -> i64 {
        match self {
            Self::Interactive => 0,
            Self::Batch => 1,
        }
    }

    pub(crate) fn from_stored(number: i64) -> Result<Self, Failure> {
        match number {
            0 => Ok(Self::Interactive),
            1 => Ok(Self::Batch),
            _ => Err(failure::corrupt("a priority that is not in its list")),
        }
    }
}

/// The kernel's health at the moment of a submission, as its caller knows it
/// (spec section 224). The scheduler does not measure it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthState {
    /// Everything works.
    Ready,
    /// The kernel takes reads and no writes. `cause` is the code of what is
    /// wrong.
    Degraded {
        /// The code of what is wrong.
        cause: FailureCode,
    },
    /// The kernel takes no work.
    Stopped {
        /// The code of what is wrong.
        cause: FailureCode,
    },
}

/// Checks the two parts of a key and joins them as `kind:name`.
fn checked_key(kind: &str, name: &str) -> Result<String, Failure> {
    let kind_ok = !kind.is_empty()
        && kind.len() <= 32
        && kind
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    let name_ok = !name.is_empty()
        && name.len() <= 200
        && name
            .chars()
            .all(|character| !character.is_control() && !character.is_whitespace());
    if kind_ok && name_ok {
        Ok(format!("{kind}:{name}"))
    } else {
        Err(failure::invalid(
            "a key is a lowercase kind and a name of at most 200 characters without spaces",
        ))
    }
}

/// Something at most one job may hold at a time: a session that takes one
/// writer, a screen, a device link, a file, the lock on a record. The
/// scheduler does not know what any of these is; it compares keys.
///
/// Keys are ordered by their text. That order is the rule that keeps two
/// jobs from waiting for each other: see [`Scheduler::acquire`].
///
/// [`Scheduler::acquire`]: crate::Scheduler::acquire
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResourceKey(String);

impl ResourceKey {
    /// A key of `kind`, such as `screen`, named `name`.
    ///
    /// # Errors
    ///
    /// `input`/`schema_invalid` when the kind is not a lowercase word of at
    /// most 32 characters, or the name is empty, longer than 200 characters,
    /// or holds a space or a control character.
    pub fn new(kind: &str, name: &str) -> Result<Self, Failure> {
        checked_key(kind, name).map(Self)
    }

    /// The key of the lock on one record: the workspace, target,
    /// environment, and keyed digest of the record key, as the ledger's lock
    /// table names it. The ledger records who holds it; the scheduler only
    /// queues those who wait.
    #[must_use]
    pub fn record(
        workspace: WorkspaceId,
        target: &TargetName,
        environment: &Environment,
        key_hash: &Digest,
    ) -> Self {
        Self(format!(
            "record:{workspace}/{target}/{environment}/{key_hash}"
        ))
    }

    /// The key of what one write holds alone on one surface of a target in
    /// one environment: `kind` is the word the write's mode declares, such
    /// as `surface`, `file`, `link`, or `screen`, and `surface` is the
    /// surface's name in the target's files. Whoever submits a job that
    /// writes puts one such key in [`JobRequirements::exclusive`] for every
    /// surface the job may write on, so that every submitter names the same
    /// thing by the same key. The scheduler compares the key and does not
    /// know what the kind means.
    ///
    /// # Errors
    ///
    /// As [`ResourceKey::new`].
    pub fn held_alone(
        kind: &str,
        workspace: WorkspaceId,
        target: &TargetName,
        environment: &Environment,
        surface: &str,
    ) -> Result<Self, Failure> {
        checked_key(
            kind,
            &format!("{workspace}/{target}/{environment}/{surface}"),
        )
        .map(Self)
    }

    /// The key of the one place a session has for a writer.
    #[must_use]
    pub fn session(session: SessionId) -> Self {
        Self(format!("session:{session}"))
    }

    /// The key, as the store keeps it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn from_stored(text: &str) -> Self {
        Self(text.to_owned())
    }
}

impl std::fmt::Display for ResourceKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The name of a rate budget: a token bucket for one target, one surface,
/// one session, or one caller.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BudgetKey(String);

impl BudgetKey {
    /// A budget of `kind` named `name`.
    ///
    /// # Errors
    ///
    /// As [`ResourceKey::new`].
    pub fn new(kind: &str, name: &str) -> Result<Self, Failure> {
        checked_key(kind, name).map(Self)
    }

    /// The budget of one target in one environment.
    #[must_use]
    pub fn target(workspace: WorkspaceId, target: &TargetName, environment: &Environment) -> Self {
        Self(format!("target:{workspace}/{target}/{environment}"))
    }

    /// The budget of one surface of a target. `surface` is the surface's
    /// name in the target's files.
    ///
    /// # Errors
    ///
    /// As [`ResourceKey::new`].
    pub fn surface(
        workspace: WorkspaceId,
        target: &TargetName,
        environment: &Environment,
        surface: &str,
    ) -> Result<Self, Failure> {
        checked_key(
            "surface",
            &format!("{workspace}/{target}/{environment}/{surface}"),
        )
        .map(Self)
    }

    /// The budget of one session.
    #[must_use]
    pub fn session(session: SessionId) -> Self {
        Self(format!("session:{session}"))
    }

    /// The budget of one caller.
    #[must_use]
    pub fn caller(workspace: WorkspaceId, principal: PrincipalId) -> Self {
        Self(format!("caller:{workspace}/{principal}"))
    }

    /// The key, as the store keeps it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A token bucket's limit: it holds at most `capacity` requests and gains
/// `refill` requests every `per`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    /// The most requests the bucket holds, and so the largest grant.
    pub capacity: u32,
    /// How many requests it gains every `per`.
    pub refill: u32,
    /// The time in which it gains `refill` requests. Between one millisecond
    /// and one day.
    pub per: Duration,
}

/// Some requests a job may send, and the time over which to spread them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateGrant {
    /// How many requests. Never more than was asked for, and never more than
    /// the bucket held.
    pub requests: u32,
    /// The time in which the budget gains that many again. Spending the
    /// grant evenly over it keeps to the limit.
    pub over: Duration,
}

/// An operating system, as a runner reports it and a job asks for it. The
/// scheduler compares the two words and knows no list of them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OsName(String);

impl OsName {
    /// Checks `name` and keeps it.
    ///
    /// # Errors
    ///
    /// `input`/`schema_invalid` when the name is not a lowercase word of at
    /// most 32 characters.
    pub fn new(name: &str) -> Result<Self, Failure> {
        checked_key(name, "os").map(|_| Self(name.to_owned()))
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The live session a job needs, from the session keeper's records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionNeed {
    /// The session.
    pub session: SessionId,
    /// The runner that holds the session live, when one does. The job goes
    /// to that runner.
    pub home: Option<RunnerId>,
    /// Whether another runner may take the job when the home runner is gone,
    /// and have the keeper restore the session there. A session bound to a
    /// person's device is never movable.
    pub movable: bool,
}

/// How many jobs may work on one session at once. The lowest numbers across
/// the operation's candidate bindings, filled in by the caller from the
/// providers' manifests and the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLimits {
    /// Jobs that only read.
    pub reads: u32,
    /// Jobs that write.
    pub writes: u32,
}

mod millis {
    //! A duration, kept as whole milliseconds.

    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(
        duration: &Duration,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Duration, D::Error> {
        u64::deserialize(deserializer).map(Duration::from_millis)
    }
}

/// What a job needs from the runner that takes it, and from the scheduler
/// before it is handed out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRequirements {
    /// What the job is for.
    pub kind: JobKind,
    /// The target.
    pub target: TargetName,
    /// The environment.
    pub environment: Environment,
    /// The runner must offer one of these, with a free slot. Compared by
    /// name only. Empty: any runner with a free slot.
    pub modes: Vec<Mode>,
    /// The operating system the runner must report, when it matters.
    pub os: Option<OsName>,
    /// The person the job acts as. A runner that is a person's own device
    /// takes only jobs that act as that person.
    pub acts_as: Option<PrincipalId>,
    /// The live session the job needs.
    pub session: Option<SessionNeed>,
    /// How many jobs may work on that session at once.
    pub session_limits: SessionLimits,
    /// What the job must hold alone for as long as it is leased: at most one
    /// leased job holds each key.
    pub exclusive: Vec<ResourceKey>,
    /// The longest step time limit in the tool. It sets the quarantine after
    /// a lost lease, when it is longer than the setting.
    #[serde(with = "millis")]
    pub max_step: Duration,
    /// Whether the job may write to the system.
    pub writes: bool,
}

/// A run to admit, with its first job.
#[derive(Debug, Clone)]
pub struct Submission {
    /// The run row. Its caller, tool version, and input hash are the run's
    /// identity: an unfinished run with the same three is returned instead
    /// of a second one (ledger rule 6).
    pub run: NewRun,
    /// What the first job needs.
    pub job: JobRequirements,
    /// Whether someone is waiting.
    pub priority: Priority,
    /// The kernel's health, as the caller knows it now.
    pub health: HealthState,
    /// How long the job may wait for a runner. `None`: the setting.
    pub queue_wait: Option<Duration>,
    /// Budgets a submission spends one request of each from, such as the
    /// caller's. A spent budget refuses the submission.
    pub budgets: Vec<BudgetKey>,
}

/// A further job for a run that exists: the execute job after an approval,
/// or a resume.
#[derive(Debug, Clone)]
pub struct Enqueue {
    /// The run.
    pub run: RunId,
    /// What the job needs.
    pub job: JobRequirements,
    /// Whether someone is waiting.
    pub priority: Priority,
    /// The kernel's health, as the caller knows it now.
    pub health: HealthState,
    /// How long the job may wait for a runner. `None`: the setting.
    pub queue_wait: Option<Duration>,
}

/// What a submission gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admitted {
    /// A new run, queued with its first job.
    New {
        /// The run.
        run: RunId,
        /// Its first job.
        job: JobId,
    },
    /// The unfinished run the same caller already has for the same tool
    /// version and inputs. Not a refusal.
    Existing {
        /// That run.
        run: RunId,
    },
}

/// What a runner offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerOffer {
    /// The runner.
    pub runner: RunnerId,
    /// The workspace it serves.
    pub workspace: WorkspaceId,
    /// The targets it serves. Empty: every target of the workspace.
    pub targets: Vec<TargetName>,
    /// The modes it can work in, by name.
    pub modes: Vec<Mode>,
    /// Its operating system.
    pub os: Option<OsName>,
    /// The person whose device it is. Such a runner is given only jobs that
    /// act as that person.
    pub owner: Option<PrincipalId>,
    /// The protocol version it speaks.
    pub protocol: u32,
}

/// What a runner is told when it joins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunnerTerms {
    /// How often to renew a lease.
    pub heartbeat: Duration,
    /// How long a lease lasts from the moment the request was sent.
    pub lease: Duration,
    /// The protocol version the scheduler speaks.
    pub protocol: u32,
}

/// How many more jobs a runner can take now.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Slots {
    /// The most jobs one claim returns.
    pub total: u32,
    /// Free slots for each mode, by name. A job that names modes takes one
    /// slot of the first of its modes that has one free.
    pub per_mode: BTreeMap<Mode, u32>,
}

/// The right to work on one job for a limited time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobLease {
    /// The job.
    pub job: JobId,
    /// The run it belongs to. `None` for a system job.
    pub run: Option<RunId>,
    /// The lease number. For a job of a run it is the run's, and the ledger
    /// refuses anything that carries another.
    pub epoch: Epoch,
    /// How long the lease lasts, from the moment the claim was sent. A
    /// duration, never a clock time.
    pub length: Duration,
    /// The mode whose slot the job took, when it names modes.
    pub mode: Option<Mode>,
    /// How many times the job has been handed out, this time included.
    pub attempt: u32,
    /// What the job needs.
    pub requirements: JobRequirements,
}

impl JobLease {
    /// What every later call about this lease carries.
    #[must_use]
    pub fn lease_ref(&self) -> LeaseRef {
        LeaseRef {
            job: self.job,
            epoch: self.epoch,
        }
    }

    /// The right to change the run in the ledger, for a job of a run.
    #[must_use]
    pub fn fence(&self) -> Option<Fence> {
        self.run.map(|run| Fence {
            run,
            epoch: self.epoch,
        })
    }
}

/// A job and the lease number its holder was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaseRef {
    /// The job.
    pub job: JobId,
    /// The lease number.
    pub epoch: Epoch,
}

/// What a heartbeat's answer tells the runner to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Directive {
    /// Carry on.
    Continue,
    /// Finish the commit point in hand, record it, and end the run as
    /// cancelled.
    Cancel,
    /// Take no new work, finish what is held, and leave.
    Drain,
    /// Stop at the next commit point and leave the run resumable: the work
    /// is covered by a hold.
    Pause,
}

/// The answer to a heartbeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Renewal {
    /// How long the lease lasts from the moment the heartbeat was sent.
    pub remaining: Duration,
    /// What to do.
    pub directive: Directive,
}

/// How a job ended. The run's own state is in the ledger, written by the
/// engine under the lease; the scheduler reads it and keeps a copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobEnd {
    /// The work is finished: the run reached a final state, or a system job
    /// did what it was for.
    Done,
    /// The run waits for a person or an approval; a later job continues it.
    Rests,
    /// The run stopped with a failure and can be resumed.
    Stopped {
        /// The class of the failure.
        class: FailureClass,
        /// Its code.
        code: FailureCode,
        /// Whether a record may have been written without being read back.
        /// The scheduler then queues a reconcile job.
        in_doubt: bool,
    },
    /// The runner gives the job back before its lease runs out, having
    /// stopped between commit points. The job is handed out again.
    LeaseEnding,
}

/// What cancelling a run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelOutcome {
    /// The run is cancelled now. No runner held it.
    Cancelled,
    /// A runner holds the run. It is told at its next heartbeat, finishes the
    /// commit point in hand, and cancels the run itself.
    Requested,
    /// The run had already ended.
    AlreadyFinal(RunState),
}

/// What no job is handed out for while it is on hold.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum HoldScope {
    /// Every job: the stop-everything switch.
    Everything,
    /// Every job of one workspace.
    Workspace(WorkspaceId),
    /// Every job on one target of a workspace.
    Target(WorkspaceId, TargetName),
    /// Every job of one caller.
    Caller(WorkspaceId, PrincipalId),
}

impl HoldScope {
    /// The scope's word, its workspace, and its name, as the store keeps
    /// them.
    pub(crate) fn stored(&self) -> (&'static str, String, String) {
        match self {
            Self::Everything => ("everything", String::new(), String::new()),
            Self::Workspace(workspace) => ("workspace", workspace.to_string(), String::new()),
            Self::Target(workspace, target) => {
                ("target", workspace.to_string(), target.as_str().to_owned())
            }
            Self::Caller(workspace, principal) => {
                ("caller", workspace.to_string(), principal.to_string())
            }
        }
    }

    /// What a refusal calls the scope.
    pub(crate) const fn word(&self) -> &'static str {
        match self {
            Self::Everything => "installation",
            Self::Workspace(_) => "workspace",
            Self::Target(..) => "target",
            Self::Caller(..) => "caller",
        }
    }
}

/// A job, as the store holds it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRow {
    /// The job.
    pub id: JobId,
    /// Its run. `None` for a system job.
    pub run: Option<RunId>,
    /// What it is for.
    pub kind: JobKind,
    /// Where it stands.
    pub state: JobState,
    /// Its priority.
    pub priority: Priority,
    /// The lease number last given out for it.
    pub epoch: Epoch,
    /// The runner that holds or last held it.
    pub runner: Option<RunnerId>,
    /// How many times it was handed out.
    pub attempts: u32,
    /// The moment from which it may be handed out.
    pub available_at: Timestamp,
    /// The moment its lease runs out on the scheduler's count.
    pub lease_until: Option<Timestamp>,
    /// What the next heartbeat tells its runner, holds and drains aside.
    pub cancel_requested: bool,
    /// The run's state when the job ended.
    pub end_state: Option<RunState>,
    /// The failure it ended with.
    pub end: Option<(FailureClass, FailureCode)>,
}

/// What one sweep did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SweepReport {
    /// Jobs whose lease ended without a result. Each run was stopped as
    /// `possible_commit`, `runner_lost` under a new lease number.
    pub lost: Vec<JobId>,
    /// Reconcile jobs queued for those runs, to be handed out when the
    /// quarantine ends.
    pub reconcile: Vec<JobId>,
    /// Jobs no runner took within their queue limit.
    pub expired: Vec<JobId>,
    /// Waits whose time ran out: claims, lock and resource waits, and waits
    /// for a budget to refill.
    pub waits_ended: usize,
}

/// The scheduler's settings. The defaults are the ones of spec sections 211
/// and 223.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedSettings {
    /// How many jobs may wait for one target in one environment.
    pub queue_length: u32,
    /// How many jobs of one caller may wait at once.
    pub caller_queue_cap: u32,
    /// How long a job may wait for a runner before its run is told `busy`.
    pub queue_wait: Duration,
    /// How long a lease lasts.
    pub lease_length: Duration,
    /// How often a runner renews its lease.
    pub heartbeat_interval: Duration,
    /// How long after its own count of a lease runs out the scheduler
    /// declares it lost.
    pub lost_after: Duration,
    /// How long nobody touches a run's records after its lease was lost. The
    /// job's longest step is used when it is longer.
    pub quarantine: Duration,
    /// The limit of a budget nobody set: a conservative default.
    pub default_rate: RateLimit,
}

impl Default for SchedSettings {
    fn default() -> Self {
        Self {
            queue_length: 100,
            caller_queue_cap: 25,
            queue_wait: Duration::from_secs(300),
            lease_length: Duration::from_secs(30),
            heartbeat_interval: Duration::from_secs(10),
            lost_after: Duration::from_secs(5),
            quarantine: Duration::from_secs(60),
            default_rate: RateLimit {
                capacity: 5,
                refill: 1,
                per: Duration::from_secs(1),
            },
        }
    }
}
