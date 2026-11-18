//! The failure codes this crate produces, each with its one class. The codes
//! of a watch are the last fourteen rows, which `WATCH_CODES` lists.
//!
//! Whether a `busy` code passes by itself, and why:
//!
//! - `storage_full` does not: a person frees space in the data directory.
//! - `store_unavailable` does not: most often the store is closed, its
//!   threads have stopped, or its file cannot be opened, and only a restart
//!   or a repair brings it back.
//! - `store_corrupt` does not: the store is restored from a backup.
//! - `store_version_unsupported` does not: another engine version opens it.
//! - `record_locked` does: the run that holds the lock finishes or is
//!   reconciled, with nobody acting.
//! - `shadow_write_not_built` does not: it is a part not built yet.
//! - `watch_check_running` does: the check under way ends by itself.
//! - `watch_paused` does not: what paused the watch is settled by its
//!   person, such as a sign-in.
//! - `watch_check_unfinished` does: the check's run reaches a final state
//!   by itself.
//! - `watch_due_missed` does not: it records due times that passed, which
//!   stay missed.
//! - `watch_check_not_completed` does not: the check's run was cancelled or
//!   expired, and stays so.

a2m_core::failure_codes! {
    STORAGE_FULL = Busy, "storage_full",
        meaning "A blob or a row cannot be written for lack of space.",
        next "Free space in the data directory, then try again.",
        passes not_by_itself;
    STORE_UNAVAILABLE = Busy, "store_unavailable",
        meaning "The database cannot commit, or the store is not ready.",
        next "Try again; if it lasts, run `anything2mcp doctor run` and repair what it names.",
        passes not_by_itself;
    STORE_CORRUPT = Busy, "store_corrupt",
        meaning "The integrity check, a stored row, a stored plan's hash, or the audit chain fails.",
        next "Stop writing, keep the data directory as it is, and restore the last backup.",
        passes not_by_itself;
    STORE_VERSION_UNSUPPORTED = Busy, "store_version_unsupported",
        meaning "The store was written by a newer engine, or a migration it applied is not the one this engine carries.",
        next "Open the store with the engine version that last wrote it, or a newer one.",
        passes not_by_itself;
    RECORD_LOCKED = Busy, "record_locked",
        meaning "Another run holds the lock on the record.",
        next "Call again when that run has finished or been reconciled, after the wait the failure gives where it gives one.",
        passes by_itself;
    RUNNER_LOST = PossibleCommit, "runner_lost",
        meaning "A call carries a lease number other than the run's, or a lease ended without a result, so a write may or may not have been sent.",
        next "Stop; wait for the run to be reconciled under its current lease, then resume it.";
    APPROVAL_MISSING = Policy, "approval_missing",
        meaning "A write reached the write phase with no usable approval.",
        next "Have a person approve the run's diff, then resume the run.";
    APPROVAL_STALE = Policy, "approval_stale",
        meaning "The approval is expired, used, declined, revoked, or for another diff.",
        next "Preview the run again and have a person approve the new diff.";
    READ_BACK_DIFFERS = Mismatch, "read_back_differs",
        meaning "The digest read back is not the intended one.",
        next "Read the record again; if it still differs, stop and look at the evidence.";
    LEDGER_STATE_UNEXPECTED = UnknownVariant, "ledger_state_unexpected",
        meaning "A transition no rule allows.",
        next "Read the run's state again before doing anything else.";
    TABLE_NOT_OWNED = UnknownVariant, "table_not_owned",
        meaning "A statement named a table its crate does not own.",
        next "Reach the other crate's tables through its own interface.";
    NOT_FOUND = Input, "not_found",
        meaning "The thing asked for is not there.",
        next "Check the identifier, and give one an earlier result gave.";
    RECONCILE_REQUIRED = PossibleCommit, "reconcile_required",
        meaning "A record of the run is still in doubt from an earlier lease and has not been read back.",
        next "Reconcile the run: read each record in doubt and settle it, then continue.";
    CHANGED_SINCE_PREVIEW = Stale, "changed_since_preview",
        meaning "A record no longer matches what the preview saw.",
        next "Preview again and ask for the new diff to be approved.";
    IDEMPOTENCY_KEY_INVALID = Input, "idempotency_key_invalid",
        meaning "An idempotency key that is not 1 to 200 printable characters.",
        next "Send a short printable key, the same for every repeat of one call.";
    SHADOW_NOT_COUNTED = Policy, "shadow_not_counted",
        meaning "A comparison is not counted: the store is an offline test's, or the run it was made beside is not a live read under way.",
        next "Compare a shadow only beside a live read under way.";
    SHADOW_NOT_PROMOTABLE = Policy, "shadow_not_promotable",
        meaning "A person asked to promote a binding the evidence does not let them promote.",
        next "See `anything2mcp shadow report` for how the binding compared, and promote it when it has earned it.";
    SHADOW_WRITE_NOT_BUILT = Busy, "shadow_write_not_built",
        meaning "The shadow of a binding of an operation that writes is not built: its comparison cannot be stored, and it is never promotable through shadow.",
        next "Keep the operation on its verified binding.",
        passes not_by_itself;
    // The codes of a watch: the last fourteen rows, which `WATCH_CODES` lists.
    WATCH_NOT_A_READ = Policy, "watch_not_a_read",
        meaning "A watch was asked for an operation that is not a read.",
        next "Watch an operation whose effect class is `read`.";
    WATCH_NOT_CONFIRMED = Policy, "watch_not_confirmed",
        meaning "A check was asked of a watch its person has not confirmed.",
        next "Ask the person to confirm the watch on their own page.";
    WATCH_NOT_YOURS = Policy, "watch_not_yours",
        meaning "Someone other than the person a watch acts for tried to confirm it.",
        next "Ask the person the watch acts for to confirm it on their own page.";
    WATCH_CHECK_RUNNING = Busy, "watch_check_running",
        meaning "A check of the watch is under way already.",
        next "Wait for the check to end, then ask again.",
        passes by_itself;
    WATCH_PAUSED = Busy, "watch_paused",
        meaning "The watch is paused.",
        next "Read the watch's changes for why it paused, and settle that.",
        passes not_by_itself;
    WATCH_ENDED = Policy, "watch_ended",
        meaning "The watch has ended.",
        next "Ask for a new watch.";
    WATCH_RUN_NOT_ITS_OWN = Policy, "watch_run_not_its_own",
        meaning "A run that is not the watch's own read was offered as its check.",
        next "End the check with the watch's own run and that run's own result.";
    WATCH_NEEDS_CONFIRMATION = Policy, "watch_needs_confirmation",
        meaning "A paused watch cannot go on without a person confirming a new one: its version changed or its read drifted.",
        next "End this watch and ask for a new one, for its person to confirm.";
    WATCH_CHECK_UNFINISHED = Busy, "watch_check_unfinished",
        meaning "A check's run has not reached an end the watch can take.",
        next "End the check when its run reaches a final state.",
        passes by_itself;
    WATCH_DUE_MISSED = Busy, "watch_due_missed",
        meaning "Due times passed with no check: the engine was not running, the watch was paused or held, or a check could not be admitted in time. A gap carries it.",
        next "Keep the engine running while a watch is due; the gap says which due times were missed.",
        passes not_by_itself;
    WATCH_CHECK_NOT_COMPLETED = Busy, "watch_check_not_completed",
        meaning "A check's run ended without completing and gave no failure: it was cancelled, or it expired.",
        next "Look at the check's run for why it ended; the watch goes on with the next check.",
        passes not_by_itself;
    WATCH_KEY_REPEATED = UnknownVariant, "watch_key_repeated",
        meaning "A record's key appears twice in one check: a named fault of that check, never a merge.",
        next "Check the key the operation declares for its records; the watch goes on and compares the next check.";
    WATCH_SHAPES_MIXED = UnknownVariant, "watch_shapes_mixed",
        meaning "A check holds keyed records and a whole result at once, or more than one whole result.",
        next "Check the operation's outputs; the watch goes on and compares the next check.";
    WATCH_RESULT_NOT_ITS_SHAPE = UnknownVariant, "watch_result_not_its_shape",
        meaning "A check's result is not of the shape the watch was asked with: its list output is missing or not a list of records, or a record lacks a field of its key.",
        next "Check the operation's outputs and the key it declares; the watch goes on and compares the next check.";
}

/// Every failure code of a watch the ledger produces, with its one class:
/// the last fourteen rows of the table.
pub const WATCH_CODES: &[(a2m_core::FailureClass, &str)] = ALL.split_at(ALL.len() - 14).1;

#[cfg(test)]
mod tests {
    use super::*;

    /// No source of this crate gives a failure a class of its own: each is
    /// made from a row of a table, which gives it its class. `ledger.rs`
    /// rebuilds a stored failure with the class its row recorded.
    #[test]
    fn every_failure_takes_its_class_from_a_table() {
        fn walk(dir: &std::path::Path, found: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                if path.is_dir() {
                    if name != "tests" {
                        walk(&path, found);
                    }
                } else if name.ends_with(".rs") && name != "tests.rs" && name != "ledger.rs" {
                    let text = std::fs::read_to_string(&path).unwrap();
                    let text = text.split("#[cfg(test)]").next().unwrap();
                    for made in ["Failure::new(", "class: FailureClass::"] {
                        if text.contains(made) {
                            found.push(format!("{}: {made}", path.display()));
                        }
                    }
                }
            }
        }
        let mut found = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut found,
        );
        assert!(
            found.is_empty(),
            "made with a class of their own: {found:?}"
        );
        for row in TABLE {
            let made = row.failure("A case.", row.next);
            assert_eq!((made.class(), made.code().as_str()), (row.class, row.code));
        }
    }
}
