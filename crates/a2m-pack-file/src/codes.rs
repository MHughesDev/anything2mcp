//! The failure codes of the file pack, each with its one class, in one
//! table (decision 0258). Each is declared in the pack's manifest too, and
//! a test holds the two together; [`PASSED_ON`] names the codes of other
//! crates the pack gives as they are, which the manifest declares too.
//!
//! Whether a `busy` code passes by itself, and why: neither does.
//! `file_locked` holds while another program, most often one a person has
//! the file open in, keeps it; `storage_full` holds until someone frees
//! space.

use a2m_core::{CodeEntry, Failure, FailureCode};
use a2m_provider::StepFailure;

a2m_core::failure_codes! {
    FILE_MISSING = Drift, "file_missing",
        meaning "The file is not at its declared path.",
        next "Put the file back, or correct `file` in the document.";
    FILE_UNREADABLE = Drift, "file_unreadable",
        meaning "The file does not parse as its declared format.",
        next "Correct the file, or correct `format` and `dialect` in the document.";
    PLACE_NOT_FOUND = Drift, "place_not_found",
        meaning "A named place, a header, a table, a name, or a key is not in the file.",
        next "Look at what changed in the file, and correct the document or put the place back.";
    KEY_NOT_UNIQUE = Drift, "key_not_unique",
        meaning "Two rows or elements share one key.",
        next "Remove the duplicate from the file, or declare a key that is unique.";
    FINGERPRINT_MISMATCH = Drift, "fingerprint_mismatch",
        meaning "The file's layout is not the one the document declares.",
        next "Look at what changed in the file, and correct the document or put the file's layout back.";
    ROUND_TRIP_FAILED = Drift, "round_trip_failed",
        meaning "The patch editor would change something it was not asked to change.",
        next "Report this with the file's layout; until the editor covers it, the file is read-only.";
    FORMULA_AT_PLACE = UnknownVariant, "formula_at_place",
        meaning "A formula where a value was declared.",
        next "Mark the field `formula: true` to read its kept result, or point it at the cell that holds the value.";
    METADATA_NOT_KEPT = UnknownVariant, "metadata_not_kept",
        meaning "The old file's owner, group, or permissions cannot be carried to the new file.",
        next "Check who owns the folder and the file, and whether the engine's user may give the new file the old one's owner, group, and permissions.";
    VALUE_UNREADABLE = UnknownVariant, "value_unreadable",
        meaning "A place holds something its field type cannot read.",
        next "Correct the value in the file, or the field's type in the document.";
    SHAPE_UNSUPPORTED = UnknownVariant, "shape_unsupported",
        meaning "A shape of file, place, or value the pack has no rule for.",
        next "Change the document or the file so that the pack's rules cover it, or report the missing rule.";
    FILE_PROTECTED = Policy, "file_protected",
        meaning "The file is encrypted, signed, or marked read-only.",
        next "Point the document at a file that is not protected, or remove the mark if the file is meant to be changed.";
    FILE_TOO_LARGE = Policy, "file_too_large",
        meaning "The file is larger than the pack's limits allow.",
        next "Raise the limit the message names, or read or write less.";
    FILE_LOCKED = Busy, "file_locked",
        meaning "The lock signal is present, or the system refuses to share the file.",
        next "Close the file in the other program, then try again.",
        passes not_by_itself;
    STORAGE_FULL = Busy, "storage_full",
        meaning "The disk or the folder is full.",
        next "Free space in the folder, then try again.",
        passes not_by_itself;
    CHANGED_SINCE_PREVIEW = Stale, "changed_since_preview",
        meaning "The file changed between the read and the replace.",
        next "Read the record again, look at the new preview, and approve again.";
    VALUE_BEGINS_FORMULA = Input, "value_begins_formula",
        meaning "A value for delimited text begins with a character that makes a spreadsheet program run the text as a formula.",
        next "Give a value that begins otherwise, or mark the field `may_begin_formula: true` in the document if no spreadsheet program opens the file.";
    NAME_CASE_CONFLICT = Input, "name_case_conflict",
        meaning "A folder holds a name that differs from the one given only in case.",
        next "Give the name exactly as the folder holds it, or choose a name that differs in more than case.";
    FOLDER_NOT_EMPTY = Policy, "folder_not_empty",
        meaning "A folder to remove is not empty.",
        next "Remove its entries first, each as its own write.";
    VALUE_UNPARSABLE = Input, "value_unparsable",
        meaning "A value given to a step its rule refuses, such as a position a growing file's read did not give.",
        next "Give a value of the kind the message names.";
}

/// The compiled target's section for this pack does not decode: the core's
/// code.
pub const PAYLOAD_UNREADABLE: FailureCode = a2m_core::codes::PAYLOAD_UNREADABLE;

/// The codes of other crates the pack gives as they are, which the
/// manifest declares too: the core's `payload_unreadable`, and
/// `value_out_of_range`, which the expression library makes.
pub const PASSED_ON: &[FailureCode] = &[PAYLOAD_UNREADABLE, a2m_core::codes::VALUE_OUT_OF_RANGE];

/// A failure of the code of `row`, in the class its table gives it.
pub(crate) fn failure(
    row: &CodeEntry,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    row.failure(message, next)
}

/// A failure before anything was written.
pub(crate) fn stop(
    row: &CodeEntry,
    message: impl Into<String>,
    next: impl Into<String>,
) -> StepFailure {
    StepFailure::not_sent(failure(row, message, next))
}

/// The compiled target does not hold what the driver needs. Nothing was read.
pub(crate) fn compiled_target_unreadable(what: &str) -> StepFailure {
    stop(
        &a2m_core::codes::row::PAYLOAD_UNREADABLE,
        format!("The compiled target cannot be used by the file pack: {what}."),
        "Compile the target again with the version of the file pack that is enabled.",
    )
}

/// A shape the pack has no rule for. Nothing was written.
pub(crate) fn unsupported(what: &str) -> StepFailure {
    stop(
        &row::SHAPE_UNSUPPORTED,
        format!("The file pack has no rule for this: {what}."),
        "Change the document or the file so that the pack's rules cover it, or report the missing rule.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No source of this crate gives a failure a class of its own: each is
    /// made from a row of a table, which gives it its class.
    #[test]
    fn every_failure_takes_its_class_from_a_table() {
        let found = a2m_core::testing::classes_of_their_own(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &[],
        )
        .unwrap();
        assert!(
            found.is_empty(),
            "made with a class of their own: {found:#?}"
        );
        for row in TABLE {
            let made = failure(row, "A case.", row.next);
            assert_eq!((made.class(), made.code().as_str()), (row.class, row.code));
        }
    }
}
