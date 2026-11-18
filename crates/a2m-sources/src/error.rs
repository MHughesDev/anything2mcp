//! The failures this crate makes, and the list of problems a rejected input
//! carries. The codes are declared in [`crate::codes`], with the next step a
//! failure carries when its case adds nothing to it. No message ever holds a
//! value from the input: a message names a place, a header, or a rule.

use std::fmt;

use a2m_core::{Failure, FailureClass, FailureCode};

use crate::model::CellRef;

pub use crate::codes::{
    FILE_TYPE_MISMATCH, FORMULA_WITHOUT_VALUE, KEY_NOT_UNIQUE, SOURCE_DEFINITION_INVALID,
    SOURCE_ENCODING_UNSUPPORTED, SOURCE_MALFORMED, SOURCE_PROTECTED, SOURCE_READER_UNKNOWN,
    SOURCE_TEMPLATE_UNKNOWN, SOURCE_TOO_LARGE,
};

/// A column, a header, a row, or a value does not have the shape the
/// definition requires. Declared by `a2m-core`.
pub const SCHEMA_INVALID: FailureCode = FailureCode::SCHEMA_INVALID;
/// A value cannot be represented in its field type. Declared by `a2m-core`.
pub const VALUE_OUT_OF_RANGE: FailureCode = FailureCode::VALUE_OUT_OF_RANGE;

/// Makes a failure of class `input`, the class of every failure here: the
/// input was refused and nothing ran.
pub(crate) fn failure(code: FailureCode, message: impl Into<String>, next: &str) -> Failure {
    Failure::new(FailureClass::Input, code, message, next)
}

/// A failure of class `input` whose case adds nothing to the next step its
/// code's table row gives.
fn refused(code: FailureCode, message: impl Into<String>) -> Failure {
    let next = crate::codes::next_for(&code);
    failure(code, message, next)
}

pub(crate) fn malformed(what: impl fmt::Display) -> Failure {
    refused(
        SOURCE_MALFORMED,
        format!("The input breaks the rules of its format: {what}."),
    )
}

pub(crate) fn mismatch(what: impl fmt::Display) -> Failure {
    refused(
        FILE_TYPE_MISMATCH,
        format!("The input is not the format its source definition names: {what}."),
    )
}

pub(crate) fn too_large(what: impl fmt::Display) -> Failure {
    refused(
        SOURCE_TOO_LARGE,
        format!("The input is larger than the limits allow: {what}."),
    )
}

pub(crate) fn encoding(what: impl fmt::Display) -> Failure {
    refused(
        SOURCE_ENCODING_UNSUPPORTED,
        format!("The input's text cannot be read: {what}."),
    )
}

pub(crate) fn protected() -> Failure {
    refused(
        SOURCE_PROTECTED,
        "The workbook is encrypted with a password, so its content cannot be read.",
    )
}

pub(crate) fn definition(what: impl fmt::Display) -> Failure {
    refused(
        SOURCE_DEFINITION_INVALID,
        format!("The compiled source definition does not hold together: {what}."),
    )
}

/// One thing wrong with an input: where, in which row field, which rule, and
/// a sentence. The sentence never holds a value from the input.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Problem {
    /// The cell, when the problem has one.
    pub at: Option<CellRef>,
    /// The row field or the header, when the problem has one.
    pub field: Option<String>,
    /// Which rule was broken.
    pub code: FailureCode,
    /// What is wrong, in a sentence.
    pub message: String,
}

impl Problem {
    pub(crate) fn new(code: FailureCode, message: impl Into<String>) -> Self {
        Self {
            at: None,
            field: None,
            code,
            message: message.into(),
        }
    }

    pub(crate) const fn at(mut self, at: CellRef) -> Self {
        self.at = Some(at);
        self
    }

    pub(crate) fn field(mut self, field: &str) -> Self {
        self.field = Some(field.to_owned());
        self
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(at) = self.at {
            write!(f, "{at}: ")?;
        }
        if let Some(field) = &self.field {
            write!(f, "`{field}`: ")?;
        }
        write!(f, "{} ({})", self.message, self.code.as_str())
    }
}

/// An input that was refused: the failure, and every problem found, at once.
/// Nothing of a rejected input is accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    /// The failure the run stops with.
    pub failure: Failure,
    /// Every problem, in the order of the input. Empty when the failure says
    /// everything, such as an input that is not its format.
    pub problems: Vec<Problem>,
}

impl Rejected {
    /// A rejection for a list of problems. The failure carries the code of
    /// the first problem and counts the rest.
    pub(crate) fn of(problems: Vec<Problem>) -> Self {
        let code = problems
            .first()
            .map_or(SCHEMA_INVALID, |first| first.code.clone());
        let failure = failure(
            code,
            format!(
                "The input was refused: {} problem(s) were found, and nothing was accepted.",
                problems.len()
            ),
            "Correct each problem listed and give the input again.",
        );
        Self { failure, problems }
    }
}

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.failure.message())?;
        for problem in &self.problems {
            write!(f, "\n  {problem}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Rejected {}

impl From<Failure> for Rejected {
    fn from(failure: Failure) -> Self {
        Self {
            failure,
            problems: Vec::new(),
        }
    }
}

impl From<Rejected> for Failure {
    fn from(rejected: Rejected) -> Self {
        rejected.failure
    }
}
