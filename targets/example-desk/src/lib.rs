//! Test stand: one desktop program's accessibility tree, the smallest that
//! shows what makes a desktop application hard to read.
//!
//! The stand is a library a test hosts in its own process. It starts a
//! session bus of the test's own, behind which the system's accessibility
//! bus launcher and registry run ([`Session`]), and publishes one
//! application on that accessibility bus over AT-SPI, as a toolkit does
//! ([`Desk`]): a window titled [`WINDOW`] with the labelled values of one
//! order, a table of its line items, a control with no accessible name, two
//! controls of one role and name, a password field, a value that appears
//! late, and a Save button. It never draws a pixel: what a desktop pack
//! reads is the tree, and the tree is real AT-SPI on a real bus, through
//! the real registry daemon.
//!
//! Its control interface ([`Desk::truth`]) says what it holds and what it
//! was asked: every method by name, how many times its password field's
//! text was asked for, and how many times Save was pressed. A test of the
//! desktop pack asks it, and never proves anything through the pack's own
//! read alone.
//!
//! On Windows the same window is made of plain Win32 controls, in a process
//! of its own that the test starts and stops, and UI Automation reads it
//! through the proxies the system ships for its standard controls
//! (decision 0209). Every subclassed control counts the messages a client
//! sends it, which is what the stand can witness there; a test binary that
//! starts it declares [`stand_child!`]. On macOS [`Session::start`] finds
//! nothing to start, as on a Linux machine without the programs.

#[cfg(not(windows))]
mod session;
#[cfg(windows)]
mod win;

#[cfg(target_os = "linux")]
mod app;

use std::collections::BTreeMap;
use std::fmt;

#[cfg(not(windows))]
pub use session::Session;
#[cfg(windows)]
pub use win::{CHILD_TEST, Session};

/// Declares, in a test binary, the test that serves the stand on Windows:
/// there the stand's process is this test binary, copied and started again
/// with this one test alone. In every other run, and on every other
/// system, the test returns at once. Every test binary that starts a
/// [`Desk`] declares it once.
#[macro_export]
macro_rules! stand_child {
    () => {
        /// Serves the desktop stand when this process is the stand's own
        /// (on Windows); otherwise passes at once.
        #[test]
        fn example_desk_stand_child() {
            $crate::serve_if_child();
        }
    };
}

/// Serves the stand and ends the process, when this process was started
/// to be the stand on Windows; otherwise returns at once. Called by the
/// test [`stand_child!`] declares.
pub fn serve_if_child() {
    #[cfg(windows)]
    win::serve_if_child();
}

/// The name the application has on the accessibility bus.
pub const APPLICATION: &str = "example-desk";
/// The title of its one window.
pub const WINDOW: &str = "Example order";
/// What the password field holds. It must appear in no result, finding,
/// draft, or log of the pack under test. Plainly made up.
pub const SECRET: &str = "secret-for-tests-desk-access-code";
/// The value that appears late in the field labelled `Tracking`.
pub const LATE: &str = "TRK-0001";
/// The variable that makes a missing program a failure rather than a skip,
/// for the machine that must run these tests.
pub const REQUIRE_ENV: &str = "A2M_REQUIRE_DESKTOP";

/// One line of the order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The item code.
    pub code: String,
    /// What the item is.
    pub description: String,
    /// How many, as the window shows it.
    pub quantity: String,
}

/// What the window shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    /// The order's number, labelled `Order number`.
    pub number: String,
    /// Its customer, labelled `Customer`.
    pub customer: String,
    /// Its status, labelled `Status`.
    pub status: String,
    /// Its line items, in the table `Line items`.
    pub lines: Vec<Line>,
}

impl Order {
    /// The order the stand shows unless a test gives another.
    #[must_use]
    pub fn example() -> Self {
        let line = |code: &str, description: &str, quantity: &str| Line {
            code: code.to_owned(),
            description: description.to_owned(),
            quantity: quantity.to_owned(),
        };
        Self {
            number: "A-100".to_owned(),
            customer: "Example Supplies".to_owned(),
            status: "open".to_owned(),
            lines: vec![
                line("L-101", "Hinge, steel", "4"),
                line("L-102", "Bracket", "2"),
                line("L-103", "Screw, brass", "24"),
            ],
        }
    }
}

/// What the stand holds and what it was asked, from its control interface.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Truth {
    /// How many times Save was pressed. Each press is a write.
    pub writes: u64,
    /// How many times the password field's text, or its length, was asked
    /// for.
    pub secret_reads: u64,
    /// Every method and property asked of the application, as
    /// `interface.member`, with how many times.
    pub asked: BTreeMap<String, u64>,
    /// What the field labelled `Tracking` holds now.
    pub tracking: String,
}

impl Truth {
    /// How many times the value of a control was asked for as text: on
    /// Linux through AT-SPI's text interface, on Windows by a message that
    /// gets an edit's text. A name is not a value.
    #[must_use]
    pub fn values_read(&self) -> u64 {
        self.asked
            .iter()
            .filter(|(member, _)| {
                member.as_str() == "Text.GetText"
                    || (member.starts_with("edit:")
                        && (member.ends_with(".WM_GETTEXT") || member.ends_with(".EM_GETLINE")))
            })
            .map(|(_, times)| *times)
            .sum()
    }

    /// Whether anything was asked that could change the application: a
    /// press, a text set or typed, a value set, a selection, a focus.
    #[must_use]
    pub fn acted(&self) -> bool {
        self.asked.keys().any(|member| {
            let name = member.rsplit('.').next().unwrap_or_default();
            ACTING_MEMBERS.contains(&name)
        })
    }
}

/// The members of the AT-SPI interfaces, and the window messages, that
/// change an application. The Linux stand answers some of the members, so
/// that a test can see whether one was asked; the Windows stand counts the
/// messages its controls are sent.
pub const ACTING_MEMBERS: &[&str] = &[
    "DoAction",
    "SetTextContents",
    "InsertText",
    "DeleteText",
    "CutText",
    "PasteText",
    "CopyText",
    "SetCurrentValue",
    "SelectChild",
    "DeselectChild",
    "SelectAll",
    "ClearSelection",
    "GrabFocus",
    "SetSelection",
    "AddSelection",
    "SetCaretOffset",
    "ScrollTo",
    "SetPosition",
    "SetExtents",
    "SetSize",
    "WM_SETTEXT",
    "EM_REPLACESEL",
    "EM_SETSEL",
    "EM_UNDO",
    "WM_CHAR",
    "WM_KEYDOWN",
    "WM_LBUTTONDOWN",
    "WM_SETFOCUS",
    "WM_PASTE",
    "WM_CUT",
    "WM_CLEAR",
    "BM_CLICK",
    "BM_SETCHECK",
    "BM_SETSTATE",
    "LVM_SETITEMSTATE",
    "LVM_SETITEMTEXTW",
    "LVM_ENSUREVISIBLE",
    "LVM_SCROLL",
];

/// Why the stand could not do what a test asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandError(pub String);

impl fmt::Display for StandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the desktop stand: {}", self.0)
    }
}

impl std::error::Error for StandError {}

/// Whether the machine must run the stand's tests.
#[must_use]
pub fn required() -> bool {
    std::env::var(REQUIRE_ENV).is_ok_and(|value| value == "1")
}

/// The stand's application, published on a session's accessibility bus.
/// Dropping it takes the application off the bus.
pub struct Desk {
    #[cfg(target_os = "linux")]
    inner: app::App,
    #[cfg(windows)]
    inner: win::Stand,
}

/// Two of the Windows stand's controls, by their window handles in number
/// form, for the stand's own test of its witness.
#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handles {
    /// Save.
    pub save: isize,
    /// The password edit.
    pub password: isize,
}

impl fmt::Debug for Desk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Desk").finish_non_exhaustive()
    }
}

impl Desk {
    /// Publishes the application, showing `order`, on the session's
    /// accessibility bus, and returns once the registry has embedded it.
    ///
    /// # Errors
    ///
    /// When the accessibility bus cannot be reached or the registry does
    /// not embed the application; on Windows, when the stand's process does
    /// not show its window; on any other system, always.
    pub async fn start(session: &Session, order: Order) -> Result<Self, StandError> {
        #[cfg(target_os = "linux")]
        {
            Ok(Self {
                inner: app::App::start(session.address(), order).await?,
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                inner: win::Stand::start(session, order).await?,
            })
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            let _ = (session, order);
            Err(StandError(
                "the stand publishes over AT-SPI, which this system does not have".to_owned(),
            ))
        }
    }

    /// What the stand holds and what it was asked.
    ///
    /// # Panics
    ///
    /// On Windows, when the stand's process does not answer: a test can
    /// prove nothing past a stand that cannot say what it holds, and the
    /// message says what it did say.
    #[must_use]
    pub fn truth(&self) -> Truth {
        #[cfg(target_os = "linux")]
        {
            self.inner.truth()
        }
        #[cfg(windows)]
        {
            self.inner.truth().unwrap_or_else(|error| panic!("{error}"))
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            Truth::default()
        }
    }

    /// Save and the password edit, for the stand's own test.
    #[cfg(windows)]
    #[must_use]
    pub fn handles(&self) -> Handles {
        Handles {
            save: self.inner.save,
            password: self.inner.password,
        }
    }

    /// The control interface: the field labelled `Tracking` shows [`LATE`]
    /// once `after` has passed, and the application signals the change as
    /// a toolkit does.
    ///
    /// # Panics
    ///
    /// On Windows, when the stand's process does not take the command.
    pub fn show_late(&self, after: std::time::Duration) {
        #[cfg(target_os = "linux")]
        self.inner.show_late(after);
        #[cfg(windows)]
        self.inner
            .show_late(after)
            .unwrap_or_else(|error| panic!("{error}"));
        #[cfg(not(any(target_os = "linux", windows)))]
        let _ = after;
    }
}
