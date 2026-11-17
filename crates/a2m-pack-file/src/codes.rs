//! The failure codes of the file pack. Each belongs to exactly one class,
//! and each is declared in the pack's manifest.

use a2m_core::{Failure, FailureClass, FailureCode};
use a2m_provider::StepFailure;

/// The file is not at its declared path.
pub const FILE_MISSING: FailureCode = FailureCode::fixed("file_missing");
/// The file does not parse as its declared format.
pub const FILE_UNREADABLE: FailureCode = FailureCode::fixed("file_unreadable");
/// A named place, a header, a table, a name, or a key is not in the file.
pub const PLACE_NOT_FOUND: FailureCode = FailureCode::fixed("place_not_found");
/// Two rows or elements share one key.
pub const KEY_NOT_UNIQUE: FailureCode = FailureCode::fixed("key_not_unique");
/// The file's layout is not the one the document declares.
pub const FINGERPRINT_MISMATCH: FailureCode = FailureCode::fixed("fingerprint_mismatch");
/// The patch editor would change something it was not asked to change.
pub const ROUND_TRIP_FAILED: FailureCode = FailureCode::fixed("round_trip_failed");
/// A formula where a value was declared.
pub const FORMULA_AT_PLACE: FailureCode = FailureCode::fixed("formula_at_place");
/// The old file's permissions cannot be carried to the new file.
pub const METADATA_NOT_KEPT: FailureCode = FailureCode::fixed("metadata_not_kept");
/// A place holds something its field type cannot read.
pub const VALUE_UNREADABLE: FailureCode = FailureCode::fixed("value_unreadable");
/// A shape of file, place, or value the pack has no rule for.
pub const SHAPE_UNSUPPORTED: FailureCode = FailureCode::fixed("shape_unsupported");
/// The file is encrypted, signed, or marked read-only.
pub const FILE_PROTECTED: FailureCode = FailureCode::fixed("file_protected");
/// The file is larger than the pack's limits allow.
pub const FILE_TOO_LARGE: FailureCode = FailureCode::fixed("file_too_large");
/// The lock signal is present, or the system refuses to share the file.
pub const FILE_LOCKED: FailureCode = FailureCode::fixed("file_locked");
/// The disk or the folder is full.
pub const STORAGE_FULL: FailureCode = FailureCode::fixed("storage_full");
/// The file changed between the read and the replace.
pub const CHANGED_SINCE_PREVIEW: FailureCode = FailureCode::fixed("changed_since_preview");
/// The plan's section for this pack does not decode.
pub const PAYLOAD_UNREADABLE: FailureCode = FailureCode::PAYLOAD_UNREADABLE;

/// Every code of the pack with its one class, as the manifest declares them.
pub const ALL: &[(FailureClass, FailureCode)] = &[
    (FailureClass::Drift, FILE_MISSING),
    (FailureClass::Drift, FILE_UNREADABLE),
    (FailureClass::Drift, PLACE_NOT_FOUND),
    (FailureClass::Drift, KEY_NOT_UNIQUE),
    (FailureClass::Drift, FINGERPRINT_MISMATCH),
    (FailureClass::Drift, ROUND_TRIP_FAILED),
    (FailureClass::UnknownVariant, FORMULA_AT_PLACE),
    (FailureClass::UnknownVariant, METADATA_NOT_KEPT),
    (FailureClass::UnknownVariant, VALUE_UNREADABLE),
    (FailureClass::UnknownVariant, SHAPE_UNSUPPORTED),
    (FailureClass::Policy, FILE_PROTECTED),
    (FailureClass::Policy, FILE_TOO_LARGE),
    (FailureClass::Busy, FILE_LOCKED),
    (FailureClass::Busy, STORAGE_FULL),
    (FailureClass::Stale, CHANGED_SINCE_PREVIEW),
    (FailureClass::Input, PAYLOAD_UNREADABLE),
];

fn class_of(code: &FailureCode) -> FailureClass {
    ALL.iter()
        .find(|(_, known)| known == code)
        .map_or(FailureClass::UnknownVariant, |(class, _)| *class)
}

/// A failure of one of the pack's codes, in that code's class.
pub(crate) fn failure(
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    Failure::new(class_of(&code), code, message, next)
}

/// A failure before anything was written.
pub(crate) fn stop(
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> StepFailure {
    StepFailure::not_sent(failure(code, message, next))
}

/// The plan does not hold what the driver needs. Nothing was read.
pub(crate) fn plan_unreadable(what: &str) -> StepFailure {
    stop(
        PAYLOAD_UNREADABLE,
        format!("The plan cannot be used by the file pack: {what}."),
        "Compile the target again with the version of the file pack that is enabled.",
    )
}

/// A shape the pack has no rule for. Nothing was written.
pub(crate) fn unsupported(what: &str) -> StepFailure {
    stop(
        SHAPE_UNSUPPORTED,
        format!("The file pack has no rule for this: {what}."),
        "Change the document or the file so that the pack's rules cover it, or report the missing rule.",
    )
}
