//! Decides when and where work may run: admission, the queue, job leases,
//! waiting for record locks and other exclusive resources, rate budgets, and
//! when each check of a watch is due.
//!
//! The scheduler never carries out a step. It hands out a lease on a job; a
//! runner claims it and calls the engine; a role joins the two. It names no
//! mode and no pack: a runner's modes are names it compares, a limit that
//! differs by mode reaches it as a number, and a thing only one job may use
//! at a time reaches it as a [`ResourceKey`].
//!
//! Everything it decides is kept in the store, in tables it owns the meaning
//! of (`jobs`, `runners`, `resource_holds`, `caller_turns`,
//! `scheduler_holds`, `budgets`), and the run rows are changed through the
//! ledger in the same transaction. Its time is the store's clock. Nothing in
//! this crate sleeps: a wait ends when what it waits for happens, or when
//! [`Scheduler::sweep`] finds that its moment has come.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: layers 1 to 5, and never the
//! engine.

mod budget;
mod failure;
mod scheduler;
mod tables;
mod types;
mod waits;
mod watch;

pub use scheduler::{
    ADMITTED_COUNT, DueWatch, LEASED_COUNT, LOST_COUNT, QUEUE_WAIT, REFUSED_COUNT, Scheduler,
};
pub use types::{
    Admitted, BudgetKey, CancelOutcome, Directive, Enqueue, HealthState, HoldScope, JobEnd,
    JobKind, JobLease, JobRequirements, JobRow, JobState, LeaseRef, OsName, PROTOCOL_VERSION,
    Priority, RateGrant, RateLimit, Reconciling, Renewal, ResourceKey, Resumed, RunnerOffer,
    RunnerTerms, SchedSettings, SessionLimits, SessionNeed, Slots, Submission, SweepReport,
};
pub use waits::LockTicket;
pub use watch::{
    CheckAdmission, CheckStarted, DueCheck, DuePlan, DueWatches, SHORTEST_INTERVAL_FLOOR,
    WatchLimits, due_at, plan,
};

/// The failure codes this crate produces. Each belongs to exactly one class.
pub mod codes {
    pub use crate::failure::{
        CALLER_QUEUE_FULL, ENGINE_DEGRADED, LEASE_SUPERSEDED, NOT_FOUND, ON_HOLD, QUEUE_FULL,
        RATE_LIMITED, RECORD_LOCKED, RESOURCE_IN_USE, RESOURCE_ORDER, RUN_RECONCILING, RUNNER_LOST,
        RUNNER_NOT_ENROLLED, RUNNER_PROTOCOL_UNSUPPORTED, RUNNER_UNAVAILABLE, SESSION_IN_USE,
    };
    pub use crate::watch::{
        ALL as WATCH_CODES, WATCH_INTERVAL_TOO_SHORT, WATCH_OVER_BUDGET_SHARE, WATCH_TOO_LONG,
        WATCH_TOO_MANY,
    };
}
