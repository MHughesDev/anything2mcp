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
//!   button "Save"                              (a press is counted as a write; it stores what the edit fields hold when the application takes it)
//!   label "New customer"  text field ""        (holds at most CUSTOMER_MOST characters, and cuts the rest)
//!   label "New status"    text field ""
//!   label "Message"       text field, not editable ("Saved." once Save stored)
//!   label "Problem"       text field, not editable, hidden but while the application refuses a value
//! ```
//!
//! Every value field, the ones that are not editable too, says it is a
//! text field (`AXTextField`), as the read-only edits of the Windows stand
//! and the read-only entries of the Linux stand say they are entries: left
//! to AppKit, a text field that is not editable says it is a static text
//! (decision 0999).
//!
//! A value a client sets into a field (`setAccessibilityValue:`) is kept in
//! the book with its window and its label, shown by the field, and not
//! stored until Save; the fields labelled `Customer` and `Status` show what
//! is stored. A field that is not editable, or not enabled, takes nothing
//! set into it and says its value may not be set (decision 0399).
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

// AppKit is reached only through calls its bindings mark unsafe and through
// the classes `define_class!` makes, which are unsafe by construction; each
// block in this module says why it is sound.
#![allow(unsafe_code)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::io::BufRead;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityLayoutChangedNotification, NSAccessibilityPostNotification,
    NSAccessibilityTextFieldRole, NSAccessibilityUnignoredDescendant,
    NSAccessibilityValueChangedNotification, NSApplication, NSApplicationActivationPolicy,
    NSBackingStoreType, NSButton, NSControl, NSControlTextEditingDelegate, NSResponder,
    NSScrollView, NSSecureTextField, NSTableColumn, NSTableView, NSTableViewDataSource,
    NSTableViewDelegate, NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSInteger, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::child::{FieldFault, ORDER_ENV, PREFIX, Pressed, arm, book, decode_order};
use crate::{CUSTOMER_MOST, DIALOG, FieldSet, LATE, Order, SAVED, SECRET, WINDOW};

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
    /// The value fields, by the name their requests are counted under.
    static FIELDS: RefCell<BTreeMap<&'static str, Retained<ValueField>>> =
        const { RefCell::new(BTreeMap::new()) };
    /// The dialog a fault raised, kept while the process lives.
    static DIALOG_KEPT: RefCell<Option<Retained<NSWindow>>> = const { RefCell::new(None) };
}

/// The field fault the main thread is to carry out next, and whether it
/// found the field.
static FIELD_FAULT: Mutex<Option<FieldFault>> = Mutex::new(None);
static FAULT_FOUND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Counts one request a client made of a control, as `control.request`.
fn count(member: &str) {
    *lock(&ASKED).entry(member.to_owned()).or_insert(0) += 1;
}

/// The element AppKit's accessibility gives for a label: the label's view
/// when that is an element, else the element under it that stands for it,
/// as AppKit lists a view's children.
fn as_listed(label: &NSTextField) -> Option<Retained<AnyObject>> {
    let label: &AnyObject = label;
    // SAFETY: the label is a live view of the stand's window, on the main
    // thread, where every request of AppKit's accessibility is answered.
    unsafe { NSAccessibilityUnignoredDescendant(label) }
}

/// What a value field keeps: the name its requests are counted under, and
/// the label that labels it, when one does.
struct Field {
    counted: &'static str,
    /// The label's text, which a set is kept under.
    label: &'static str,
    title: Option<Retained<NSTextField>>,
}

define_class!(
    // SAFETY: a text field may be subclassed; the class has no `Drop`, and
    // each method hands the request on to the superclass's own with the
    // arguments it was given, after counting it, except the title element,
    // which the field answers itself with the label it keeps.
    #[unsafe(super(NSTextField, NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ExampleDeskValueField"]
    #[ivars = Field]
    struct ValueField;

    impl ValueField {
        /// The field's role: a text field, whether it is editable or not.
        /// AppKit's own answer for a text field that is not editable is a
        /// static text's, which would make `Message` and `Problem`, and a
        /// field the read-only fault locks, a label here where the Linux
        /// and Windows stands show a read-only entry, and the one screen
        /// model the three share could find none of them (decision 0999).
        /// Whether a client may set its value is still answered below.
        #[unsafe(method_id(accessibilityRole))]
        fn accessibility_role(&self) -> Option<Retained<NSString>> {
            // SAFETY: AppKit's own constant, which lives as long as the
            // process and is never written.
            Some(unsafe { NSAccessibilityTextFieldRole }.retain())
        }

        #[unsafe(method_id(accessibilityValue))]
        fn accessibility_value(&self) -> Option<Retained<AnyObject>> {
            count(&format!("edit:{}.AXValue", self.ivars().counted));
            // SAFETY: the superclass's own answer to the same request.
            let answer = unsafe { msg_send![super(self), accessibilityValue] };
            if self.ivars().counted == "order_number" {
                after_key_read(self.mtm());
            }
            answer
        }

        /// A client's set of the field's value: kept in the book, refused
        /// by a field that is not editable or not enabled, cut to
        /// `CUSTOMER_MOST` in `New customer`, and shown as AppKit's own
        /// editing would, with the notice that the value changed.
        #[unsafe(method(setAccessibilityValue:))]
        fn set_accessibility_value(&self, value: Option<&AnyObject>) {
            count(&format!("edit:{}.AXSetValue", self.ivars().counted));
            if self.isEditable() && self.isEnabled() {
                set_value(self, value);
            }
        }

        /// Whether a client may make the request: a set of the value only
        /// while the field is editable and enabled.
        #[unsafe(method(isAccessibilitySelectorAllowed:))]
        fn is_accessibility_selector_allowed(&self, selector: Sel) -> bool {
            if selector == sel!(setAccessibilityValue:) {
                self.isEditable() && self.isEnabled()
            } else {
                // SAFETY: the superclass's own answer to the same request.
                unsafe { msg_send![super(self), isAccessibilitySelectorAllowed: selector] }
            }
        }

        #[unsafe(method(setAccessibilityFocused:))]
        fn set_accessibility_focused(&self, focused: bool) {
            count(&format!("edit:{}.AXSetFocused", self.ivars().counted));
            // SAFETY: as above.
            unsafe { msg_send![super(self), setAccessibilityFocused: focused] }
        }

        /// The label that labels the field, as a screen reader asks for it
        /// (`AXTitleUIElement`): the element AppKit's accessibility gives
        /// for the label, which is the one it lists among the window's
        /// children. The runs on macOS showed that a text field given its
        /// label through `setAccessibilityTitleUIElement:` alone answers the
        /// request with nothing, and that one answering with the label's
        /// view itself names an element that is not the listed static text.
        #[unsafe(method_id(accessibilityTitleUIElement))]
        fn accessibility_title_ui_element(&self) -> Option<Retained<AnyObject>> {
            self.ivars().title.as_ref().and_then(|label| as_listed(label))
        }
    }
);

/// A client's value, set into a field that takes it: kept in the book,
/// cut to [`CUSTOMER_MOST`] in `New customer`, and shown.
fn set_value(field: &ValueField, value: Option<&AnyObject>) {
    let text = value
        .and_then(|value| value.downcast_ref::<NSString>())
        .map(ToString::to_string)
        .unwrap_or_default();
    book().sets.push(FieldSet {
        window: WINDOW.to_owned(),
        field: field.ivars().label.to_owned(),
        value: text.clone(),
    });
    let held: String = if field.ivars().counted == "new_customer" {
        text.chars().take(CUSTOMER_MOST).collect()
    } else {
        text
    };
    show(field, &held);
    if ["new_customer", "new_status"].contains(&field.ivars().counted) {
        clear_said();
    }
}

impl ValueField {
    fn new(
        mtm: MainThreadMarker,
        counted: &'static str,
        title: Option<(&'static str, &Retained<NSTextField>)>,
        frame: NSRect,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Field {
            counted,
            label: title.map_or("", |(label, _)| label),
            title: title.map(|(_, label)| label.clone()),
        });
        // SAFETY: a text field's own initializer, with a plain frame.
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

define_class!(
    // SAFETY: a secure text field may be subclassed; the class has no
    // `Drop`, and each method hands the request on to the superclass's own,
    // except the title element, which the field answers with its label.
    #[unsafe(super(NSSecureTextField, NSTextField, NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ExampleDeskSecretField"]
    #[ivars = Retained<NSTextField>]
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

        /// The label that labels the field, as for a value field.
        #[unsafe(method_id(accessibilityTitleUIElement))]
        fn accessibility_title_ui_element(&self) -> Option<Retained<AnyObject>> {
            as_listed(self.ivars())
        }
    }
);

impl SecretField {
    fn new(mtm: MainThreadMarker, title: &Retained<NSTextField>, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(title.clone());
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
        /// Save's action: a write, and the application's rule.
        #[unsafe(method(save:))]
        fn save(&self, _sender: Option<&AnyObject>) {
            WRITES.fetch_add(1, Ordering::SeqCst);
            press();
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
            let text = column
                .and_then(|column| column.identifier().to_string().parse::<usize>().ok())
                .zip(usize::try_from(row).ok())
                .and_then(|(at, row)| self.ivars().get(row).and_then(|line| line.get(at)));
            text.map(|text| {
                let cell = NSTextField::labelWithString(&NSString::from_str(text), self.mtm());
                Retained::into_super(Retained::into_super(cell))
            })
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
        "{PREFIX}truth\t{}\t{}\t{}\t{}\t{}",
        WRITES.load(Ordering::SeqCst),
        SECRET_READS.load(Ordering::SeqCst),
        lock(&SHOWN_LATE),
        asked.join(";"),
        book().line()
    )
}

/// A field shows `text`, and the application says its value changed, as
/// AppKit's own editing would.
fn show(field: &NSTextField, text: &str) {
    field.setStringValue(&NSString::from_str(text));
    let element: &AnyObject = field;
    // SAFETY: the field is a live view of the stand's window, on the main
    // thread; the name is AppKit's own constant.
    unsafe { NSAccessibilityPostNotification(element, NSAccessibilityValueChangedNotification) };
}

/// The value field counted under `counted`, on the main thread.
fn field(counted: &str) -> Option<Retained<ValueField>> {
    FIELDS.with(|fields| fields.borrow().get(counted).cloned())
}

/// The window's layout changed: a field was shown or hidden.
fn laid_out() {
    WINDOW_KEPT.with(|kept| {
        if let Some(window) = kept.borrow().as_ref() {
            let element: &AnyObject = window;
            // SAFETY: the stand's live window, on the main thread; the name
            // is AppKit's own constant.
            unsafe {
                NSAccessibilityPostNotification(element, NSAccessibilityLayoutChangedNotification);
            }
        }
    });
}

/// What the window says, cleared once a field changes: the message, and
/// the problem, hidden.
fn clear_said() {
    if let Some(message) = field("message") {
        show(&message, "");
    }
    book().message.clear();
    if let Some(problem) = field("problem") {
        show(&problem, "");
        if !problem.isHidden() {
            problem.setHidden(true);
            laid_out();
        }
    }
}

/// A press of Save: the application's rule, and what the window shows.
fn press() {
    let text = |counted: &str| field(counted).map(|field| field.stringValue().to_string());
    let customer = text("new_customer").unwrap_or_default();
    let status = text("new_status").unwrap_or_default();
    let pressed = book().press(&customer, &status);
    match pressed {
        Pressed::Silent => {}
        Pressed::Refused(said) => {
            if let Some(problem) = field("problem") {
                problem.setHidden(false);
                show(&problem, said);
                laid_out();
            }
        }
        Pressed::Stored => {
            for (counted, value) in [("customer", &customer), ("status", &status)] {
                if let Some(shown) = field(counted) {
                    show(&shown, value);
                }
            }
            if let Some(message) = field("message") {
                show(&message, SAVED);
            }
            SAVED.clone_into(&mut book().message);
        }
    }
}

/// What the faults armed for the next read of the order number do once
/// it has been answered: the window moves to another record, or the
/// application raises its dialog.
fn after_key_read(mtm: MainThreadMarker) {
    let (record, dialog) = {
        let mut book = book();
        (
            book.record_after_read.take(),
            std::mem::take(&mut book.dialog_after_read),
        )
    };
    if let Some(other) = record
        && let Some(number) = field("order_number")
    {
        show(&number, &other);
    }
    if dialog {
        // SAFETY: a window's own initializer, with plain values; the window
        // is kept by the stand until the process ends, and not released
        // when it closes.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(80.0, 80.0, 300.0, 120.0),
                NSWindowStyleMask::Titled,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: as above.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(DIALOG));
        window.orderFrontRegardless();
        DIALOG_KEPT.with(|kept| *kept.borrow_mut() = Some(window));
    }
}

/// On the main thread: the field fault the control interface left, carried
/// out on the field after the label it names.
extern "C" fn apply_fault(_context: *mut c_void) {
    let fault = lock(&FIELD_FAULT).take();
    let found = fault.is_some_and(|fault| {
        let (FieldFault::ReadOnly(label) | FieldFault::Disabled(label)) = &fault;
        let named = FIELDS.with(|fields| {
            fields
                .borrow()
                .values()
                .find(|field| field.ivars().label == label)
                .cloned()
        });
        named.is_some_and(|named| {
            match fault {
                FieldFault::ReadOnly(_) => named.setEditable(false),
                FieldFault::Disabled(_) => named.setEnabled(false),
            }
            true
        })
    });
    FAULT_FOUND.store(found, Ordering::SeqCst);
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

/// Makes the window and its controls, and shows it without making the
/// stand the active application.
fn make(mtm: MainThreadMarker, order: &Order) -> Result<(), String> {
    // SAFETY: a window's own initializer, with plain values; the window is
    // kept by the stand until the process ends, and not released when it
    // closes, as the bindings ask of a window owned by Rust.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            rect(40.0, 40.0, 560.0, 800.0),
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
    let top = 760.0;
    // The field nothing labels comes first.
    let unnamed = ValueField::new(mtm, "unnamed", None, rect(160.0, top, 300.0, 24.0));
    unnamed.setStringValue(&NSString::from_str("a value nothing labels"));
    content.addSubview(&unnamed);
    let rows: [(&'static str, &'static str, &str); 6] = [
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
        let field = ValueField::new(
            mtm,
            counted,
            Some((*name, &label)),
            rect(160.0, y, 300.0, 24.0),
        );
        field.setStringValue(&NSString::from_str(value));
        let labelling: &AnyObject = &label;
        // SAFETY: the label is a view of the same window, kept by it for as
        // long as the field is; the property is weak.
        unsafe { field.setAccessibilityTitleUIElement(Some(labelling)) };
        content.addSubview(&field);
        if *counted == "tracking" {
            TRACKING.with(|kept| *kept.borrow_mut() = Some(field.clone()));
        }
        FIELDS.with(|fields| fields.borrow_mut().insert(*counted, field.clone()));
    }
    let y = top - 32.0 * 7.0;
    let label = NSTextField::labelWithString(&NSString::from_str("Access code"), mtm);
    label.setFrame(rect(10.0, y, 140.0, 24.0));
    content.addSubview(&label);
    let secret = SecretField::new(mtm, &label, rect(160.0, y, 300.0, 24.0));
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
    let save = SaveButton::new(mtm, rect(10.0, y - 190.0, 100.0, 30.0));
    save.setTitle(&NSString::from_str("Save"));
    let target: &AnyObject = &save;
    // SAFETY: the button is its own target, kept by the window; `save:` is
    // a method the class defines, which takes the sender.
    unsafe {
        save.setTarget(Some(target));
        save.setAction(Some(sel!(save:)));
    }
    content.addSubview(&save);
    // The edit fields, and what the application says, below Save.
    let edits: [(&'static str, &'static str, bool); 4] = [
        ("New customer", "new_customer", true),
        ("New status", "new_status", true),
        ("Message", "message", false),
        ("Problem", "problem", false),
    ];
    for (row, (name, counted, editable)) in edits.iter().enumerate() {
        let y = y - 230.0 - 32.0 * row as f64;
        let label = NSTextField::labelWithString(&NSString::from_str(name), mtm);
        label.setFrame(rect(10.0, y, 140.0, 24.0));
        content.addSubview(&label);
        let field = ValueField::new(
            mtm,
            counted,
            Some((*name, &label)),
            rect(160.0, y, 300.0, 24.0),
        );
        field.setEditable(*editable);
        let labelling: &AnyObject = &label;
        // SAFETY: as for the fields above.
        unsafe { field.setAccessibilityTitleUIElement(Some(labelling)) };
        // The problem is shown only while the application refuses a value.
        field.setHidden(*counted == "problem");
        content.addSubview(&field);
        FIELDS.with(|fields| fields.borrow_mut().insert(*counted, field.clone()));
    }
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
    fn dispatch_sync_f(queue: *const Queue, context: *mut c_void, work: extern "C" fn(*mut c_void));
}

/// Arms a fault, or has the main thread carry out a field fault and waits
/// until it has; the answer the control interface gives.
fn fault(line: &str) -> String {
    match arm(line) {
        Err(()) => "fault\tunknown".to_owned(),
        Ok(None) => "fault".to_owned(),
        Ok(Some(field)) => {
            *lock(&FIELD_FAULT) = Some(field);
            FAULT_FOUND.store(false, Ordering::SeqCst);
            // SAFETY: the main queue is the system's own object, which lives
            // as long as the process; the function takes no context. This
            // thread is not the main thread, so the wait cannot block on
            // itself.
            unsafe {
                dispatch_sync_f(
                    &raw const _dispatch_main_q,
                    std::ptr::null_mut(),
                    apply_fault,
                )
            };
            if FAULT_FOUND.load(Ordering::SeqCst) {
                "fault".to_owned()
            } else {
                "fault\tno field has that label".to_owned()
            }
        }
    }
}

/// On the main thread: the tracking field shows the late value, and the
/// application says so, as AppKit's own editing would.
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
        } else if line.starts_with("fault ") {
            println!("{PREFIX}{}", fault(line));
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
    book().open(&order);
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
