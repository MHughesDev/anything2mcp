//! The stand's window on macOS, in the stand's own process: AppKit's own
//! controls, which the system's accessibility interface reads through
//! AppKit's own accessibility, with no provider of the stand's own.
//!
//! ```text
//! window "Example order"                       (shown without making the stand the active application)
//!   text field, first of all, that nothing labels: "a value nothing labels"
//!   label "Order number"  text field "A-100"   (the label is the field's title element)
//!   label "Customer"      text field "Example Supplies"
//!   label "Status"        text field "open"
//!   label "Tracking"      text field ""        (shows LATE when the control interface says so)
//!   label "Reference"     text field "R-1"
//!   label "Reference"     text field "R-2"
//!   label "Access code"   secure text field holding SECRET
//!   label "Line items"    table "Line items" in a scroll view: three columns, one row a line
//!   button "Save"                              (a press is counted as a write)
//! ```
//!
//! What the stand witnesses: the text fields, the secure text field, and
//! Save are subclasses of AppKit's own, which count each time AppKit's
//! accessibility asks one for its value (`accessibilityValue`), sets its
//! value or its focus (`setAccessibilityValue:`, `setAccessibilityFocused:`),
//! or presses it (`accessibilityPerformPress`), and Save counts each time
//! its action runs, as a write. What it cannot witness: a question AppKit
//! answers without asking the control itself (its title, its role, its
//! children, a label's text, a table's cells), anything the client does
//! with an answer, and, unproven until a run on macOS shows it, whether
//! AppKit's accessibility asks the subclass's own methods for each request
//! a client makes rather than another object of AppKit's. The stand's own
//! test proves the count of the password field's value against a client
//! that asks it. That the desktop pack never asks for the password's value
//! is proven by the pack's own list of the calls it may make; the stand's
//! count is the second witness.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::io::BufRead;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityPostNotification, NSAccessibilityValueChangedNotification,
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton, NSControl,
    NSControlTextEditingDelegate, NSResponder, NSScrollView, NSSecureTextField, NSTableColumn,
    NSTableView, NSTableViewDataSource, NSTableViewDelegate, NSTextField, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{NSInteger, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::child::{ORDER_ENV, PREFIX, decode_order};
use crate::{LATE, Order, SECRET, WINDOW};

static WRITES: AtomicU64 = AtomicU64::new(0);
static SECRET_READS: AtomicU64 = AtomicU64::new(0);
static ASKED: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
static SHOWN_LATE: Mutex<String> = Mutex::new(String::new());

thread_local! {
    /// The objects the stand keeps on the main thread: its window, the
    /// table's source of lines, and the tracking field.
    static WINDOW_KEPT: RefCell<Option<Retained<NSWindow>>> = const { RefCell::new(None) };
    static LINES_KEPT: RefCell<Option<Retained<Lines>>> = const { RefCell::new(None) };
    static TRACKING: RefCell<Option<Retained<ValueField>>> = const { RefCell::new(None) };
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Counts one request a client made of a control, as `control.request`.
fn count(member: &str) {
    *lock(&ASKED).entry(member.to_owned()).or_insert(0) += 1;
}

define_class!(
    // SAFETY: a text field may be subclassed; the class has no `Drop`, and
    // each method hands the request on to the superclass's own with the
    // arguments it was given, after counting it.
    #[unsafe(super(NSTextField, NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ExampleDeskValueField"]
    #[ivars = &'static str]
    struct ValueField;

    impl ValueField {
        #[unsafe(method_id(accessibilityValue))]
        fn accessibility_value(&self) -> Option<Retained<AnyObject>> {
            count(&format!("edit:{}.AXValue", self.ivars()));
            // SAFETY: the superclass's own answer to the same request.
            unsafe { msg_send![super(self), accessibilityValue] }
        }

        #[unsafe(method(setAccessibilityValue:))]
        fn set_accessibility_value(&self, value: Option<&AnyObject>) {
            count(&format!("edit:{}.AXSetValue", self.ivars()));
            // SAFETY: as above, with the value the request carried.
            unsafe { msg_send![super(self), setAccessibilityValue: value] }
        }

        #[unsafe(method(setAccessibilityFocused:))]
        fn set_accessibility_focused(&self, focused: bool) {
            count(&format!("edit:{}.AXSetFocused", self.ivars()));
            // SAFETY: as above.
            unsafe { msg_send![super(self), setAccessibilityFocused: focused] }
        }
    }
);

impl ValueField {
    fn new(mtm: MainThreadMarker, name: &'static str, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(name);
        // SAFETY: a text field's own initializer, with a plain frame.
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

define_class!(
    // SAFETY: a secure text field may be subclassed; the class has no
    // `Drop`, and each method hands the request on to the superclass's own.
    #[unsafe(super(NSSecureTextField, NSTextField, NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ExampleDeskSecretField"]
    struct SecretField;

    impl SecretField {
        #[unsafe(method_id(accessibilityValue))]
        fn accessibility_value(&self) -> Option<Retained<AnyObject>> {
            count("edit:access_code.AXValue");
            SECRET_READS.fetch_add(1, Ordering::SeqCst);
            // SAFETY: the superclass's own answer to the same request.
            unsafe { msg_send![super(self), accessibilityValue] }
        }

        #[unsafe(method(setAccessibilityValue:))]
        fn set_accessibility_value(&self, value: Option<&AnyObject>) {
            count("edit:access_code.AXSetValue");
            // SAFETY: as above, with the value the request carried.
            unsafe { msg_send![super(self), setAccessibilityValue: value] }
        }
    }
);

impl SecretField {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: a secure text field's own initializer, with a plain frame.
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

define_class!(
    // SAFETY: a button may be subclassed; the class has no `Drop`, and its
    // press hands the request on to the superclass's own.
    #[unsafe(super(NSButton, NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ExampleDeskSaveButton"]
    struct SaveButton;

    impl SaveButton {
        /// Save's action: a write.
        #[unsafe(method(save:))]
        fn save(&self, _sender: Option<&AnyObject>) {
            WRITES.fetch_add(1, Ordering::SeqCst);
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_perform_press(&self) -> bool {
            count("button:save.AXPress");
            // SAFETY: the superclass's own press.
            unsafe { msg_send![super(self), accessibilityPerformPress] }
        }
    }
);

impl SaveButton {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: a button's own initializer, with a plain frame.
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

define_class!(
    // SAFETY: a plain object may be subclassed; the class has no `Drop`;
    // its methods are the table's source and delegate, on the main thread.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ExampleDeskLines"]
    #[ivars = Vec<[String; 3]>]
    struct Lines;

    unsafe impl NSObjectProtocol for Lines {}

    unsafe impl NSControlTextEditingDelegate for Lines {}

    unsafe impl NSTableViewDataSource for Lines {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _table: &NSTableView) -> NSInteger {
            NSInteger::try_from(self.ivars().len()).unwrap_or(0)
        }
    }

    unsafe impl NSTableViewDelegate for Lines {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn view_for(
            &self,
            _table: &NSTableView,
            column: Option<&NSTableColumn>,
            row: NSInteger,
        ) -> Option<Retained<NSView>> {
            let at = column?.identifier().to_string().parse::<usize>().ok()?;
            let line = self.ivars().get(usize::try_from(row).ok()?)?;
            let cell = NSTextField::labelWithString(&NSString::from_str(line.get(at)?), self.mtm());
            Some(Retained::into_super(Retained::into_super(cell)))
        }
    }
);

impl Lines {
    fn new(mtm: MainThreadMarker, order: &Order) -> Retained<Self> {
        let lines = order
            .lines
            .iter()
            .map(|line| {
                [
                    line.code.clone(),
                    line.description.clone(),
                    line.quantity.clone(),
                ]
            })
            .collect();
        let this = Self::alloc(mtm).set_ivars(lines);
        // SAFETY: the plain object's own initializer.
        unsafe { msg_send![super(this), init] }
    }
}

/// The control interface's answer to `truth`: one line, tab-separated, as
/// the Windows stand writes it.
fn truth_line() -> String {
    let asked: Vec<String> = lock(&ASKED)
        .iter()
        .map(|(member, times)| format!("{member}={times}"))
        .collect();
    format!(
        "{PREFIX}truth\t{}\t{}\t{}\t{}",
        WRITES.load(Ordering::SeqCst),
        SECRET_READS.load(Ordering::SeqCst),
        lock(&SHOWN_LATE),
        asked.join(";")
    )
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

/// Makes the window and its controls, and shows it without making the
/// stand the active application.
#[allow(unsafe_code)] // calls into AppKit, explained below
fn make(mtm: MainThreadMarker, order: &Order) -> Result<(), String> {
    // SAFETY: a window's own initializer, with plain values; the window is
    // kept by the stand until the process ends, and not released when it
    // closes, as the bindings ask of a window owned by Rust.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            rect(40.0, 40.0, 560.0, 640.0),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: as above.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(WINDOW));
    let content = window
        .contentView()
        .ok_or_else(|| "the window has no content view".to_owned())?;
    let top = 600.0;
    // The field nothing labels comes first.
    let unnamed = ValueField::new(mtm, "unnamed", rect(160.0, top, 300.0, 24.0));
    unnamed.setStringValue(&NSString::from_str("a value nothing labels"));
    content.addSubview(&unnamed);
    let rows: [(&str, &'static str, &str); 6] = [
        ("Order number", "order_number", order.number.as_str()),
        ("Customer", "customer", order.customer.as_str()),
        ("Status", "status", order.status.as_str()),
        ("Tracking", "tracking", ""),
        ("Reference", "reference_1", "R-1"),
        ("Reference", "reference_2", "R-2"),
    ];
    for (row, (name, counted, value)) in rows.iter().enumerate() {
        let y = top - 32.0 * (row as f64 + 1.0);
        let label = NSTextField::labelWithString(&NSString::from_str(name), mtm);
        label.setFrame(rect(10.0, y, 140.0, 24.0));
        content.addSubview(&label);
        let field = ValueField::new(mtm, counted, rect(160.0, y, 300.0, 24.0));
        field.setStringValue(&NSString::from_str(value));
        let labelling: &AnyObject = &label;
        // SAFETY: the label is a view of the same window, kept by it for as
        // long as the field is; the property is weak.
        unsafe { field.setAccessibilityTitleUIElement(Some(labelling)) };
        content.addSubview(&field);
        if *counted == "tracking" {
            TRACKING.with(|kept| *kept.borrow_mut() = Some(field.clone()));
        }
    }
    let y = top - 32.0 * 7.0;
    let label = NSTextField::labelWithString(&NSString::from_str("Access code"), mtm);
    label.setFrame(rect(10.0, y, 140.0, 24.0));
    content.addSubview(&label);
    let secret = SecretField::new(mtm, rect(160.0, y, 300.0, 24.0));
    secret.setStringValue(&NSString::from_str(SECRET));
    let labelling: &AnyObject = &label;
    // SAFETY: as for the fields above.
    unsafe { secret.setAccessibilityTitleUIElement(Some(labelling)) };
    content.addSubview(&secret);
    let y = top - 32.0 * 8.0;
    let label = NSTextField::labelWithString(&NSString::from_str("Line items"), mtm);
    label.setFrame(rect(10.0, y, 140.0, 24.0));
    content.addSubview(&label);
    let table = NSTableView::initWithFrame(NSTableView::alloc(mtm), rect(0.0, 0.0, 520.0, 140.0));
    for (at, heading) in ["Item code", "Description", "Quantity"].iter().enumerate() {
        let column = NSTableColumn::initWithIdentifier(
            NSTableColumn::alloc(mtm),
            &NSString::from_str(&at.to_string()),
        );
        column.setTitle(&NSString::from_str(heading));
        column.setWidth(160.0);
        table.addTableColumn(&column);
    }
    let lines = Lines::new(mtm, order);
    // SAFETY: the source and the delegate are kept by the stand for as
    // long as the table lives; the table holds them weakly.
    unsafe {
        table.setDataSource(Some(ProtocolObject::from_ref(&*lines)));
        table.setDelegate(Some(ProtocolObject::from_ref(&*lines)));
    }
    table.setAccessibilityLabel(Some(&NSString::from_str("Line items")));
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        rect(10.0, y - 150.0, 520.0, 140.0),
    );
    scroll.setDocumentView(Some(&table));
    scroll.setHasVerticalScroller(true);
    content.addSubview(&scroll);
    table.reloadData();
    let save = SaveButton::new(mtm, rect(10.0, 20.0, 100.0, 30.0));
    save.setTitle(&NSString::from_str("Save"));
    let target: &AnyObject = &save;
    // SAFETY: the button is its own target, kept by the window; `save:` is
    // a method the class defines, which takes the sender.
    unsafe {
        save.setTarget(Some(target));
        save.setAction(Some(sel!(save:)));
    }
    content.addSubview(&save);
    window.orderFrontRegardless();
    WINDOW_KEPT.with(|kept| *kept.borrow_mut() = Some(window));
    LINES_KEPT.with(|kept| *kept.borrow_mut() = Some(lines));
    Ok(())
}

/// The main queue of the process, and the system's call that runs a
/// function on it after a time; from the system's C library.
#[repr(C)]
struct Queue {
    _private: [u8; 0],
}

unsafe extern "C" {
    static _dispatch_main_q: Queue;
    fn dispatch_time(when: u64, delta: i64) -> u64;
    fn dispatch_after_f(
        when: u64,
        queue: *const Queue,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
}

/// On the main thread: the tracking field shows the late value, and the
/// application says so, as AppKit's own editing would.
#[allow(unsafe_code)] // one call into AppKit, explained below
extern "C" fn show_late(_context: *mut c_void) {
    if MainThreadMarker::new().is_none() {
        return;
    }
    TRACKING.with(|kept| {
        if let Some(field) = kept.borrow().as_ref() {
            field.setStringValue(&NSString::from_str(LATE));
            let element: &AnyObject = field;
            // SAFETY: the field is a live view of the stand's window, on
            // the main thread; the name is AppKit's own constant.
            unsafe {
                NSAccessibilityPostNotification(element, NSAccessibilityValueChangedNotification);
            }
            LATE.clone_into(&mut lock(&SHOWN_LATE));
        }
    });
}

/// Has the main thread show the late value once `after` milliseconds have
/// passed, from the control interface's thread.
#[allow(unsafe_code)] // two calls into the system, explained below
fn late(after: u64) {
    let nanoseconds = i64::try_from(after.saturating_mul(1_000_000)).unwrap_or(i64::MAX);
    // SAFETY: plain values; 0 is "now". The main queue is the system's own
    // object, which lives as long as the process, and the function takes
    // no context.
    unsafe {
        let when = dispatch_time(0, nanoseconds);
        dispatch_after_f(
            when,
            &raw const _dispatch_main_q,
            std::ptr::null_mut(),
            show_late,
        );
    }
}

/// The control interface: one command a line on standard input, answered
/// on standard output. The end of the input, as when the test is gone, or
/// `quit`, ends the process.
fn control_interface() {
    let input = std::io::stdin();
    for line in input.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line == "truth" {
            println!("{}", truth_line());
        } else if let Some(after) = line.strip_prefix("late ") {
            late(after.parse::<u64>().unwrap_or(0));
            println!("{PREFIX}late");
        } else if line == "quit" {
            break;
        }
    }
    std::process::exit(0);
}

/// Runs the stand on the main thread: the window, the control interface,
/// and the application's own event loop, until the process ends. Returns
/// the process's exit code when it could not begin.
pub(crate) fn serve() -> i32 {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("example-desk: the stand must be served on the main thread");
        return 2;
    };
    let order = std::env::var(ORDER_ENV)
        .ok()
        .and_then(|text| decode_order(&text))
        .unwrap_or_else(Order::example);
    let app = NSApplication::sharedApplication(mtm);
    let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    if let Err(why) = make(mtm, &order) {
        eprintln!("example-desk: {why}");
        return 2;
    }
    std::thread::spawn(control_interface);
    println!("{PREFIX}ready\t{}\t0\t0", std::process::id());
    app.run();
    0
}
