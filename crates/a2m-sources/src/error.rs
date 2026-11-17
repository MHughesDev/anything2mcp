//! The failures this crate names, and the list of problems a rejected input
//! carries. No message ever holds a value from the input: a message names a
//! place, a header, or a rule.

use std::fmt;

use a2m_core::{Failure, FailureClass, FailureCode};

use crate::model::CellRef;

/// The input's structure matches no template version the definition knows.
pub const SOURCE_TEMPLATE_UNKNOWN: FailureCode = FailureCode::fixed("source_template_unknown");
/// The content is not the format its reader reads.
pub const FILE_TYPE_MISMATCH: FailureCode = FailureCode::fixed("file_type_mismatch");
/// A column, a header, a row, or a value does not have the shape the
/// definition requires.
pub const SCHEMA_INVALID: FailureCode = FailureCode::SCHEMA_INVALID;
/// A value cannot be represented in its field type.
pub const VALUE_OUT_OF_RANGE: FailureCode = FailureCode::VALUE_OUT_OF_RANGE;
/// A formula cell in the data has no cached value.
pub const FORMULA_WITHOUT_VALUE: FailureCode = FailureCode::fixed("formula_without_value");
/// The bytes, the rows, the cells, or one cell exceed the caller's limits.
pub const SOURCE_TOO_LARGE: FailureCode = FailureCode::fixed("source_too_large");
/// The content claims its format and breaks the format's rules.
pub const SOURCE_MALFORMED: FailureCode = FailureCode::fixed("source_malformed");
/// The text is in an encoding the readers do not read.
pub const SOURCE_ENCODING_UNSUPPORTED: FailureCode =
    FailureCode::fixed("source_encoding_unsupported");
/// The workbook is encrypted, so it cannot be read without its password.
pub const SOURCE_PROTECTED: FailureCode = FailureCode::fixed("source_protected");
/// The definition names a reader that is not registered.
pub const SOURCE_READER_UNKNOWN: FailureCode = FailureCode::fixed("source_reader_unknown");
/// The compiled source definition does not hold together.
pub const SOURCE_DEFINITION_INVALID: FailureCode = FailureCode::fixed("source_definition_invalid");
/// Two rows have the same key.
pub const KEY_NOT_UNIQUE: FailureCode = FailureCode::fixed("key_not_unique");

/// Makes a failure of class `input`, the class of every failure here: the
/// input was refused and nothing ran.
pub(crate) fn failure(code: FailureCode, message: impl Into<String>, next: &str) -> Failure {
    Failure::new(FailureClass::Input, code, message, next)
}

pub(crate) fn malformed(what: impl fmt::Display) -> Failure {
    failure(
        SOURCE_MALFORMED,
        format!("The input breaks the rules of its format: {what}."),
        "Save the file again from the program that made it, or correct it, and try again.",
    )
}

pub(crate) fn mismatch(what: impl fmt::Display) -> Failure {
    failure(
        FILE_TYPE_MISMATCH,
        format!("The input is not the format its source definition names: {what}."),
        "Give a file of the format the source definition names.",
    )
}

pub(crate) fn too_large(what: impl fmt::Display) -> Failure {
    failure(
        SOURCE_TOO_LARGE,
        format!("The input is larger than the limits allow: {what}."),
        "Give a smaller input, or have the limits raised by whoever sets them.",
    )
}

pub(crate) fn encoding(what: impl fmt::Display) -> Failure {
    failure(
        SOURCE_ENCODING_UNSUPPORTED,
        format!("The input's text cannot be read: {what}."),
        "Save the file as UTF-8 and try again.",
    )
}

pub(crate) fn protected() -> Failure {
    failure(
        SOURCE_PROTECTED,
        "The workbook is encrypted with a password, so its content cannot be read.",
        "Save a copy without the password and give that copy.",
    )
}

pub(crate) fn definition(what: impl fmt::Display) -> Failure {
    failure(
        SOURCE_DEFINITION_INVALID,
        format!("The compiled source definition does not hold together: {what}."),
        "Compile the target again with this engine.",
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
