//! The failure codes this pack produces, each with its one class, and the
//! names it registers under. The manifest declares the same codes; a test
//! holds the two lists together.
//!
//! Whether a `busy` code passes by itself, and why: `program_busy` does,
//! because the program, or the other start that holds the surface, ends.

use a2m_core::{
    Failure, FailureClass, FailureCode, FileKindName, FindingKind, LintCode, Mode, StepKind,
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
        meaning "A step this driver cannot carry out as the plan wrote it: a command that writes on a step that is no commit point, or a step whose program was not started first.",
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
/// The discoverer's name.
pub const DISCOVERER: &str = "help";
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

/// A lint code of the pack. `rule` is one of the fixed rule names of this
/// crate, each of which a test makes once.
#[must_use]
pub fn lint(rule: &'static str) -> LintCode {
    LintCode::new(&format!("{PACK}.{rule}"))
        .unwrap_or_else(|_| unreachable!("a rule name of the command pack is well formed"))
}

pub(crate) fn failure(
    class: FailureClass,
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    Failure::new(class, code, message, next)
}

/// A failure before any program was started.
pub(crate) fn not_sent(
    class: FailureClass,
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> StepFailure {
    StepFailure::not_sent(failure(class, code, message, next))
}

/// A failure the program itself answered with: it ran to its end.
pub(crate) fn answered(
    class: FailureClass,
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> StepFailure {
    StepFailure::answered(failure(class, code, message, next))
}

/// A write that was started and whose result nothing confirms.
pub(crate) fn possible_commit(what: &str) -> StepFailure {
    StepFailure::unknown(failure(
        FailureClass::PossibleCommit,
        NO_CONFIRMATION,
        format!("A program that writes was started and {what}, so nothing confirms the write."),
        "Reconcile the record: read it back before anything is written again.",
    ))
}

/// A step the plan wrote in a way this driver cannot carry out. Nothing
/// was started.
pub(crate) fn not_usable(what: &str) -> StepFailure {
    not_sent(
        FailureClass::Input,
        STEP_NOT_USABLE,
        format!("The `cli` driver cannot carry out this step as the plan wrote it: {what}."),
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
