//! The failure codes this pack produces, each with its one class, and the
//! names it registers under. The manifest declares the same codes; a test
//! holds the two lists together.

use a2m_core::{
    Failure, FailureClass, FailureCode, FileKindName, FindingKind, LintCode, Mode, StepKind,
    SurfaceKind,
};
use a2m_provider::StepFailure;

/// The pack's name: the owner of its lint codes and of `packs.command`.
pub const PACK: &str = "command";

macro_rules! codes {
    ($( $(#[$doc:meta])* $name:ident = $class:ident, $text:literal; )+) => {
        $( $(#[$doc])* pub const $name: FailureCode = FailureCode::fixed($text); )+

        /// Every code the pack's manifest declares, with its one class.
        pub const ALL: &[(FailureClass, &str)] = &[ $( (FailureClass::$class, $text) ),+ ];
    };
}

codes! {
    /// The program's path is missing, is not a file, or cannot be run.
    PROGRAM_MISSING = Drift, "program_missing";
    /// The file's digest is not one listed for this platform.
    PROGRAM_DIGEST_CHANGED = Drift, "program_digest_changed";
    /// The version is absent, does not match, or is not a supported one.
    PROGRAM_VERSION_UNSUPPORTED = Drift, "program_version_unsupported";
    /// A read's output does not parse as its declared form.
    OUTPUT_UNPARSED = Drift, "output_unparsed";
    /// The path is not absolute, is a script, is a shell, or the file or a
    /// folder above it can be written by another user.
    PROGRAM_PATH_UNSAFE = Policy, "program_path_unsafe";
    /// A stream passed its ceiling.
    LIMIT_EXCEEDED = Policy, "limit_exceeded";
    /// A value holds a NUL character, or would become an option.
    ARGUMENT_UNSAFE = Input, "argument_unsafe";
    /// A value breaks its declared pattern or kind, or an exit says so.
    SCHEMA_INVALID = Input, "schema_invalid";
    /// An exit declared `rejected`.
    APP_VALIDATION_ERROR = Rejected, "app_validation_error";
    /// An exit declared `stale`.
    VERSION_CONFLICT = Stale, "version_conflict";
    /// A write's exit says the record it names is not there.
    RECORD_ABSENT = Stale, "record_absent";
    /// An exit declared `busy`, or another start holds the surface.
    PROGRAM_BUSY = Busy, "program_busy";
    /// An exit declared `transient`.
    UPSTREAM_UNAVAILABLE = Transient, "upstream_unavailable";
    /// A read ended with a status nothing declares, or by a signal.
    UNEXPECTED_EXIT_STATUS = UnknownVariant, "unexpected_exit_status";
    /// A read's output holds a declared prompt.
    UNEXPECTED_PROMPT = UnknownVariant, "unexpected_prompt";
    /// A lookup that must match one record matched more than one.
    AMBIGUOUS_MATCH = PossibleCommit, "ambiguous_match";
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

/// Text from outside, cut and cleaned before it is shown in a message.
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
