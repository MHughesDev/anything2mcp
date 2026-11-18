//! The failures this crate produces. Each code belongs to exactly one class.
//!
//! A message is built from names, states, and counts, never from a value a
//! system returned or a caller sent.

use a2m_core::Failure;

pub use crate::codes::CHANGED_SINCE_PREVIEW;

pub(crate) fn unavailable(what: &str) -> Failure {
    crate::codes::row::STORE_UNAVAILABLE.failure(
        format!("The store cannot be used: {what}."),
        "Try again; if it lasts, run `anything2mcp doctor run`.",
    )
}

pub(crate) fn full() -> Failure {
    crate::codes::row::STORAGE_FULL.failure(
        "The store has no space left to write.",
        "Free space in the data directory, then try again.",
    )
}

pub(crate) fn corrupt(what: &str) -> Failure {
    crate::codes::row::STORE_CORRUPT.failure(
        format!("The store failed a check of its own content: {what}."),
        "Stop writing, keep the data directory as it is, and restore the last backup.",
    )
}

pub(crate) fn version_unsupported(what: &str) -> Failure {
    crate::codes::row::STORE_VERSION_UNSUPPORTED.failure(
        format!("This engine cannot open the store: {what}."),
        "Open the store with the engine version that last wrote it, or a newer one.",
    )
}

pub(crate) fn locked() -> Failure {
    crate::codes::row::RECORD_LOCKED.failure(
        "Another run holds the lock on this record.",
        "Wait for that run to finish or to be reconciled, then try again.",
    )
}

pub(crate) fn runner_lost() -> Failure {
    crate::codes::row::RUNNER_LOST.failure(
        "The call carries a lease number that is not the run's current one.",
        "Stop; the run is reconciled under its current lease.",
    )
}

pub(crate) fn approval_missing(what: &str) -> Failure {
    crate::codes::row::APPROVAL_MISSING.failure(
        format!("The run has no clearance to write: {what}."),
        "Ask for the diff to be approved, then run again.",
    )
}

pub(crate) fn approval_stale() -> Failure {
    crate::codes::row::APPROVAL_STALE.failure(
        "The clearance was given for another diff than the run's.",
        "Preview again and ask for the new diff to be approved.",
    )
}

pub(crate) fn read_back_differs() -> Failure {
    crate::codes::row::READ_BACK_DIFFERS.failure(
        "The value read back is not the value intended.",
        "Read the record again; if it still differs, stop and look at the evidence.",
    )
}

pub(crate) fn unexpected(what: impl AsRef<str>) -> Failure {
    crate::codes::row::LEDGER_STATE_UNEXPECTED.failure(
        format!(
            "The ledger refused a change no rule allows: {}.",
            what.as_ref()
        ),
        "Read the run's state again before doing anything else.",
    )
}

pub(crate) fn not_owned(what: &str) -> Failure {
    crate::codes::row::TABLE_NOT_OWNED.failure(
        format!("A statement was refused: {what}."),
        "A crate reaches only its own tables; use the owner's interface for the others.",
    )
}

pub(crate) fn not_found(what: &str) -> Failure {
    crate::codes::row::NOT_FOUND.failure(
        format!("There is no such {what} in the store."),
        "Check the identifier.",
    )
}

pub(crate) fn invalid(what: &str) -> Failure {
    a2m_core::codes::row::SCHEMA_INVALID.failure(
        format!("The store was given something it cannot keep: {what}."),
        "Correct the input.",
    )
}

pub(crate) fn reconcile_required() -> Failure {
    crate::codes::row::RECONCILE_REQUIRED.failure(
        "A record of this run may have been written under an earlier lease and has not been read back.",
        "Reconcile the run: read each record in doubt and settle it, then continue.",
    )
}

pub(crate) fn changed_since_preview() -> Failure {
    crate::codes::row::CHANGED_SINCE_PREVIEW.failure(
        "The record holds neither the values intended nor the values seen before the write.",
        "Preview again and ask for the new diff to be approved.",
    )
}
