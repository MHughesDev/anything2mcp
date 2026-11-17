//! Errors: a compile error with a place and a fix, and the run-time failures.
//!
//! Every error of this crate is a [`Failure`] of class `input` with one of the
//! codes below. A run-time failure names the host function and the kind of
//! thing that went wrong. It never carries the value that caused it, because a
//! value may be business data and a failure's sentence is shown to callers.

use std::fmt;

use a2m_core::{Failure, FailureClass, FailureCode};

/// The text does not parse, or names something the environment does not declare.
pub const EXPRESSION_INVALID: FailureCode = FailureCode::fixed("expression_invalid");
/// A type does not fit, including any use of a float.
pub const EXPRESSION_TYPE_ERROR: FailureCode = FailureCode::fixed("expression_type_error");
/// A host function was given a value it cannot represent.
pub const VALUE_OUT_OF_RANGE: FailureCode = FailureCode::fixed("value_out_of_range");
/// A parse function met text it cannot read.
pub const VALUE_UNPARSABLE: FailureCode = FailureCode::fixed("value_unparsable");
/// A value was absent where the expression needs one.
pub const VALUE_ABSENT: FailureCode = FailureCode::fixed("value_absent");
/// Evaluation passed its cost limit.
pub const EXPRESSION_COST_EXCEEDED: FailureCode = FailureCode::fixed("expression_cost_exceeded");
/// A plan's expression names a library this engine does not have, no longer
/// checks as the type it states, or was run without a name it reads.
pub const PLAN_EXPRESSION_UNLOADABLE: FailureCode =
    FailureCode::fixed("plan_expression_unloadable");

/// A place inside an expression: byte offsets into the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Span {
    /// The first byte.
    pub start: u32,
    /// One past the last byte.
    pub end: u32,
}

impl Span {
    /// A span from `start` to `end`.
    #[must_use]
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// The span from the start of `self` to the end of `other`.
    #[must_use]
    pub(crate) const fn to(self, other: Span) -> Span {
        Span {
            start: self.start,
            end: other.end,
        }
    }
}

/// A compile error: a code, a sentence, and a place inside the expression.
///
/// `a2m-files` adds the file and line of the expression itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprError {
    /// `expression_invalid` or `expression_type_error`.
    pub code: FailureCode,
    /// What is wrong, in one sentence.
    pub message: String,
    /// Where, inside the expression.
    pub at: Span,
    /// What to write instead, when the checker knows.
    pub fix: Option<String>,
}

impl ExprError {
    pub(crate) fn invalid(message: impl Into<String>, at: Span) -> Self {
        Self {
            code: EXPRESSION_INVALID,
            message: message.into(),
            at,
            fix: None,
        }
    }

    pub(crate) fn type_error(message: impl Into<String>, at: Span) -> Self {
        Self {
            code: EXPRESSION_TYPE_ERROR,
            message: message.into(),
            at,
            fix: None,
        }
    }

    pub(crate) fn with_fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} (at bytes {} to {} of the expression)",
            self.message, self.at.start, self.at.end
        )
    }
}

impl std::error::Error for ExprError {}

impl From<ExprError> for Failure {
    fn from(error: ExprError) -> Self {
        let message = error.to_string();
        let next_step = error
            .fix
            .unwrap_or_else(|| "Correct the expression and compile again.".to_owned());
        Failure::new(FailureClass::Input, error.code, message, next_step)
    }
}

/// A run-time failure of a host function or of the evaluator, before it is
/// given the expression's context. Holds no value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fault {
    pub code: FailureCode,
    pub what: &'static str,
}

impl Fault {
    pub(crate) const fn range(what: &'static str) -> Self {
        Self {
            code: VALUE_OUT_OF_RANGE,
            what,
        }
    }

    pub(crate) const fn unparsable(what: &'static str) -> Self {
        Self {
            code: VALUE_UNPARSABLE,
            what,
        }
    }

    pub(crate) const fn absent(what: &'static str) -> Self {
        Self {
            code: VALUE_ABSENT,
            what,
        }
    }

    pub(crate) const fn cost() -> Self {
        Self {
            code: EXPRESSION_COST_EXCEEDED,
            what: "the expression did more work than one evaluation may",
        }
    }

    pub(crate) const fn unloadable(what: &'static str) -> Self {
        Self {
            code: PLAN_EXPRESSION_UNLOADABLE,
            what,
        }
    }

    /// The failure a caller sees. `place` names the function or operator.
    pub(crate) fn into_failure(self, place: &str) -> Failure {
        let next_step = if self.code == VALUE_UNPARSABLE {
            "Correct the value, or the field type's format, and run again."
        } else if self.code == VALUE_OUT_OF_RANGE {
            "Give a value the field type can hold, and run again."
        } else if self.code == VALUE_ABSENT {
            "Give the value, or a default with coalesce, and run again."
        } else if self.code == EXPRESSION_COST_EXCEEDED {
            "Give the expression less to work over, such as fewer rows, or simplify it."
        } else {
            "Compile the target again with this engine."
        };
        let message = if place.is_empty() {
            format!("An expression stopped: {}.", self.what)
        } else {
            format!("An expression stopped in `{place}`: {}.", self.what)
        };
        Failure::new(FailureClass::Input, self.code, message, next_step)
    }
}
