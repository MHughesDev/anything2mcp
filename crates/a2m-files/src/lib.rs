//! What a target is: the schemas of target files, the loader that reads
//! them with a file, a line, and a column for every value, the linter, the
//! compiler that turns them into a hashed `Plan`, and the drafter that
//! writes draft files from findings.
//!
//! The crate is neutral. Every file kind, verb lowering, lint rule, and
//! drafting rule that belongs to one kind of system arrives from a pack
//! through the registration traits in [`register`]; the kernel names no
//! mode, no surface kind, and no control. The plan's types, its encoding,
//! and the check of its hash are `a2m-core`'s; this crate is the only code
//! that makes a plan.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: `a2m-core` and `a2m-expr`.

pub mod codes;
mod compile;
mod diag;
mod doc;
mod draft;
pub mod kernel;
mod load;
mod register;
mod schema;
mod source;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod types;
mod view;
mod yaml;

pub use compile::{
    COMPILER_VERSION, Compiled, DRAFT_VERSION, MINIMUM_ENGINE, ReviewItem, ReviewSummary, compile,
    lint_one,
};
pub use diag::{
    DRAFT_CONFLICT, Diagnostic, Diagnostics, LINT_FAILED, Level, PATH_NOT_ALLOWED,
    REGISTRATION_CONFLICT, Stage, TARGET_UNREADABLE,
};
pub use doc::{Doc, Entry, Located, Place, RelPath};
pub use draft::{
    DRAFT_LEVEL, DraftContext, DraftDoc, DraftReport, DraftValue, Drafter, OpenQuestion,
};
pub use load::{SourceFile, Sources, TargetRoot, load, load_files};
pub use register::{
    CompileMode, CompiledSection, CompilerSet, CompilerSetBuilder, DeclaredName, DefinedName,
    DraftingRule, ExprSite, FileKind, LintRule, Lowered, Lowering, Possible, SurfaceKindSpec,
};
pub use schema::{Checked, JsonSchema, OPEN_QUESTION, Reference};
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
}
