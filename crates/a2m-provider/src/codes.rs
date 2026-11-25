//! The failure codes this crate produces. Each belongs to exactly one class.
//!
//! Whether a `busy` code passes by itself, and why:
//!
//! - `provider_unavailable` does not: the provider is missing something
//!   here, such as a program it needs, or a setting names a provider that
//!   brings no driver, and a person changes that.
//! - `queue_full` does: the queue drains as its work is done.
//! - `cleanup_incomplete` does not: what an earlier stop left running is
//!   ended by a person, as `anything2mcp doctor` shows it.
//! - `subsystem_limit` does: a running subsystem finishes, or lets go of
//!   the place.
//! - `shutting_down` does not: a supervisor that is stopping never starts
//!   again; the engine is started anew.
//! - `start_failed` does not: the operating system refused the program, or
//!   the engine's own state is broken.
//! - `path_locked` does not: the holder is another program, which a person
//!   closes.
//! - `disk_full` does not: a person frees space.

use a2m_core::{CodeEntry, Failure, FailureCode};

a2m_core::failure_codes! {
    MANIFEST_INVALID = Input, "manifest_invalid",
        meaning "A manifest does not parse, or a field of it is wrong.",
        next "Correct the field of the provider's manifest the message names, then load it again.";
    CONTRACT_UNSUPPORTED = Input, "contract_unsupported",
        meaning "A manifest was written for a contract this build does not accept.",
        next "Use a version of the provider written for this contract.";
    MODE_CONFLICT = Input, "mode_conflict",
        meaning "A mode or surface kind is registered twice, or with two archetypes.",
        next "State each mode and surface kind once, with its one archetype, or disable one of the two providers.";
    PATH_NOT_ALLOWED = Input, "path_not_allowed",
        meaning "A path or a link leaves the folder it must stay inside, or is not a form a path may take.",
        next "Name a file inside its root by a relative path with forward slashes and no `..`, or a folder by its absolute path where a setting asks for one.";
    FILE_UNAVAILABLE = Input, "file_unavailable",
        meaning "A file under a checked path could not be read or written.",
        next "Check that the file exists and that the engine may use it, then try again.";
    PATH_MISSING = Input, "path_missing",
        meaning "A file, or the folder that holds it, is not there.",
        next "Make the file and its folder exist, or name one that does.";
    PATH_DENIED = Input, "path_denied",
        meaning "The engine's user may not write the file, or the file system is read only.",
        next "Let the engine's user write the file and its folder, or name another file.";
    PROJECT_FOLDER_MISSING = Input, "project_folder_missing",
        meaning "The run has no project folder, and a file of the project was asked for.",
        next "Call the tool from inside the project, through a door that states its folder.";
    PROVIDER_AMBIGUOUS = Input, "provider_ambiguous",
        meaning "Two ready providers drive one mode and no setting names one.",
        next "Name the provider for the mode in the settings, or disable one of the two.";
    MODE_UNKNOWN = Input, "mode_unknown",
        meaning "Nobody registered the mode.",
        next "Enable the provider that brings the mode; `anything2mcp provider list` shows them.";
    NOT_REGISTERED = Input, "not_registered",
        meaning "Nobody registered the reader, action, or discoverer asked for.",
        next "Enable the provider that brings it.";
    KIND_OWNER_MISSING = Input, "kind_owner_missing",
        meaning "A kind a provider reads has no enabled owner.",
        next "Enable the provider that owns the kind, as the message names it.";
    KIND_OWNER_CONFLICT = Input, "kind_owner_conflict",
        meaning "Two enabled providers own one kind for one surface kind.",
        next "Disable one of the two providers; a kind has one owner for a surface kind.";
    PROVIDER_DUPLICATE = Input, "provider_duplicate",
        meaning "Two providers carry one name.",
        next "Remove or disable one of the two.";
    PACK_SETTINGS_INVALID = Input, "pack_settings_invalid",
        meaning "A pack's settings group does not fit the schema its manifest declares.",
        next "Correct the setting, or remove it to take the default.";
    PROGRAM_NOT_FOUND = Input, "program_not_found",
        meaning "A program to start was not found where it was declared.",
        next "Install the program at the absolute path that is declared, or correct the declaration.";
    DECOMPILER_SPEC_INVALID = Input, "decompiler_spec_invalid",
        meaning "A decompiling engine is stated with no pinned digest, or without one `{file}` argument.",
        next "State the digests of the engine's releases and one `{file}` argument.";
    EFFECT_NOT_DECLARED = Policy, "effect_not_declared",
        meaning "A registration, a start, or a root beyond what the manifest declares.",
        next "Declare it in the provider's manifest, where a person can review it, or keep to what the manifest declares.";
    FILE_NOT_ON_THIS_MACHINE = Policy, "file_not_on_this_machine",
        meaning "A file's bytes are not on this machine: a sync program keeps them elsewhere, and reading or replacing the file would make it fetch them. Nothing was read or written.",
        next "In the sync program's own settings, make the file kept on this machine, then try again.";
    SIGNATURE_INVALID = Policy, "signature_invalid",
        meaning "A plug-in's signature does not verify.",
        next "Install the plug-in again from its author, signed by a key this engine trusts.";
    INTENT_MISSING = Policy, "intent_missing",
        meaning "A step that commits was given no intent token, or the wrong one.",
        next "Store the record's intent in the ledger first, and pass the token it gives for this record.";
    READ_ONLY_POSTURE = Policy, "read_only_posture",
        meaning "A step that commits was asked for where nothing may be written.",
        next "Run the operation live, or against a declared test record.";
    DECOMPILER_NOT_PINNED = Policy, "decompiler_not_pinned",
        meaning "The program named as the decompiling engine has a digest the product does not pin, or it changed after it was looked at. It is not started.",
        next "Install a release of the engine the product pins, or use a product version that pins the one you have.";
    ENGINE_VARIABLE_REFUSED = Policy, "engine_variable_refused",
        meaning "A launch names a variable that belongs to the engine.",
        next "Remove the variable from the launch; a program is given only variables of its own.";
    VARIABLE_REFUSED = Policy, "variable_refused",
        meaning "A launch names a variable that makes a program load code other than its own.",
        next "Remove the variable from the target's files: a program is never told through its environment to load other code.";
    DECOMPILER_NOT_INSTALLED = Busy, "decompiler_not_installed",
        meaning "No decompiling engine is installed where the settings say, or none is named. The scout reports it and goes on.",
        next "Install the engine yourself and name its program in the settings; the product never fetches one.",
        passes not_by_itself;
    PROVIDER_UNAVAILABLE = Busy, "provider_unavailable",
        meaning "The provider is not ready here, now.",
        next "Run `anything2mcp doctor` to see what the provider needs, or name another provider for the mode.",
        passes not_by_itself;
    QUEUE_FULL = Busy, "queue_full",
        meaning "A queue is full.",
        next "Call again after the wait the failure gives.",
        passes by_itself;
    CLEANUP_INCOMPLETE = Busy, "cleanup_incomplete",
        meaning "Leftovers of an earlier stop block a new start.",
        next "Run `anything2mcp doctor` to see what remains, end it, and try again.",
        passes not_by_itself;
    SUBSYSTEM_LIMIT = Busy, "subsystem_limit",
        meaning "As many subsystems of this kind run as may, or the place is taken.",
        next "Call again when one has finished, or raise the limit in the settings.",
        passes by_itself;
    SHUTTING_DOWN = Busy, "shutting_down",
        meaning "The supervisor is stopping and starts nothing.",
        next "Start the engine again.",
        passes not_by_itself;
    START_FAILED = Busy, "start_failed",
        meaning "A program could not be started by the operating system.",
        next "Check that the program can be run on this machine, then try again.",
        passes not_by_itself;
    PATH_LOCKED = Busy, "path_locked",
        meaning "Another program holds the file, or a lock on it.",
        next "Close the program that holds the file, then call again.",
        passes not_by_itself;
    DISK_FULL = Busy, "disk_full",
        meaning "The disk, or the user's share of it, is full.",
        next "Free space on the disk, then call again.",
        passes not_by_itself;
    DECOMPILER_FAILED = Rejected, "decompiler_failed",
        meaning "A decompiling engine ended without success on the file it was handed. Nothing it printed was used.",
        next "Try the file again, or hand the engine a file it can read.";
    DECOMPILER_OUTPUT_UNKNOWN = UnknownVariant, "decompiler_output_unknown",
        meaning "A decompiling engine printed something that is not in the form the product reads; none of it was used.",
        next "Use an engine and an adapter that print one JSON object to a line, as the decompiler's documentation says.";
    HANDSHAKE_FAILED = UnknownVariant, "handshake_failed",
        meaning "A subsystem failed its handshake.",
        next "Check that the subsystem's version is one this engine supports, and report it to the provider's author if it is.";
    UNDECLARED_CODE = UnknownVariant, "undeclared_code",
        meaning "A driver returned a failure code nothing declares.",
        next "Report this to the provider's author; every failure code is declared in the manifest.";
    UNEXPECTED_ABSENT = UnknownVariant, "unexpected_absent",
        meaning "A step that is no lookup reported its record absent.",
        next "Report this to the provider's author; only a lookup may find nothing.";
    STEP_KIND_UNKNOWN = UnknownVariant, "step_kind_unknown",
        meaning "A step of a kind its mode did not declare.",
        next "Compile the target again with the provider that is enabled now.";
    FIXTURE_NO_ANSWER = UnknownVariant, "fixture_no_answer",
        meaning "An offline test's fixture records no answer for a step.",
        next "Record the step's answer in the test's fixture, as a file named after it.";
    RECONCILE_TOO_LATE = UnknownVariant, "reconcile_too_late",
        meaning "A committing step returned values for reconcile, which is too late.",
        next "Read the record back; then report this to the provider's author.";
    FINDING_NOT_DECLARED = UnknownVariant, "finding_not_declared",
        meaning "A discoverer made a finding of a kind or level it did not declare.",
        next "Report this to the provider's author; a discoverer returns only the kinds and levels it declares.";
    DISCOVERER_CANNOT_WATCH = UnknownVariant, "discoverer_cannot_watch",
        meaning "A discoverer its manifest says a person drives was built without the method that watches a capture, so nothing was recorded.",
        next "Report this to the provider's author; a discoverer driven by a person implements `watch`.";
    CONNECTION_LOST = Transient, "connection_lost",
        meaning "A stream to a subsystem broke.",
        next "Try again; if the stream keeps breaking, run `anything2mcp doctor`.";
    TIMEOUT = Transient, "timeout",
        meaning "A deadline passed on something that commits nothing.",
        next "Try again; if the run was to write, read the record back before anything is written again.";
    NO_CONFIRMATION = PossibleCommit, "no_confirmation",
        meaning "A write was sent and nothing says whether it happened.",
        next "Reconcile the record: read it back before anything is written again.";
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

/// A failure of the code of `row`, in the class its table gives it.
pub(crate) fn failure(
    row: &CodeEntry,
    message: impl Into<String>,
    next_step: impl Into<String>,
) -> Failure {
    row.failure(message, next_step)
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
