//! Errors: a compile error with a place and a fix, and the run-time failures.
//!
//! Every error of this crate is a [`Failure`] of class `input` with one of the
//! codes of its table, [`crate::codes`], whose next step it carries unless
//! the case gives a better one. A run-time failure names the host function
//! and the kind of thing that went wrong. It never carries the value that
//! caused it, because a value may be business data and a failure's sentence
//! is shown to callers.

use std::fmt;

use a2m_core::{Failure, FailureClass, FailureCode};

pub use crate::codes::{
    COMPILED_EXPRESSION_UNLOADABLE, EXPRESSION_COST_EXCEEDED, EXPRESSION_INVALID,
    EXPRESSION_TYPE_ERROR, VALUE_ABSENT, VALUE_OUT_OF_RANGE, VALUE_UNPARSABLE,
};

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
            .unwrap_or_else(|| crate::codes::next_for(&error.code).to_owned());
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
            code: COMPILED_EXPRESSION_UNLOADABLE,
            what,
        }
    }

    /// The failure a caller sees. `place` names the function or operator.
    pub(crate) fn into_failure(self, place: &str) -> Failure {
        let next_step = crate::codes::next_for(&self.code);
        let message = if place.is_empty() {
            format!("An expression stopped: {}.", self.what)
        } else {
            format!("An expression stopped in `{place}`: {}.", self.what)
        };
        Failure::new(FailureClass::Input, self.code, message, next_step)
    }
}
