//! The back end: one per operating system, behind one look-only handle.
//!
//! [`Look`] is the only way the pack reaches an application, and its type
//! has no method that sets a value, invokes an action, moves focus, or
//! sends a key or a click: it finds the declared application, lists its
//! windows, walks a window's tree within bounds, reads one element's text,
//! and waits for the application to signal a change. Whatever a target
//! file or a caller holds, nothing else can be asked through it.
//!
//! On Linux it is AT-SPI over the session's accessibility bus. Elsewhere it
//! is a stand-in whose every method stops with `system_not_built`.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux::{Look, Watch};

#[cfg(not(target_os = "linux"))]
mod none;
#[cfg(not(target_os = "linux"))]
pub(crate) use none::{Look, Watch};

use std::time::Duration;

use a2m_core::{Failure, FailureClass};

use crate::codes;
use crate::tree::El;

/// How far one walk of a window may go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Limits {
    /// The deepest element below the window.
    pub depth: u32,
    /// The most elements.
    pub nodes: usize,
    /// The longest text one read of an element may give, in characters.
    pub chars: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            depth: 32,
            nodes: 2_000,
            chars: 65_536,
        }
    }
}

/// The application a surface declares, found and proven: its connection on
/// the accessibility bus and its process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct App {
    /// Its root element.
    pub root: El,
    /// The process the bus daemon says owns its connection.
    pub process: u32,
}

/// One of the application's windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Window {
    pub el: El,
    pub role: &'static str,
    pub title: String,
}

/// Why the back end could not do what it was asked. Nothing that could
/// change the application was ever sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AxError {
    /// No running application is the program and the name.
    NotRunning,
    /// Several are.
    Ambiguous(usize),
    /// The session's accessibility switch is off.
    AccessibilityOff,
    /// The program's digest is not the one stated.
    NotProven,
    /// A control that holds a secret was asked for its value.
    Secret,
    /// A text longer than the bound.
    TooLarge,
    /// The bus, or the application, went away or answered with an error.
    Lost(String),
    /// The deadline passed.
    Timeout,
    /// This system has no back end.
    NotBuilt,
}

impl AxError {
    /// The failure the driver reports, with no value of the application in
    /// it.
    pub(crate) fn failure(&self, program: &str) -> Failure {
        let program = codes::shown(program);
        match self {
            Self::NotRunning => codes::failure(
                FailureClass::Busy,
                codes::APPLICATION_NOT_RUNNING,
                format!(
                    "No running application in this session is the program `{program}` with the name the surface states. The desktop pack starts nothing."
                ),
                "Start the application as you normally do, then run the tool again.",
            ),
            Self::Ambiguous(count) => codes::failure(
                FailureClass::UnknownVariant,
                codes::APPLICATION_AMBIGUOUS,
                format!(
                    "{count} running applications are the program `{program}` with the name the surface states; the pack reads one only when it is the only one."
                ),
                "Close the other copies of the application, then run the tool again.",
            ),
            Self::AccessibilityOff => codes::failure(
                FailureClass::Busy,
                codes::ACCESSIBILITY_OFF,
                "The session's accessibility setting is off, and the application publishes no accessibility tree while it is. The pack changes no setting.".to_owned(),
                "Turn on the accessibility setting of your desktop (the toolkit accessibility switch), restart the application, and run the tool again.",
            ),
            Self::NotProven => codes::failure(
                FailureClass::UnknownVariant,
                codes::APPLICATION_NOT_PROVEN,
                format!(
                    "The running program `{program}` does not have the digest the surface states."
                ),
                "Review the program that is installed, then state its digest in the surface, or reinstall the reviewed one.",
            ),
            Self::Secret => codes::failure(
                FailureClass::Policy,
                codes::PASSWORD_READ_REFUSED,
                "The element is a control that holds a secret; its value is never read.".to_owned(),
                "Read another field. A secret is never an output of a tool.",
            ),
            Self::TooLarge => codes::failure(
                FailureClass::UnknownVariant,
                codes::READ_TOO_LARGE,
                "A value of the window is longer than the desktop pack reads.".to_owned(),
                "Read a field that holds less.",
            ),
            Self::Lost(_) => codes::failure(
                FailureClass::Transient,
                a2m_provider::codes::CONNECTION_LOST,
                "The accessibility bus, or the application on it, went away or did not answer.".to_owned(),
                "Check that the application is still running, then run the tool again.",
            ),
            Self::Timeout => codes::failure(
                FailureClass::Transient,
                a2m_provider::codes::TIMEOUT,
                "The application did not answer in time.".to_owned(),
                "Run the tool again.",
            ),
            Self::NotBuilt => codes::system_not_built(),
        }
    }
}

/// The longest one call to the application may take.
pub(crate) const CALL_LIMIT: Duration = Duration::from_secs(5);
