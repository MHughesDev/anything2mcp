//! The scheduler's half of watches (decision 0247): when each check of a
//! watch is due, what happens to a due time that passed with no check, and
//! how a check is admitted as the lowest work there is.
//!
//! A due time is a moment of the store's clock, computed from the watch's
//! row and nothing else: the moment its person confirmed it, plus a fixed
//! phase, plus a whole number of intervals. The phase spreads watches so
//! that many do not fire together; it is made from the watch's identifier,
//! so it is the same on every machine and on every start, and nothing in a
//! due time is random. A due time that passed with no check is recorded as
//! a gap and never checked late as if it were on time; after a gap there is
//! one check, marked as the catch-up. A watch's check is admitted as `batch`
//! work, after every interactive job, and spends the same budgets as any
//! read of its surface, leaving a share of each to the caller's own reads.

use std::time::Duration;

use a2m_core::{Digest, Failure, FailureClass, FailureCode, JobId, RunId, Timestamp};
use a2m_ledger::{
    CheckRow, Due, Durability, Missed, NewWatch, Tx, WatchId, WatchReason, WatchRow, WatchState,
};

use crate::failure;
use crate::scheduler::Scheduler;
use crate::tables;
use crate::types::{Admitted, Priority, RateLimit, Submission};

/// The shortest interval any installation may give a watch. A setting may
/// raise the floor above it and never lower it below it, so that a watch is
/// never a fast poll of a system.
pub const SHORTEST_INTERVAL_FLOOR: Duration = Duration::from_secs(60);

/// A watch's interval is below the floor. Class `input`.
pub const WATCH_INTERVAL_TOO_SHORT: FailureCode = FailureCode::fixed("watch_interval_too_short");
/// A watch's end is further away than the longest a watch may last. Class
/// `input`.
pub const WATCH_TOO_LONG: FailureCode = FailureCode::fixed("watch_too_long");
/// The person has as many watches as they may have. Class `busy`.
pub const WATCH_TOO_MANY: FailureCode = FailureCode::fixed("watch_too_many");
/// The watches on one surface would take more of its budget than their
/// share. Class `busy`.
pub const WATCH_OVER_BUDGET_SHARE: FailureCode = FailureCode::fixed("watch_over_budget_share");

/// Every failure code of a watch the scheduler produces, with its one class.
/// The ledger's codes of a watch are in `a2m_ledger::codes::WATCH_CODES`;
/// no code is in both.
pub const ALL: &[(FailureClass, &str)] = &[
    (FailureClass::Input, "watch_interval_too_short"),
    (FailureClass::Input, "watch_too_long"),
    (FailureClass::Busy, "watch_too_many"),
    (FailureClass::Busy, "watch_over_budget_share"),
];

const DAY_MS: u128 = 86_400_000;

/// The refusals of admission a check waits out: it is tried again later,
/// and its due time becomes a gap only if the next one passes first.
const ADMISSION_WAITS: [FailureCode; 6] = [
    crate::failure::ON_HOLD,
    crate::failure::QUEUE_FULL,
    crate::failure::CALLER_QUEUE_FULL,
    crate::failure::RUNNER_UNAVAILABLE,
    crate::failure::RATE_LIMITED,
    crate::failure::ENGINE_DEGRADED,
];

/// The limits on watches, from the installation's settings (decision 0247).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchLimits {
    /// The shortest interval: `watch.shortest_interval_seconds`, 60 by
    /// default and never below [`SHORTEST_INTERVAL_FLOOR`].
    pub shortest_interval: Duration,
    /// The longest a watch lasts after it is asked for:
    /// `watch.longest_days`, 7 by default.
    pub longest: Duration,
    /// The most watches one person has that have not ended: `watch.most`,
    /// 20 by default.
    pub most_per_person: u32,
    /// The largest phase: a watch's checks fall at most this long, and at
    /// most a tenth of its interval, after the regular times. Five minutes.
    pub jitter_most: Duration,
    /// The share of a surface's sustained rate that all watches on it may
    /// take together, in percent: 10.
    pub budget_share_percent: u32,
    /// The share of each budget's capacity a check leaves for others, in
    /// percent: 50.
    pub reserve_percent: u32,
}

impl Default for WatchLimits {
    fn default() -> Self {
        Self {
            shortest_interval: SHORTEST_INTERVAL_FLOOR,
            longest: Duration::from_secs(7 * 86_400),
            most_per_person: 20,
            jitter_most: Duration::from_secs(300),
            budget_share_percent: 10,
            reserve_percent: 50,
        }
    }
}

impl WatchLimits {
    /// The floor in force: the setting, never below
    /// [`SHORTEST_INTERVAL_FLOOR`].
    #[must_use]
    pub fn floor(&self) -> Duration {
        self.shortest_interval.max(SHORTEST_INTERVAL_FLOOR)
    }

    /// Checks a new watch's interval and end.
    ///
    /// # Errors
    ///
    /// `input`/`watch_interval_too_short` for an interval below the floor
    /// or not a whole number of seconds; `input`/`watch_too_long` for an end
    /// further than the longest a watch lasts.
    pub fn check_new(
        &self,
        interval: Duration,
        now: Timestamp,
        ends_at: Timestamp,
    ) -> Result<(), Failure> {
        if interval < self.floor() || interval.subsec_nanos() != 0 {
            return Err(Failure::new(
                FailureClass::Input,
                WATCH_INTERVAL_TOO_SHORT,
                format!(
                    "A watch's interval is a whole number of seconds, at least {}.",
                    self.floor().as_secs()
                ),
                "Ask for a longer interval.",
            ));
        }
        let longest = i64::try_from(self.longest.as_millis()).unwrap_or(i64::MAX);
        if ends_at
            .as_unix_millis()
            .saturating_sub(now.as_unix_millis())
            > longest
        {
            return Err(Failure::new(
                FailureClass::Input,
                WATCH_TOO_LONG,
                format!(
                    "A watch lasts at most {} days from when it is asked for.",
                    self.longest.as_secs() / 86_400
                ),
                "Ask for an earlier end.",
            ));
        }
        Ok(())
    }

    /// The watch's fixed phase: a share of the smaller of a tenth of its
    /// interval and [`WatchLimits::jitter_most`], chosen by the digest of its
    /// identifier. The same on every machine, every time.
    #[must_use]
    pub fn phase(&self, watch: WatchId, interval: Duration) -> Duration {
        let span = (interval.as_millis() / 10).min(self.jitter_most.as_millis());
        if span == 0 {
            return Duration::ZERO;
        }
        let digest = Digest::of(watch.as_bytes());
        let mut first = [0u8; 8];
        first.copy_from_slice(&digest.as_bytes()[..8]);
        let at = u128::from(u64::from_le_bytes(first)) % span;
        Duration::from_millis(u64::try_from(at).unwrap_or(0))
    }

    /// Whether a watch whose check sends `requests` every `interval` fits in
    /// its surface's share beside the watches already on it, each given as
    /// its interval and requests. All the watches on one surface together
    /// take at most [`WatchLimits::budget_share_percent`] of its sustained
    /// rate, counted per day in whole requests, rounded against the watch.
    ///
    /// # Errors
    ///
    /// `busy`/`watch_over_budget_share`.
    pub fn fits_budget(
        &self,
        limit: &RateLimit,
        interval: Duration,
        requests: u32,
        others: &[(Duration, u32)],
    ) -> Result<(), Failure> {
        let per_day = |interval: Duration, requests: u32| -> u128 {
            let ms = interval.as_millis().max(1);
            (u128::from(requests) * DAY_MS).div_ceil(ms)
        };
        let wanted: u128 = others
            .iter()
            .map(|(interval, requests)| per_day(*interval, *requests))
            .sum::<u128>()
            + per_day(interval, requests);
        let budget = u128::from(limit.refill) * DAY_MS / limit.per.as_millis().max(1);
        if wanted * 100 > budget * u128::from(self.budget_share_percent) {
            return Err(Failure::new(
                FailureClass::Busy,
                WATCH_OVER_BUDGET_SHARE,
                format!(
                    "The watches on this surface would take more than {} in every hundred of the requests it allows.",
                    self.budget_share_percent
                ),
                "Ask for a longer interval, or end another watch on the same surface.",
            ));
        }
        Ok(())
    }
}

/// Due time `index` of a watch confirmed at `confirmed_at`.
#[must_use]
pub fn due_at(
    confirmed_at: Timestamp,
    phase: Duration,
    interval: Duration,
    index: u64,
) -> Timestamp {
    let step = u128::from(index) * interval.as_millis() + phase.as_millis();
    let step = i64::try_from(step).unwrap_or(i64::MAX);
    Timestamp::from_unix_millis(confirmed_at.as_unix_millis().saturating_add(step))
}

/// What a watch's row says should happen now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuePlan {
    /// Nothing before this moment.
    Waiting {
        /// When to look again.
        at: Timestamp,
    },
    /// A check is due: the next due time has come and the one after has
    /// not. It may be late, waiting for its budget; it is not missed.
    Due(Due),
    /// Due times passed with no check: they are one gap, and one check
    /// follows for the latest of them.
    Missed {
        /// The first missed due time's index.
        from: u64,
        /// The index of the check that follows; the missed ones are those
        /// from `from` up to it.
        until: u64,
        /// What the gap records.
        missed: Missed,
        /// The check that follows.
        then: Due,
    },
    /// The watch's end has come. Due times that passed before it are a gap.
    EndReached {
        /// The gap, when due times passed: from, until, and which.
        missed: Option<(u64, u64, Missed)>,
    },
}

/// What should happen now to a confirmed watch. `None` for a watch that is
/// asked, running, paused, or ended: only a confirmed watch's due times are
/// looked at, and those that pass while it is paused are a gap when it goes
/// on.
#[must_use]
pub fn plan(watch: &WatchRow, now: Timestamp) -> Option<DuePlan> {
    if watch.state != WatchState::Confirmed {
        return None;
    }
    let (_, confirmed_at) = watch.confirmed?;
    let phase = watch.phase?;
    let at = |index: u64| due_at(confirmed_at, phase, watch.interval, index);
    let next = watch.next_due_index;
    // The last index whose due time has come, if any.
    let latest = latest_due(confirmed_at, phase, watch.interval, now);
    if now >= watch.ends_at {
        let last_before_end = latest_due(confirmed_at, phase, watch.interval, watch.ends_at);
        let missed = match last_before_end {
            Some(last) if last >= next => Some((
                next,
                last + 1,
                Missed {
                    first: at(next),
                    last: at(last),
                    count: last + 1 - next,
                },
            )),
            _ => None,
        };
        return Some(DuePlan::EndReached { missed });
    }
    let waiting_until = |moment: Timestamp| match watch.not_before {
        Some(not_before) if not_before > moment => not_before,
        _ => moment,
    };
    match latest {
        Some(latest) if latest >= next => {
            if let Some(not_before) = watch.not_before
                && not_before > now
                && latest == next
            {
                return Some(DuePlan::Waiting { at: not_before });
            }
            if latest == next {
                return Some(DuePlan::Due(Due {
                    index: next,
                    at: at(next),
                    catch_up: false,
                }));
            }
            Some(DuePlan::Missed {
                from: next,
                until: latest,
                missed: Missed {
                    first: at(next),
                    last: at(latest - 1),
                    count: latest - next,
                },
                then: Due {
                    index: latest,
                    at: at(latest),
                    catch_up: true,
                },
            })
        }
        _ => Some(DuePlan::Waiting {
            at: waiting_until(at(next)).min(watch.ends_at),
        }),
    }
}

fn latest_due(
    confirmed_at: Timestamp,
    phase: Duration,
    interval: Duration,
    now: Timestamp,
) -> Option<u64> {
    let first = due_at(confirmed_at, phase, interval, 0);
    if now < first {
        return None;
    }
    let since = u128::try_from(now.as_unix_millis() - first.as_unix_millis()).unwrap_or(0);
    u64::try_from(since / interval.as_millis().max(1)).ok()
}

/// A check that is due, as [`Scheduler::due_watches`] gives it: the watch's
/// row, from which the caller builds the check's run, and the due time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueCheck {
    /// The watch.
    pub watch: WatchRow,
    /// The due time.
    pub due: Due,
}

/// A check that was admitted and begun.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckStarted {
    /// The run of the read.
    pub run: RunId,
    /// Its job, when the run is new. A run the same person already had
    /// unfinished for the same read and input (ledger rule 6) has its job.
    pub job: Option<JobId>,
    /// The check.
    pub check: CheckRow,
}

/// What a check's admission gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckAdmission {
    /// Begun.
    Started(CheckStarted),
    /// Not admitted now: the failure the admission gave, and the moment
    /// before which no check of the watch is tried again. The due time is
    /// not missed by this; if it passes the next one, it becomes a gap.
    Deferred {
        /// What refused it.
        refused: Failure,
        /// Not before.
        until: Timestamp,
    },
}

fn ask_in(tx: &mut Tx<'_>, limits: &WatchLimits, new: &NewWatch) -> Result<WatchRow, Failure> {
    limits.check_new(new.interval, tx.now(), new.ends_at)?;
    let open = tx.watches().not_ended(&new.workspace, &new.acts_for)?;
    if open >= u64::from(limits.most_per_person) {
        return Err(Failure::new(
            FailureClass::Busy,
            WATCH_TOO_MANY,
            format!(
                "The person has {} watches that have not ended, the most they may have.",
                limits.most_per_person
            ),
            "End a watch, then ask again.",
        ));
    }
    tx.watches().ask(new)
}

impl Scheduler {
    /// Takes an agent's request for a watch: checks its interval and end
    /// against the limits and counts the person's watches, in one
    /// transaction with the ledger's row. The watch does nothing until its
    /// person confirms it.
    ///
    /// # Errors
    ///
    /// `input`/`watch_interval_too_short`, `input`/`watch_too_long`,
    /// `busy`/`watch_too_many`, and the ledger's refusals of the watch.
    pub async fn ask_watch(&self, new: NewWatch) -> Result<WatchRow, Failure> {
        let limits = self.settings().watch;
        self.store()
            .write(Durability::Synced, move |tx| ask_in(tx, &limits, &new))
            .await
    }

    /// Records the confirmation of the watch's own person, with the phase
    /// made from its identifier. Its first due time is now plus that phase.
    ///
    /// # Errors
    ///
    /// The ledger's refusals: `policy`/`watch_not_yours`, a watch that is not
    /// `asked`.
    pub async fn confirm_watch(
        &self,
        watch: WatchId,
        by: a2m_core::PrincipalId,
    ) -> Result<WatchRow, Failure> {
        let limits = self.settings().watch;
        let row = self
            .store()
            .write(Durability::Synced, move |tx| {
                let interval = tx.watches().get(watch)?.interval;
                tx.watches()
                    .confirm(watch, by, limits.phase(watch, interval))
            })
            .await?;
        self.wake();
        Ok(row)
    }

    /// Looks over every confirmed watch, in one transaction: records a gap
    /// for due times that passed with no check, ends a watch whose end has
    /// come, and answers the checks due now. Running and paused watches are
    /// passed over: a watch never runs two checks at once.
    ///
    /// # Errors
    ///
    /// `busy` failures of the store.
    pub async fn due_watches(&self) -> Result<Vec<DueCheck>, Failure> {
        self.store()
            .write(Durability::Synced, move |tx| {
                let now = tx.now();
                let mut due = Vec::new();
                for watch in tx.watches().open()? {
                    match plan(&watch, now) {
                        None | Some(DuePlan::Waiting { .. }) => {}
                        Some(DuePlan::Due(at)) => due.push(DueCheck { watch, due: at }),
                        Some(DuePlan::Missed {
                            from,
                            until,
                            missed,
                            then,
                        }) => {
                            tx.watches().missed(watch.id, from, until, missed)?;
                            let watch = tx.watches().get(watch.id)?;
                            due.push(DueCheck { watch, due: then });
                        }
                        Some(DuePlan::EndReached { missed }) => {
                            if let Some((from, until, missed)) = missed {
                                tx.watches().missed(watch.id, from, until, missed)?;
                            }
                            tx.watches().end(watch.id, WatchReason::EndReached)?;
                        }
                    }
                }
                Ok(due)
            })
            .await
    }

    /// The earliest moment at which [`Scheduler::due_watches`] has something
    /// to do; `None` when no watch is confirmed. Whoever calls `sweep` waits
    /// on the earlier of this and [`Scheduler::next_due`].
    ///
    /// # Errors
    ///
    /// `busy` failures of the store.
    pub async fn next_watch_due(&self) -> Result<Option<Timestamp>, Failure> {
        self.store()
            .read(move |tx| {
                let now = tx.now();
                let mut earliest: Option<Timestamp> = None;
                for watch in tx.watches().open()? {
                    let at = match plan(&watch, now) {
                        None => continue,
                        Some(DuePlan::Waiting { at }) => at,
                        Some(_) => now,
                    };
                    earliest = Some(earliest.map_or(at, |earliest| earliest.min(at)));
                }
                Ok(earliest)
            })
            .await
    }

    /// Admits a due check as a job of its run and begins it, in one
    /// transaction: the run must be the watch's own read, which the ledger
    /// checks; the job is `batch`, whatever the submission said, and writes
    /// nothing; the budgets it spends leave their reserve to others. When
    /// admission refuses (a hold, a full queue, a spent budget), nothing is
    /// kept of it, and the watch is not tried again before the moment the
    /// answer gives.
    ///
    /// # Errors
    ///
    /// `input`/`schema_invalid` for a submission that writes; the ledger's
    /// refusals of the check (`watch_run_not_its_own`, `watch_check_running`,
    /// `watch_paused`, `watch_ended`, `watch_not_confirmed`); `busy`
    /// failures of the store.
    pub async fn submit_check(
        &self,
        check: &DueCheck,
        submission: Submission,
    ) -> Result<CheckAdmission, Failure> {
        if submission.job.writes || submission.run.effect != a2m_core::EffectClass::Read {
            return Err(failure::invalid("a watch's check that writes"));
        }
        let settings = *self.settings();
        let (watch, due) = (check.watch.id, check.due);
        let submission = Submission {
            priority: Priority::Batch,
            ..submission
        };
        let admitted = self
            .store()
            .write(Durability::Relaxed, move |tx| {
                // The watch first: a check that cannot begin admits nothing.
                // Any refusal takes the whole transaction back, the run
                // admission may have opened with it.
                let row = tx.watches().get(watch)?;
                if row.state != WatchState::Confirmed {
                    return Err(tx
                        .watches()
                        .begin_check(watch, due, RunId::from_bytes([0; 16]))
                        .err()
                        .unwrap_or_else(|| failure::invalid("a watch that is not confirmed")));
                }
                let (run, job) =
                    match tables::admit(tx, &settings, submission, settings.watch.reserve_percent)?
                    {
                        Admitted::New { run, job } => (run, Some(job)),
                        Admitted::Existing { run } => (run, None),
                    };
                let check = tx.watches().begin_check(watch, due, run)?;
                Ok(CheckStarted { run, job, check })
            })
            .await;
        match admitted {
            Ok(started) => {
                self.wake();
                Ok(CheckAdmission::Started(started))
            }
            Err(refused) if ADMISSION_WAITS.contains(refused.code()) => {
                // Not before a lease's length, nor past the next due time.
                let wait = settings.lease_length.min(check.watch.interval);
                let until = tables::plus(self.now(), wait);
                self.store()
                    .write(Durability::Relaxed, move |tx| {
                        tx.watches().defer(watch, until)
                    })
                    .await?;
                Ok(CheckAdmission::Deferred { refused, until })
            }
            Err(refused) => Err(refused),
        }
    }
}
