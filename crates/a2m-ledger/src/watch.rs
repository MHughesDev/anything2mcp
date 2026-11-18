//! Watches: one published read with a fixed input, repeated on a schedule a
//! person confirmed, and what changed between its checks (decision 0247,
//! which builds the ledger's half of decision 0234).
//!
//! The ledger keeps what a watch is, each check, each change, and the
//! digests the next check is compared with. It keeps no value of what a
//! check read: a check's values stay in its run's result, kept like any
//! run's, and reach this crate only to be reduced to keyed digests inside
//! the transaction that ends the check. The ledger never starts a run: a
//! check is a run the scheduler admitted, and the ledger takes it only when
//! it is the watch's own read.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use a2m_core::{
    Digest, EffectClass, Environment, Failure, FailureClass, FailureCode, OperationName,
    PrincipalId, RecordKey, RunId, RunState, TargetName, Timestamp, Values, WorkspaceId,
};
use serde_json::{Map, Value as Json};

use crate::blob::{BlobRef, RetentionClass, words};
use crate::codec::{count, moment, parse, parse_opt, stored};
use crate::failure;
use crate::tx::{Row, SqlValue, Tx};
use crate::watch_compare::{
    Baseline, CheckDigests, CheckFault, Comparison, Held, RecordDigests, WatchKey, compare,
};

/// A watch was asked for an operation that is not a read. Class `policy`.
pub const WATCH_NOT_A_READ: FailureCode = FailureCode::fixed("watch_not_a_read");
/// A check was asked of a watch its person has not confirmed. Class
/// `policy`.
pub const WATCH_NOT_CONFIRMED: FailureCode = FailureCode::fixed("watch_not_confirmed");
/// Someone other than the person a watch acts for tried to confirm it.
/// Class `policy`.
pub const WATCH_NOT_YOURS: FailureCode = FailureCode::fixed("watch_not_yours");
/// A check of the watch is under way already. Class `busy`.
pub const WATCH_CHECK_RUNNING: FailureCode = FailureCode::fixed("watch_check_running");
/// The watch is paused. Class `busy`.
pub const WATCH_PAUSED: FailureCode = FailureCode::fixed("watch_paused");
/// The watch has ended. Class `policy`.
pub const WATCH_ENDED: FailureCode = FailureCode::fixed("watch_ended");
/// A run that is not the watch's own read was offered as its check. Class
/// `policy`.
pub const WATCH_RUN_NOT_ITS_OWN: FailureCode = FailureCode::fixed("watch_run_not_its_own");
/// A paused watch cannot go on without a person confirming a new one: its
/// version changed or its read drifted. Class `policy`.
pub const WATCH_NEEDS_CONFIRMATION: FailureCode = FailureCode::fixed("watch_needs_confirmation");
/// A check's run has not reached an end the watch can take. Class `busy`.
pub const WATCH_CHECK_UNFINISHED: FailureCode = FailureCode::fixed("watch_check_unfinished");
/// Due times passed with no check: the engine was not running, the watch
/// was paused or held, or a check could not be admitted in time. The code of
/// a `gap`, class `busy`.
pub const WATCH_DUE_MISSED: FailureCode = FailureCode::fixed("watch_due_missed");

/// A check's run ended without completing and gave no failure: it was
/// cancelled, or it expired. Class `busy`.
pub const WATCH_CHECK_NOT_COMPLETED: FailureCode = FailureCode::fixed("watch_check_not_completed");

/// Every failure code of a watch the ledger produces, with its one class.
pub const ALL: &[(FailureClass, &str)] = &[
    (FailureClass::Policy, "watch_not_a_read"),
    (FailureClass::Policy, "watch_not_confirmed"),
    (FailureClass::Policy, "watch_not_yours"),
    (FailureClass::Busy, "watch_check_running"),
    (FailureClass::Busy, "watch_paused"),
    (FailureClass::Policy, "watch_ended"),
    (FailureClass::Policy, "watch_run_not_its_own"),
    (FailureClass::Policy, "watch_needs_confirmation"),
    (FailureClass::Busy, "watch_check_unfinished"),
    (FailureClass::Busy, "watch_due_missed"),
    (FailureClass::Busy, "watch_check_not_completed"),
    (FailureClass::UnknownVariant, "watch_key_repeated"),
    (FailureClass::UnknownVariant, "watch_shapes_mixed"),
];

words! {
    /// Why a watch paused or ended: a state of the watch, not a failure.
    /// Where a check's failure lies behind it, the row keeps that failure's
    /// class and code beside it.
    WatchReason {
        /// A check found the session needs its person to sign in.
        NeedsLogin => "needs_login",
        /// A check's read needs input the watch cannot give.
        NeedsInput => "needs_input",
        /// A check's read drifted. Only a new watch, confirmed again, goes
        /// on.
        Drifted => "drifted",
        /// The target's published version is no longer the one it was
        /// confirmed for. Only a new watch, confirmed again, goes on.
        VersionChanged => "version_changed",
        /// A kill switch covers its target.
        OnHold => "on_hold",
        /// Its person paused it.
        PausedByPerson => "paused_by_person",
        /// Its person ended it.
        EndedByPerson => "ended_by_person",
        /// The caller that asked for it ended it.
        EndedByCaller => "ended_by_caller",
        /// It reached the end it was asked with.
        EndReached => "end_reached",
        /// Its read's version was withdrawn.
        VersionWithdrawn => "version_withdrawn",
        /// Its person declined it before confirming.
        Declined => "declined",
    }
}

impl WatchReason {
    /// Whether a watch paused for this reason may go on: never after its
    /// read changed or drifted.
    #[must_use]
    pub const fn may_resume(self) -> bool {
        !matches!(self, Self::Drifted | Self::VersionChanged)
    }
}

/// One watch.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WatchId([u8; 16]);

impl WatchId {
    /// What the identifier starts with when written, before the underscore.
    pub const PREFIX: &'static str = "watch";

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

impl std::fmt::Display for WatchId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}_", Self::PREFIX)?;
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for WatchId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WatchId({self})")
    }
}

impl std::str::FromStr for WatchId {
    type Err = Failure;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        // The same shape as every other identifier, under its own prefix.
        let rest = text
            .strip_prefix("watch_")
            .ok_or_else(|| failure::invalid("an identifier of a watch"))?;
        let as_run: RunId = format!("{}_{rest}", RunId::PREFIX)
            .parse()
            .map_err(|_| failure::invalid("an identifier of a watch"))?;
        Ok(Self(*as_run.as_bytes()))
    }
}

words! {
    /// Where a watch stands.
    WatchState {
        /// An agent asked for it; it does nothing until its person confirms.
        Asked => "asked",
        /// Its person confirmed it; it waits for its next due time.
        Confirmed => "confirmed",
        /// One check of it is under way.
        Running => "running",
        /// It waits for something its row names: a sign-in, a released hold,
        /// or a person.
        Paused => "paused",
        /// It ended. Final.
        Ended => "ended",
    }
}

words! {
    /// How a check ended.
    CheckOutcome {
        /// Under way.
        Running => "running",
        /// Its run completed and the read returned every record.
        Complete => "complete",
        /// Its run completed and the read said more records exist than it
        /// returned.
        CutShort => "cut_short",
        /// Its run did not complete.
        Failed => "failed",
        /// Its run completed and what it returned cannot be compared.
        Faulted => "faulted",
    }
}

words! {
    /// What a watch tells.
    ChangeKind {
        /// Its person confirmed it.
        Confirmed => "confirmed",
        /// A record the watch had not seen.
        Added => "added",
        /// A record seen before whose values differ.
        Changed => "changed",
        /// A record seen before that a complete check did not see, after a
        /// complete check.
        Removed => "removed",
        /// A record the last check saw and this one did not, where removal
        /// is not proven.
        Unseen => "unseen",
        /// A check that did not compare, or due times that passed with no
        /// check.
        Gap => "gap",
        /// It paused.
        Paused => "paused",
        /// It went on after a pause.
        Resumed => "resumed",
        /// It ended.
        Ended => "ended",
    }
}

/// A watch an agent asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewWatch {
    /// The workspace.
    pub workspace: WorkspaceId,
    /// The target.
    pub target: TargetName,
    /// The environment.
    pub environment: Environment,
    /// The published read.
    pub operation: OperationName,
    /// The published version: its plan's hash.
    pub target_version: Digest,
    /// The operation's compiled effect class. Anything but `read` is
    /// refused.
    pub effect: EffectClass,
    /// The digest of the fixed input, as a run's input hash is made.
    pub input_hash: Digest,
    /// The fixed input, stored as a run's input is.
    pub input: Option<BlobRef>,
    /// Outputs left out of the digests, each checked against the output
    /// schema by the caller.
    pub ignore: BTreeSet<String>,
    /// The time between due times: whole seconds, at least one.
    pub interval: Duration,
    /// When it ends at the latest.
    pub ends_at: Timestamp,
    /// The caller that asked.
    pub asked_by: PrincipalId,
    /// The person whose watch it is: whose session its checks use and who
    /// alone confirms it.
    pub acts_for: PrincipalId,
    /// Which client asked. A label, never a decision.
    pub client_label: String,
    /// How long its history is kept: the class its read's runs take.
    pub retention: RetentionClass,
}

/// A watch, as the store holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchRow {
    /// The watch.
    pub id: WatchId,
    /// The workspace.
    pub workspace: WorkspaceId,
    /// The target.
    pub target: TargetName,
    /// The environment.
    pub environment: Environment,
    /// The read.
    pub operation: OperationName,
    /// The version it was asked and confirmed for.
    pub target_version: Digest,
    /// The digest of the fixed input.
    pub input_hash: Digest,
    /// The fixed input.
    pub input: Option<BlobRef>,
    /// Outputs left out of the digests.
    pub ignore: BTreeSet<String>,
    /// The time between due times.
    pub interval: Duration,
    /// When it ends at the latest.
    pub ends_at: Timestamp,
    /// Who asked.
    pub asked_by: PrincipalId,
    /// Whose watch it is.
    pub acts_for: PrincipalId,
    /// Which client asked.
    pub client_label: String,
    /// When it was asked for.
    pub asked_at: Timestamp,
    /// How long its history is kept.
    pub retention: RetentionClass,
    /// Who confirmed it, and when.
    pub confirmed: Option<(PrincipalId, Timestamp)>,
    /// Its fixed offset from the moment of confirmation, set then.
    pub phase: Option<Duration>,
    /// Where it stands.
    pub state: WatchState,
    /// Why it paused or ended.
    pub reason: Option<WatchReason>,
    /// The failure of the check that made it pause, when one did.
    pub reason_failure: Option<(FailureClass, FailureCode)>,
    /// Since when it stands so.
    pub state_since: Timestamp,
    /// The index of the next due time not yet handled.
    pub next_due_index: u64,
    /// No check is begun before this moment: a refused admission's retry
    /// time.
    pub not_before: Option<Timestamp>,
    /// The check under way.
    pub running_check: Option<u64>,
    /// How many checks it has begun.
    pub checks: u64,
    /// The last check whose digests the baseline took.
    pub baseline_through: Option<u64>,
    /// Whether a complete check is in the baseline.
    pub baseline_complete: bool,
}

impl WatchRow {
    /// Whether it is open: confirmed, running, or paused.
    #[must_use]
    pub fn is_open(&self) -> bool {
        matches!(
            self.state,
            WatchState::Confirmed | WatchState::Running | WatchState::Paused
        )
    }
}

/// A due time a check is begun for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Due {
    /// Its index: due time `n` is the moment of confirmation, plus the
    /// phase, plus `n` intervals.
    pub index: u64,
    /// Its moment.
    pub at: Timestamp,
    /// Whether it is the one check made after due times were missed.
    pub catch_up: bool,
}

/// One check, as the store holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRow {
    /// The watch.
    pub watch: WatchId,
    /// Its number in the watch, from 0.
    pub seq: u64,
    /// The due time it was for.
    pub due: Due,
    /// The run of the read.
    pub run: RunId,
    /// When it began.
    pub started_at: Timestamp,
    /// How it ended.
    pub outcome: CheckOutcome,
    /// When it ended.
    pub ended_at: Option<Timestamp>,
    /// For a failed or faulted check, what stopped it.
    pub why: Option<(FailureClass, FailureCode)>,
    /// How many records it saw.
    pub records_seen: Option<u64>,
    /// The counts its comparison gave: added, changed, removed, unseen,
    /// unchanged. `None` when it was not compared.
    pub counts: Option<ChangeCounts>,
}

/// The counts of one comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChangeCounts {
    /// Records added.
    pub added: u64,
    /// Records changed.
    pub changed: u64,
    /// Records removed.
    pub removed: u64,
    /// Records unseen.
    pub unseen: u64,
    /// Records unchanged, counted and not listed.
    pub unchanged: u64,
}

/// Due times that passed with no check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Missed {
    /// The first.
    pub first: Timestamp,
    /// The last.
    pub last: Timestamp,
    /// How many.
    pub count: u64,
}

/// One change, as the store holds it. It holds no value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeRow {
    /// Its number, from the store's one sequence of changes.
    pub seq: u64,
    /// The watch.
    pub watch: WatchId,
    /// What it tells.
    pub kind: ChangeKind,
    /// The check it was found by.
    pub check: Option<u64>,
    /// The check that last saw the record before.
    pub older_check: Option<u64>,
    /// The record.
    pub key: Option<WatchKey>,
    /// For `changed`, the outputs that differ, by name.
    pub outputs: Vec<String>,
    /// For a gap, and a pause a check's failure made: the failure's class
    /// and code.
    pub why: Option<(FailureClass, FailureCode)>,
    /// For a pause or an end: why.
    pub reason: Option<WatchReason>,
    /// For a gap of due times that passed: which.
    pub missed: Option<Missed>,
    /// When it was recorded.
    pub at: Timestamp,
}

/// One record a check's read returned, as its run's result holds it after
/// normalization. Reduced to digests in the transaction that ends the check
/// and never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedRecord {
    /// Its key; `None` for a read compared whole.
    pub key: Option<RecordKey>,
    /// Its normalized values, by output name.
    pub values: Values,
}

/// What a completed check's read returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    /// The records, in any order.
    pub records: Vec<ObservedRecord>,
    /// Whether the read returned every record that matches its input: no
    /// `more`, the pager's last page. The caller says so only where the
    /// read reports it; otherwise it is false, and no removal is reported.
    pub complete: bool,
}

/// What ending a check gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckEnded {
    /// The check, as it ended.
    pub check: CheckRow,
    /// The watch's state after it.
    pub state: WatchState,
    /// The comparison, when there was one: the first check of a watch makes
    /// its baseline and is compared with nothing.
    pub comparison: Option<Comparison>,
}

/// Watches, inside one transaction.
pub struct Watches<'t, 'a> {
    tx: &'t mut Tx<'a>,
}

impl<'a> Tx<'a> {
    /// Watches, their checks, and their changes.
    pub fn watches(&mut self) -> Watches<'_, 'a> {
        Watches { tx: self }
    }
}

fn refused(class: FailureClass, code: FailureCode, what: &str, next: &str) -> Failure {
    Failure::new(class, code, what.to_owned(), next.to_owned())
}

fn state_refusal(state: WatchState) -> Failure {
    match state {
        WatchState::Asked => refused(
            FailureClass::Policy,
            WATCH_NOT_CONFIRMED,
            "The watch has not been confirmed by its person, so it does nothing.",
            "Ask the person to confirm the watch on their own page.",
        ),
        WatchState::Running => refused(
            FailureClass::Busy,
            WATCH_CHECK_RUNNING,
            "A check of the watch is under way; a watch never runs two at once.",
            "Wait for the check to end.",
        ),
        WatchState::Paused => refused(
            FailureClass::Busy,
            WATCH_PAUSED,
            "The watch is paused.",
            "See the watch's changes for why; it goes on when that is settled.",
        ),
        WatchState::Confirmed | WatchState::Ended => refused(
            FailureClass::Policy,
            WATCH_ENDED,
            "The watch has ended.",
            "Ask for a new watch.",
        ),
    }
}

fn split(failure: Option<&(FailureClass, FailureCode)>) -> (Option<String>, Option<String>) {
    match failure {
        Some((class, code)) => (
            Some(class.as_str().to_owned()),
            Some(code.as_str().to_owned()),
        ),
        None => (None, None),
    }
}

fn failure_of(
    class: Option<&str>,
    code: Option<&str>,
) -> Result<Option<(FailureClass, FailureCode)>, Failure> {
    match (class, code) {
        (Some(class), Some(code)) => Ok(Some((
            parse(class)?,
            FailureCode::new(code).map_err(|_| failure::corrupt("a stored code"))?,
        ))),
        (None, None) => Ok(None),
        _ => Err(failure::corrupt(
            "a stored failure with no class or no code",
        )),
    }
}

fn names_json(names: &BTreeSet<String>) -> Result<String, Failure> {
    serde_json::to_string(names).map_err(|_| failure::invalid("the names of outputs"))
}

fn read_names(text: &str) -> Result<Vec<String>, Failure> {
    serde_json::from_str(text)
        .map_err(|_| failure::corrupt("stored output names that no longer read"))
}

fn digests_json(outputs: &BTreeMap<String, Digest>) -> String {
    let map: Map<String, Json> = outputs
        .iter()
        .map(|(name, digest)| (name.clone(), Json::String(digest.to_string())))
        .collect();
    Json::Object(map).to_string()
}

fn read_digests(text: &str) -> Result<BTreeMap<String, Digest>, Failure> {
    let parsed: BTreeMap<String, String> = serde_json::from_str(text)
        .map_err(|_| failure::corrupt("stored digests that no longer read"))?;
    parsed
        .into_iter()
        .map(|(name, digest)| Ok((name, parse(&digest)?)))
        .collect()
}

fn whole_seconds(interval: Duration) -> Result<i64, Failure> {
    if interval.subsec_nanos() != 0 || interval.as_secs() == 0 {
        return Err(failure::invalid(
            "an interval of a watch that is not a whole number of seconds, at least one",
        ));
    }
    i64::try_from(interval.as_secs()).map_err(|_| failure::invalid("an interval too long"))
}

fn millis(duration: Duration) -> Result<i64, Failure> {
    i64::try_from(duration.as_millis()).map_err(|_| failure::invalid("a duration too long"))
}

/// The columns [`watch_row`] reads, in its order.
macro_rules! watch_columns {
    () => {
        "id, workspace_id, target, environment, operation, target_version_hash, input_hash,
         input_ref, ignore_outputs, interval_seconds, ends_at, asked_by, acts_for, client_label,
         asked_at, retention_class, confirmed_by, confirmed_at, phase_ms, state, reason,
         reason_failure_class, reason_failure_code, state_since, next_due_index, not_before, running_check, checks,
         baseline_through, baseline_complete"
    };
}

fn watch_row(row: &Row) -> Result<WatchRow, Failure> {
    let confirmed = match (row.opt_text(16)?, row.opt_int(17)?) {
        (Some(by), Some(at)) => Some((parse(by)?, moment(at))),
        (None, None) => None,
        _ => {
            return Err(failure::corrupt(
                "a watch's confirmation that no longer reads",
            ));
        }
    };
    let reason = parse_opt(row.opt_text(20)?)?;
    let reason_failure = failure_of(row.opt_text(21)?, row.opt_text(22)?)?;
    let names = read_names(row.text(8)?)?;
    Ok(WatchRow {
        id: parse(row.text(0)?)?,
        workspace: parse(row.text(1)?)?,
        target: parse(row.text(2)?)?,
        environment: parse(row.text(3)?)?,
        operation: parse(row.text(4)?)?,
        target_version: parse(row.text(5)?)?,
        input_hash: parse(row.text(6)?)?,
        input: parse_opt(row.opt_text(7)?)?,
        ignore: names.into_iter().collect(),
        interval: Duration::from_secs(count(row.int(9)?)?),
        ends_at: moment(row.int(10)?),
        asked_by: parse(row.text(11)?)?,
        acts_for: parse(row.text(12)?)?,
        client_label: row.text(13)?.to_owned(),
        asked_at: moment(row.int(14)?),
        retention: parse(row.text(15)?)?,
        confirmed,
        phase: row
            .opt_int(18)?
            .map(|ms| count(ms).map(Duration::from_millis))
            .transpose()?,
        state: parse(row.text(19)?)?,
        reason,
        reason_failure,
        state_since: moment(row.int(23)?),
        next_due_index: count(row.int(24)?)?,
        not_before: row.opt_int(25)?.map(moment),
        running_check: row.opt_int(26)?.map(count).transpose()?,
        checks: count(row.int(27)?)?,
        baseline_through: row.opt_int(28)?.map(count).transpose()?,
        baseline_complete: row.int(29)? != 0,
    })
}

/// The columns [`check_row`] reads, in its order.
macro_rules! check_columns {
    () => {
        "watch_id, seq, due_index, due_at, catch_up, run_id, started_at, outcome, ended_at,
         why_class, why_code, records_seen, added, changed, removed, unseen, unchanged"
    };
}

fn check_row(row: &Row) -> Result<CheckRow, Failure> {
    let why = match (row.opt_text(9)?, row.opt_text(10)?) {
        (Some(class), Some(code)) => Some((
            parse(class)?,
            FailureCode::new(code).map_err(|_| failure::corrupt("a stored code"))?,
        )),
        _ => None,
    };
    let counts = match row.opt_int(12)? {
        Some(added) => Some(ChangeCounts {
            added: count(added)?,
            changed: count(row.int(13)?)?,
            removed: count(row.int(14)?)?,
            unseen: count(row.int(15)?)?,
            unchanged: count(row.int(16)?)?,
        }),
        None => None,
    };
    Ok(CheckRow {
        watch: parse(row.text(0)?)?,
        seq: count(row.int(1)?)?,
        due: Due {
            index: count(row.int(2)?)?,
            at: moment(row.int(3)?),
            catch_up: row.int(4)? != 0,
        },
        run: parse(row.text(5)?)?,
        started_at: moment(row.int(6)?),
        outcome: parse(row.text(7)?)?,
        ended_at: row.opt_int(8)?.map(moment),
        why,
        records_seen: row.opt_int(11)?.map(count).transpose()?,
        counts,
    })
}

fn change_row(row: &Row) -> Result<ChangeRow, Failure> {
    let why = failure_of(row.opt_text(7)?, row.opt_text(8)?)?;
    let reason = parse_opt(row.opt_text(9)?)?;
    let missed = match (row.opt_int(10)?, row.opt_int(11)?, row.opt_int(12)?) {
        (Some(first), Some(last), Some(missed)) => Some(Missed {
            first: moment(first),
            last: moment(last),
            count: count(missed)?,
        }),
        _ => None,
    };
    Ok(ChangeRow {
        seq: count(row.int(0)?)?,
        watch: parse(row.text(1)?)?,
        kind: parse(row.text(2)?)?,
        check: row.opt_int(3)?.map(count).transpose()?,
        older_check: row.opt_int(4)?.map(count).transpose()?,
        key: row
            .opt_text(5)?
            .map(|text| {
                WatchKey::from_stored(text)
                    .ok_or_else(|| failure::corrupt("a stored record key that no longer reads"))
            })
            .transpose()?,
        outputs: row
            .opt_text(6)?
            .map(read_names)
            .transpose()?
            .unwrap_or_default(),
        why,
        reason,
        missed,
        at: moment(row.int(13)?),
    })
}

/// A change to write.
#[derive(Default)]
struct NewChange {
    check: Option<u64>,
    older_check: Option<u64>,
    key: Option<WatchKey>,
    outputs: Option<Vec<String>>,
    why: Option<(FailureClass, FailureCode)>,
    reason: Option<WatchReason>,
    missed: Option<Missed>,
}

/// What a check's run's row says about how the check ends, read from the
/// row and never from the caller's word.
enum RunEnd {
    Completed,
    Pause(WatchReason, (FailureClass, FailureCode)),
    Gap(FailureClass, FailureCode),
}

fn not_completed() -> (FailureClass, FailureCode) {
    (FailureClass::Busy, WATCH_CHECK_NOT_COMPLETED)
}

fn run_end(
    state: RunState,
    failure: Option<(FailureClass, FailureCode)>,
) -> Result<RunEnd, Failure> {
    match (state, failure) {
        (RunState::Completed, _) => Ok(RunEnd::Completed),
        (RunState::NeedsLogin, failure) => Ok(RunEnd::Pause(
            WatchReason::NeedsLogin,
            failure.unwrap_or_else(not_completed),
        )),
        (RunState::NeedsInput, failure) => Ok(RunEnd::Pause(
            WatchReason::NeedsInput,
            failure.unwrap_or_else(not_completed),
        )),
        (_, Some((FailureClass::SessionExpired, code))) => Ok(RunEnd::Pause(
            WatchReason::NeedsLogin,
            (FailureClass::SessionExpired, code),
        )),
        (_, Some((FailureClass::Drift, code))) => Ok(RunEnd::Pause(
            WatchReason::Drifted,
            (FailureClass::Drift, code),
        )),
        (
            RunState::Failed
            | RunState::Stopped
            | RunState::Rejected
            | RunState::Cancelled
            | RunState::Expired,
            failure,
        ) => {
            let (class, code) = failure.unwrap_or_else(not_completed);
            Ok(RunEnd::Gap(class, code))
        }
        _ => Err(refused(
            FailureClass::Busy,
            WATCH_CHECK_UNFINISHED,
            "The check's run has not ended.",
            "End the check when its run reaches a final state.",
        )),
    }
}

impl Watches<'_, '_> {
    fn load(&mut self, id: WatchId) -> Result<WatchRow, Failure> {
        let row = self
            .tx
            .row(
                concat!("SELECT ", watch_columns!(), " FROM watches WHERE id = ?1"),
                &[id.to_string().into()],
            )?
            .ok_or_else(|| failure::not_found("no watch with that identifier"))?;
        watch_row(&row)
    }

    fn change(
        &mut self,
        watch: WatchId,
        kind: ChangeKind,
        change: NewChange,
    ) -> Result<u64, Failure> {
        let outputs = change
            .outputs
            .map(|names| serde_json::to_string(&names))
            .transpose()
            .map_err(|_| failure::invalid("the names of outputs"))?;
        let (why_class, why_code) = split(change.why.as_ref());
        let missed_count = change
            .missed
            .map(|missed| stored(missed.count))
            .transpose()?;
        self.tx.exec(
            "INSERT INTO watch_changes
                (watch_id, kind, check_seq, older_check, key_digest, outputs, why_class, why_code,
                 reason, first_missed, last_missed, missed, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            &[
                watch.to_string().into(),
                kind.as_str().into(),
                change.check.map(stored).transpose()?.into(),
                change.older_check.map(stored).transpose()?.into(),
                change.key.map(|key| key.stored()).into(),
                outputs.into(),
                why_class.into(),
                why_code.into(),
                change.reason.map(WatchReason::as_str).into(),
                change.missed.map(|missed| missed.first).into(),
                change.missed.map(|missed| missed.last).into(),
                missed_count.into(),
                self.tx.now().into(),
            ],
        )?;
        let row = self
            .tx
            .row("SELECT last_insert_rowid()", &[])?
            .ok_or_else(|| failure::corrupt("a row that gave no number"))?;
        count(row.int(0)?)
    }

    /// Asks for a watch. It starts `asked` and does nothing until its person
    /// confirms it.
    ///
    /// # Errors
    ///
    /// `policy`/`watch_not_a_read` for an operation whose effect class is not
    /// `read`. `input`/`schema_invalid` for an interval that is not whole
    /// seconds, or an end not after now.
    pub fn ask(&mut self, new: &NewWatch) -> Result<WatchRow, Failure> {
        if new.effect != EffectClass::Read {
            return Err(refused(
                FailureClass::Policy,
                WATCH_NOT_A_READ,
                "Only a read can be watched; a watch never writes.",
                "Watch an operation whose effect class is `read`.",
            ));
        }
        let interval = whole_seconds(new.interval)?;
        let now = self.tx.now();
        if new.ends_at <= now {
            return Err(failure::invalid("a watch that ends before it starts"));
        }
        let id = WatchId(*RunId::from_parts(now, self.tx.random()?)?.as_bytes());
        let label = self.tx.shared.attached()?.redact.redact(&new.client_label);
        self.tx.exec(
            "INSERT INTO watches
                (id, workspace_id, target, environment, operation, target_version_hash, effect,
                 input_hash, input_ref, ignore_outputs, interval_seconds, ends_at, asked_by,
                 acts_for, client_label, asked_at, retention_class, state, state_since)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'read', ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                     ?16, 'asked', ?15)",
            &[
                id.to_string().into(),
                new.workspace.to_string().into(),
                new.target.as_str().into(),
                new.environment.as_str().into(),
                new.operation.as_str().into(),
                new.target_version.to_string().into(),
                new.input_hash.to_string().into(),
                new.input.into(),
                names_json(&new.ignore)?.into(),
                interval.into(),
                new.ends_at.into(),
                new.asked_by.to_string().into(),
                new.acts_for.to_string().into(),
                label.into(),
                now.into(),
                new.retention.as_str().into(),
            ],
        )?;
        self.load(id)
    }

    /// Records the confirmation of the watch's own person, with the fixed
    /// phase the scheduler chose. Its first due time is now plus the phase.
    ///
    /// # Errors
    ///
    /// `policy`/`watch_not_yours` when `by` is not the person it acts for;
    /// the refusal of its state when it is not `asked`; `input`/
    /// `schema_invalid` for a phase not shorter than the interval.
    pub fn confirm(
        &mut self,
        id: WatchId,
        by: PrincipalId,
        phase: Duration,
    ) -> Result<WatchRow, Failure> {
        let watch = self.load(id)?;
        match watch.state {
            WatchState::Asked => {}
            WatchState::Ended => return Err(state_refusal(WatchState::Ended)),
            _ => return Err(failure::invalid("a watch that is confirmed already")),
        }
        if by != watch.acts_for {
            return Err(refused(
                FailureClass::Policy,
                WATCH_NOT_YOURS,
                "Only the person a watch acts for confirms it.",
                "Ask that person to confirm it on their own page.",
            ));
        }
        if phase >= watch.interval {
            return Err(failure::invalid("a phase not shorter than the interval"));
        }
        let now = self.tx.now();
        self.tx.exec(
            "UPDATE watches SET confirmed_by = ?2, confirmed_at = ?3, phase_ms = ?4,
                    state = 'confirmed', state_since = ?3
             WHERE id = ?1",
            &[
                id.to_string().into(),
                by.to_string().into(),
                now.into(),
                millis(phase)?.into(),
            ],
        )?;
        self.change(id, ChangeKind::Confirmed, NewChange::default())?;
        self.load(id)
    }

    /// Begins a check for `due`, on `run`, which the scheduler admitted in
    /// the same transaction. The run must be the watch's own read: its
    /// operation, version, and input, for the person it acts for.
    ///
    /// # Errors
    ///
    /// The refusal of the watch's state unless it is `confirmed`;
    /// `policy`/`watch_run_not_its_own`; `input`/`schema_invalid` for a due
    /// time handled already.
    pub fn begin_check(&mut self, id: WatchId, due: Due, run: RunId) -> Result<CheckRow, Failure> {
        let watch = self.load(id)?;
        if watch.state != WatchState::Confirmed {
            return Err(state_refusal(watch.state));
        }
        if due.index < watch.next_due_index {
            return Err(failure::invalid("a due time that was handled already"));
        }
        let row = self.tx.ledger().run(run)?;
        let own = row.effect == EffectClass::Read
            && row.tool_name == watch.operation
            && row.target_version == watch.target_version
            && row.input_hash == watch.input_hash
            && row.principal == watch.acts_for
            && row.workspace == watch.workspace
            && row.target == watch.target
            && row.environment == watch.environment;
        if !own {
            return Err(refused(
                FailureClass::Policy,
                WATCH_RUN_NOT_ITS_OWN,
                "A watch's check is its own read, with its own input, for its own person, and this run is not.",
                "Nothing to do; a watch runs nothing else.",
            ));
        }
        let seq = watch.checks;
        let next = due
            .index
            .checked_add(1)
            .ok_or_else(|| failure::unexpected("the due times have run out"))?;
        let now = self.tx.now();
        self.tx.exec(
            "UPDATE watches SET state = 'running', state_since = ?2, running_check = ?3,
                    checks = ?4, next_due_index = ?5, not_before = NULL
             WHERE id = ?1",
            &[
                id.to_string().into(),
                now.into(),
                stored(seq)?.into(),
                stored(seq + 1)?.into(),
                stored(next)?.into(),
            ],
        )?;
        self.tx.exec(
            "INSERT INTO watch_checks
                (watch_id, seq, due_index, due_at, catch_up, run_id, started_at, outcome)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'running')",
            &[
                id.to_string().into(),
                stored(seq)?.into(),
                stored(due.index)?.into(),
                due.at.into(),
                due.catch_up.into(),
                run.to_string().into(),
                now.into(),
            ],
        )?;
        self.check(id, seq)
    }

    /// Ends the check under way. How it ends is read from its run's row:
    /// completed, it is compared, and `observed` must be given; waiting for
    /// a sign-in or input, or drifted, the watch pauses; any other end is a
    /// gap with the run's class and code. The values in `observed` are
    /// reduced to keyed digests here and never stored.
    ///
    /// # Errors
    ///
    /// `busy`/`watch_check_unfinished` while the run has not ended;
    /// `unknown_variant`/`ledger_state_unexpected` when `seq` is not the
    /// check under way; `input`/`schema_invalid` for a completed run with
    /// nothing observed.
    pub fn end_check(
        &mut self,
        id: WatchId,
        seq: u64,
        observed: Option<&Observed>,
    ) -> Result<CheckEnded, Failure> {
        let watch = self.load(id)?;
        if watch.state != WatchState::Running || watch.running_check != Some(seq) {
            return Err(failure::unexpected(
                "a check that is not the watch's check under way",
            ));
        }
        let check = self.check(id, seq)?;
        let run = self.tx.ledger().run(check.run)?;
        let end = run_end(run.state, run.failure.clone())?;
        let observed = match end {
            RunEnd::Completed => observed
                .ok_or_else(|| failure::invalid("a completed check with nothing observed"))?,
            RunEnd::Pause(reason, why) => {
                self.close_check(id, seq, CheckOutcome::Failed, Some(why.clone()), None, None)?;
                self.change(
                    id,
                    ChangeKind::Gap,
                    NewChange {
                        check: Some(seq),
                        why: Some(why.clone()),
                        ..NewChange::default()
                    },
                )?;
                self.leave_running(id, WatchState::Paused, Some(reason), Some(&why))?;
                self.change(
                    id,
                    ChangeKind::Paused,
                    NewChange {
                        why: Some(why),
                        reason: Some(reason),
                        ..NewChange::default()
                    },
                )?;
                return self.ended(id, seq, None);
            }
            RunEnd::Gap(class, code) => {
                self.close_check(
                    id,
                    seq,
                    CheckOutcome::Failed,
                    Some((class, code.clone())),
                    None,
                    None,
                )?;
                self.change(
                    id,
                    ChangeKind::Gap,
                    NewChange {
                        check: Some(seq),
                        why: Some((class, code)),
                        ..NewChange::default()
                    },
                )?;
                self.leave_running(id, WatchState::Confirmed, None, None)?;
                return self.ended(id, seq, None);
            }
        };
        let digests = {
            let digest = std::sync::Arc::clone(&self.tx.shared.attached()?.digest);
            let records = observed.records.iter().map(|record| {
                let key = match &record.key {
                    Some(key) => WatchKey::of(digest.as_ref(), &watch.workspace, key),
                    None => WatchKey::Whole,
                };
                (
                    key,
                    RecordDigests::of(
                        digest.as_ref(),
                        &watch.workspace,
                        &record.values,
                        &watch.ignore,
                    ),
                )
            });
            CheckDigests::new(observed.complete, records)
        };
        let digests = match digests {
            Ok(digests) => digests,
            Err(fault) => return self.faulted(id, seq, &fault),
        };
        let seen = u64::try_from(digests.len()).unwrap_or(u64::MAX);
        let outcome = if digests.complete() {
            CheckOutcome::Complete
        } else {
            CheckOutcome::CutShort
        };
        let (baseline, comparison) = match self.baseline(&watch)? {
            Some(older) => {
                let compared = compare(&older, &digests);
                (older.after(seq, &digests), Some(compared))
            }
            None => (Baseline::first(seq, &digests), None),
        };
        if let Some(compared) = &comparison {
            for key in &compared.added {
                self.change(
                    id,
                    ChangeKind::Added,
                    NewChange {
                        check: Some(seq),
                        key: Some(*key),
                        ..NewChange::default()
                    },
                )?;
            }
            for changed in &compared.changed {
                self.change(
                    id,
                    ChangeKind::Changed,
                    NewChange {
                        check: Some(seq),
                        older_check: Some(changed.older),
                        key: Some(changed.key),
                        outputs: Some(changed.outputs.clone()),
                        ..NewChange::default()
                    },
                )?;
            }
            for (kind, list) in [
                (ChangeKind::Removed, &compared.removed),
                (ChangeKind::Unseen, &compared.unseen),
            ] {
                for missing in list {
                    self.change(
                        id,
                        kind,
                        NewChange {
                            check: Some(seq),
                            older_check: Some(missing.older),
                            key: Some(missing.key),
                            ..NewChange::default()
                        },
                    )?;
                }
            }
        }
        self.store_baseline(&watch, &baseline)?;
        let counts = comparison.as_ref().map(|compared| ChangeCounts {
            added: compared.added.len() as u64,
            changed: compared.changed.len() as u64,
            removed: compared.removed.len() as u64,
            unseen: compared.unseen.len() as u64,
            unchanged: u64::from(compared.unchanged),
        });
        self.close_check(
            id,
            seq,
            outcome,
            None,
            Some(seen),
            Some(counts.unwrap_or_default()),
        )?;
        self.tx.exec(
            "UPDATE watches SET baseline_through = ?2, baseline_complete = ?3 WHERE id = ?1",
            &[
                id.to_string().into(),
                stored(seq)?.into(),
                baseline.complete.into(),
            ],
        )?;
        self.leave_running(id, WatchState::Confirmed, None, None)?;
        self.ended(id, seq, comparison)
    }

    fn faulted(
        &mut self,
        id: WatchId,
        seq: u64,
        fault: &CheckFault,
    ) -> Result<CheckEnded, Failure> {
        let failure = fault.failure();
        self.close_check(
            id,
            seq,
            CheckOutcome::Faulted,
            Some((failure.class(), failure.code().clone())),
            None,
            None,
        )?;
        self.change(
            id,
            ChangeKind::Gap,
            NewChange {
                check: Some(seq),
                why: Some((failure.class(), failure.code().clone())),
                ..NewChange::default()
            },
        )?;
        self.leave_running(id, WatchState::Confirmed, None, None)?;
        self.ended(id, seq, None)
    }

    fn ended(
        &mut self,
        id: WatchId,
        seq: u64,
        comparison: Option<Comparison>,
    ) -> Result<CheckEnded, Failure> {
        let check = self.check(id, seq)?;
        let state = self.load(id)?.state;
        Ok(CheckEnded {
            check,
            state,
            comparison,
        })
    }

    /// Ends the watch's check under way, if any, and moves it to `state`,
    /// in one statement, so the row never says `running` without a check.
    fn leave_running(
        &mut self,
        id: WatchId,
        state: WatchState,
        reason: Option<WatchReason>,
        failure: Option<&(FailureClass, FailureCode)>,
    ) -> Result<(), Failure> {
        let (class, code) = split(failure);
        self.tx.exec(
            "UPDATE watches SET running_check = NULL, state = ?2, reason = ?3,
                    reason_failure_class = ?4, reason_failure_code = ?5, state_since = ?6
             WHERE id = ?1",
            &[
                id.to_string().into(),
                state.as_str().into(),
                reason.map(WatchReason::as_str).into(),
                class.into(),
                code.into(),
                self.tx.now().into(),
            ],
        )?;
        Ok(())
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "one statement's columns, each named where it is called"
    )]
    fn close_check(
        &mut self,
        id: WatchId,
        seq: u64,
        outcome: CheckOutcome,
        why: Option<(FailureClass, FailureCode)>,
        seen: Option<u64>,
        counts: Option<ChangeCounts>,
    ) -> Result<(), Failure> {
        let (class, code) = match why {
            Some((class, code)) => (
                Some(class.as_str().to_owned()),
                Some(code.as_str().to_owned()),
            ),
            None => (None, None),
        };
        let count_of = |pick: fn(&ChangeCounts) -> u64| -> Result<SqlValue, Failure> {
            Ok(counts
                .as_ref()
                .map(|counts| stored(pick(counts)))
                .transpose()?
                .into())
        };
        self.tx.exec(
            "UPDATE watch_checks SET outcome = ?3, ended_at = ?4, why_class = ?5, why_code = ?6,
                    records_seen = ?7, added = ?8, changed = ?9, removed = ?10, unseen = ?11,
                    unchanged = ?12
             WHERE watch_id = ?1 AND seq = ?2",
            &[
                id.to_string().into(),
                stored(seq)?.into(),
                outcome.as_str().into(),
                self.tx.now().into(),
                class.into(),
                code.into(),
                seen.map(stored).transpose()?.into(),
                count_of(|counts| counts.added)?,
                count_of(|counts| counts.changed)?,
                count_of(|counts| counts.removed)?,
                count_of(|counts| counts.unseen)?,
                count_of(|counts| counts.unchanged)?,
            ],
        )?;
        Ok(())
    }

    /// The watch's baseline, when a check has made one.
    ///
    /// # Errors
    ///
    /// `busy`/`store_corrupt` for rows that no longer read.
    pub fn baseline(&mut self, watch: &WatchRow) -> Result<Option<Baseline>, Failure> {
        let Some(through) = watch.baseline_through else {
            return Ok(None);
        };
        let rows = self.tx.rows(
            "SELECT key_digest, record_digest, outputs, last_seen FROM watch_baseline
             WHERE watch_id = ?1 ORDER BY key_digest",
            &[watch.id.to_string().into()],
        )?;
        let mut records = BTreeMap::new();
        for row in rows {
            let key = WatchKey::from_stored(row.text(0)?)
                .ok_or_else(|| failure::corrupt("a stored record key that no longer reads"))?;
            records.insert(
                key,
                Held {
                    digests: RecordDigests {
                        whole: parse(row.text(1)?)?,
                        outputs: read_digests(row.text(2)?)?,
                    },
                    last_seen: count(row.int(3)?)?,
                },
            );
        }
        Ok(Some(Baseline {
            complete: watch.baseline_complete,
            through,
            records,
        }))
    }

    fn store_baseline(&mut self, watch: &WatchRow, baseline: &Baseline) -> Result<(), Failure> {
        // Only what changed is written: a record proven gone leaves, a
        // record seen now is replaced, a record not seen stays as it was.
        let id = watch.id.to_string();
        let held = self
            .baseline(watch)?
            .map(|older| older.records)
            .unwrap_or_default();
        for key in held
            .keys()
            .filter(|key| !baseline.records.contains_key(key))
        {
            self.tx.exec(
                "DELETE FROM watch_baseline WHERE watch_id = ?1 AND key_digest = ?2",
                &[id.clone().into(), key.stored().into()],
            )?;
        }
        for (key, record) in &baseline.records {
            if held.get(key) == Some(record) {
                continue;
            }
            self.tx.exec(
                "INSERT INTO watch_baseline (watch_id, key_digest, record_digest, outputs, last_seen)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (watch_id, key_digest) DO UPDATE
                 SET record_digest = ?3, outputs = ?4, last_seen = ?5",
                &[
                    id.clone().into(),
                    key.stored().into(),
                    record.digests.whole.to_string().into(),
                    digests_json(&record.digests.outputs).into(),
                    stored(record.last_seen)?.into(),
                ],
            )?;
        }
        Ok(())
    }

    /// Records due times that passed with no check, from `from` up to and
    /// not including `until`, as one gap. They are never checked after.
    ///
    /// # Errors
    ///
    /// The refusal of its state unless it is `confirmed` or `paused`;
    /// `input`/`schema_invalid` for a range that is empty or handled
    /// already.
    pub fn missed(
        &mut self,
        id: WatchId,
        from: u64,
        until: u64,
        missed: Missed,
    ) -> Result<u64, Failure> {
        let watch = self.load(id)?;
        if !matches!(watch.state, WatchState::Confirmed | WatchState::Paused) {
            return Err(state_refusal(watch.state));
        }
        if from != watch.next_due_index || until <= from || missed.count != until - from {
            return Err(failure::invalid(
                "missed due times that are not the next ones",
            ));
        }
        self.tx.exec(
            "UPDATE watches SET next_due_index = ?2 WHERE id = ?1",
            &[id.to_string().into(), stored(until)?.into()],
        )?;
        self.change(
            id,
            ChangeKind::Gap,
            NewChange {
                why: Some((FailureClass::Busy, WATCH_DUE_MISSED)),
                missed: Some(missed),
                ..NewChange::default()
            },
        )
    }

    /// No check of the watch is begun before `until`: what a refused
    /// admission's retry time asks.
    ///
    /// # Errors
    ///
    /// The refusal of its state unless it is `confirmed`.
    pub fn defer(&mut self, id: WatchId, until: Timestamp) -> Result<(), Failure> {
        let watch = self.load(id)?;
        if watch.state != WatchState::Confirmed {
            return Err(state_refusal(watch.state));
        }
        self.tx.exec(
            "UPDATE watches SET not_before = ?2 WHERE id = ?1",
            &[id.to_string().into(), until.into()],
        )?;
        Ok(())
    }

    /// Pauses a confirmed watch, with the reason.
    ///
    /// # Errors
    ///
    /// The refusal of its state unless it is `confirmed`.
    pub fn pause(&mut self, id: WatchId, reason: WatchReason) -> Result<WatchRow, Failure> {
        let watch = self.load(id)?;
        if watch.state != WatchState::Confirmed {
            return Err(state_refusal(watch.state));
        }
        self.leave_running(id, WatchState::Paused, Some(reason), None)?;
        self.change(
            id,
            ChangeKind::Paused,
            NewChange {
                reason: Some(reason),
                ..NewChange::default()
            },
        )?;
        self.load(id)
    }

    /// Lets a paused watch go on. A watch paused because its version changed
    /// or its read drifted never goes on: only a new watch, confirmed again.
    /// Due times that passed while it was paused are a gap the scheduler
    /// records when it next looks.
    ///
    /// # Errors
    ///
    /// `policy`/`watch_needs_confirmation`; the refusal of its state unless
    /// it is `paused`.
    pub fn resume(&mut self, id: WatchId) -> Result<WatchRow, Failure> {
        let watch = self.load(id)?;
        match watch.state {
            WatchState::Paused => {}
            WatchState::Ended => return Err(state_refusal(WatchState::Ended)),
            _ => return Err(failure::invalid("a watch that is not paused")),
        }
        if let Some(reason) = watch.reason
            && !reason.may_resume()
        {
            return Err(refused(
                FailureClass::Policy,
                WATCH_NEEDS_CONFIRMATION,
                "The watch paused because its read changed; it goes on only as a new watch its person confirms.",
                "End this watch and ask for a new one.",
            ));
        }
        self.leave_running(id, WatchState::Confirmed, None, None)?;
        self.change(id, ChangeKind::Resumed, NewChange::default())?;
        self.load(id)
    }

    /// Ends the watch, from any state but `ended`: always possible. A check
    /// under way is closed as failed with `watch_ended`; its run is left to
    /// end on its own. The baseline's digests go; the watch, its checks, and
    /// its changes stay.
    ///
    /// # Errors
    ///
    /// `policy`/`watch_ended` for a watch that ended already.
    pub fn end(&mut self, id: WatchId, reason: WatchReason) -> Result<WatchRow, Failure> {
        let watch = self.load(id)?;
        if watch.state == WatchState::Ended {
            return Err(state_refusal(WatchState::Ended));
        }
        if let Some(seq) = watch.running_check {
            self.close_check(
                id,
                seq,
                CheckOutcome::Failed,
                Some((FailureClass::Policy, WATCH_ENDED)),
                None,
                None,
            )?;
        }
        self.tx.exec(
            "DELETE FROM watch_baseline WHERE watch_id = ?1",
            &[id.to_string().into()],
        )?;
        self.leave_running(id, WatchState::Ended, Some(reason), None)?;
        self.change(
            id,
            ChangeKind::Ended,
            NewChange {
                reason: Some(reason),
                ..NewChange::default()
            },
        )?;
        self.load(id)
    }

    /// A target's published version is now `current`: every confirmed
    /// watch of it tied to another version pauses with `version_changed`,
    /// and goes on only as a new watch its person confirms. Answers the
    /// watches of another version whose check is under way, which pause
    /// when the caller asks again after their checks end.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn version_changed(
        &mut self,
        workspace: &WorkspaceId,
        target: &TargetName,
        current: &Digest,
    ) -> Result<Vec<WatchId>, Failure> {
        let rows = self.tx.rows(
            "SELECT id, state FROM watches
             WHERE workspace_id = ?1 AND target = ?2 AND target_version_hash <> ?3
               AND state IN ('confirmed', 'running')
             ORDER BY id",
            &[
                workspace.to_string().into(),
                target.as_str().into(),
                current.to_string().into(),
            ],
        )?;
        let mut running = Vec::new();
        for row in rows {
            let id: WatchId = parse(row.text(0)?)?;
            if row.text(1)? == WatchState::Running.as_str() {
                running.push(id);
            } else {
                self.pause(id, WatchReason::VersionChanged)?;
            }
        }
        Ok(running)
    }

    /// One watch.
    ///
    /// # Errors
    ///
    /// `input`/`not_found`.
    pub fn get(&mut self, id: WatchId) -> Result<WatchRow, Failure> {
        self.load(id)
    }

    /// The watches of one person in a workspace, oldest first; with
    /// `open_only`, only those confirmed, running, or paused.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn list(
        &mut self,
        workspace: &WorkspaceId,
        person: &PrincipalId,
        open_only: bool,
    ) -> Result<Vec<WatchRow>, Failure> {
        let rows = self.tx.rows(
            "SELECT id FROM watches WHERE workspace_id = ?1 AND acts_for = ?2
               AND (?3 = 0 OR state IN ('confirmed', 'running', 'paused'))
             ORDER BY id",
            &[
                workspace.to_string().into(),
                person.to_string().into(),
                open_only.into(),
            ],
        )?;
        self.by_ids(&rows)
    }

    /// How many watches one person has that are asked, confirmed, running,
    /// or paused: what the most a person may have is counted against.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn not_ended(
        &mut self,
        workspace: &WorkspaceId,
        person: &PrincipalId,
    ) -> Result<u64, Failure> {
        let row = self
            .tx
            .row(
                "SELECT COUNT(*) FROM watches WHERE workspace_id = ?1 AND acts_for = ?2
                   AND state <> 'ended'",
                &[workspace.to_string().into(), person.to_string().into()],
            )?
            .ok_or_else(|| failure::corrupt("a count that gave no row"))?;
        count(row.int(0)?)
    }

    /// Every open watch of the store: what the scheduler looks over.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn open(&mut self) -> Result<Vec<WatchRow>, Failure> {
        let rows = self.tx.rows(
            "SELECT id FROM watches WHERE state IN ('confirmed', 'running', 'paused') ORDER BY id",
            &[],
        )?;
        self.by_ids(&rows)
    }

    fn by_ids(&mut self, rows: &[Row]) -> Result<Vec<WatchRow>, Failure> {
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(self.load(parse(row.text(0)?)?)?);
        }
        Ok(out)
    }

    /// One check.
    ///
    /// # Errors
    ///
    /// `input`/`not_found`.
    pub fn check(&mut self, id: WatchId, seq: u64) -> Result<CheckRow, Failure> {
        let row = self
            .tx
            .row(
                concat!(
                    "SELECT ",
                    check_columns!(),
                    " FROM watch_checks WHERE watch_id = ?1 AND seq = ?2"
                ),
                &[id.to_string().into(), stored(seq)?.into()],
            )?
            .ok_or_else(|| failure::not_found("no such check of the watch"))?;
        check_row(&row)
    }

    /// The check a run is, when it is one under way.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn check_of_run(&mut self, run: RunId) -> Result<Option<(WatchId, u64)>, Failure> {
        self.tx
            .row(
                "SELECT watch_id, seq FROM watch_checks WHERE run_id = ?1 AND outcome = 'running'",
                &[run.to_string().into()],
            )?
            .map(|row| Ok((parse(row.text(0)?)?, count(row.int(1)?)?)))
            .transpose()
    }

    /// The watch's checks after `after`, oldest first, at most `limit`.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn checks(
        &mut self,
        id: WatchId,
        after: Option<u64>,
        limit: u32,
    ) -> Result<Vec<CheckRow>, Failure> {
        let after = after.map(stored).transpose()?.unwrap_or(-1);
        let rows = self.tx.rows(
            concat!(
                "SELECT ",
                check_columns!(),
                " FROM watch_checks WHERE watch_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3"
            ),
            &[id.to_string().into(), after.into(), limit.into()],
        )?;
        rows.iter().map(check_row).collect()
    }

    /// The watch's changes after the change numbered `after`, oldest first,
    /// at most `limit`.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn changes(
        &mut self,
        id: WatchId,
        after: u64,
        limit: u32,
    ) -> Result<Vec<ChangeRow>, Failure> {
        let rows = self.tx.rows(
            "SELECT seq, watch_id, kind, check_seq, older_check, key_digest, outputs, why_class,
                    why_code, reason, first_missed, last_missed, missed, at
             FROM watch_changes WHERE watch_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
            &[id.to_string().into(), stored(after)?.into(), limit.into()],
        )?;
        rows.iter().map(change_row).collect()
    }

    /// The name a watch gives a record's key, so that a door can find, in a
    /// check run's result, the record a change names.
    ///
    /// # Errors
    ///
    /// `busy`/`store_unavailable` while no vault is attached.
    pub fn key_of(
        &mut self,
        workspace: &WorkspaceId,
        key: &RecordKey,
    ) -> Result<WatchKey, Failure> {
        let digest = std::sync::Arc::clone(&self.tx.shared.attached()?.digest);
        Ok(WatchKey::of(digest.as_ref(), workspace, key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words_of() -> Vec<&'static str> {
        let mut words: Vec<&'static str> = Vec::new();
        words.extend(WatchState::ALL.iter().map(|word| word.as_str()));
        words.extend(CheckOutcome::ALL.iter().map(|word| word.as_str()));
        words.extend(ChangeKind::ALL.iter().map(|word| word.as_str()));
        words.extend(WatchReason::ALL.iter().map(|word| word.as_str()));
        words
    }

    #[test]
    fn every_word_of_a_watch_is_in_its_migration_and_the_ledger_schema() {
        let migration = include_str!("../migrations/0021-ledger-watches-checks-and-changes.sql");
        let schema = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("docs")
                .join("reference")
                .join("ledger-schema.md"),
        )
        .unwrap();
        let section = schema
            .split("## watches, watch_checks, watch_changes, watch_baseline")
            .nth(1)
            .unwrap()
            .split("\n## ")
            .next()
            .unwrap();
        for word in words_of() {
            assert!(
                migration.contains(&format!("'{word}'")),
                "the migration lacks `{word}`"
            );
            assert!(
                section.contains(&format!("`{word}`")),
                "the ledger schema lacks `{word}`"
            );
        }
    }

    #[test]
    fn every_code_of_a_watch_is_declared_once_with_its_class() {
        let constants = [
            (WATCH_NOT_A_READ, FailureClass::Policy),
            (WATCH_NOT_CONFIRMED, FailureClass::Policy),
            (WATCH_NOT_YOURS, FailureClass::Policy),
            (WATCH_CHECK_RUNNING, FailureClass::Busy),
            (WATCH_PAUSED, FailureClass::Busy),
            (WATCH_ENDED, FailureClass::Policy),
            (WATCH_RUN_NOT_ITS_OWN, FailureClass::Policy),
            (WATCH_NEEDS_CONFIRMATION, FailureClass::Policy),
            (WATCH_CHECK_UNFINISHED, FailureClass::Busy),
            (WATCH_DUE_MISSED, FailureClass::Busy),
            (WATCH_CHECK_NOT_COMPLETED, FailureClass::Busy),
            (
                crate::watch_compare::WATCH_KEY_REPEATED,
                FailureClass::UnknownVariant,
            ),
            (
                crate::watch_compare::WATCH_SHAPES_MIXED,
                FailureClass::UnknownVariant,
            ),
        ];
        assert_eq!(constants.len(), ALL.len());
        for (code, class) in constants {
            assert!(ALL.contains(&(class, code.as_str())), "{code:?}");
        }
        for (class, code) in [
            state_refusal(WatchState::Asked),
            state_refusal(WatchState::Running),
            state_refusal(WatchState::Paused),
            state_refusal(WatchState::Ended),
        ]
        .iter()
        .map(|failure| (failure.class(), failure.code().as_str()))
        {
            assert!(ALL.contains(&(class, code)));
        }
    }
}
