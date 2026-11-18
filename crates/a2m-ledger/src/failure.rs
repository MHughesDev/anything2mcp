//! The failures this crate produces. Each code belongs to exactly one class.
//!
//! A message is built from names, states, and counts, never from a value a
//! system returned or a caller sent.

use a2m_core::{Failure, FailureClass, FailureCode};

pub use crate::codes::{
    APPROVAL_MISSING, APPROVAL_STALE, CHANGED_SINCE_PREVIEW, LEDGER_STATE_UNEXPECTED, NOT_FOUND,
    READ_BACK_DIFFERS, RECONCILE_REQUIRED, RECORD_LOCKED, RUNNER_LOST, STORAGE_FULL, STORE_CORRUPT,
    STORE_UNAVAILABLE, STORE_VERSION_UNSUPPORTED, TABLE_NOT_OWNED,
};

pub(crate) fn unavailable(what: &str) -> Failure {
    Failure::new(
        FailureClass::Busy,
        STORE_UNAVAILABLE,
        format!("The store cannot be used: {what}."),
        "Try again; if it lasts, run `anything2mcp doctor run`.",
    )
}

pub(crate) fn full() -> Failure {
    Failure::new(
        FailureClass::Busy,
        STORAGE_FULL,
        "The store has no space left to write.",
        "Free space in the data directory, then try again.",
    )
}

pub(crate) fn corrupt(what: &str) -> Failure {
    Failure::new(
        FailureClass::Busy,
        STORE_CORRUPT,
        format!("The store failed a check of its own content: {what}."),
        "Stop writing, keep the data directory as it is, and restore the last backup.",
    )
}

pub(crate) fn version_unsupported(what: &str) -> Failure {
    Failure::new(
        FailureClass::Busy,
        STORE_VERSION_UNSUPPORTED,
        format!("This engine cannot open the store: {what}."),
        "Open the store with the engine version that last wrote it, or a newer one.",
    )
}

pub(crate) fn locked() -> Failure {
    Failure::new(
        FailureClass::Busy,
        RECORD_LOCKED,
        "Another run holds the lock on this record.",
        "Wait for that run to finish or to be reconciled, then try again.",
    )
}

pub(crate) fn runner_lost() -> Failure {
    Failure::new(
        FailureClass::PossibleCommit,
        RUNNER_LOST,
        "The call carries a lease number that is not the run's current one.",
        "Stop; the run is reconciled under its current lease.",
    )
}

pub(crate) fn approval_missing(what: &str) -> Failure {
    Failure::new(
        FailureClass::Policy,
        APPROVAL_MISSING,
        format!("The run has no clearance to write: {what}."),
        "Ask for the diff to be approved, then run again.",
    )
}

pub(crate) fn approval_stale() -> Failure {
    Failure::new(
        FailureClass::Policy,
        APPROVAL_STALE,
        "The clearance was given for another diff than the run's.",
        "Preview again and ask for the new diff to be approved.",
    )
}

pub(crate) fn read_back_differs() -> Failure {
    Failure::new(
        FailureClass::Mismatch,
        READ_BACK_DIFFERS,
        "The value read back is not the value intended.",
        "Read the record again; if it still differs, stop and look at the evidence.",
    )
}

pub(crate) fn unexpected(what: impl AsRef<str>) -> Failure {
    Failure::new(
        FailureClass::UnknownVariant,
        LEDGER_STATE_UNEXPECTED,
        format!(
            "The ledger refused a change no rule allows: {}.",
            what.as_ref()
        ),
        "Read the run's state again before doing anything else.",
    )
}

pub(crate) fn not_owned(what: &str) -> Failure {
    Failure::new(
        FailureClass::UnknownVariant,
        TABLE_NOT_OWNED,
        format!("A statement was refused: {what}."),
        "A crate reaches only its own tables; use the owner's interface for the others.",
    )
}

pub(crate) fn not_found(what: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        NOT_FOUND,
        format!("There is no such {what} in the store."),
        "Check the identifier.",
    )
}

pub(crate) fn invalid(what: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        FailureCode::SCHEMA_INVALID,
        format!("The store was given something it cannot keep: {what}."),
        "Correct the input.",
    )
}

pub(crate) fn reconcile_required() -> Failure {
    Failure::new(
        FailureClass::PossibleCommit,
        RECONCILE_REQUIRED,
        "A record of this run may have been written under an earlier lease and has not been read back.",
        "Reconcile the run: read each record in doubt and settle it, then continue.",
    )
}

pub(crate) fn changed_since_preview() -> Failure {
    Failure::new(
        FailureClass::Stale,
        CHANGED_SINCE_PREVIEW,
        "The record holds neither the values intended nor the values seen before the write.",
        "Preview again and ask for the new diff to be approved.",
    )
}
