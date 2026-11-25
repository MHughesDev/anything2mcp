//! The failure codes this crate produces, each with its one class. Four of
//! them, `record_locked`, `runner_lost`, `not_found`, and the store's, are
//! the ledger's, and stay its constants here.
//!
//! Whether a `busy` code passes by itself, and why:
//!
//! - `queue_full` does: the target's queue drains as its jobs run, and the
//!   failure says how long that takes.
//! - `caller_queue_full` does: the caller's waiting jobs start as the queue
//!   drains, with nobody acting.
//! - `runner_unavailable` does not: most often no enrolled runner offers
//!   what the job needs, and a person starts or enrolls one.
//! - `engine_degraded` does not: what degraded the engine is repaired by a
//!   person, as `anything2mcp doctor run` names it.
//! - `session_in_use` does: the work already on the session finishes.
//! - `resource_in_use` does: the job that holds the resource lets it go when
//!   it ends, and the failure says how long that may take.
//! - `resource_order` does not: the call asks in the wrong order, and asks
//!   the same way again.
//! - `rate_limited` does: the budget refills, and the failure says when.
//! - `scheduler_paused` does not: a person set the pause, and a person releases it.
//! - `run_reconciling` does: the scheduler hands the reconcile out itself,
//!   and it ends without anyone acting.
//! - `watch_too_many` does not: the person ends a watch first.
//! - `watch_over_budget_share` does not: the watch asks for a longer
//!   interval, or another watch on the surface is ended.

use a2m_core::FailureCode;

/// A wait for a record lock ran out. Class `busy`. The same code the ledger
/// answers with when the lock is held.
pub const RECORD_LOCKED: FailureCode = a2m_ledger::codes::RECORD_LOCKED;
/// Written on a run whose lease was lost. Class `possible_commit`. The same
/// code the ledger answers with to a call under an old lease.
pub const RUNNER_LOST: FailureCode = a2m_ledger::codes::RUNNER_LOST;
/// There is no such job. Class `input`.
pub const NOT_FOUND: FailureCode = a2m_ledger::codes::NOT_FOUND;

a2m_core::failure_codes! {
    QUEUE_FULL = Busy, "queue_full",
        meaning "The target's queue is full.",
        next "Call again after the wait the failure gives.",
        passes by_itself;
    CALLER_QUEUE_FULL = Busy, "caller_queue_full",
        meaning "The caller has as many jobs waiting as it may.",
        next "Wait for one of the caller's jobs to start, or cancel one, then call again.",
        passes by_itself;
    RUNNER_UNAVAILABLE = Busy, "runner_unavailable",
        meaning "No runner can serve the job.",
        next "Start or enroll a runner that offers what the message names as missing, then call again.",
        passes not_by_itself;
    ENGINE_DEGRADED = Busy, "engine_degraded",
        meaning "The kernel is degraded and takes no writes, or has stopped.",
        next "Run `anything2mcp doctor run`, repair what it names, then call again.",
        passes not_by_itself;
    SESSION_IN_USE = Busy, "session_in_use",
        meaning "A wait for a place on a session ran out.",
        next "Call again when the work on that session has finished.",
        passes by_itself;
    RESOURCE_IN_USE = Busy, "resource_in_use",
        meaning "A wait for an exclusive resource ran out.",
        next "Call again after the wait the failure gives.",
        passes by_itself;
    RESOURCE_ORDER = Busy, "resource_order",
        meaning "A job asked for an exclusive resource that sorts before one it holds.",
        next "Release the resources that sort after the one asked for, then ask for all of them together.",
        passes not_by_itself;
    RATE_LIMITED = Busy, "rate_limited",
        meaning "A rate budget is spent and does not refill inside the lease.",
        next "Call again after the wait the failure gives.",
        passes by_itself;
    SCHEDULER_PAUSED = Busy, "scheduler_paused",
        meaning "The work is covered by a scheduler pause, such as the stop-everything switch.",
        next "Have whoever set the pause release it, then call again.",
        passes not_by_itself;
    LEASE_SUPERSEDED = Policy, "lease_superseded",
        meaning "A call carries a lease that is not the job's current one, or one that has run out.",
        next "Stop work on this job; it is recovered under a new lease.";
    RUNNER_NOT_ENROLLED = Policy, "runner_not_enrolled",
        meaning "A runner that never said `hello` asked for work.",
        next "Say `hello` first, then ask for work.";
    RUNNER_PROTOCOL_UNSUPPORTED = UnknownVariant, "runner_protocol_unsupported",
        meaning "A runner speaks a protocol version this scheduler does not.",
        next "Upgrade the runner or the control plane so that their versions are at most one apart.";
    RUN_RECONCILING = Busy, "run_reconciling",
        meaning "A resume found the run's reconcile as its one job: what is in doubt is settled first.",
        next "Wait for the reconcile to end, then resume the run again.",
        passes by_itself;
    // The codes of a watch: the last four rows, which `WATCH_CODES` lists.
    WATCH_INTERVAL_TOO_SHORT = Input, "watch_interval_too_short",
        meaning "A watch's interval is below the floor, or not a whole number of seconds.",
        next "Ask for a longer interval, in whole seconds.";
    WATCH_TOO_LONG = Input, "watch_too_long",
        meaning "A watch's end is further away than the longest a watch may last.",
        next "Ask for an earlier end.";
    WATCH_TOO_MANY = Busy, "watch_too_many",
        meaning "The person has as many watches as they may have.",
        next "End a watch, then ask again.",
        passes not_by_itself;
    WATCH_OVER_BUDGET_SHARE = Busy, "watch_over_budget_share",
        meaning "The watches on one surface would take more of its budget than their share.",
        next "Ask for a longer interval, or end another watch on the same surface.",
        passes not_by_itself;
}

/// Every failure code of a watch the scheduler produces, with its one class:
/// the last four rows of the table. The ledger's codes of a watch are in
/// `a2m_ledger::codes::WATCH_CODES`; no code is in both.
pub const WATCH_CODES: &[(a2m_core::FailureClass, &str)] = ALL.split_at(ALL.len() - 4).1;
