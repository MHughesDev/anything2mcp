//! Expressions: checked and compiled once, when a compiled target is built,
//! and evaluated many times, when a compiled target runs.
//!
//! The syntax is a subset of CEL, read by this crate's own parser, type
//! checker, and evaluator (decision 0072). There is no float: a number with a
//! decimal point is refused when an expression is compiled, and a decimal is
//! `a2m-core`'s exact `Decimal`, written `decimal("1.5")`, with `+`, `-`, `*`,
//! and the comparisons as operators and division only through `div`
//! (decision 0073). The crate also holds the host functions every expression
//! and every field type may call, and the normalizer for each field-type
//! base. It never reads a file, a clock, or the network.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: `a2m-core`.

mod check;
pub mod codes;
mod dates;
mod error;
mod eval;
mod functions;
mod library;
mod measure;
mod normalize;
mod numbers;
pub mod parse;
mod syntax;
mod text;

pub use check::{Checked, Compiled, TypeEnv, compile};
pub use dates::DateSystem;
pub use error::{
    COMPILED_EXPRESSION_UNLOADABLE, EXPRESSION_COST_EXCEEDED, EXPRESSION_INVALID,
    EXPRESSION_TYPE_ERROR, ExprError, Span, VALUE_ABSENT, VALUE_OUT_OF_RANGE, VALUE_UNPARSABLE,
};
pub use eval::{Bindings, Program};
pub use library::{COST_LIMIT, HostFn, HostFnName, Library, LibraryVersion, library};
pub use normalize::{
    DISPLAY_FEET_INCHES, FieldRule, NORMALIZER_INPUT, Normalized, Normalizer, Whitespace,
    check_format,
};
pub use numbers::{DecimalMark, currency_places};

/// The names the file formats give an expression to read. The compiler
/// declares the ones a place has; nothing here makes them special.
pub mod names {
    /// The inputs of the operation or pipeline.
    pub const INPUT: &str = "input";
    /// The result, in `expect`, `success`, and later steps.
    pub const RESULT: &str = "result";
    /// The current row, inside `for_each` and mappings.
    pub const ROW: &str = "row";
    /// The source, inside `for_each` and mappings.
    pub const SOURCE: &str = "source";
    /// Earlier steps of a pipeline: `steps.<id>.result`.
    pub const STEPS: &str = "steps";
    /// The environment's name and kind.
    pub const ENV: &str = "env";
    /// In a binding's steps: what the system held when the engine last read
    /// the record, as the read gave it; null when it does not exist.
    pub const CURRENT: &str = "current";
    /// In a binding's steps: the record's key in the system, `key`, its
    /// parts as text; null when the record has none yet. Never the engine's
    /// own name for a record in a run.
    pub const RECORD: &str = "record";
}
