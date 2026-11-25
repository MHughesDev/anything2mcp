//! The failures this crate produces. Each code belongs to exactly one class.
//!
//! A message is built from names, counts, and times, never from a value a
//! system returned or a caller sent.

use std::time::Duration;

use a2m_core::{Failure, FailureClass, FailureCode};

use crate::types::Reconciling;

pub use crate::codes::{
    CALLER_QUEUE_FULL, ENGINE_DEGRADED, LEASE_SUPERSEDED, NOT_FOUND, QUEUE_FULL, RATE_LIMITED,
    RECORD_LOCKED, RESOURCE_IN_USE, RESOURCE_ORDER, RUN_RECONCILING, RUNNER_LOST,
    RUNNER_NOT_ENROLLED, RUNNER_PROTOCOL_UNSUPPORTED, RUNNER_UNAVAILABLE, SCHEDULER_PAUSED,
    SESSION_IN_USE,
};

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
    .with_retry_after(retry)
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
    .with_retry_after(retry)
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
    .with_retry_after(retry)
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
    .with_retry_after(retry)
}

pub(crate) fn scheduler_paused(scope: &str) -> Failure {
    busy(
        SCHEDULER_PAUSED,
        format!("No work is handed out for this {scope} while it is paused."),
        "Release the pause, then try again.",
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

/// When the reconcile a resume found reads the system, in words.
fn reconcile_when(found: &Reconciling) -> String {
    if found.leased {
        "A runner is reading the system for it now; if it ends in doubt, the next reconcile is handed out at once".to_owned()
    } else if let Some(left) = found.quarantine_left {
        format!(
            "It is handed out {}, when the quarantine that began with a lost lease has passed: until then the lost runner may still be sending",
            about(left)
        )
    } else {
        "It is handed out now".to_owned()
    }
}

pub(crate) fn run_reconciling(found: &Reconciling) -> Failure {
    busy(
        RUN_RECONCILING,
        format!(
            "The run is being reconciled: what may have been written is read before the run goes on. {}.",
            reconcile_when(found)
        ),
        "Wait for the reconcile to end, then resume the run again.",
    )
}

/// The refusal of [`crate::Scheduler::enqueue`] for a run whose reconcile is
/// its one job. The code stays `schema_invalid`, which its callers read as
/// "the run has its job"; [`crate::Scheduler::resume`] names it.
pub(crate) fn reconciling_refused(found: &Reconciling) -> Failure {
    Failure::new(
        FailureClass::Input,
        FailureCode::SCHEMA_INVALID,
        format!(
            "The run already has its one job, a reconcile, which settles what is in doubt first. {}.",
            reconcile_when(found)
        ),
        "Wait for the reconcile to end, then queue the job again.",
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[test]
    fn a_computed_wait_is_carried_as_data_as_well_as_in_words() {
        let wait = Duration::from_millis(2_500);
        for failure in [
            super::queue_full(3, wait),
            super::record_locked(wait),
            super::resource_in_use(wait),
            super::rate_limited(wait),
        ] {
            assert_eq!(failure.retry_after_ms(), Some(2_500), "{failure}");
            assert!(
                failure.next_step().contains("in about 3 seconds"),
                "{failure}"
            );
        }
        assert_eq!(super::session_in_use().retry_after_ms(), None);
    }
}
