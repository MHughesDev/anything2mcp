//! The failures this crate produces. Each code belongs to exactly one class.
//!
//! A message is built from names, counts, and times, never from a value a
//! system returned or a caller sent.

use std::time::Duration;

use a2m_core::{Failure, FailureClass, FailureCode};

/// The target's queue is full. Class `busy`.
pub const QUEUE_FULL: FailureCode = FailureCode::fixed("queue_full");
/// The caller has as many jobs waiting as it may. Class `busy`.
pub const CALLER_QUEUE_FULL: FailureCode = FailureCode::fixed("caller_queue_full");
/// No runner can serve the job. Class `busy`.
pub const RUNNER_UNAVAILABLE: FailureCode = FailureCode::fixed("runner_unavailable");
/// The kernel is degraded and takes no writes, or has stopped. Class `busy`.
pub const ENGINE_DEGRADED: FailureCode = FailureCode::fixed("engine_degraded");
/// A wait for a record lock ran out. Class `busy`. The same code the ledger
/// answers with when the lock is held.
pub const RECORD_LOCKED: FailureCode = a2m_ledger::codes::RECORD_LOCKED;
/// A wait for a place on a session ran out. Class `busy`.
pub const SESSION_IN_USE: FailureCode = FailureCode::fixed("session_in_use");
/// A wait for an exclusive resource ran out. Class `busy`.
pub const RESOURCE_IN_USE: FailureCode = FailureCode::fixed("resource_in_use");
/// A job asked for an exclusive resource that sorts before one it holds.
/// Class `busy`.
pub const RESOURCE_ORDER: FailureCode = FailureCode::fixed("resource_order");
/// A rate budget is spent and does not refill inside the lease. Class `busy`.
pub const RATE_LIMITED: FailureCode = FailureCode::fixed("rate_limited");
/// The work is covered by a hold, such as the stop-everything switch. Class
/// `busy`.
pub const ON_HOLD: FailureCode = FailureCode::fixed("on_hold");
/// A call carries a lease that is not the job's current one, or one that has
/// run out. Class `policy`.
pub const LEASE_SUPERSEDED: FailureCode = FailureCode::fixed("lease_superseded");
/// A runner that never said `hello` asked for work. Class `policy`.
pub const RUNNER_NOT_ENROLLED: FailureCode = FailureCode::fixed("runner_not_enrolled");
/// Written on a run whose lease was lost. Class `possible_commit`. The same
/// code the ledger answers with to a call under an old lease.
pub const RUNNER_LOST: FailureCode = a2m_ledger::codes::RUNNER_LOST;
/// A runner speaks a protocol version this scheduler does not. Class
/// `unknown_variant`.
pub const RUNNER_PROTOCOL_UNSUPPORTED: FailureCode =
    FailureCode::fixed("runner_protocol_unsupported");
/// There is no such job. Class `input`.
pub const NOT_FOUND: FailureCode = a2m_ledger::codes::NOT_FOUND;

/// "in about N seconds", rounded up to a whole second and never zero.
fn about(wait: Duration) -> String {
    let seconds = wait.as_millis().div_ceil(1000).max(1);
    if seconds == 1 {
        "in about 1 second".to_owned()
    } else {
        format!("in about {seconds} seconds")
    }
}

fn busy(code: FailureCode, message: impl Into<String>, next_step: impl Into<String>) -> Failure {
    Failure::new(FailureClass::Busy, code, message, next_step)
}

pub(crate) fn queue_full(length: u32, retry: Duration) -> Failure {
    busy(
        QUEUE_FULL,
        format!("The queue of this target is full: {length} jobs are waiting."),
        format!("Try again {}.", about(retry)),
    )
}

pub(crate) fn caller_queue_full(cap: u32) -> Failure {
    busy(
        CALLER_QUEUE_FULL,
        format!("This caller already has {cap} jobs waiting, which is as many as it may."),
        "Wait for one of them to start, or cancel one, then try again.",
    )
}

pub(crate) fn runner_unavailable(missing: &str) -> Failure {
    busy(
        RUNNER_UNAVAILABLE,
        format!("No runner can serve this job: {missing}."),
        "Start or enroll a runner that offers what is missing, then try again.",
    )
}

pub(crate) fn engine_degraded(stopped: bool, cause: &FailureCode) -> Failure {
    let message = if stopped {
        format!("The engine has stopped and takes no work (`{cause}`).")
    } else {
        format!("The engine is degraded and takes no writes (`{cause}`).")
    };
    busy(
        ENGINE_DEGRADED,
        message,
        "Run `anything2mcp doctor run`, repair what it names, then try again.",
    )
}

pub(crate) fn record_locked(retry: Duration) -> Failure {
    busy(
        RECORD_LOCKED,
        "Another run still holds the lock on this record.",
        format!(
            "Try again {}, when that run has finished or been reconciled.",
            about(retry)
        ),
    )
}

pub(crate) fn session_in_use() -> Failure {
    busy(
        SESSION_IN_USE,
        "The session already carries as much work at once as it may.",
        "Try again when the work on that session has finished.",
    )
}

pub(crate) fn resource_in_use(retry: Duration) -> Failure {
    busy(
        RESOURCE_IN_USE,
        "Another job still holds a resource this job must hold alone.",
        format!("Try again {}.", about(retry)),
    )
}

pub(crate) fn resource_order() -> Failure {
    busy(
        RESOURCE_ORDER,
        "The job asked for a resource that sorts before one it already holds; resources are taken in key order so that two jobs never wait for each other.",
        "Release the resources that sort after the one asked for, then ask for all of them together.",
    )
}

pub(crate) fn rate_limited(retry: Duration) -> Failure {
    busy(
        RATE_LIMITED,
        "The request budget is spent.",
        format!("Try again {}.", about(retry)),
    )
}

pub(crate) fn on_hold(scope: &str) -> Failure {
    busy(
        ON_HOLD,
        format!("No work is handed out for this {scope} while it is on hold."),
        "Release the hold, then try again.",
    )
}

pub(crate) fn lease_superseded(what: &str) -> Failure {
    Failure::new(
        FailureClass::Policy,
        LEASE_SUPERSEDED,
        format!("The lease is not valid: {what}."),
        "Stop work on this job; it is recovered under a new lease.",
    )
}

pub(crate) fn runner_not_enrolled() -> Failure {
    Failure::new(
        FailureClass::Policy,
        RUNNER_NOT_ENROLLED,
        "The runner is not known to the scheduler.",
        "Say `hello` first, then ask for work.",
    )
}

pub(crate) fn runner_lost() -> Failure {
    Failure::new(
        FailureClass::PossibleCommit,
        RUNNER_LOST,
        "The runner's lease ended without a result, so a write may or may not have been sent.",
        "Wait for the run to be reconciled, then resume it.",
    )
}

pub(crate) fn protocol_unsupported(offered: u32) -> Failure {
    Failure::new(
        FailureClass::UnknownVariant,
        RUNNER_PROTOCOL_UNSUPPORTED,
        format!("The runner speaks protocol version {offered}, which this scheduler does not."),
        "Upgrade the runner or the control plane so that their versions are at most one apart.",
    )
}

pub(crate) fn not_found(what: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        NOT_FOUND,
        format!("There is no such {what}."),
        "Check the identifier.",
    )
}

pub(crate) fn invalid(what: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        FailureCode::SCHEMA_INVALID,
        format!("The scheduler was given something it cannot take: {what}."),
        "Correct the input.",
    )
}

pub(crate) fn corrupt(what: &str) -> Failure {
    busy(
        a2m_ledger::codes::STORE_CORRUPT,
        format!("The store holds something the scheduler cannot read: {what}."),
        "Stop writing, keep the data directory as it is, and restore the last backup.",
    )
}

pub(crate) fn stopped() -> Failure {
    busy(
        a2m_ledger::codes::STORE_UNAVAILABLE,
        "The scheduler has stopped.",
        "Try again; if it lasts, run `anything2mcp doctor run`.",
    )
}
