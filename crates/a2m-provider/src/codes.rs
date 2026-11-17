//! The failure codes this crate produces. Each belongs to exactly one class.

use a2m_core::{Failure, FailureClass, FailureCode};

macro_rules! codes {
    ($( $(#[$doc:meta])* $name:ident = $class:ident, $text:literal; )+) => {
        $( $(#[$doc])* pub const $name: FailureCode = FailureCode::fixed($text); )+

        /// Every code of this crate with its one class.
        pub const ALL: &[(FailureClass, &str)] = &[ $( (FailureClass::$class, $text) ),+ ];
    };
}

codes! {
    /// A manifest does not parse, or a field of it is wrong.
    MANIFEST_INVALID = Input, "manifest_invalid";
    /// A manifest was written for a contract this build does not accept.
    CONTRACT_UNSUPPORTED = Input, "contract_unsupported";
    /// A mode or surface kind is registered twice, or with two archetypes.
    MODE_CONFLICT = Input, "mode_conflict";
    /// A path is outside its root, or is not a form a path may take.
    PATH_NOT_ALLOWED = Input, "path_not_allowed";
    /// A file under a checked path could not be read or written.
    FILE_UNAVAILABLE = Input, "file_unavailable";
    /// A file, or the folder that holds it, is not there.
    FILE_MISSING = Input, "file_missing";
    /// The engine's user may not write the file, or the file system is read
    /// only.
    FILE_ACCESS_DENIED = Input, "file_access_denied";
    /// The run has no project folder, and a file of the project was asked
    /// for.
    PROJECT_FOLDER_MISSING = Input, "project_folder_missing";
    /// Two ready providers drive one mode and no setting names one.
    PROVIDER_AMBIGUOUS = Input, "provider_ambiguous";
    /// Nobody registered the mode.
    MODE_UNKNOWN = Input, "mode_unknown";
    /// Nobody registered the reader, action, or discoverer asked for.
    NOT_REGISTERED = Input, "not_registered";
    /// A kind a provider reads has no enabled owner.
    KIND_OWNER_MISSING = Input, "kind_owner_missing";
    /// Two enabled providers own one kind for one surface kind.
    KIND_OWNER_CONFLICT = Input, "kind_owner_conflict";
    /// Two providers carry one name.
    PROVIDER_DUPLICATE = Input, "provider_duplicate";
    /// A pack's settings group does not fit the schema its manifest declares.
    PACK_SETTINGS_INVALID = Input, "pack_settings_invalid";
    /// A program to start was not found where it was declared.
    PROGRAM_NOT_FOUND = Input, "program_not_found";
    /// A registration, a start, or a root beyond what the manifest declares.
    EFFECT_NOT_DECLARED = Policy, "effect_not_declared";
    /// A plug-in's signature does not verify.
    SIGNATURE_INVALID = Policy, "signature_invalid";
    /// A step that commits was given no intent token, or the wrong one.
    INTENT_MISSING = Policy, "intent_missing";
    /// A step that commits was asked for where nothing may be written.
    READ_ONLY_POSTURE = Policy, "read_only_posture";
    /// A launch names a variable that belongs to the engine.
    ENGINE_VARIABLE_REFUSED = Policy, "engine_variable_refused";
    /// The provider is not ready here, now.
    PROVIDER_UNAVAILABLE = Busy, "provider_unavailable";
    /// A subsystem's queue is full.
    QUEUE_FULL = Busy, "queue_full";
    /// Leftovers of an earlier stop block a new start.
    CLEANUP_INCOMPLETE = Busy, "cleanup_incomplete";
    /// As many subsystems of this kind run as may, or the place is taken.
    SUBSYSTEM_LIMIT = Busy, "subsystem_limit";
    /// The supervisor is stopping and starts nothing.
    SHUTTING_DOWN = Busy, "shutting_down";
    /// A program could not be started by the operating system.
    START_FAILED = Busy, "start_failed";
    /// Another program holds the file, or a lock on it.
    FILE_LOCKED = Busy, "file_locked";
    /// The disk, or the user's share of it, is full.
    STORAGE_FULL = Busy, "storage_full";
    /// A subsystem failed its handshake.
    HANDSHAKE_FAILED = UnknownVariant, "handshake_failed";
    /// A driver returned a failure code nothing declares.
    UNDECLARED_CODE = UnknownVariant, "undeclared_code";
    /// A step that is no lookup reported its record absent.
    UNEXPECTED_ABSENT = UnknownVariant, "unexpected_absent";
    /// A step of a kind its mode did not declare.
    STEP_KIND_UNKNOWN = UnknownVariant, "step_kind_unknown";
    /// A committing step returned values for reconcile, which is too late.
    RECONCILE_TOO_LATE = UnknownVariant, "reconcile_too_late";
    /// A discoverer made a finding of a kind or level it did not declare.
    FINDING_NOT_DECLARED = UnknownVariant, "finding_not_declared";
    /// A stream to a subsystem broke.
    CONNECTION_LOST = Transient, "connection_lost";
    /// A deadline passed on something that commits nothing.
    TIMEOUT = Transient, "timeout";
    /// A write was sent and nothing says whether it happened.
    NO_CONFIRMATION = PossibleCommit, "no_confirmation";
}

/// Codes that state the kernel's own bookkeeping about a run, a lease, or
/// an approval, and never anything a system answered. The engine, the
/// ledger, and the scheduler above and below this contract produce them,
/// and some of their logic goes by the code alone. No provider may declare
/// one, and the handle lets no driver return one, whatever class it gives.
///
/// The list is kept here because this contract is where a provider's codes
/// are checked, and it cannot see the crates above it.
pub const KERNEL_ONLY: &[&str] = &[
    "blocked_by_parent",
    "runner_lost",
    "cannot_reconcile",
    "reconcile_required",
    "approval_missing",
    "approval_stale",
    "lease_superseded",
];

/// Whether the code is one of [`KERNEL_ONLY`].
#[must_use]
pub fn is_kernel_only(code: &FailureCode) -> bool {
    KERNEL_ONLY.contains(&code.as_str())
}

pub(crate) fn failure(
    class: FailureClass,
    code: FailureCode,
    message: impl Into<String>,
    next_step: impl Into<String>,
) -> Failure {
    Failure::new(class, code, message, next_step)
}

pub(crate) fn input(
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    failure(FailureClass::Input, code, message, next)
}

pub(crate) fn policy(
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    failure(FailureClass::Policy, code, message, next)
}

pub(crate) fn busy(
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    failure(FailureClass::Busy, code, message, next)
}

pub(crate) fn unknown_variant(
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    failure(FailureClass::UnknownVariant, code, message, next)
}

/// Text from outside, cut before it is shown in a message.
pub(crate) fn shown(text: &str) -> String {
    const SHOWN: usize = 80;
    let clean: String = text
        .chars()
        .filter(|c| !c.is_control())
        .take(SHOWN)
        .collect();
    if text.chars().count() > SHOWN {
        format!("{clean}…")
    } else {
        clean
    }
}
