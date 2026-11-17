//! Diagnostics: what the loader, the compiler, and the linter say about a
//! target's files. Each has a stable code, one plain sentence, a place, and
//! a fix.

use std::fmt;

use a2m_core::{Failure, FailureClass, FailureCode, LintCode};

use crate::doc::Place;

/// Any error diagnostic: the files do not compile.
pub const LINT_FAILED: FailureCode = FailureCode::fixed("lint_failed");
/// A path or a link that leaves the target folder.
pub const PATH_NOT_ALLOWED: FailureCode = FailureCode::fixed("path_not_allowed");
/// The drafter would overwrite a file the developer changed.
pub const DRAFT_CONFLICT: FailureCode = FailureCode::fixed("draft_conflict");
/// Two registrations for the same key, or one that is not the registering
/// pack's to make.
pub const REGISTRATION_CONFLICT: FailureCode = FailureCode::fixed("registration_conflict");
/// The target folder could not be read.
pub const TARGET_UNREADABLE: FailureCode = FailureCode::fixed("target_unreadable");

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// The files do not compile.
    Error,
    /// The files compile; the developer should look.
    Warning,
}

/// The compiler stage that found a problem (spec section 206).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// 1: reading YAML.
    Parse,
    /// 2: checking against schemas.
    Validate,
    /// 3: linking names across files.
    Resolve,
    /// 4: checking expressions.
    TypeCheck,
    /// 5: turning bindings into steps.
    Lower,
    /// 6: commit points, effects, identity, manifests.
    Analyze,
    /// 7: lint rules.
    Lint,
    /// 8: tool names, descriptions, schemas.
    Describe,
    /// 9: encoding and hashing.
    Emit,
}

/// One thing wrong with a target's files.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Diagnostic {
    /// The stable code: `kernel.` and a rule, or a pack's name and a rule.
    pub code: LintCode,
    /// Whether the files still compile.
    pub level: Level,
    /// What is wrong, in one plain sentence.
    pub message: String,
    /// Where.
    pub at: Place,
    /// What to write instead.
    pub fix: String,
    /// The stage that found it.
    pub stage: Stage,
}

impl Diagnostic {
    /// An error.
    pub fn error(
        code: LintCode,
        stage: Stage,
        at: &Place,
        message: impl Into<String>,
        fix: impl Into<String>,
    ) -> Self {
        Self {
            code,
            level: Level::Error,
            message: message.into(),
            at: at.clone(),
            fix: fix.into(),
            stage,
        }
    }

    /// The same diagnostic as a warning.
    #[must_use]
    pub fn as_warning(mut self) -> Self {
        self.level = Level::Warning;
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.level {
            Level::Error => "error",
            Level::Warning => "warning",
        };
        write!(
            f,
            "{}: {level} {}: {} Fix: {}",
            self.at, self.code, self.message, self.fix
        )
    }
}

/// Diagnostics, sorted by file, line, column, and code, with none twice.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
#[serde(transparent)]
pub struct Diagnostics(Vec<Diagnostic>);

impl Diagnostics {
    /// No diagnostics.
    #[must_use]
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    /// Adds one, keeping the order.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.0.push(diagnostic);
        self.sort();
    }

    /// Adds several, keeping the order.
    pub fn extend(&mut self, more: impl IntoIterator<Item = Diagnostic>) {
        self.0.extend(more);
        self.sort();
    }

    fn sort(&mut self) {
        self.0.sort_by(|a, b| {
            (&a.at, &a.code, &a.message).cmp(&(&b.at, &b.code, &b.message))
        });
        self.0.dedup();
    }

    /// Whether any is an error.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.0.iter().any(|item| item.level == Level::Error)
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// How many there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The diagnostics, in order.
    pub fn iter(&self) -> std::slice::Iter<'_, Diagnostic> {
        self.0.iter()
    }

    /// The first with this code.
    #[must_use]
    pub fn find(&self, code: &str) -> Option<&Diagnostic> {
        self.0.iter().find(|item| item.code.as_str() == code)
    }

    /// Splits into the errors and the warnings.
    #[must_use]
    pub fn split(self) -> (Self, Self) {
        let (errors, warnings) = self
            .0
            .into_iter()
            .partition(|item| item.level == Level::Error);
        (Self(errors), Self(warnings))
    }

    /// The diagnostics as JSON, for an agent.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

impl From<Diagnostic> for Diagnostics {
    fn from(diagnostic: Diagnostic) -> Self {
        Self(vec![diagnostic])
    }
}

impl FromIterator<Diagnostic> for Diagnostics {
    fn from_iter<I: IntoIterator<Item = Diagnostic>>(iter: I) -> Self {
        let mut all = Self::new();
        all.extend(iter);
        all
    }
}

impl IntoIterator for Diagnostics {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a Diagnostics {
    type Item = &'a Diagnostic;
    type IntoIter = std::slice::Iter<'a, Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl fmt::Display for Diagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for item in &self.0 {
            writeln!(f, "{item}")?;
        }
        Ok(())
    }
}

impl From<Diagnostics> for Failure {
    /// The failure an action reports when a target's files do not compile:
    /// class `input`, code `lint_failed`, or `path_not_allowed` when that is
    /// what the loader met. It names the first problem and counts the rest.
    fn from(all: Diagnostics) -> Self {
        let errors: Vec<&Diagnostic> = all
            .0
            .iter()
            .filter(|item| item.level == Level::Error)
            .collect();
        let Some(first) = errors.first() else {
            return Self::new(
                FailureClass::Input,
                LINT_FAILED,
                "The target's files have warnings and no errors.",
                "Read the warnings; nothing stops the compile.",
            );
        };
        let code = if first.code.as_str() == crate::codes::PATH_OUTSIDE_TARGET {
            PATH_NOT_ALLOWED
        } else {
            LINT_FAILED
        };
        let more = errors.len() - 1;
        let message = if more == 0 {
            format!("{}: {} ({})", first.at, first.message, first.code)
        } else {
            format!(
                "{}: {} ({}), and {more} more.",
                first.at, first.message, first.code
            )
        };
        Self::new(FailureClass::Input, code, message, first.fix.clone())
    }
}
