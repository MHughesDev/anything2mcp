//! Following a run: its events from a sequence number on, and a wait for
//! the next one.
//!
//! The ledger keeps the events of a run numbered from 1 with no gap. A
//! front door that shows progress reads them here, a page at a time, and
//! waits on the kernel's notice that a run changed, which the in-process
//! `Control`, the runner loop, and the approval desk give whenever one of
//! them appends. Nothing here polls.
//!
//! This is not an action and records no decision: it shows a caller only
//! the events of a run that the same caller started, which the decision
//! about the call that started the run already covered. Everything else
//! about a run is read through `run.get` and `trace.show`.

use std::time::Duration;

use a2m_core::{Failure, RunId, RunState, Timestamp};
use a2m_sched::Scheduler;
use serde_json::Value as Json;

use crate::codes;
use crate::dispatch::{Principal, redact_json};
use crate::kernel::Kernel;

/// The most events one read gives.
pub const EVENTS_PAGE: u32 = 200;

/// One event of a run, as a caller follows it.
#[derive(Debug, Clone, PartialEq)]
pub struct RunEventView {
    /// Its number within the run, from 1, with no gap.
    pub seq: u64,
    /// The number of the lease it was written under.
    pub lease: u64,
    /// When it was recorded, on the store's clock.
    pub at: Timestamp,
    /// What kind of thing happened: `state_changed`, `step_started`,
    /// `intent`, `verified`, and the other kinds of the ledger.
    pub kind: String,
    /// Small JSON about it, redacted.
    pub detail: Json,
}

/// A page of a run's events.
#[derive(Debug, Clone, PartialEq)]
pub struct RunEvents {
    /// The run.
    pub run: RunId,
    /// The events after the number asked for, in order.
    pub events: Vec<RunEventView>,
    /// The number to ask after next: the last event given, or the number
    /// that was asked after when there was none.
    pub last: u64,
    /// The state of the run when the page was read.
    pub state: RunState,
    /// Whether a job for the run was waiting or in a runner's hands when
    /// the page was read. A run rests only when its state is a resting one
    /// and this is false; until then more events may come under the same
    /// state.
    pub working: bool,
    /// Whether the run has ended. Once it has and a page comes back empty,
    /// no further event will come.
    pub ended: bool,
}

impl RunEvents {
    /// Whether nothing more will come until someone acts: the run has
    /// ended, or it rests or has stopped and no job is waiting or in hand
    /// for it. A follower then reads the run with `run.get` and stops.
    #[must_use]
    pub const fn settled(&self) -> bool {
        self.ended
            || (!self.working
                && !matches!(
                    self.state,
                    RunState::Queued | RunState::Validating | RunState::Running
                ))
    }
}

/// The work this engine has in hand, for whoever decides whether it may
/// stop for want of use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunsInHand {
    /// Jobs of the scheduler that are waiting or in a runner's hands: runs
    /// that are queued or being worked on, reconciles included.
    pub working: u64,
    /// Runs that rest until a person approves, signs in, or answers.
    pub waiting_for_person: u64,
}

impl RunsInHand {
    /// Whether anything is in hand.
    #[must_use]
    pub const fn any(&self) -> bool {
        self.working > 0 || self.waiting_for_person > 0
    }
}

impl Kernel {
    /// How much work the engine has in hand: the scheduler's unfinished
    /// jobs, and the runs of this installation that rest for a person. One
    /// read of the store, answered from indexes. A run that stopped and
    /// can be resumed is not in hand: nothing happens to it until someone
    /// resumes it.
    ///
    /// # Errors
    ///
    /// `busy`/`engine_degraded` when the store is not open; the store's
    /// failures.
    pub async fn runs_in_hand(&self) -> Result<RunsInHand, Failure> {
        let workspace = self.installation()?.workspace;
        self.store()?
            .read(move |tx| {
                let working = Scheduler::unfinished_jobs(tx)?;
                let mut waiting_for_person = 0_u64;
                for state in [
                    RunState::NeedsApproval,
                    RunState::NeedsLogin,
                    RunState::NeedsInput,
                ] {
                    waiting_for_person = waiting_for_person
                        .saturating_add(tx.ledger().count_runs(&workspace, state)?);
                }
                Ok(RunsInHand {
                    working,
                    waiting_for_person,
                })
            })
            .await
    }

    /// The events of `run` with a number above `after`, in order, at most
    /// `limit` and never more than [`EVENTS_PAGE`], redacted like every
    /// result. Only for a run the caller started.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` when the caller started no such run; the store's
    /// failures.
    pub async fn events(
        &self,
        caller: &Principal,
        run: RunId,
        after: u64,
        limit: u32,
    ) -> Result<RunEvents, Failure> {
        let (workspace, person) = (caller.workspace, caller.person());
        let limit = limit.clamp(1, EVENTS_PAGE);
        let (row, events, working) = self
            .store()?
            .read(move |tx| {
                let working = Scheduler::unfinished_job(tx, run)?.is_some();
                let mut ledger = tx.ledger();
                let row = ledger.run(run)?;
                if row.workspace != workspace || Some(row.principal) != person {
                    return Err(codes::input(
                        codes::NOT_FOUND,
                        "There is no such run.",
                        "Follow a run with the `run_id` its own call returned.",
                    ));
                }
                let events = ledger.events(run, after, limit)?;
                Ok((row, events, working))
            })
            .await?;
        let redact = self.redact();
        let events: Vec<RunEventView> = events
            .iter()
            .map(|event| RunEventView {
                seq: event.seq,
                lease: event.epoch.get(),
                at: event.at,
                kind: event.kind.as_str().to_owned(),
                detail: redact_json(
                    redact.as_ref(),
                    &serde_json::from_str::<Json>(&event.payload).unwrap_or(Json::Null),
                ),
            })
            .collect();
        Ok(RunEvents {
            run,
            last: events.last().map_or(after, |event| event.seq),
            events,
            state: row.state,
            working,
            ended: row.state.is_final(),
        })
    }

    /// As [`Kernel::events`], and when there is no event after `after` yet
    /// it waits for one: until one is appended, the run has ended, or
    /// `wait` has passed, whichever comes first. A page that comes back
    /// empty with `ended` false means the wait ran out. A follower goes on
    /// until [`RunEvents::settled`].
    ///
    /// # Errors
    ///
    /// As [`Kernel::events`].
    pub async fn next_events(
        &self,
        caller: &Principal,
        run: RunId,
        after: u64,
        limit: u32,
        wait: Duration,
    ) -> Result<RunEvents, Failure> {
        let mut changed = self.run_watch();
        let give_up = tokio::time::sleep(wait);
        tokio::pin!(give_up);
        loop {
            // Marked seen before the read, so that an event appended
            // between the read and the wait still wakes this.
            changed.borrow_and_update();
            let page = self.events(caller, run, after, limit).await?;
            if !page.events.is_empty() || page.ended {
                return Ok(page);
            }
            tokio::select! {
                alive = changed.changed() => {
                    if alive.is_err() {
                        return Ok(page);
                    }
                }
                () = &mut give_up => return Ok(page),
            }
        }
    }
}
