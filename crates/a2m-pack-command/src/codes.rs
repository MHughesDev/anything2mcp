//! The failure codes this pack produces, each with its one class, and the
//! names it registers under. The manifest declares the same codes; a test
//! holds the two lists together.
//!
//! Whether a `busy` code passes by itself, and why: `program_busy` does,
//! because the program, or the other start that holds the surface, ends.

use a2m_core::{
    CodeEntry, Failure, FailureCode, FileKindName, FindingKind, LintCode, Mode, StepKind,
    SurfaceKind,
};
use a2m_provider::StepFailure;

/// The pack's name: the owner of its lint codes and of `packs.command`.
pub const PACK: &str = "command";

a2m_core::failure_codes! {
    PROGRAM_MISSING = Drift, "program_missing",
        meaning "The program's path is missing, is not a file, or cannot be run.",
        next "Install the program at the path its surface declares, or correct the path in the target's files.";
    PROGRAM_DIGEST_CHANGED = Drift, "program_digest_changed",
        meaning "The file's digest is not one listed for this platform.",
        next "Review the program that is installed now, add its SHA-256 under `program.digests` for this platform, and publish.";
    PROGRAM_VERSION_UNSUPPORTED = Drift, "program_version_unsupported",
        meaning "The version is absent, does not match, or is not a supported one.",
        next "Install a supported version, or check the target's files against this one and add it to `program.version.supported`.";
    OUTPUT_UNPARSED = Drift, "output_unparsed",
        meaning "A read's output does not parse as its declared form.",
        next "Compare what the program prints now with the command's `output` in `commands/`, and correct the file if the program changed.";
    PROGRAM_PATH_UNSAFE = Policy, "program_path_unsafe",
        meaning "The path is not absolute, is a script, is a shell, or the file or a folder above it can be written by another user.",
        next "Name the program by its absolute path, and give the file and every folder above it to the engine's user or the administrator, with no write access for anyone else.";
    LIMIT_EXCEEDED = Policy, "limit_exceeded",
        meaning "A stream passed its ceiling.",
        next "Narrow what the command lists, or raise `limits` in its `commands/` file and the setting `packs.command.max_output_bytes`.";
    ARGUMENT_UNSAFE = Input, "argument_unsafe",
        meaning "A value holds a NUL character, or would become an option.",
        next "Give a value that is plain data for this parameter.";
    SCHEMA_INVALID = Input, "schema_invalid",
        meaning "A value breaks its declared pattern or kind, or an exit says so.",
        next "Correct the values and call again.";
    STEP_NOT_USABLE = Input, "step_not_usable",
        meaning "A step this driver cannot carry out as the compiled target wrote it: a command that writes on a step that is no commit point, or a step whose program was not started first.",
        next "Compile the target again with the version of the command pack that is enabled.";
    APP_VALIDATION_ERROR = Rejected, "app_validation_error",
        meaning "An exit declared `rejected`.",
        next "Correct the values and call again.";
    VERSION_CONFLICT = Stale, "version_conflict",
        meaning "An exit declared `stale`.",
        next "Read the record again and decide whether the change still applies.";
    RECORD_ABSENT = Stale, "record_absent",
        meaning "A write's exit says the record it names is not there.",
        next "Read the record again; it may have been removed.";
    PROGRAM_BUSY = Busy, "program_busy",
        meaning "An exit declared `busy`, or another start holds the surface.",
        next "Try again when the other start has ended.",
        passes by_itself;
    UPSTREAM_UNAVAILABLE = Transient, "upstream_unavailable",
        meaning "An exit declared `transient`.",
        next "Try again shortly.";
    UNEXPECTED_EXIT_STATUS = UnknownVariant, "unexpected_exit_status",
        meaning "A read ended with a status nothing declares, or by a signal.",
        next "Compare how the program ends now with the exits its command file declares, and correct the file if the program changed.";
    UNEXPECTED_PROMPT = UnknownVariant, "unexpected_prompt",
        meaning "A read's output holds a declared prompt.",
        next "Add the argument that makes the command not ask, such as `--yes`, to its `args` in `commands/`.";
    COMMAND_FILE_UNREADABLE = Input, "command_file_unreadable",
        meaning "A file handed to the scout as a completion file or a manual page cannot be read as one: it is not UTF-8 text, it is compressed in a form other than gzip, its gzip form is damaged, or it is no manual page in the `man` or the `mdoc` macros.",
        next "Hand the file as plain UTF-8 text, or a manual page compressed with gzip; uncompress a page compressed any other way first.";
    POWERSHELL_FILE_UNREADABLE = Input, "powershell_file_unreadable",
        meaning "A file handed to the scout as a PowerShell help file or a module manifest cannot be read as one: it is not UTF-8 text, it is not well-formed XML or declares a document type, it passes a bound of the reader, or it is no help file (`helpItems`) or no manifest (one table of data).",
        next "Hand the module's XML help file, `<module>-help.xml`, or its manifest, `<module>.psd1`, as it ships; a help file that declares a document type is never read.";
    BINARY_KIND_UNKNOWN = UnknownVariant, "binary_kind_unknown",
        meaning "A file handed to the scout as a binary is of no kind the reader has a rule for: it is not an ELF, a PE, or a Mach-O file, or it is a PE file with no PE signature.",
        next "Hand a program of one of the three kinds the reader has a rule for: an ELF, a PE, or a Mach-O file.";
    BINARY_TRUNCATED = Input, "binary_truncated",
        meaning "A file handed to the scout as a binary is shorter than its own headers say: a header, a table, a segment, a section, or a slice of it is cut by the end of the file.",
        next "Copy the whole file again; a file the copy cut short cannot be read.";
    BINARY_CYCLIC = Input, "binary_cyclic",
        meaning "A file handed to the scout as a binary leads back to a part of itself that is being read: a resource table below itself, a slice of a fat file that is the fat header or another fat file.",
        next "Do not use this file: a program that is whole never leads back to a part it is being read from.";
    BINARY_MALFORMED = Input, "binary_malformed",
        meaning "A file handed to the scout as a binary has a kind the reader knows and is not well formed in a way that is not a cut: a table with an entry out of line, a tree deeper than its format allows, or more entries than the reader takes.",
        next "Hand the program as it was built; a file a tool has changed by hand may no longer be one.";
    AMBIGUOUS_MATCH = PossibleCommit, "ambiguous_match",
        meaning "A lookup that must match one record matched more than one.",
        next "Find the records by hand, and decide which is the one that was written.";
}

/// From the provider contract: a write was sent and nothing confirms it.
pub const NO_CONFIRMATION: FailureCode = a2m_provider::codes::NO_CONFIRMATION;
/// From the provider contract: the time limit passed on a read.
pub const TIMEOUT: FailureCode = a2m_provider::codes::TIMEOUT;
/// From the provider contract: a stopped program could not be cleaned up.
pub const CLEANUP_INCOMPLETE: FailureCode = a2m_provider::codes::CLEANUP_INCOMPLETE;
/// From the provider contract: nothing may be written in this posture.
pub const READ_ONLY_POSTURE: FailureCode = a2m_provider::codes::READ_ONLY_POSTURE;
/// From the session keeper: the program says it is not signed in.
pub const NEEDS_LOGIN: FailureCode = a2m_auth::codes::NEEDS_LOGIN;

/// The mode the pack brings.
pub const MODE: &str = "cli";
/// The surface kind the pack serves.
pub const SURFACE_KIND: SurfaceKind = SurfaceKind::fixed("program");
/// The file kind the pack owns, in the folder `commands/`.
pub const FILE_KIND: FileKindName = FileKindName::fixed("command");
/// The folder of the file kind.
pub const FOLDER: &str = "commands";
/// The kind of finding the pack's discoverer makes.
pub const FINDING_KIND: &str = "command";
/// The kind of finding about one file handed to the discoverer: a
/// completion file or a manual page, and how much of it was read.
pub const FILE_FINDING_KIND: &str = "command_file";
/// The kind of finding about one command a PowerShell help file handed to
/// the discoverer describes.
pub const POWERSHELL_FINDING_KIND: &str = "powershell_command";
/// The kind of finding about the plain facts of a binary handed to the
/// binary discoverer.
pub const BINARY_FINDING_KIND: &str = "binary";
/// The discoverer's name.
pub const DISCOVERER: &str = "help";
/// The name of the discoverer of a binary's plain facts.
pub const BINARY_DISCOVERER: &str = "binary";
/// The step that starts a program and waits for it to end.
pub const SPAWN: StepKind = StepKind::fixed("spawn");
/// The step that checks how the program ended.
pub const EXPECT: StepKind = StepKind::fixed("expect");
/// The step that parses what the program printed.
pub const PARSE: StepKind = StepKind::fixed("parse");

/// The mode, as a checked name. `cli` is well formed, so the fallback is
/// never taken; a test holds that.
#[must_use]
pub fn mode() -> Mode {
    Mode::new(MODE).unwrap_or_else(|_| unreachable!("`cli` is a well-formed mode name"))
}

/// The finding kind, as a checked name.
#[must_use]
pub fn finding_kind() -> FindingKind {
    FindingKind::new(FINDING_KIND)
        .unwrap_or_else(|_| unreachable!("`command` is a well-formed finding kind"))
}

/// The kind of finding about a input file, as a checked name.
#[must_use]
pub fn file_finding_kind() -> FindingKind {
    FindingKind::new(FILE_FINDING_KIND)
        .unwrap_or_else(|_| unreachable!("`command_file` is a well-formed finding kind"))
}

/// The kind of finding about a command a PowerShell help file describes,
/// as a checked name.
#[must_use]
pub fn powershell_finding_kind() -> FindingKind {
    FindingKind::new(POWERSHELL_FINDING_KIND)
        .unwrap_or_else(|_| unreachable!("`powershell_command` is a well-formed finding kind"))
}

/// The kind of finding about a binary's plain facts, as a checked name.
#[must_use]
pub fn binary_finding_kind() -> FindingKind {
    FindingKind::new(BINARY_FINDING_KIND)
        .unwrap_or_else(|_| unreachable!("`binary` is a well-formed finding kind"))
}

/// A lint code of the pack. `rule` is one of the fixed rule names of this
/// crate, each of which a test makes once.
#[must_use]
pub fn lint(rule: &'static str) -> LintCode {
    LintCode::new(&format!("{PACK}.{rule}"))
        .unwrap_or_else(|_| unreachable!("a rule name of the command pack is well formed"))
}

/// A failure of the code of `row`, in the class its table gives it.
pub(crate) fn failure(
    row: &CodeEntry,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    row.failure(message, next)
}

/// A failure before any program was started.
pub(crate) fn not_sent(
    row: &CodeEntry,
    message: impl Into<String>,
    next: impl Into<String>,
) -> StepFailure {
    StepFailure::not_sent(failure(row, message, next))
}

/// A failure the program itself answered with: it ran to its end.
pub(crate) fn answered(
    row: &CodeEntry,
    message: impl Into<String>,
    next: impl Into<String>,
) -> StepFailure {
    StepFailure::answered(failure(row, message, next))
}

/// A write that was started and whose result nothing confirms.
pub(crate) fn possible_commit(what: &str) -> StepFailure {
    StepFailure::unknown(failure(
        &a2m_provider::codes::row::NO_CONFIRMATION,
        format!("A program that writes was started and {what}, so nothing confirms the write."),
        "Reconcile the record: read it back before anything is written again.",
    ))
}

/// A step the compiled target wrote in a way this driver cannot carry out.
/// Nothing was started.
pub(crate) fn not_usable(what: &str) -> StepFailure {
    not_sent(
        &row::STEP_NOT_USABLE,
        format!(
            "The `cli` driver cannot carry out this step as the compiled target wrote it: {what}."
        ),
        "Compile the target again with the version of the command pack that is enabled.",
    )
}

/// Text from outside, cut and cleaned before it is shown in a message.
///
/// The cut comes last. Text a program printed is redacted whole before it
/// is given to this function: a secret that the cut divides leaves a part
/// no redactor can match.
pub(crate) fn shown(text: &str) -> String {
    const SHOWN: usize = 120;
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

    /// No source of the pack gives a failure a class of its own: each is
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
