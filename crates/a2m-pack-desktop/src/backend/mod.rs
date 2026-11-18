//! The back end: one per operating system, behind one look-only handle.
//!
//! [`Look`] is the only way the pack reaches an application, and its type
//! has no method that sets a value, invokes an action, moves focus, or
//! sends a key or a click: it finds the declared application, lists its
//! windows, walks a window's tree within bounds, reads one element's text,
//! and waits for the application to signal a change. Whatever a target
//! file or a caller holds, nothing else can be asked through it.
//!
//! On Linux it is AT-SPI over the session's accessibility bus (decision
//! 0198); on Windows, UI Automation on a thread of the handle's own
//! (decision 0209). Elsewhere it is a stand-in whose every method stops
//! with `system_not_built`.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux::{Acted, Heard, Look, Proof, Settable};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use self::windows::Look;

#[cfg(not(any(target_os = "linux", windows)))]
mod none;
#[cfg(not(any(target_os = "linux", windows)))]
pub(crate) use none::Look;

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
/// the accessibility bus, or on Windows its process, and its process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct App {
    /// Its root element.
    pub root: El,
    /// The process the bus daemon says owns its connection, or on Windows
    /// the process its windows belong to.
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
    #[cfg(any(target_os = "linux", windows))]
    NotRunning,
    /// Several are.
    #[cfg(any(target_os = "linux", windows))]
    Ambiguous(usize),
    /// The session's accessibility switch is off.
    #[cfg(target_os = "linux")]
    AccessibilityOff,
    /// The program runs at a higher level than the engine.
    #[cfg(windows)]
    Elevated,
    /// The program runs as another user, or in another session.
    #[cfg(windows)]
    NotOwned,
    /// The program's digest is not the one stated.
    #[cfg(any(target_os = "linux", windows))]
    NotProven,
    /// Before an act, the application's connection is no longer owned by
    /// the process proven when the run was prepared, or the element is not
    /// the application's.
    #[cfg(target_os = "linux")]
    Replaced,
    /// A control that holds a secret was asked for its value.
    #[cfg(any(target_os = "linux", windows))]
    Secret,
    /// A text longer than the bound.
    #[cfg(any(target_os = "linux", windows))]
    TooLarge,
    /// The interface, or the application, went away or answered with an
    /// error: what the system said, with no value of the application.
    #[cfg(any(target_os = "linux", windows))]
    Lost(String),
    /// The deadline passed.
    Timeout,
    /// This system has no back end.
    #[cfg(not(any(target_os = "linux", windows)))]
    NotBuilt,
}

impl AxError {
    /// The failure the driver reports, with no value of the application in
    /// it.
    pub(crate) fn failure(&self, program: &str) -> Failure {
        #[cfg(any(target_os = "linux", windows))]
        let program = codes::shown(program);
        #[cfg(not(any(target_os = "linux", windows)))]
        let _ = program;
        match self {
            #[cfg(any(target_os = "linux", windows))]
            Self::NotRunning => codes::failure(
                FailureClass::Busy,
                codes::APPLICATION_NOT_RUNNING,
                format!(
                    "No application in this session is the program `{program}` with the name the surface states and {}. The desktop pack starts nothing.",
                    if cfg!(windows) {
                        "shows a window"
                    } else {
                        "publishes an accessibility tree"
                    }
                ),
                "Start the application as you normally do, then run the tool again.",
            ),
            #[cfg(any(target_os = "linux", windows))]
            Self::Ambiguous(count) => codes::failure(
                FailureClass::UnknownVariant,
                codes::APPLICATION_AMBIGUOUS,
                format!(
                    "{count} running applications are the program `{program}` with the name the surface states; the pack reads one only when it is the only one."
                ),
                "Close the other copies of the application, then run the tool again.",
            ),
            #[cfg(target_os = "linux")]
            Self::AccessibilityOff => codes::failure(
                FailureClass::Busy,
                codes::ACCESSIBILITY_OFF,
                "The session's accessibility setting is off, and the application publishes no accessibility tree while it is. The pack changes no setting.".to_owned(),
                "Turn on the accessibility setting of your desktop (the toolkit accessibility switch), restart the application, and run the tool again.",
            ),
            #[cfg(windows)]
            Self::Elevated => codes::failure(
                FailureClass::Busy,
                codes::TARGET_ELEVATED,
                format!(
                    "The program `{program}` runs at a higher level than the engine (as administrator), and the pack does not reach it."
                ),
                "Start the application as yourself, without administrator rights, then run the tool again.",
            ),
            #[cfg(windows)]
            Self::NotOwned => codes::failure(
                FailureClass::Policy,
                codes::APPLICATION_NOT_OWNED,
                format!(
                    "The program `{program}` runs as another user, or in another session, and the pack reads only the applications of the user the engine runs as, in its own session."
                ),
                "Start the application in your own session, as yourself, then run the tool again.",
            ),
            #[cfg(any(target_os = "linux", windows))]
            Self::NotProven => codes::failure(
                FailureClass::UnknownVariant,
                codes::APPLICATION_NOT_PROVEN,
                format!(
                    "The running program `{program}` does not have the digest the surface states."
                ),
                "Review the program that is installed, then state its digest in the surface, or reinstall the reviewed one.",
            ),
            #[cfg(target_os = "linux")]
            Self::Replaced => codes::failure(
                FailureClass::UnknownVariant,
                codes::APPLICATION_NOT_PROVEN,
                format!(
                    "Before acting, the application is no longer the process of the program `{program}` that was proven when the run began, or the element is not that application's. Nothing was sent."
                ),
                "Check which program shows the window, then run the tool again.",
            ),
            #[cfg(any(target_os = "linux", windows))]
            Self::Secret => codes::failure(
                FailureClass::Policy,
                codes::PASSWORD_READ_REFUSED,
                "The element is a control that holds a secret; its value is never read.".to_owned(),
                "Read another field. A secret is never an output of a tool.",
            ),
            #[cfg(any(target_os = "linux", windows))]
            Self::TooLarge => codes::failure(
                FailureClass::UnknownVariant,
                codes::READ_TOO_LARGE,
                "A value of the window is longer than the desktop pack reads.".to_owned(),
                "Read a field that holds less.",
            ),
            #[cfg(any(target_os = "linux", windows))]
            Self::Lost(why) => codes::failure(
                FailureClass::Transient,
                a2m_provider::codes::CONNECTION_LOST,
                format!(
                    "The accessibility interface, or the application on it, went away or did not answer: {}.",
                    codes::shown(why)
                ),
                "Check that the application is still running, then run the tool again.",
            ),
            Self::Timeout => codes::failure(
                FailureClass::Transient,
                a2m_provider::codes::TIMEOUT,
                "The application did not answer in time.".to_owned(),
                "Run the tool again.",
            ),
            #[cfg(not(any(target_os = "linux", windows)))]
            Self::NotBuilt => codes::system_not_built(),
        }
    }
}
