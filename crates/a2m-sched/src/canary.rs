//! Canaries on a schedule of their own, apart from a watch (decision 0482).
//!
//! A canary is one published read, with an input fixed when it is asked
//! for, sent at a fixed interval until an end, to find whether the
//! knowledge the read goes through still fits the system. What it leaves is
//! drift marks, kept by its drift and lifted by its pass (decision 0380),
//! and its last outcome here; it keeps nothing of what it read and compares
//! nothing.
//!
//! Who confirms it, and what it may send, is the point of this module:
//!
//! - A canary is kept `asked` and sends nothing until the person it acts
//!   for confirms it, by an act of their own; [`Scheduler::due_canaries`]
//!   plans confirmed canaries alone. A canary nobody confirmed ends at its
//!   end, having sent nothing.
//! - It sends one read, of the version published when it was asked: its
//!   confirmation is refused under any other version, and a canary whose
//!   target has another published version ends, `version_changed`, before
//!   another check is admitted. Its job writes nothing and is of the kind
//!   `canary`, which the engine refuses for any step that writes.
//! - It is held to the limits of a watch: the interval's floor, the longest
//!   it lasts, as many per person as watches, and, with the watches on the
//!   same surface, the share of the surface's budget watches may take. A
//!   check is `batch` work, leaves the reserve of every budget to the
//!   caller's own reads, and spends the share at run time.
//!
//! Due times are a watch's: the moment of confirmation, plus a phase made
//! from the canary's identifier, plus a whole number of intervals. A due
//! time that passed with no check is counted as missed and never checked
//! late; one check follows for the latest. After a drift a canary goes on
//! at its interval, so that a pass after the system comes back lifts the
//! mark; that is where it differs from a watch, which pauses.

use std::time::Duration;

use a2m_core::{
    Digest, EffectClass, Environment, Failure, FailureClass, FailureCode, JobId, OperationName,
    PrincipalId, RunId, SurfaceName, TargetName, Timestamp, WorkspaceId,
};
use a2m_ledger::{BlobRef, Due, Durability, Owner, Row, SqlValue, Tx, WatchId};

use crate::codes::{CANARY_NOT_A_READ, CANARY_NOT_ASKED, CANARY_TOO_MANY, CANARY_VERSION_CHANGED};
use crate::failure;
use crate::scheduler::Scheduler;
use crate::tables;
use crate::types::{Admitted, BudgetKey, CancelOutcome, Priority, RateLimit, Submission};
use crate::watch::{ADMISSION_WAITS, DuePlan, DueTimes, plan_times, share_key_of};

/// One canary.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanaryId([u8; 16]);

impl CanaryId {
    /// What the identifier starts with when written, before the underscore.
    pub const PREFIX: &'static str = "canary";

    /// The identifier's 16 bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// An identifier from its 16 bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }
}

impl std::fmt::Display for CanaryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}_", Self::PREFIX)?;
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for CanaryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CanaryId({self})")
    }
}

impl std::str::FromStr for CanaryId {
    type Err = Failure;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        // The same shape as every other identifier, under its own prefix.
        let rest = text
            .strip_prefix("canary_")
            .ok_or_else(|| failure::invalid("an identifier of a canary"))?;
        let as_run: RunId = format!("{}_{rest}", RunId::PREFIX)
            .parse()
            .map_err(|_| failure::invalid("an identifier of a canary"))?;
        Ok(Self(*as_run.as_bytes()))
    }
}

/// Where a canary stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanaryState {
    /// Asked for, and waiting for its person: it sends nothing.
    Asked,
    /// Confirmed by its person: its checks are admitted when they are due.
    Confirmed,
    /// One check is under way.
    Running,
    /// Ended, for good.
    Ended,
}

impl CanaryState {
    /// The word the store and every face use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Asked => "asked",
            Self::Confirmed => "confirmed",
            Self::Running => "running",
            Self::Ended => "ended",
        }
    }

    /// Every state's word.
    pub const NAMES: &'static [&'static str] = &["asked", "confirmed", "running", "ended"];

    fn parse(word: &str) -> Result<Self, Failure> {
        Ok(match word {
            "asked" => Self::Asked,
            "confirmed" => Self::Confirmed,
            "running" => Self::Running,
            "ended" => Self::Ended,
            _ => return Err(failure::corrupt("a canary's state")),
        })
    }
}

/// Why a canary ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanaryEnd {
    /// Its end came.
    EndReached,
    /// Its person ended it.
    EndedByPerson,
    /// The caller that asked for it ended it.
    EndedByCaller,
    /// Its target has another published version: what a check would send
    /// is not what its person confirmed.
    VersionChanged,
}

impl CanaryEnd {
    /// The word the store and every face use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EndReached => "end_reached",
            Self::EndedByPerson => "ended_by_person",
            Self::EndedByCaller => "ended_by_caller",
            Self::VersionChanged => "version_changed",
        }
    }

    /// Every reason's word.
    pub const NAMES: &'static [&'static str] = &[
        "end_reached",
        "ended_by_person",
        "ended_by_caller",
        "version_changed",
    ];

    fn parse(word: &str) -> Result<Self, Failure> {
        Ok(match word {
            "end_reached" => Self::EndReached,
            "ended_by_person" => Self::EndedByPerson,
            "ended_by_caller" => Self::EndedByCaller,
            "version_changed" => Self::VersionChanged,
            _ => return Err(failure::corrupt("why a canary ended")),
        })
    }
}

/// What a canary's last check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanaryOutcome {
    /// The read passed: what it went through vouched for again.
    Passed,
    /// The read failed as drift: the knowledge it names is marked.
    Drifted,
    /// The read failed for another reason.
    Failed,
    /// The read's run waited for a person, such as for a sign-in; it was
    /// let go, and nothing was asked of anyone.
    Rested,
}

impl CanaryOutcome {
    /// The word the store and every face use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Drifted => "drifted",
            Self::Failed => "failed",
            Self::Rested => "rested",
        }
    }

    /// Every outcome's word.
    pub const NAMES: &'static [&'static str] = &["passed", "drifted", "failed", "rested"];

    fn parse(word: &str) -> Result<Self, Failure> {
        Ok(match word {
            "passed" => Self::Passed,
            "drifted" => Self::Drifted,
            "failed" => Self::Failed,
            "rested" => Self::Rested,
            _ => return Err(failure::corrupt("a canary's outcome")),
        })
    }
}

/// A canary as it is asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCanary {
    /// The workspace.
    pub workspace: WorkspaceId,
    /// The target.
    pub target: TargetName,
    /// The environment.
    pub environment: Environment,
    /// The published read.
    pub operation: OperationName,
    /// The published version: its compiled target's hash.
    pub target_version: Digest,
    /// The operation's compiled effect class. Anything but `read` is
    /// refused.
    pub effect: EffectClass,
    /// The digest of the fixed input, as a run's input hash is made.
    pub input_hash: Digest,
    /// The fixed input, stored as a run's input is.
    pub input: Option<BlobRef>,
    /// The surface the read uses.
    pub surface: SurfaceName,
    /// How many requests one check sends, at least one.
    pub requests_per_check: u32,
    /// The time between due times: whole seconds, at least the floor.
    pub interval: Duration,
    /// When it ends at the latest.
    pub ends_at: Timestamp,
    /// The caller that asked.
    pub asked_by: PrincipalId,
    /// The person whose canary it is: whose session its reads use and who
    /// alone confirms it.
    pub acts_for: PrincipalId,
}

/// A canary as the store holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanaryRow {
    /// Its identifier.
    pub id: CanaryId,
    /// The workspace.
    pub workspace: WorkspaceId,
    /// The target.
    pub target: TargetName,
    /// The environment.
    pub environment: Environment,
    /// The published read.
    pub operation: OperationName,
    /// The version it was asked under.
    pub target_version: Digest,
    /// The digest of its input.
    pub input_hash: Digest,
    /// Its input.
    pub input: Option<BlobRef>,
    /// The surface its read uses.
    pub surface: SurfaceName,
    /// How many requests one check sends.
    pub requests_per_check: u32,
    /// The time between due times.
    pub interval: Duration,
    /// When it ends at the latest.
    pub ends_at: Timestamp,
    /// The caller that asked.
    pub asked_by: PrincipalId,
    /// Its person.
    pub acts_for: PrincipalId,
    /// When it was asked for.
    pub asked_at: Timestamp,
    /// Who confirmed it, and when: its person, or nobody yet.
    pub confirmed: Option<(PrincipalId, Timestamp)>,
    /// Its phase, from the confirmation on.
    pub phase: Option<Duration>,
    /// Where it stands.
    pub state: CanaryState,
    /// Why it ended, once it has.
    pub ended: Option<CanaryEnd>,
    /// The index of the next due time not yet handled.
    pub next_due_index: u64,
    /// Not tried before this moment, after an admission that waits.
    pub not_before: Option<Timestamp>,
    /// The run of the check under way.
    pub running_run: Option<RunId>,
    /// Whether the check under way opened its run.
    pub opened_run: bool,
    /// How many checks ended.
    pub checks: u64,
    /// How many due times passed with no check.
    pub missed: u64,
    /// What the last check found.
    pub outcome: Option<LastCheck>,
}

/// What a canary's last check found, the failure it gave, and when it
/// ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastCheck {
    /// What it found.
    pub outcome: CanaryOutcome,
    /// The failure its run ended with, if any.
    pub failure: Option<(FailureClass, FailureCode)>,
    /// When it ended.
    pub at: Timestamp,
}

impl CanaryRow {
    /// The next due time, for a confirmed canary.
    #[must_use]
    pub fn next_due_at(&self) -> Option<Timestamp> {
        let (_, confirmed_at) = self.confirmed?;
        match self.state {
            CanaryState::Confirmed | CanaryState::Running => Some(crate::watch::due_at(
                confirmed_at,
                self.phase?,
                self.interval,
                self.next_due_index,
            )),
            CanaryState::Asked | CanaryState::Ended => None,
        }
    }

    fn times(&self) -> Option<DueTimes> {
        let (_, confirmed_at) = self.confirmed?;
        Some(DueTimes {
            confirmed_at,
            phase: self.phase?,
            interval: self.interval,
            next: self.next_due_index,
            not_before: self.not_before,
            ends_at: self.ends_at,
        })
    }
}

/// A check that is due, as [`Scheduler::due_canaries`] gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueCanary {
    /// The canary.
    pub canary: CanaryRow,
    /// The due time.
    pub due: Due,
}

/// What a canary check's admission gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanaryAdmission {
    /// Begun: the run of the read, and its job when the check opened it.
    Started {
        /// The run.
        run: RunId,
        /// Its job, when the run is new.
        job: Option<JobId>,
    },
    /// Not admitted now: the refusal, and the moment before which it is not
    /// tried again.
    Deferred {
        /// What refused it.
        refused: Failure,
        /// Not before.
        until: Timestamp,
    },
}

/// The columns [`canary_row`] reads, in its order.
macro_rules! canary_columns {
    () => {
        "SELECT id, workspace_id, target, environment, operation, target_version_hash,
                input_hash, input_ref, surface, requests_per_check, interval_seconds, ends_at,
                asked_by, acts_for, asked_at, confirmed_by, confirmed_at, phase_ms, state,
                ended_reason, next_due_index, not_before, running_run, opened_run, checks,
                missed, outcome, outcome_class, outcome_code, outcome_at
         FROM canaries"
    };
}

fn parse<T: std::str::FromStr>(text: &str) -> Result<T, Failure> {
    text.parse()
        .map_err(|_| failure::corrupt("a canary's column that no longer reads"))
}

fn count(value: i64) -> Result<u64, Failure> {
    u64::try_from(value).map_err(|_| failure::corrupt("a canary's count below zero"))
}

fn moment(ms: i64) -> Timestamp {
    Timestamp::from_unix_millis(ms)
}

fn canary_row(row: &Row) -> Result<CanaryRow, Failure> {
    let confirmed = match (row.opt_text(15)?, row.opt_int(16)?) {
        (Some(by), Some(at)) => Some((parse(by)?, moment(at))),
        (None, None) => None,
        _ => return Err(failure::corrupt("a canary's confirmation")),
    };
    let outcome = match (row.opt_text(26)?, row.opt_int(29)?) {
        (Some(word), Some(at)) => {
            let failure = match (row.opt_text(27)?, row.opt_text(28)?) {
                (Some(class), Some(code)) => Some((parse(class)?, parse(code)?)),
                (None, None) => None,
                _ => return Err(failure::corrupt("a canary's failure")),
            };
            Some(LastCheck {
                outcome: CanaryOutcome::parse(word)?,
                failure,
                at: moment(at),
            })
        }
        (None, _) => None,
        _ => return Err(failure::corrupt("a canary's outcome")),
    };
    Ok(CanaryRow {
        id: parse(row.text(0)?)?,
        workspace: parse(row.text(1)?)?,
        target: parse(row.text(2)?)?,
        environment: parse(row.text(3)?)?,
        operation: parse(row.text(4)?)?,
        target_version: parse(row.text(5)?)?,
        input_hash: parse(row.text(6)?)?,
        input: row.opt_text(7)?.map(parse).transpose()?,
        surface: parse(row.text(8)?)?,
        requests_per_check: u32::try_from(row.int(9)?)
            .map_err(|_| failure::corrupt("a canary's requests"))?,
        interval: Duration::from_secs(count(row.int(10)?)?),
        ends_at: moment(row.int(11)?),
        asked_by: parse(row.text(12)?)?,
        acts_for: parse(row.text(13)?)?,
        asked_at: moment(row.int(14)?),
        confirmed,
        phase: row
            .opt_int(17)?
            .map(|ms| count(ms).map(Duration::from_millis))
            .transpose()?,
        state: CanaryState::parse(row.text(18)?)?,
        ended: row.opt_text(19)?.map(CanaryEnd::parse).transpose()?,
        next_due_index: count(row.int(20)?)?,
        not_before: row.opt_int(21)?.map(moment),
        running_run: row.opt_text(22)?.map(parse).transpose()?,
        opened_run: row.int(23)? != 0,
        checks: count(row.int(24)?)?,
        missed: count(row.int(25)?)?,
        outcome,
    })
}

fn ms(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn secs(duration: Duration) -> i64 {
    i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
}

fn int(value: u64) -> SqlValue {
    SqlValue::from(i64::try_from(value).unwrap_or(i64::MAX))
}

/// One canary, by its identifier.
///
/// # Errors
///
/// `input`/`not_found` for no such canary; `busy` failures of the store.
pub fn get(tx: &mut Tx<'_>, id: CanaryId) -> Result<CanaryRow, Failure> {
    let row = tx.tables(Owner::Sched).query_first(
        concat!(canary_columns!(), " WHERE id = ?1"),
        &[SqlValue::from(id.to_string())],
    )?;
    match row {
        Some(row) => canary_row(&row),
        None => Err(failure::not_found("canary")),
    }
}

/// The canaries of `person`, oldest first; with `open_only`, those not
/// ended.
///
/// # Errors
///
/// `busy` failures of the store.
pub fn of_person(
    tx: &mut Tx<'_>,
    workspace: &WorkspaceId,
    person: &PrincipalId,
    open_only: bool,
) -> Result<Vec<CanaryRow>, Failure> {
    let rows = tx.tables(Owner::Sched).query(
        concat!(
            canary_columns!(),
            " WHERE workspace_id = ?1 AND acts_for = ?2 AND (?3 = 0 OR state <> 'ended')
             ORDER BY asked_at, id"
        ),
        &[
            SqlValue::from(workspace.to_string()),
            SqlValue::from(person.to_string()),
            SqlValue::from(i64::from(open_only)),
        ],
    )?;
    rows.iter().map(canary_row).collect()
}

/// Every canary that has not ended, oldest first.
///
/// # Errors
///
/// `busy` failures of the store.
pub fn open(tx: &mut Tx<'_>) -> Result<Vec<CanaryRow>, Failure> {
    let rows = tx.tables(Owner::Sched).query(
        concat!(
            canary_columns!(),
            " WHERE state <> 'ended' ORDER BY asked_at, id"
        ),
        &[],
    )?;
    rows.iter().map(canary_row).collect()
}

/// The canary whose check is under way on `run`, if any.
///
/// # Errors
///
/// `busy` failures of the store.
pub fn of_run(tx: &mut Tx<'_>, run: RunId) -> Result<Option<CanaryRow>, Failure> {
    let row = tx.tables(Owner::Sched).query_first(
        concat!(canary_columns!(), " WHERE running_run = ?1"),
        &[SqlValue::from(run.to_string())],
    )?;
    row.as_ref().map(canary_row).transpose()
}

/// The interval and requests of every confirmed or running canary on one
/// surface, as a watch's confirmation counts them against the share.
pub(crate) fn on_surface(
    tx: &mut Tx<'_>,
    workspace: &WorkspaceId,
    target: &TargetName,
    environment: &Environment,
    surface: &SurfaceName,
) -> Result<Vec<(Duration, u32)>, Failure> {
    let rows = tx.tables(Owner::Sched).query(
        "SELECT interval_seconds, requests_per_check FROM canaries
         WHERE workspace_id = ?1 AND target = ?2 AND environment = ?3 AND surface = ?4
           AND state IN ('confirmed', 'running')",
        &[
            SqlValue::from(workspace.to_string()),
            SqlValue::from(target.as_str()),
            SqlValue::from(environment.as_str()),
            SqlValue::from(surface.as_str()),
        ],
    )?;
    rows.iter()
        .map(|row| {
            Ok((
                Duration::from_secs(count(row.int(0)?)?),
                u32::try_from(row.int(1)?).map_err(|_| failure::corrupt("a canary's requests"))?,
            ))
        })
        .collect()
}

/// The budget a canary's check spends of its surface, beside the budgets
/// its job names: the share watches and canaries take together.
pub(crate) fn share_key(
    tx: &mut Tx<'_>,
    default: &RateLimit,
    limits: &crate::WatchLimits,
    canary: &CanaryRow,
) -> Result<BudgetKey, Failure> {
    let surface = BudgetKey::surface(
        canary.workspace,
        &canary.target,
        &canary.environment,
        canary.surface.as_str(),
    )?;
    share_key_of(tx, default, limits, &surface)
}

/// Ends `id` for `why`. A check under way is let go: its run is cancelled
/// when the check opened it, in the same transaction. Whether a run was
/// cancelled at once, and whether a job let resources go.
fn end_in(tx: &mut Tx<'_>, row: &CanaryRow, why: CanaryEnd) -> Result<(bool, bool), Failure> {
    let mut cancels = (false, false);
    if let (Some(run), true) = (row.running_run, row.opened_run) {
        let (outcome, freed) = tables::cancel(tx, run)?;
        cancels = (outcome == CancelOutcome::Cancelled, freed);
    }
    tx.tables(Owner::Sched).execute(
        "UPDATE canaries SET state = 'ended', ended_reason = ?2, running_run = NULL,
             opened_run = 0, not_before = NULL
         WHERE id = ?1 AND state <> 'ended'",
        &[
            SqlValue::from(row.id.to_string()),
            SqlValue::from(why.as_str()),
        ],
    )?;
    Ok(cancels)
}

fn not_asked(state: CanaryState) -> Failure {
    Failure::new(
        FailureClass::Policy,
        CANARY_NOT_ASKED,
        format!(
            "The canary is `{}`, so there is nothing to confirm.",
            state.as_str()
        ),
        "Call `canary.get` to see where it stands.",
    )
}

fn version_changed() -> Failure {
    Failure::new(
        FailureClass::Policy,
        CANARY_VERSION_CHANGED,
        "The target's published version is not the one the canary was asked under.",
        "Ask for a new canary of the version published now.",
    )
}

fn ask_in(
    tx: &mut Tx<'_>,
    limits: &crate::WatchLimits,
    new: &NewCanary,
) -> Result<CanaryRow, Failure> {
    if new.effect != EffectClass::Read {
        return Err(Failure::new(
            FailureClass::Policy,
            CANARY_NOT_A_READ,
            format!(
                "`{}` is not a read, and a canary sends reads alone.",
                new.operation
            ),
            "Ask for a canary of an operation whose effect is `read`.",
        ));
    }
    if new.requests_per_check == 0 {
        return Err(failure::invalid("a canary whose check sends no request"));
    }
    let now = tx.now();
    limits.check_new(new.interval, now, new.ends_at)?;
    if new.ends_at <= now {
        return Err(failure::invalid("a canary that ends before it starts"));
    }
    let open = tx.tables(Owner::Sched).query_first(
        "SELECT COUNT(*) FROM canaries
         WHERE workspace_id = ?1 AND acts_for = ?2 AND state <> 'ended'",
        &[
            SqlValue::from(new.workspace.to_string()),
            SqlValue::from(new.acts_for.to_string()),
        ],
    )?;
    let open = match open {
        Some(row) => count(row.int(0)?)?,
        None => 0,
    };
    if open >= u64::from(limits.most_per_person) {
        return Err(Failure::new(
            FailureClass::Busy,
            CANARY_TOO_MANY,
            format!(
                "The person has {} canaries that have not ended, the most they may have.",
                limits.most_per_person
            ),
            "End a canary, then ask again.",
        ));
    }
    let id = CanaryId(*RunId::from_parts(now, tx.random()?)?.as_bytes());
    tx.tables(Owner::Sched).execute(
        "INSERT INTO canaries
            (id, workspace_id, target, environment, operation, target_version_hash,
             input_hash, input_ref, surface, requests_per_check, interval_seconds, ends_at,
             asked_by, acts_for, asked_at, state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, 'asked')",
        &[
            SqlValue::from(id.to_string()),
            SqlValue::from(new.workspace.to_string()),
            SqlValue::from(new.target.as_str()),
            SqlValue::from(new.environment.as_str()),
            SqlValue::from(new.operation.as_str()),
            SqlValue::from(new.target_version.to_string()),
            SqlValue::from(new.input_hash.to_string()),
            new.input
                .map_or(SqlValue::Null, |blob| SqlValue::from(blob.to_string())),
            SqlValue::from(new.surface.as_str()),
            SqlValue::from(new.requests_per_check),
            SqlValue::from(secs(new.interval)),
            SqlValue::from(new.ends_at.as_unix_millis()),
            SqlValue::from(new.asked_by.to_string()),
            SqlValue::from(new.acts_for.to_string()),
            SqlValue::from(now.as_unix_millis()),
        ],
    )?;
    get(tx, id)
}

impl Scheduler {
    /// Takes a request for a canary: checks it is a read, its interval and
    /// end against a watch's limits, and counts the person's canaries, in
    /// one transaction with its row. The canary sends nothing until its
    /// person confirms it.
    ///
    /// # Errors
    ///
    /// `policy`/`canary_not_a_read`; `input`/`watch_interval_too_short`,
    /// `input`/`watch_too_long`; `busy`/`canary_too_many`.
    pub async fn ask_canary(&self, new: NewCanary) -> Result<CanaryRow, Failure> {
        let limits = self.settings().watch;
        self.store()
            .write(Durability::Synced, move |tx| ask_in(tx, &limits, &new))
            .await
    }

    /// Records the confirmation of the canary's own person, `by`, while
    /// `current` is its target's published version, with the phase made
    /// from its identifier. In the same transaction the canary must fit, with
    /// the watches and canaries on its surface, in their share of `limit`.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` for a canary of another person (it is no canary
    /// of theirs); `policy`/`canary_not_asked`;
    /// `policy`/`canary_version_changed`; `busy`/`watch_over_budget_share`.
    pub async fn confirm_canary(
        &self,
        id: CanaryId,
        by: PrincipalId,
        current: Digest,
        limit: RateLimit,
    ) -> Result<CanaryRow, Failure> {
        let limits = self.settings().watch;
        let row = self
            .store()
            .write(Durability::Synced, move |tx| {
                let row = get(tx, id)?;
                if row.acts_for != by {
                    return Err(failure::not_found("canary"));
                }
                if row.state != CanaryState::Asked {
                    return Err(not_asked(row.state));
                }
                if row.target_version != current {
                    return Err(version_changed());
                }
                let mut others: Vec<(Duration, u32)> = tx
                    .watches()
                    .on_surface(&row.workspace, &row.target, &row.environment, &row.surface)?
                    .into_iter()
                    .map(|watch| (watch.interval, watch.requests_per_check))
                    .collect();
                others.extend(on_surface(
                    tx,
                    &row.workspace,
                    &row.target,
                    &row.environment,
                    &row.surface,
                )?);
                limits.fits_budget(&limit, row.interval, row.requests_per_check, &others)?;
                let phase = limits.phase(WatchId::from_bytes(id.0), row.interval);
                let now = tx.now();
                tx.tables(Owner::Sched).execute(
                    "UPDATE canaries SET state = 'confirmed', confirmed_by = ?2,
                         confirmed_at = ?3, phase_ms = ?4
                     WHERE id = ?1 AND state = 'asked'",
                    &[
                        SqlValue::from(id.to_string()),
                        SqlValue::from(by.to_string()),
                        SqlValue::from(now.as_unix_millis()),
                        SqlValue::from(ms(phase)),
                    ],
                )?;
                get(tx, id)
            })
            .await?;
        self.wake();
        Ok(row)
    }

    /// One canary.
    ///
    /// # Errors
    ///
    /// `input`/`not_found`; `busy` failures of the store.
    pub async fn canary(&self, id: CanaryId) -> Result<CanaryRow, Failure> {
        self.store().read(move |tx| get(tx, id)).await
    }

    /// Ends a canary that has not ended, for `why`; a check under way is let
    /// go, and its run cancelled when the check opened it. An ended canary
    /// is answered as it is.
    ///
    /// # Errors
    ///
    /// `input`/`not_found`; `busy` failures of the store.
    pub async fn end_canary(&self, id: CanaryId, why: CanaryEnd) -> Result<CanaryRow, Failure> {
        let (row, (at_once, freed)) = self
            .store()
            .write(Durability::Synced, move |tx| {
                let row = get(tx, id)?;
                if row.state == CanaryState::Ended {
                    return Ok((row, (false, false)));
                }
                let cancels = end_in(tx, &row, why)?;
                Ok((get(tx, id)?, cancels))
            })
            .await?;
        if at_once || freed {
            self.after_cancels(at_once, freed).await;
        }
        self.wake();
        Ok(row)
    }

    /// Looks over every canary, in one transaction: ends each whose end has
    /// come, and each whose target has another published version, letting
    /// a check under way go; counts the due times of a confirmed canary that
    /// passed with no check; and answers the checks due now. An asked
    /// canary has no check due, ever.
    ///
    /// # Errors
    ///
    /// `busy` failures of the store.
    pub async fn due_canaries(&self) -> Result<Vec<DueCanary>, Failure> {
        let (due, at_once, freed) = self
            .store()
            .write(Durability::Synced, move |tx| {
                let now = tx.now();
                let (mut at_once, mut freed) = (false, false);
                let mut due = Vec::new();
                for row in open(tx)? {
                    let current = tx.versions().current(&row.workspace, &row.target)?;
                    let why = if now >= row.ends_at {
                        Some(CanaryEnd::EndReached)
                    } else if current != Some(row.target_version) {
                        Some(CanaryEnd::VersionChanged)
                    } else {
                        None
                    };
                    if let Some(why) = why {
                        // The due times that passed before the end are
                        // counted as missed.
                        if why == CanaryEnd::EndReached
                            && row.state == CanaryState::Confirmed
                            && let Some(times) = row.times()
                            && let DuePlan::EndReached {
                                missed: Some((_, _, missed)),
                            } = plan_times(&times, now)
                        {
                            add_missed(tx, row.id, missed.count, None)?;
                        }
                        let (cancelled, let_go) = end_in(tx, &row, why)?;
                        at_once |= cancelled;
                        freed |= let_go;
                        continue;
                    }
                    if row.state != CanaryState::Confirmed {
                        continue;
                    }
                    let Some(times) = row.times() else {
                        continue;
                    };
                    match plan_times(&times, now) {
                        DuePlan::Waiting { .. } | DuePlan::EndReached { .. } => {}
                        DuePlan::Due(at) => due.push(DueCanary {
                            canary: row,
                            due: at,
                        }),
                        DuePlan::Missed {
                            until,
                            missed,
                            then,
                            ..
                        } => {
                            add_missed(tx, row.id, missed.count, Some(until))?;
                            due.push(DueCanary {
                                canary: get(tx, row.id)?,
                                due: then,
                            });
                        }
                    }
                }
                Ok((due, at_once, freed))
            })
            .await?;
        if at_once || freed {
            self.after_cancels(at_once, freed).await;
        }
        Ok(due)
    }

    /// The earliest moment at which [`Scheduler::due_canaries`] has
    /// something to do: a confirmed canary's next due time, or the end of
    /// any canary not ended. `None` when no canary is open.
    ///
    /// # Errors
    ///
    /// `busy` failures of the store.
    pub async fn next_canary_due(&self) -> Result<Option<Timestamp>, Failure> {
        self.store()
            .read(move |tx| {
                let now = tx.now();
                let mut earliest: Option<Timestamp> = None;
                for row in open(tx)? {
                    let at = match (row.state, row.times()) {
                        (CanaryState::Confirmed, Some(times)) => match plan_times(&times, now) {
                            DuePlan::Waiting { at } => at,
                            _ => now,
                        },
                        _ => row.ends_at.max(now),
                    };
                    earliest = Some(earliest.map_or(at, |earliest: Timestamp| earliest.min(at)));
                }
                Ok(earliest)
            })
            .await
    }

    /// Admits a due check as a run of the canary's read and begins it, in
    /// one transaction: the job is `batch`, whatever the submission said,
    /// and writes nothing; its budgets leave their reserve to others; it
    /// spends the share of its surface. A refusal to wait out keeps nothing,
    /// and the canary is not tried again before the moment the answer gives,
    /// never past its interval.
    ///
    /// # Errors
    ///
    /// `input`/`schema_invalid` for a submission that writes or for a canary
    /// that is not confirmed; any refusal of admission not waited out.
    pub async fn submit_canary(
        &self,
        check: &DueCanary,
        submission: Submission,
    ) -> Result<CanaryAdmission, Failure> {
        if submission.job.writes || submission.run.effect != EffectClass::Read {
            return Err(failure::invalid("a canary's check that writes"));
        }
        let settings = *self.settings();
        let (id, due) = (check.canary.id, check.due);
        let submission = Submission {
            priority: Priority::Batch,
            ..submission
        };
        let admitted = self
            .store()
            .write(Durability::Relaxed, move |tx| {
                let row = get(tx, id)?;
                if row.state != CanaryState::Confirmed {
                    return Err(failure::invalid("a canary that is not confirmed"));
                }
                if submission.run.target_version != row.target_version
                    || submission.run.tool_name != row.operation
                    || submission.run.input_hash != row.input_hash
                    || submission.run.principal != row.acts_for
                {
                    return Err(failure::invalid("a run that is not the canary's own read"));
                }
                let share = share_key(tx, &settings.default_rate, &settings.watch, &row)?;
                let (run, job) = match tables::admit(
                    tx,
                    &settings,
                    submission,
                    settings.watch.reserve_percent,
                    Some(share),
                )? {
                    Admitted::New { run, job } => (run, Some(job)),
                    Admitted::Existing { run } => (run, None),
                };
                tx.tables(Owner::Sched).execute(
                    "UPDATE canaries SET state = 'running', running_run = ?2, opened_run = ?3,
                         next_due_index = ?4, not_before = NULL
                     WHERE id = ?1 AND state = 'confirmed'",
                    &[
                        SqlValue::from(id.to_string()),
                        SqlValue::from(run.to_string()),
                        SqlValue::from(i64::from(job.is_some())),
                        int(due.index.saturating_add(1)),
                    ],
                )?;
                Ok((run, job))
            })
            .await;
        match admitted {
            Ok((run, job)) => {
                self.wake();
                Ok(CanaryAdmission::Started { run, job })
            }
            Err(refused) if ADMISSION_WAITS.contains(refused.code()) => {
                let wait = match refused.retry_after_ms() {
                    Some(ms) if refused.code() == &crate::failure::RATE_LIMITED => {
                        Duration::from_millis(ms)
                    }
                    _ => settings.lease_length,
                }
                .min(check.canary.interval);
                let until = tables::plus(self.now(), wait);
                self.store()
                    .write(Durability::Relaxed, move |tx| {
                        tx.tables(Owner::Sched).execute(
                            "UPDATE canaries SET not_before = ?2 WHERE id = ?1",
                            &[
                                SqlValue::from(id.to_string()),
                                SqlValue::from(until.as_unix_millis()),
                            ],
                        )?;
                        Ok(())
                    })
                    .await?;
                Ok(CanaryAdmission::Deferred { refused, until })
            }
            Err(refused) => Err(refused),
        }
    }

    /// Puts off a canary's next try until `until`, after a refusal its
    /// caller does not wait out.
    ///
    /// # Errors
    ///
    /// `busy` failures of the store.
    pub async fn defer_canary(&self, id: CanaryId, until: Timestamp) -> Result<(), Failure> {
        self.store()
            .write(Durability::Relaxed, move |tx| {
                tx.tables(Owner::Sched).execute(
                    "UPDATE canaries SET not_before = ?2 WHERE id = ?1 AND state = 'confirmed'",
                    &[
                        SqlValue::from(id.to_string()),
                        SqlValue::from(until.as_unix_millis()),
                    ],
                )?;
                Ok(())
            })
            .await
    }

    /// Ends the check under way of `id` on `run`, with what it found: the
    /// canary is confirmed again, and goes on at its interval whatever the
    /// outcome. When the run rests, waiting for a person or stopped by a
    /// drift, and the check opened it, the run is cancelled in the same
    /// transaction: a canary asks nothing of anyone, and its next check is a
    /// run of its own. A check that is no longer under way on `run`
    /// changes nothing. The row as it is after.
    ///
    /// # Errors
    ///
    /// `input`/`not_found`; `busy` failures of the store.
    pub async fn end_canary_check(
        &self,
        id: CanaryId,
        run: RunId,
        outcome: CanaryOutcome,
        failure: Option<(FailureClass, FailureCode)>,
    ) -> Result<CanaryRow, Failure> {
        let (row, at_once, freed) = self
            .store()
            .write(Durability::Synced, move |tx| {
                let row = get(tx, id)?;
                if row.state != CanaryState::Running || row.running_run != Some(run) {
                    return Ok((row, false, false));
                }
                let (mut at_once, mut freed) = (false, false);
                // A run that rests is let go, whatever it rests for, such as
                // a drift that stopped it: a later check is a run of its own.
                if row.opened_run && !tx.ledger().run(run)?.state.is_final() {
                    let (cancelled, let_go) = tables::cancel(tx, run)?;
                    at_once = cancelled == CancelOutcome::Cancelled;
                    freed = let_go;
                }
                let (class, code) = match &failure {
                    Some((class, code)) => (
                        SqlValue::from(class.as_str()),
                        SqlValue::from(code.as_str()),
                    ),
                    None => (SqlValue::Null, SqlValue::Null),
                };
                let now = tx.now();
                tx.tables(Owner::Sched).execute(
                    "UPDATE canaries SET state = 'confirmed', running_run = NULL, opened_run = 0,
                         checks = checks + 1, outcome = ?2, outcome_class = ?3,
                         outcome_code = ?4, outcome_at = ?5
                     WHERE id = ?1 AND state = 'running'",
                    &[
                        SqlValue::from(id.to_string()),
                        SqlValue::from(outcome.as_str()),
                        class,
                        code,
                        SqlValue::from(now.as_unix_millis()),
                    ],
                )?;
                Ok((get(tx, id)?, at_once, freed))
            })
            .await?;
        if at_once || freed {
            self.after_cancels(at_once, freed).await;
        }
        self.wake();
        Ok(row)
    }
}

fn add_missed(
    tx: &mut Tx<'_>,
    id: CanaryId,
    missed: u64,
    next: Option<u64>,
) -> Result<(), Failure> {
    match next {
        Some(next) => tx.tables(Owner::Sched).execute(
            "UPDATE canaries SET missed = missed + ?2, next_due_index = ?3 WHERE id = ?1",
            &[SqlValue::from(id.to_string()), int(missed), int(next)],
        )?,
        None => tx.tables(Owner::Sched).execute(
            "UPDATE canaries SET missed = missed + ?2 WHERE id = ?1",
            &[SqlValue::from(id.to_string()), int(missed)],
        )?,
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_identifier_reads_back_and_words_round_trip() {
        let id = CanaryId::from_bytes([7; 16]);
        let text = id.to_string();
        assert!(text.starts_with("canary_"));
        assert_eq!(text.parse::<CanaryId>().unwrap(), id);
        assert!("watch_07".parse::<CanaryId>().is_err());
        for word in CanaryState::NAMES {
            assert_eq!(CanaryState::parse(word).unwrap().as_str(), *word);
        }
        for word in CanaryEnd::NAMES {
            assert_eq!(CanaryEnd::parse(word).unwrap().as_str(), *word);
        }
        for word in CanaryOutcome::NAMES {
            assert_eq!(CanaryOutcome::parse(word).unwrap().as_str(), *word);
        }
    }
}
