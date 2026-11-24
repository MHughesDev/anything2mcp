//! What a target is: the schemas of target files, the loader that reads
//! them with a file, a line, and a column for every value, the linter, the
//! compiler that turns them into a hashed `CompiledTarget`, and the drafter that
//! writes draft files from findings.
//!
//! The crate is neutral. Every file kind, verb lowering, lint rule, and
//! drafting rule that belongs to one kind of system arrives from a pack
//! through the registration traits in [`register`]; the kernel names no mode,
//! no surface kind, and no control. The compiled target's types, its
//! encoding, and the check of its hash are `a2m-core`'s; this crate is the
//! only code that makes a compiled target.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: `a2m-core` and `a2m-expr`.

mod addressed;
pub mod codes;
mod compile;
mod diag;
mod doc;
mod draft;
pub mod kernel;
mod load;
mod register;
mod review;
mod schema;
mod source;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod types;
pub use types::UNTRUSTED_TEXT;
mod view;
mod yaml;

pub use compile::{
    COMPILER_VERSION, Compiled, DRAFT_VERSION, MINIMUM_ENGINE, compile, left_out_sentence,
    lint_one, names_a_secret,
};
pub use diag::{
    DRAFT_CONFLICT, Diagnostic, Diagnostics, LINT_FAILED, Level, PATH_NOT_ALLOWED,
    REGISTRATION_CONFLICT, Stage, TARGET_UNREADABLE,
};
pub use doc::{Doc, Entry, Located, Place, RelPath};
pub use draft::{
    DRAFT_LEVEL, DraftContext, DraftDoc, DraftReport, DraftValue, Drafter, NotDrafted,
    OpenQuestion, Quoting, StoredQuestion,
};
pub use load::{
    HELD_FOLDER, Held, HeldFile, HeldQuestion, LARGEST_RECORDED_FILE, LARGEST_TARGET_FILE,
    MOST_HELD_FILES, SourceFile, Sources, TargetRoot, held_files, load, load_files, read_held,
};
pub use register::{
    CompileMode, CompiledSection, CompilerSet, CompilerSetBuilder, DEFAULT_MODE_RANK, DeclaredName,
    DefinedName, DraftingRule, ExprSite, FileKind, FixedPlace, LintRule, Lowered, Lowering,
    NoticeField, Possible, ProgramField, StepChanges, StepCommits, SurfaceKindSpec,
};
pub use review::{
    BindingReview, BindingStatement, BindingStates, CredentialShown, EnvironmentVariable,
    LAUNCHER_NOTICE, NOT_STATED, PROBE_PENDING, PROVIDER_NOT_HELD, PlaceReview, ProbeReview,
    ProgramRun, REACH_NOT_STATED, Reach, ReachGroup, ReviewItem, ReviewSummary, RunReview,
    SCRIPT_NOTICE, SENT_TO_NOT_STATED, Sends, SentAddress, SentGroup, SentTo, SentToReview,
    SignInReview, Statement, VariableValue,
};
pub use schema::{Checked, JsonSchema, Misfit, Misfits, OPEN_QUESTION, Reference};
pub use source::{
    Fingerprint, FingerprintPart, HiddenRule, Locate, OPERATION_READER, PartKind, ReaderOptions,
    RowField, SourceSpec, TemplateVersion,
};
pub use view::{
    FieldTypeView, InputView, KindContext, LowerContext, OperationView, ResolvedBinding,
    ResolvedView, SurfaceView, TargetView, expr_text,
};
pub use yaml::{normalize_text, parse as parse_yaml};

#[cfg(test)]
mod tests {
    mod drafter;
    mod example;
    mod held;
    mod review;
}
