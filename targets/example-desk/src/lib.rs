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
//! starts it declares [`stand_child!`].
//!
//! On macOS the same window is made of AppKit's own controls, in a process
//! of its own that the same harness starts and stops, and the system's
//! accessibility interface reads it (decision 0244). Its fields, its
//! password field, and Save are subclasses that count what AppKit's
//! accessibility asks of them, which is what the stand can witness there.

#[cfg(any(windows, target_os = "macos"))]
mod child;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(any(windows, target_os = "macos", test))]
mod seat;
#[cfg(not(any(windows, target_os = "macos")))]
mod session;
#[cfg(windows)]
mod win;

#[cfg(target_os = "linux")]
mod app;
#[cfg(target_os = "linux")]
mod bus_service;
#[cfg(target_os = "linux")]
mod started;

use std::collections::BTreeMap;
use std::fmt;

#[cfg(target_os = "linux")]
pub use bus_service::{
    ADMIN_PASSWORD, CHILD, Calls, IDLE_NAME, LABEL, NAME as SERVICE_NAME, NUMBER, ORDER, ORDERS,
    ROOT, Service, TOTAL,
};
#[cfg(any(windows, target_os = "macos"))]
pub use child::{CHILD_TEST, Session};
#[cfg(not(any(windows, target_os = "macos")))]
pub use session::{IdleService, Session};
#[cfg(target_os = "linux")]
pub use started::{MUTE, STARTED, marker};

/// Declares, in a test binary, what serves the stand: on Windows and macOS
/// the stand's process is this test binary, copied and started again with
/// a variable that makes it serve the stand. On Windows the test this
/// declares serves it. On macOS, whose windows must be made on a process's
/// main thread, which no test runs on, a start function this declares
/// serves it on the main thread before the test harness begins; the system
/// calls such a function as it loads the program, and it returns at once
/// in every process that is not the stand's. In every other run, and on
/// Linux, both return at once. Every test binary that starts a [`Desk`]
/// declares it once.
#[macro_export]
macro_rules! stand_child {
    () => {
        /// Serves the desktop stand when this process is the stand's own
        /// (on Windows); otherwise passes at once.
        #[test]
        fn example_desk_stand_child() {
            $crate::serve_if_child();
        }

        /// On Linux, serves the desktop stand as a program of its own when
        /// this process is a copy of the test binary under one of the
        /// stand's names (decision 0999), before the test harness begins.
        // The system's loader calls each function in this section of a
        // program as it loads the program, before `main`; a function of the
        // plain C kind that takes nothing is what it calls.
        #[cfg(target_os = "linux")]
        #[allow(unsafe_code)]
        #[used]
        #[unsafe(link_section = ".init_array")]
        static EXAMPLE_DESK_STAND_STARTED: extern "C" fn() = {
            extern "C" fn start() {
                $crate::serve_if_started();
            }
            start
        };

        /// On macOS, serves the desktop stand on the main thread when this
        /// process is the stand's own, before the test harness begins.
        // The system calls each function in this section of a program as it
        // loads the program, on the main thread, before `main`; a function
        // of the plain C kind that takes nothing is what it calls.
        #[cfg(target_os = "macos")]
        #[allow(unsafe_code)]
        #[used]
        #[unsafe(link_section = "__DATA,__mod_init_func")]
        static EXAMPLE_DESK_STAND_START: extern "C" fn() = {
            extern "C" fn start() {
                $crate::serve_if_child();
            }
            start
        };
    };
}

/// Serves the stand and ends the process, when this process was started
/// to be the stand on Windows or macOS; otherwise returns at once. Called
/// by what [`stand_child!`] declares.
pub fn serve_if_child() {
    #[cfg(any(windows, target_os = "macos"))]
    child::serve_if_child();
}

/// Serves the stand as a program of its own and ends the process, when
/// this process is a copy of a test binary under one of the stand's names
/// on Linux ([`STARTED`], [`MUTE`]); otherwise returns at once. Called by
/// what [`stand_child!`] declares.
pub fn serve_if_started() {
    #[cfg(target_os = "linux")]
    started::serve_if_started();
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
/// The most characters the field labelled `New customer` holds: longer
/// text is cut, as some applications' fields do.
pub const CUSTOMER_MOST: usize = 24;
/// What the application shows under `Message` once Save has stored.
pub const SAVED: &str = "Saved.";
/// The title of the dialog [`Fault::DialogAfterRead`] raises.
pub const DIALOG: &str = "Example notice";
/// What the application says when a status is not one it knows.
#[cfg(any(target_os = "linux", windows, target_os = "macos"))]
pub(crate) const STATUS_REFUSED: &str = "Status must be open, held, or closed.";
/// What the application says when a customer is empty.
#[cfg(any(target_os = "linux", windows, target_os = "macos"))]
pub(crate) const CUSTOMER_REFUSED: &str = "Customer must not be empty.";
/// The statuses the application stores.
#[cfg(any(target_os = "linux", windows, target_os = "macos"))]
pub(crate) const STATUSES: &[&str] = &["open", "held", "closed"];

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

/// The headings of the table of a [`RecordWindow`], in order.
pub const RECORD_HEADINGS: [&str; 3] = ["Line", "Item", "Count"];

/// A window of one record, as a GTK 3 dialog program shows one (copy test
/// 10): its title, two labelled values, a labelled spin button, and a
/// table with no name whose column headers and cells are its own
/// children, with no row element between them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordWindow {
    /// The window's title, such as `Order 1001`.
    pub title: String,
    /// The value labelled `Customer`.
    pub customer: String,
    /// The value labelled `City`.
    pub city: String,
    /// The value of the spin button labelled `Lines`, as its text.
    pub lines: String,
    /// The table's rows, one cell under each of [`RECORD_HEADINGS`].
    pub rows: Vec<[String; 3]>,
    /// Whether the table states its column count, through AT-SPI's Table
    /// interface, as GTK 3 does. Without it nothing tells where a row ends.
    pub states_columns: bool,
}

impl RecordWindow {
    /// A window of the order `number`, titled `Order {number}`, whose
    /// table states its column count.
    #[must_use]
    pub fn order(number: &str, customer: &str, city: &str) -> Self {
        let row = |line: &str, item: &str, count: &str| {
            [line.to_owned(), item.to_owned(), count.to_owned()]
        };
        Self {
            title: format!("Order {number}"),
            customer: customer.to_owned(),
            city: city.to_owned(),
            lines: "2".to_owned(),
            rows: vec![
                row("1", &format!("{number}-A"), "4"),
                row("2", &format!("{number}-B"), "1"),
            ],
            states_columns: true,
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
    /// What the application stores, by name (`customer`, `status`): what
    /// the labelled values show, and what a window reached afresh shows.
    /// It changes only when Save stores.
    pub stored: BTreeMap<String, String>,
    /// How many times Save stored. A press that stores nothing (a value
    /// the application refuses, or a fault) is a write and no store.
    pub stores: u64,
    /// Every value set into a field, in order, with its window and its
    /// label. A password field's value is never kept here.
    pub sets: Vec<FieldSet>,
    /// What the field labelled `Message` shows now.
    pub message: String,
}

/// One value set into one field of the stand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSet {
    /// The title of the window the field is in.
    pub window: String,
    /// The field's label.
    pub field: String,
    /// The value set, before the field changed it; empty for a password
    /// field.
    pub value: String,
}

/// A fault a test switches on through the control interface. Each fault
/// of Save holds for the next press only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// Save stores nothing and says nothing.
    SaveSilently,
    /// Save stores, and then the application exits before it confirms.
    /// [`Desk::restart`] starts it again, with what it stored.
    ExitAfterSave,
    /// Save stores, and its confirmation appears once this has passed.
    ConfirmLate(std::time::Duration),
    /// The field with this label takes no value from now on.
    ReadOnly(String),
    /// The field with this label is disabled from now on: still editable
    /// by its state, and not enabled, so it takes no value.
    Disabled(String),
    /// Once the field labelled `Order number` has next been read, the
    /// window shows the order of this number: the application moved to
    /// another record between a reader's look and its act.
    AnotherRecordAfterRead(String),
    /// Once the field labelled `Order number` has next been read, the
    /// application raises a dialog titled [`DIALOG`], which no screen of
    /// the example target names.
    DialogAfterRead,
}

impl Truth {
    /// How many times the value of a control was asked for as text: on
    /// Linux through AT-SPI's text interface, on Windows by a message that
    /// gets an edit's text, on macOS by AppKit's accessibility asking a
    /// field for its value. A name is not a value.
    #[must_use]
    pub fn values_read(&self) -> u64 {
        self.asked
            .iter()
            .filter(|(member, _)| {
                member.as_str() == "Text.GetText"
                    || (member.starts_with("edit:")
                        && (member.ends_with(".WM_GETTEXT")
                            || member.ends_with(".EM_GETLINE")
                            || member.ends_with(".AXValue")))
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

/// The members of the AT-SPI interfaces, the window messages, and the
/// accessibility requests of AppKit that change an application. The Linux
/// stand answers some of the members, so that a test can see whether one
/// was asked; the Windows stand counts the messages its controls are sent;
/// the macOS stand counts the requests its controls are asked.
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
    "AXPress",
    "AXSetValue",
    "AXSetFocused",
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
    #[cfg(any(windows, target_os = "macos"))]
    inner: child::Stand,
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
    /// not embed the application; on Windows and macOS, when the stand's
    /// process does not show its window; on any other system, always.
    pub async fn start(session: &Session, order: Order) -> Result<Self, StandError> {
        #[cfg(target_os = "linux")]
        {
            Ok(Self {
                inner: app::App::start(session.address(), order).await?,
            })
        }
        #[cfg(any(windows, target_os = "macos"))]
        {
            Ok(Self {
                inner: child::Stand::start(session, order).await?,
            })
        }
        #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
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
    /// On Windows and macOS, when the stand's process does not answer: a
    /// test can prove nothing past a stand that cannot say what it holds,
    /// and the message says what it did say.
    #[must_use]
    pub fn truth(&self) -> Truth {
        #[cfg(target_os = "linux")]
        {
            self.inner.truth()
        }
        #[cfg(any(windows, target_os = "macos"))]
        {
            self.inner.truth().unwrap_or_else(|error| panic!("{error}"))
        }
        #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
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

    /// The process the stand runs in, for the stand's own test.
    #[cfg(target_os = "macos")]
    #[must_use]
    pub fn process(&self) -> u32 {
        self.inner.process()
    }

    /// The control interface: switches a fault on.
    ///
    /// # Errors
    ///
    /// When no field has the label a [`Fault::ReadOnly`] or a
    /// [`Fault::Disabled`] names; on Windows and macOS, for
    /// [`Fault::ExitAfterSave`] and [`Fault::ConfirmLate`], which are built
    /// for Linux only, and when the stand's process does not answer; on
    /// any other system, always.
    pub fn fault(&self, fault: &Fault) -> Result<(), StandError> {
        #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
        {
            self.inner.fault(fault)
        }
        #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
        {
            let _ = fault;
            Err(StandError(
                "the stand's faults of a write are built for Linux, Windows, and macOS only"
                    .to_owned(),
            ))
        }
    }

    /// The control interface: opens a second window of the application,
    /// titled `title`, with a field labelled `New customer` of its own.
    ///
    /// # Errors
    ///
    /// When its nodes cannot be published; on any system but Linux,
    /// always.
    pub async fn open_window(&self, title: &str) -> Result<(), StandError> {
        #[cfg(target_os = "linux")]
        {
            self.inner.open_window(title).await
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = title;
            Err(StandError(
                "the stand's second window is built for Linux only".to_owned(),
            ))
        }
    }

    /// The control interface: opens a window of one record (copy test 10's
    /// shape), as many as a test asks.
    ///
    /// # Errors
    ///
    /// When its nodes cannot be published; on any system but Linux,
    /// always.
    pub async fn open_record_window(&self, window: &RecordWindow) -> Result<(), StandError> {
        #[cfg(target_os = "linux")]
        {
            self.inner.open_record_window(window).await
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = window;
            Err(StandError(
                "the stand's window of one record is built for Linux only".to_owned(),
            ))
        }
    }

    /// The control interface: starts the application again after it
    /// exited (or ends it and starts it again), with what it stored, and
    /// its fields showing the stored values.
    ///
    /// # Errors
    ///
    /// When it cannot be published again; on any system but Linux,
    /// always.
    pub async fn restart(&mut self) -> Result<(), StandError> {
        #[cfg(target_os = "linux")]
        {
            self.inner.restart().await
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(StandError(
                "the stand's restart is built for Linux only".to_owned(),
            ))
        }
    }

    /// The control interface: the field labelled `Tracking` shows [`LATE`]
    /// once `after` has passed, and the application signals the change as
    /// a toolkit does.
    ///
    /// # Panics
    ///
    /// On Windows and macOS, when the stand's process does not take the
    /// command.
    pub fn show_late(&self, after: std::time::Duration) {
        #[cfg(target_os = "linux")]
        self.inner.show_late(after);
        #[cfg(any(windows, target_os = "macos"))]
        self.inner
            .show_late(after)
            .unwrap_or_else(|error| panic!("{error}"));
        #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
        let _ = after;
    }
}
