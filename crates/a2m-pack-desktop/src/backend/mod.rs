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
//! (decision 0209); on macOS, the system's accessibility interface on a
//! thread of the handle's own, once the person has allowed the program to
//! use it (decision 0244). Elsewhere it is a stand-in whose every method
//! stops with `system_not_built`.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux::{Acted, Heard, Look, Proof, Settable};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) mod wmi;
#[cfg(windows)]
pub(crate) use self::windows::Look;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::Look;

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
mod none;
#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
pub(crate) use none::Look;

use a2m_core::Failure;

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
    #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
    NotRunning,
    /// Several are.
    #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
    Ambiguous(usize),
    /// The session's accessibility switch is off.
    #[cfg(target_os = "linux")]
    AccessibilityOff,
    /// The program runs at a higher level than the engine.
    #[cfg(windows)]
    Elevated,
    /// The program runs as another user, or in another session.
    #[cfg(any(windows, target_os = "macos"))]
    NotOwned,
    /// The person has not allowed this program to use the system's
    /// accessibility interface.
    #[cfg(target_os = "macos")]
    NotTrusted,
    /// The program's digest is not the one stated.
    #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
    NotProven,
    /// Before an act, the application's connection is no longer owned by
    /// the process proven when the run was prepared, or the element is not
    /// the application's.
    #[cfg(target_os = "linux")]
    Replaced,
    /// The service the surface names has no owner on the session bus. Never
    /// started.
    #[cfg(target_os = "linux")]
    ServiceNotRunning,
    /// A property of a service holds a value that is not text, a flag, a
    /// number, or a path.
    #[cfg(target_os = "linux")]
    NotText,
    /// A control that holds a secret was asked for its value.
    #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
    Secret,
    /// A text longer than the bound.
    #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
    TooLarge,
    /// The interface, or the application, went away or answered with an
    /// error: what the system said, with no value of the application.
    #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
    Lost(String),
    /// The deadline passed.
    Timeout,
    /// This system has no back end.
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    NotBuilt,
}

impl AxError {
    /// The failure the driver reports, with no value of the application in
    /// it.
    pub(crate) fn failure(&self, program: &str) -> Failure {
        #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
        let program = codes::shown(program);
        #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
        let _ = program;
        match self {
            #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
            Self::NotRunning => codes::failure(
                &codes::row::APPLICATION_NOT_RUNNING,
                format!(
                    "No application in this session is the program `{program}` with the name the surface states and {}. The desktop pack starts nothing.",
                    if cfg!(any(windows, target_os = "macos")) {
                        "shows a window"
                    } else {
                        "publishes an accessibility tree"
                    }
                ),
                "Start the application as you normally do, then run the tool again.",
            ),
            #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
            Self::Ambiguous(count) => codes::failure(
                &codes::row::APPLICATION_AMBIGUOUS,
                format!(
                    "{count} running applications are the program `{program}` with the name the surface states; the pack reads one only when it is the only one."
                ),
                "Close the other copies of the application, then run the tool again.",
            ),
            #[cfg(target_os = "linux")]
            Self::AccessibilityOff => codes::failure(
                &codes::row::ACCESSIBILITY_OFF,
                "The session's accessibility setting is off, and the application publishes no accessibility tree while it is. The pack changes no setting.".to_owned(),
                "Turn on the accessibility setting of your desktop (the toolkit accessibility switch), restart the application, and run the tool again.",
            ),
            #[cfg(windows)]
            Self::Elevated => codes::failure(
                &codes::row::TARGET_ELEVATED,
                format!(
                    "The program `{program}` runs at a higher level than the engine (as administrator), and the pack does not reach it."
                ),
                "Start the application as yourself, without administrator rights, then run the tool again.",
            ),
            #[cfg(target_os = "macos")]
            Self::NotTrusted => codes::failure(
                &codes::row::ACCESSIBILITY_OFF,
                "This program may not use the system's accessibility interface: the person has not allowed it, and the desktop pack reads nothing without it. The pack never allows it, and never asks the system to show its prompt.".to_owned(),
                &trust_next(),
            ),
            #[cfg(any(windows, target_os = "macos"))]
            Self::NotOwned => codes::failure(
                &codes::row::APPLICATION_NOT_OWNED,
                format!(
                    "The program `{program}` runs as another user{}, and the pack reads only the applications of the user the engine runs as{}.",
                    if cfg!(windows) { ", or in another session" } else { "" },
                    if cfg!(windows) { ", in its own session" } else { "" },
                ),
                "Start the application in your own session, as yourself, then run the tool again.",
            ),
            #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
            Self::NotProven => codes::failure(
                &codes::row::APPLICATION_NOT_PROVEN,
                format!(
                    "The running program `{program}` does not have the digest the surface states."
                ),
                "Review the program that is installed, then state its digest in the surface, or reinstall the reviewed one.",
            ),
            #[cfg(target_os = "linux")]
            Self::Replaced => codes::failure(
                &codes::row::APPLICATION_NOT_PROVEN,
                format!(
                    "Before acting, the application is no longer the process of the program `{program}` that was proven when the run began, or the element is not that application's. Nothing was sent."
                ),
                "Check which program shows the window, then run the tool again.",
            ),
            #[cfg(target_os = "linux")]
            Self::ServiceNotRunning => codes::failure(
                &codes::row::SERVICE_NOT_RUNNING,
                "No connection owns the bus name the surface states as its service. The pack starts nothing, whatever a `.service` file says.".to_owned(),
                "Start the application as you normally do, then run the tool again.",
            ),
            #[cfg(target_os = "linux")]
            Self::NotText => codes::failure(
                &codes::row::PROPERTY_NOT_TEXT,
                "The property holds a value that is not text, a flag, a number, or an object path, which are the forms the pack gives.".to_owned(),
                "Read another property.",
            ),
            #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
            Self::Secret => codes::failure(
                &codes::row::PASSWORD_READ_REFUSED,
                "The element is a control that holds a secret; its value is never read.".to_owned(),
                "Read another field. A secret is never an output of a tool.",
            ),
            #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
            Self::TooLarge => codes::failure(
                &codes::row::READ_TOO_LARGE,
                "A value of the window is longer than the desktop pack reads.".to_owned(),
                "Read a field that holds less.",
            ),
            #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
            Self::Lost(why) => codes::failure(
                &a2m_provider::codes::row::CONNECTION_LOST,
                format!(
                    "The accessibility interface, or the application on it, went away or did not answer: {}.",
                    codes::shown(why)
                ),
                "Check that the application is still running, then run the tool again.",
            ),
            Self::Timeout => codes::failure(
                &a2m_provider::codes::row::TIMEOUT,
                "The application did not answer in time.".to_owned(),
                "Run the tool again.",
            ),
            #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
            Self::NotBuilt => codes::system_not_built(),
        }
    }
}

/// Where the person allows a program to use the accessibility interface,
/// and which program to allow: the one that started the engine, and, when
/// that one is allowed already, the engine's own program.
#[cfg(target_os = "macos")]
fn trust_next() -> String {
    let own = std::env::current_exe()
        .ok()
        .map(|path| format!(" (`{}`)", codes::shown(&path.to_string_lossy())))
        .unwrap_or_default();
    format!(
        "In System Settings, open Privacy & Security, then Accessibility, and allow the program that started the engine, such as the terminal or the agent application that runs it; if that program is allowed already, allow the engine's own program{own} as well. Then run the tool again, and if it still stops here, quit and start that program again."
    )
}
