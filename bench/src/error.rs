//! Why a benchmark run or a comparison could not be done.

use std::fmt;
use std::path::PathBuf;

/// Why the harness stopped.
///
/// A scenario that fails is never timed: a failed request is not a measurement.
#[derive(Debug)]
pub enum BenchError {
    /// The plan asks for no measured iteration.
    NoIterations,
    /// A scenario was asked for by a name the harness does not know.
    UnknownScenario(String),
    /// A scenario's setup failed.
    Setup {
        /// The scenario.
        scenario: String,
        /// What went wrong.
        message: String,
    },
    /// An operation or a baseline operation failed.
    Operation {
        /// The scenario.
        scenario: String,
        /// What went wrong.
        message: String,
    },
    /// A file could not be read or written.
    File {
        /// The file or folder.
        path: PathBuf,
        /// The error of the operating system.
        source: std::io::Error,
    },
    /// A file is not what the harness reads: wrong JSON, wrong schema, wrong version.
    Format {
        /// The file.
        path: PathBuf,
        /// What is wrong with it.
        message: String,
    },
    /// Two results cannot be compared, because they were not taken on the same kind of machine.
    NotComparable(String),
    /// The command line is wrong.
    Usage(String),
}

impl BenchError {
    /// A failed setup of the named scenario.
    #[must_use]
    pub fn setup(scenario: &str, message: impl fmt::Display) -> Self {
        Self::Setup {
            scenario: scenario.to_owned(),
            message: message.to_string(),
        }
    }

    /// A failed operation of the named scenario.
    #[must_use]
    pub fn operation(scenario: &str, message: impl fmt::Display) -> Self {
        Self::Operation {
            scenario: scenario.to_owned(),
            message: message.to_string(),
        }
    }
}

impl fmt::Display for BenchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoIterations => write!(f, "a run needs at least one measured iteration"),
            Self::UnknownScenario(name) => write!(f, "there is no scenario named {name}"),
            Self::Setup { scenario, message } => {
                write!(f, "the setup of scenario {scenario} failed: {message}")
            }
            Self::Operation { scenario, message } => {
                write!(f, "an operation of scenario {scenario} failed: {message}")
            }
            Self::File { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Format { path, message } => write!(f, "{}: {message}", path.display()),
            Self::NotComparable(message) => {
                write!(f, "the two results cannot be compared: {message}")
            }
            Self::Usage(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for BenchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::File { source, .. } => Some(source),
            _ => None,
        }
    }
}
