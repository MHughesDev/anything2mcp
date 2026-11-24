//! The stand's window, in the stand's own process: plain Win32 controls,
//! which UI Automation reads through the proxies the system ships for its
//! standard controls, with no toolkit and no provider of the stand's own.
//!
//! ```text
//! frame "Example order"                       (a top-level window, shown without taking focus)
//!   edit, first of all, with nothing before it to label it: "a value nothing labels"
//!   static "Order number"  edit "A-100"       (a static just before an edit names it)
//!   static "Customer"      edit "Example Supplies"
//!   static "Status"        edit "open"
//!   static "Tracking"      edit ""            (shows LATE when the control interface says so)
//!   static "Reference"     edit "R-1"
//!   static "Reference"     edit "R-2"
//!   static "Access code"   edit, password style, holding SECRET
//!   static "Line items"    list view "Line items", report style: three columns, one row a line
//!   button "Save"                             (a click is counted as a write; it stores what the edit fields hold when the application takes it)
//!   static "New customer"  edit ""            (holds at most CUSTOMER_MOST characters, and cuts the rest)
//!   static "New status"    edit ""
//!   static "Message"       edit, read-only    ("Saved." once Save stored)
//!   static "Problem"       edit, read-only    (shown only while the application refuses a value)
//! ```
//!
//! What a client sets into an edit (`WM_SETTEXT`, which UI Automation's
//! value pattern sends) is kept in the book with its window and its label,
//! and is not stored until Save; the edits labelled `Customer` and `Status`
//! show what is stored. A read-only or disabled edit takes nothing set
//! into it, whatever a client sends (decision 0399).
//!
//! What the stand witnesses: every control is subclassed, so every message
//! another process sends it passes the stand's own procedure first. It
//! counts, by control and by name, the messages through which a client
//! reads or changes a standard control (the list is [`KNOWN`]); a click of
//! Save, as the button's notice to its window; and every message that asks
//! the password edit for its text or its length. What it cannot witness:
//! a call that a UI Automation client answers without a message to the
//! control (the proxies run in the client), and anything the client does
//! with the answers. That the client never asks for the password's text
//! is proven by the desktop pack's own list of the members it may call;
//! the stand's count is the second witness. The system is a third: it
//! refuses another process's `WM_GETTEXT` to the password edit (`Access is
//! denied`) before the edit sees it, as the runner showed and the stand's
//! own test asserts.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::io::BufRead;
use std::sync::Mutex;
use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, HBRUSH};
use windows::Win32::System::Diagnostics::Debug::{
    SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SetErrorMode,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    EM_GETLINE, EM_GETLINECOUNT, EM_GETSEL, EM_REPLACESEL, EM_SETREADONLY, EM_SETSEL, EM_UNDO,
    ICC_LISTVIEW_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, LVCF_SUBITEM, LVCF_TEXT,
    LVCF_WIDTH, LVCOLUMNW, LVIF_TEXT, LVITEMW, LVM_ENSUREVISIBLE, LVM_GETCOLUMNW, LVM_GETHEADER,
    LVM_GETITEMCOUNT, LVM_GETITEMRECT, LVM_GETITEMSTATE, LVM_GETITEMTEXTW, LVM_GETITEMW,
    LVM_GETSUBITEMRECT, LVM_INSERTCOLUMNW, LVM_INSERTITEMW, LVM_SCROLL, LVM_SETITEMSTATE,
    LVM_SETITEMTEXTW, LVS_REPORT, WC_LISTVIEWW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BM_CLICK, BM_SETCHECK, BM_SETSTATE, BN_CLICKED, BS_PUSHBUTTON, CallWindowProcW,
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL, ES_PASSWORD,
    ES_READONLY, GWL_STYLE, GWLP_USERDATA, GWLP_WNDPROC, GetDlgCtrlID, GetMessageW,
    GetWindowLongPtrW, GetWindowTextW, HMENU, KillTimer, MSG, PostMessageW, PostQuitMessage,
    RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SendMessageW, SetTimer, SetWindowLongPtrW,
    SetWindowTextW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CHAR,
    WM_CLEAR, WM_CLOSE, WM_COMMAND, WM_CUT, WM_DESTROY, WM_GETOBJECT, WM_GETTEXT, WM_GETTEXTLENGTH,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_PASTE, WM_SETFOCUS, WM_SETTEXT, WM_TIMER, WNDCLASSW, WNDPROC,
    WS_BORDER, WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN, WS_DISABLED, WS_EX_CLIENTEDGE, WS_OVERLAPPED,
    WS_SYSMENU, WS_VISIBLE,
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use crate::child::{FieldFault, ORDER_ENV, PREFIX, Pressed, arm, book, decode_order};
use crate::{CUSTOMER_MOST, DIALOG, FieldSet, LATE, Order, SAVED, SECRET, WINDOW};

/// What a control of the stand is, for the names of its counts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// An edit that shows a value.
    Value,
    /// The password edit.
    Password,
    /// A static that labels the control after it.
    Label,
    /// The list view.
    Table,
    /// Save.
    Button,
}

/// Every control: its identifier, its kind, and the name its counts carry.
const CONTROLS: &[(i32, Kind, &str)] = &[
    (100, Kind::Value, "unnamed"),
    (101, Kind::Label, "order_number"),
    (102, Kind::Value, "order_number"),
    (103, Kind::Label, "customer"),
    (104, Kind::Value, "customer"),
    (105, Kind::Label, "status"),
    (106, Kind::Value, "status"),
    (107, Kind::Label, "tracking"),
    (108, Kind::Value, "tracking"),
    (109, Kind::Label, "reference_1"),
    (110, Kind::Value, "reference_1"),
    (111, Kind::Label, "reference_2"),
    (112, Kind::Value, "reference_2"),
    (113, Kind::Label, "access_code"),
    (114, Kind::Password, "access_code"),
    (115, Kind::Label, "line_items"),
    (116, Kind::Table, "line_items"),
    (117, Kind::Button, "save"),
    (118, Kind::Label, "new_customer"),
    (119, Kind::Value, "new_customer"),
    (120, Kind::Label, "new_status"),
    (121, Kind::Value, "new_status"),
    (122, Kind::Label, "message"),
    (123, Kind::Value, "message"),
    (124, Kind::Label, "problem"),
    (125, Kind::Value, "problem"),
];

/// The label of each edit, by its identifier: the name a set is kept
/// under. Each label's identifier is its edit's less one.
const LABELS: &[(i32, &str)] = &[
    (101, "Order number"),
    (103, "Customer"),
    (105, "Status"),
    (107, "Tracking"),
    (109, "Reference"),
    (111, "Reference"),
    (113, "Access code"),
    (118, "New customer"),
    (120, "New status"),
    (122, "Message"),
    (124, "Problem"),
];

const ORDER_NUMBER: i32 = 102;
const CUSTOMER: i32 = 104;
const STATUS: i32 = 106;
const NEW_CUSTOMER: i32 = 119;
const NEW_STATUS: i32 = 121;
const MESSAGE: i32 = 123;
const PROBLEM: i32 = 125;

const TRACKING: i32 = 108;
const PASSWORD: i32 = 114;
const SAVE: i32 = 117;

/// The stand's own message: show the late value after `wparam`
/// milliseconds.
const LATE_MESSAGE: u32 = WM_APP + 1;
const LATE_TIMER: usize = 1;
/// The stand's own message: carry out the field fault the control
/// interface left in [`FIELD_FAULT`], on the window's thread.
const FAULT_MESSAGE: u32 = WM_APP + 2;

/// The messages the stand counts, with whether each changes a control.
/// Every other message passes uncounted: painting, sizing, and the
/// system's own traffic are not a client asking.
pub(crate) const KNOWN: &[(u32, &str, bool)] = &[
    (WM_GETTEXT, "WM_GETTEXT", false),
    (WM_GETTEXTLENGTH, "WM_GETTEXTLENGTH", false),
    (WM_GETOBJECT, "WM_GETOBJECT", false),
    (EM_GETLINE, "EM_GETLINE", false),
    (EM_GETLINECOUNT, "EM_GETLINECOUNT", false),
    (EM_GETSEL, "EM_GETSEL", false),
    (LVM_GETITEMCOUNT, "LVM_GETITEMCOUNT", false),
    (LVM_GETITEMW, "LVM_GETITEMW", false),
    (LVM_GETITEMTEXTW, "LVM_GETITEMTEXTW", false),
    (LVM_GETHEADER, "LVM_GETHEADER", false),
    (LVM_GETCOLUMNW, "LVM_GETCOLUMNW", false),
    (LVM_GETITEMRECT, "LVM_GETITEMRECT", false),
    (LVM_GETSUBITEMRECT, "LVM_GETSUBITEMRECT", false),
    (LVM_GETITEMSTATE, "LVM_GETITEMSTATE", false),
    (WM_SETTEXT, "WM_SETTEXT", true),
    (EM_REPLACESEL, "EM_REPLACESEL", true),
    (EM_SETSEL, "EM_SETSEL", true),
    (EM_UNDO, "EM_UNDO", true),
    (WM_CHAR, "WM_CHAR", true),
    (WM_KEYDOWN, "WM_KEYDOWN", true),
    (WM_LBUTTONDOWN, "WM_LBUTTONDOWN", true),
    (WM_SETFOCUS, "WM_SETFOCUS", true),
    (WM_PASTE, "WM_PASTE", true),
    (WM_CUT, "WM_CUT", true),
    (WM_CLEAR, "WM_CLEAR", true),
    (BM_CLICK, "BM_CLICK", true),
    (BM_SETCHECK, "BM_SETCHECK", true),
    (BM_SETSTATE, "BM_SETSTATE", true),
    (LVM_SETITEMSTATE, "LVM_SETITEMSTATE", true),
    (LVM_SETITEMTEXTW, "LVM_SETITEMTEXTW", true),
    (LVM_ENSUREVISIBLE, "LVM_ENSUREVISIBLE", true),
    (LVM_SCROLL, "LVM_SCROLL", true),
];

static WRITES: AtomicU64 = AtomicU64::new(0);
static SECRET_READS: AtomicU64 = AtomicU64::new(0);
static ASKED: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
static SHOWN_LATE: Mutex<String> = Mutex::new(String::new());
/// The window and the tracking edit, as numbers: a window handle is not
/// `Send`, and the control interface's thread posts to the window.
static MAIN: AtomicIsize = AtomicIsize::new(0);
static TRACKING_EDIT: AtomicIsize = AtomicIsize::new(0);
/// The field fault the window's thread is to carry out next, and whether it
/// found the field.
static FIELD_FAULT: Mutex<Option<FieldFault>> = Mutex::new(None);

thread_local! {
    /// Set while the stand sends a message to its own control, so that it
    /// is not counted as a client's.
    static OWN: Cell<bool> = const { Cell::new(false) };
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Counts one message to a control, when it is one the stand knows.
fn count(id: i32, message: u32) {
    let Some((_, name, _)) = KNOWN.iter().find(|(known, _, _)| *known == message) else {
        return;
    };
    let Some((_, kind, control)) = CONTROLS.iter().find(|(known, _, _)| *known == id) else {
        return;
    };
    let prefix = match kind {
        Kind::Value | Kind::Password => "edit",
        Kind::Label => "label",
        Kind::Table => "table",
        Kind::Button => "button",
    };
    *lock(&ASKED)
        .entry(format!("{prefix}:{control}.{name}"))
        .or_insert(0) += 1;
    if *kind == Kind::Password && [WM_GETTEXT, WM_GETTEXTLENGTH, EM_GETLINE].contains(&message) {
        SECRET_READS.fetch_add(1, Ordering::SeqCst);
    }
}

/// The control interface's answer to `truth`: one line, tab-separated.
pub(crate) fn truth_line() -> String {
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

/// The procedure every control runs first: it counts, then hands the
/// message to the control's own procedure unchanged.
#[allow(unsafe_code)] // calls into the system and one conversion, explained below
extern "system" fn watched(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let own = OWN.with(Cell::get);
    // SAFETY: `hwnd` is the control the system is calling this procedure
    // for; asking its identifier touches no memory of ours.
    let id = unsafe { GetDlgCtrlID(hwnd) };
    if !own {
        count(id, message);
    }
    if !own && message == WM_SETTEXT {
        return client_set(hwnd, id, lparam);
    }
    let answer = own_procedure(hwnd, message, wparam, lparam);
    if !own && message == WM_GETTEXT && id == ORDER_NUMBER {
        after_key_read();
    }
    answer
}

/// Hands a message on to the control's own procedure unchanged.
#[allow(unsafe_code)] // two calls into the system and one conversion, explained below
fn own_procedure(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: the stand put the control's own procedure in the control's
    // user data before it put this one in its place, and changes neither
    // afterwards. A window procedure is a pointer the size of `isize`, and
    // `WNDPROC` is that pointer or nothing, so the conversion gives back
    // exactly what was stored. Handing the call on with the arguments the
    // system gave is what a subclass must do.
    unsafe {
        let own = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
        let own: WNDPROC = std::mem::transmute::<isize, WNDPROC>(own);
        CallWindowProcW(own, hwnd, message, wparam, lparam)
    }
}

/// The text of one of the stand's own controls, read by the stand.
#[allow(unsafe_code)] // one call into the system, explained below
fn text_of(control: HWND) -> String {
    let mut buffer = vec![0_u16; 1024];
    OWN.with(|own| own.set(true));
    // SAFETY: `control` is a control of the stand's window, on this thread;
    // the buffer is ours, and the call writes at most its length.
    let length = unsafe { GetWindowTextW(control, &mut buffer) };
    OWN.with(|own| own.set(false));
    String::from_utf16_lossy(&buffer[..usize::try_from(length).unwrap_or(0)])
}

/// Sets the text of one of the stand's own controls, as the stand.
#[allow(unsafe_code)] // one call into the system, explained below
fn show(control: HWND, text: &str) {
    OWN.with(|own| own.set(true));
    // SAFETY: `control` is a control of the stand's window, on this thread;
    // the text outlives the call.
    let _ = unsafe { SetWindowTextW(control, &HSTRING::from(text)) };
    OWN.with(|own| own.set(false));
}

/// The control of the stand's window with the identifier `id`.
#[allow(unsafe_code)] // one call into the system, explained below
fn control_of(id: i32) -> HWND {
    let main = HWND(MAIN.load(Ordering::SeqCst) as *mut _);
    // SAFETY: the window is the stand's; the call reads its child list.
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetDlgItem(Some(main), id) }
        .unwrap_or_default()
}

/// What the window says, cleared once a field changes: the message, and
/// the problem, hidden.
#[allow(unsafe_code)] // one call into the system, explained below
fn clear_said() {
    show(control_of(MESSAGE), "");
    book().message.clear();
    let problem = control_of(PROBLEM);
    show(problem, "");
    // SAFETY: a control of the stand's window, on this thread.
    let _ = unsafe { ShowWindow(problem, SW_HIDE) };
}

/// A client's `WM_SETTEXT` to a control: kept in the book, refused by a
/// read-only or disabled edit, cut to [`CUSTOMER_MOST`] in `New customer`,
/// and handed on.
#[allow(unsafe_code)] // calls into the system and a read of the client's text, explained below
fn client_set(hwnd: HWND, id: i32, lparam: LPARAM) -> LRESULT {
    let Some((_, kind, _)) = CONTROLS.iter().find(|(known, _, _)| *known == id) else {
        return own_procedure(hwnd, WM_SETTEXT, WPARAM(0), lparam);
    };
    if ![Kind::Value, Kind::Password].contains(kind) {
        return own_procedure(hwnd, WM_SETTEXT, WPARAM(0), lparam);
    }
    // SAFETY: the control is the stand's; the call reads its style.
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
    let style = u32::try_from(style & 0xffff_ffff).unwrap_or(0);
    if style & (ES_READONLY as u32) != 0 || style & WS_DISABLED.0 != 0 {
        // A field that takes no value takes none from a client.
        return LRESULT(0);
    }
    let pointer = lparam.0 as *const u16;
    let text = if pointer.is_null() {
        String::new()
    } else {
        // SAFETY: the system hands a `WM_SETTEXT` from another process to
        // this one with its text copied into this process, ended by a zero,
        // for the length of the call.
        unsafe { PCWSTR(pointer).to_string() }.unwrap_or_default()
    };
    let field = LABELS
        .iter()
        .find(|(label, _)| *label == id - 1)
        .map_or("", |(_, name)| *name);
    book().sets.push(FieldSet {
        window: WINDOW.to_owned(),
        field: field.to_owned(),
        value: if *kind == Kind::Password {
            String::new()
        } else {
            text.clone()
        },
    });
    let held: String = if id == NEW_CUSTOMER {
        text.chars().take(CUSTOMER_MOST).collect()
    } else {
        text
    };
    // The text handed on, ended by a zero, in a buffer of the stand's own
    // that outlives the call.
    let held: Vec<u16> = held.encode_utf16().chain([0]).collect();
    let answer = own_procedure(hwnd, WM_SETTEXT, WPARAM(0), LPARAM(held.as_ptr() as isize));
    if [NEW_CUSTOMER, NEW_STATUS].contains(&id) {
        clear_said();
    }
    answer
}

/// What the faults armed for the next read of the order number do once
/// it has been answered: the window moves to another record, or the
/// application raises its dialog.
#[allow(unsafe_code)] // calls into the system, explained below
fn after_key_read() {
    let (record, dialog) = {
        let mut book = book();
        (
            book.record_after_read.take(),
            std::mem::take(&mut book.dialog_after_read),
        )
    };
    if let Some(other) = record {
        show(control_of(ORDER_NUMBER), &other);
    }
    if dialog {
        // SAFETY: no argument: the module of this program.
        let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
            return;
        };
        // SAFETY: the class was registered when the window was made; the
        // title outlives the call. A top-level window of the stand's
        // process, shown without taking the focus.
        if let Ok(dialog) = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("example-desk-dialog"),
                &HSTRING::from(DIALOG),
                WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0),
                80,
                80,
                300,
                120,
                None,
                None,
                Some(HINSTANCE(module.0)),
                None,
            )
        } {
            // SAFETY: the window just made, on this thread.
            let _ = unsafe { ShowWindow(dialog, SW_SHOWNOACTIVATE) };
        }
    }
}

/// A press of Save: the application's rule, and what the window shows.
#[allow(unsafe_code)] // one call into the system, explained below
fn press() {
    let customer = text_of(control_of(NEW_CUSTOMER));
    let status = text_of(control_of(NEW_STATUS));
    let pressed = book().press(&customer, &status);
    match pressed {
        Pressed::Silent => {}
        Pressed::Refused(said) => {
            let problem = control_of(PROBLEM);
            show(problem, said);
            // SAFETY: a control of the stand's window, on this thread.
            let _ = unsafe { ShowWindow(problem, SW_SHOWNOACTIVATE) };
        }
        Pressed::Stored => {
            show(control_of(CUSTOMER), &customer);
            show(control_of(STATUS), &status);
            show(control_of(MESSAGE), SAVED);
            SAVED.clone_into(&mut book().message);
        }
    }
}

/// Carries out a field fault on the window's thread: the edit after the
/// label named becomes read-only, or disabled. `false` when no label has
/// the name.
#[allow(unsafe_code)] // calls into the system, explained below
fn field_fault(fault: &FieldFault) -> bool {
    let (FieldFault::ReadOnly(label) | FieldFault::Disabled(label)) = fault;
    let Some((id, _)) = LABELS.iter().find(|(_, name)| name == label) else {
        return false;
    };
    let edit = control_of(id + 1);
    OWN.with(|own| own.set(true));
    match fault {
        FieldFault::ReadOnly(_) => {
            // SAFETY: an edit of the stand's window, on this thread.
            let _ = unsafe { SendMessageW(edit, EM_SETREADONLY, Some(WPARAM(1)), Some(LPARAM(0))) };
        }
        FieldFault::Disabled(_) => {
            // SAFETY: an edit of the stand's window, on this thread; the
            // style is read and written back with the disabled flag.
            unsafe {
                let style = GetWindowLongPtrW(edit, GWL_STYLE);
                SetWindowLongPtrW(edit, GWL_STYLE, style | WS_DISABLED.0 as isize);
            }
        }
    }
    OWN.with(|own| own.set(false));
    true
}

/// The window's own procedure: Save counts a write; the stand's message
/// starts the late value's timer; closing ends the loop.
#[allow(unsafe_code)] // calls into the system, explained below
extern "system" fn window(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_COMMAND => {
            let code = (wparam.0 >> 16) & 0xffff;
            let id = wparam.0 & 0xffff;
            if code == BN_CLICKED as usize && id == SAVE as usize {
                WRITES.fetch_add(1, Ordering::SeqCst);
                press();
            }
            LRESULT(0)
        }
        FAULT_MESSAGE => {
            let fault = lock(&FIELD_FAULT).take();
            LRESULT(isize::from(fault.as_ref().is_some_and(field_fault)))
        }
        LATE_MESSAGE => {
            let after = u32::try_from(wparam.0).unwrap_or(u32::MAX);
            // SAFETY: `hwnd` is the stand's window, on this thread; a
            // timer with no procedure posts `WM_TIMER` to it.
            unsafe { SetTimer(Some(hwnd), LATE_TIMER, after, None) };
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == LATE_TIMER => {
            // SAFETY: as above; the timer is the stand's own.
            let _ = unsafe { KillTimer(Some(hwnd), LATE_TIMER) };
            let edit = HWND(TRACKING_EDIT.load(Ordering::SeqCst) as *mut _);
            OWN.with(|own| own.set(true));
            // SAFETY: `edit` is the tracking edit, a child of this window
            // made on this thread; the text outlives the call.
            let shown = unsafe { SetWindowTextW(edit, &HSTRING::from(LATE)) };
            OWN.with(|own| own.set(false));
            if shown.is_ok() {
                LATE.clone_into(&mut lock(&SHOWN_LATE));
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            // SAFETY: the stand's own window, on the thread that made it.
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: ends this thread's message loop; touches no memory.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // SAFETY: every other message goes to the system's default with the
        // arguments the system gave.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// The dialog's procedure: the system's default for every message.
#[allow(unsafe_code)] // one call into the system, explained below
extern "system" fn plain(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: the system's default, with the arguments the system gave.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn failed(doing: &str, error: &windows::core::Error) -> String {
    format!("{doing}: {} ({})", error.message(), error.code())
}

/// One child control of the window.
#[allow(unsafe_code)] // one call into the system, explained below
#[allow(clippy::too_many_arguments)] // the window's own layout, given in one place
fn control(
    parent: HWND,
    instance: HINSTANCE,
    class: PCWSTR,
    text: &str,
    style: u32,
    ex: WINDOW_EX_STYLE,
    id: i32,
    at: (i32, i32, i32, i32),
) -> Result<HWND, String> {
    let text = HSTRING::from(text);
    // SAFETY: the class names a registered class, the text outlives the
    // call, and the parent is the stand's window. The identifier rides in
    // the menu argument, as the system documents for a child window.
    unsafe {
        CreateWindowExW(
            ex,
            class,
            &text,
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | style),
            at.0,
            at.1,
            at.2,
            at.3,
            Some(parent),
            Some(HMENU(id as isize as *mut _)),
            Some(instance),
            None,
        )
    }
    .map_err(|error| failed("making a control", &error))
}

/// Fills the list view: three columns, one row for each line.
#[allow(unsafe_code)] // messages to the stand's own list view, explained below
fn fill(table: HWND, order: &Order) {
    let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain([0]).collect() };
    for (at, heading) in ["Item code", "Description", "Quantity"].iter().enumerate() {
        let mut text = wide(heading);
        let column = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM,
            cx: 140,
            pszText: PWSTR(text.as_mut_ptr()),
            iSubItem: at as i32,
            ..Default::default()
        };
        // SAFETY: `table` is the stand's list view, made on this thread;
        // the column and its text outlive the call, which copies them.
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                table,
                LVM_INSERTCOLUMNW,
                Some(WPARAM(at)),
                Some(LPARAM(&raw const column as isize)),
            );
        }
    }
    for (row, line) in order.lines.iter().enumerate() {
        for (at, cell) in [&line.code, &line.description, &line.quantity]
            .iter()
            .enumerate()
        {
            let mut text = wide(cell);
            let item = LVITEMW {
                mask: LVIF_TEXT,
                iItem: row as i32,
                iSubItem: at as i32,
                pszText: PWSTR(text.as_mut_ptr()),
                ..Default::default()
            };
            let message = if at == 0 {
                LVM_INSERTITEMW
            } else {
                LVM_SETITEMTEXTW
            };
            // SAFETY: as above; the item and its text outlive the call.
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                    table,
                    message,
                    Some(WPARAM(row)),
                    Some(LPARAM(&raw const item as isize)),
                );
            }
        }
    }
}

/// Puts [`watched`] in front of a control's own procedure.
#[allow(unsafe_code)] // two calls into the system, explained below
fn watch(control: HWND) {
    // SAFETY: `control` is a child of the stand's window, made on this
    // thread. Its own procedure is kept in its user data, which nothing
    // else of the stand's uses, before the new one takes its place, so the
    // first call to `watched` already finds it.
    unsafe {
        let own = GetWindowLongPtrW(control, GWLP_WNDPROC);
        SetWindowLongPtrW(control, GWLP_USERDATA, own);
        let procedure: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT = watched;
        SetWindowLongPtrW(control, GWLP_WNDPROC, procedure as usize as isize);
    }
}

/// Makes the window and its controls, and shows it without taking focus.
/// Returns the window, Save, and the password edit.
#[allow(unsafe_code)] // calls into the system, explained below
fn make(order: &Order) -> Result<(HWND, HWND, HWND), String> {
    let controls = INITCOMMONCONTROLSEX {
        dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_LISTVIEW_CLASSES,
    };
    // SAFETY: the structure is ours and outlives the call.
    if !unsafe { InitCommonControlsEx(&raw const controls) }.as_bool() {
        return Err("the common controls did not start".to_owned());
    }
    // SAFETY: no argument: the module of this program.
    let module = unsafe { GetModuleHandleW(None) }
        .map_err(|error| failed("finding this program's module", &error))?;
    let instance = HINSTANCE(module.0);
    let class = w!("example-desk-window");
    let registered = WNDCLASSW {
        lpfnWndProc: Some(window),
        hInstance: instance,
        lpszClassName: class,
        hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as isize as *mut _),
        ..Default::default()
    };
    // SAFETY: the class structure is ours; its name is a static string.
    if unsafe { RegisterClassW(&raw const registered) } == 0 {
        return Err(failed(
            "registering the window class",
            &windows::core::Error::from_thread(),
        ));
    }
    // The dialog a fault raises: a window of its own class, whose closing
    // ends nothing.
    let dialog = WNDCLASSW {
        lpfnWndProc: Some(plain),
        hInstance: instance,
        lpszClassName: w!("example-desk-dialog"),
        hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as isize as *mut _),
        ..Default::default()
    };
    // SAFETY: as above.
    if unsafe { RegisterClassW(&raw const dialog) } == 0 {
        return Err(failed(
            "registering the dialog's class",
            &windows::core::Error::from_thread(),
        ));
    }
    // SAFETY: the class was registered above; the title outlives the call.
    let main = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            &HSTRING::from(WINDOW),
            WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_CLIPCHILDREN.0),
            40,
            40,
            560,
            660,
            None,
            None,
            Some(instance),
            None,
        )
    }
    .map_err(|error| failed("making the window", &error))?;
    let edit = w!("EDIT");
    let label = w!("STATIC");
    let field = WS_BORDER.0 | ES_AUTOHSCROLL as u32;
    let none = WINDOW_EX_STYLE(0);
    let mut made = Vec::new();
    // The edit nothing labels comes first, so that no static comes before
    // it.
    made.push(control(
        main,
        instance,
        edit,
        "a value nothing labels",
        field,
        none,
        100,
        (160, 10, 300, 24),
    )?);
    let rows: [(&str, &str, i32, u32); 7] = [
        ("Order number", order.number.as_str(), 101, field),
        ("Customer", order.customer.as_str(), 103, field),
        ("Status", order.status.as_str(), 105, field),
        ("Tracking", "", 107, field),
        ("Reference", "R-1", 109, field),
        ("Reference", "R-2", 111, field),
        ("Access code", SECRET, 113, field | ES_PASSWORD as u32),
    ];
    for (row, (name, value, id, style)) in rows.iter().enumerate() {
        let y = 44 + 32 * row as i32;
        made.push(control(
            main,
            instance,
            label,
            name,
            0,
            none,
            *id,
            (10, y, 140, 24),
        )?);
        made.push(control(
            main,
            instance,
            edit,
            value,
            *style,
            none,
            id + 1,
            (160, y, 300, 24),
        )?);
    }
    made.push(control(
        main,
        instance,
        label,
        "Line items",
        0,
        none,
        115,
        (10, 276, 140, 24),
    )?);
    let table = control(
        main,
        instance,
        WC_LISTVIEWW,
        "Line items",
        WS_BORDER.0 | LVS_REPORT,
        WS_EX_CLIENTEDGE,
        116,
        (10, 304, 520, 140),
    )?;
    fill(table, order);
    made.push(table);
    let save = control(
        main,
        instance,
        w!("BUTTON"),
        "Save",
        BS_PUSHBUTTON as u32,
        none,
        SAVE,
        (10, 456, 100, 30),
    )?;
    made.push(save);
    let edits: [(&str, i32, u32); 4] = [
        ("New customer", 118, field),
        ("New status", 120, field),
        ("Message", 122, field | ES_READONLY as u32),
        ("Problem", 124, field | ES_READONLY as u32),
    ];
    for (row, (name, id, style)) in edits.iter().enumerate() {
        let y = 496 + 32 * row as i32;
        made.push(control(
            main,
            instance,
            label,
            name,
            0,
            none,
            *id,
            (10, y, 140, 24),
        )?);
        made.push(control(
            main,
            instance,
            edit,
            "",
            *style,
            none,
            id + 1,
            (160, y, 300, 24),
        )?);
    }
    for one in &made {
        watch(*one);
    }
    let password = made
        .iter()
        .copied()
        // SAFETY: each is a control the stand made on this thread.
        .find(|one| unsafe { GetDlgCtrlID(*one) } == PASSWORD)
        .ok_or_else(|| "the password edit was not made".to_owned())?;
    let tracking = made
        .iter()
        .copied()
        // SAFETY: as above.
        .find(|one| unsafe { GetDlgCtrlID(*one) } == TRACKING)
        .ok_or_else(|| "the tracking edit was not made".to_owned())?;
    TRACKING_EDIT.store(tracking.0 as isize, Ordering::SeqCst);
    MAIN.store(main.0 as isize, Ordering::SeqCst);
    // The problem is shown only while the application refuses a value.
    // SAFETY: a control of the stand's window, on this thread.
    let _ = unsafe { ShowWindow(control_of(PROBLEM), SW_HIDE) };
    // SAFETY: the stand's own window. Shown where UI Automation sees it on
    // the screen, and without taking the focus or the foreground.
    let _ = unsafe { ShowWindow(main, SW_SHOWNOACTIVATE) };
    Ok((main, save, password))
}

/// Posts one of the stand's own messages to its window, from the control
/// interface's thread.
#[allow(unsafe_code)] // one call into the system, explained below
fn post(message: u32, wparam: usize) {
    let main = HWND(MAIN.load(Ordering::SeqCst) as *mut _);
    // SAFETY: posting copies three plain values into the window's queue;
    // a window that is gone makes the call fail, which changes nothing.
    let _ = unsafe { PostMessageW(Some(main), message, WPARAM(wparam), LPARAM(0)) };
}

/// Arms a fault, or has the window's thread carry out a field fault and
/// waits until it has; the answer the control interface gives.
#[allow(unsafe_code)] // one call into the system, explained below
fn fault(line: &str) -> String {
    match arm(line) {
        Err(()) => "fault\tunknown".to_owned(),
        Ok(None) => "fault".to_owned(),
        Ok(Some(field)) => {
            *lock(&FIELD_FAULT) = Some(field);
            let main = HWND(MAIN.load(Ordering::SeqCst) as *mut _);
            // SAFETY: sends the stand's own message to its window, and
            // waits until the window's thread has carried it out; three
            // plain values.
            let done = unsafe { SendMessageW(main, FAULT_MESSAGE, None, None) };
            if done.0 == 1 {
                "fault".to_owned()
            } else {
                "fault\tno field has that label".to_owned()
            }
        }
    }
}

/// The control interface: one command a line on standard input, answered
/// on standard output. The end of the input, as when the test is gone,
/// closes the window.
fn control_interface() {
    let input = std::io::stdin();
    for line in input.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line == "truth" {
            println!("{}", truth_line());
        } else if let Some(after) = line.strip_prefix("late ") {
            post(LATE_MESSAGE, after.parse::<usize>().unwrap_or(0));
            println!("{PREFIX}late");
        } else if line.starts_with("fault ") {
            println!("{PREFIX}{}", fault(line));
        } else if line == "quit" {
            break;
        }
    }
    post(WM_CLOSE, 0);
}

/// Runs the stand: the window, the control interface, and the message loop
/// until the window closes. Returns the process's exit code.
#[allow(unsafe_code)] // calls into the system, explained below
pub(crate) fn serve() -> i32 {
    // SAFETY: sets this process's error mode; touches no memory. No dialog
    // may appear on a machine with no person.
    unsafe { SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX) };
    let order = std::env::var(ORDER_ENV)
        .ok()
        .and_then(|text| decode_order(&text))
        .unwrap_or_else(Order::example);
    book().open(&order);
    let (_, save, password) = match make(&order) {
        Ok(made) => made,
        Err(why) => {
            eprintln!("example-desk: {why}");
            return 2;
        }
    };
    std::thread::spawn(control_interface);
    println!(
        "{PREFIX}ready\t{}\t{}\t{}",
        std::process::id(),
        save.0 as isize,
        password.0 as isize
    );
    let mut message = MSG::default();
    loop {
        // SAFETY: `message` is ours; the loop is on the thread that made
        // the window.
        let got = unsafe { GetMessageW(&raw mut message, None, 0, 0) };
        if got.0 <= 0 {
            break;
        }
        // SAFETY: a message the system just gave, handed back to it.
        unsafe {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
    0
}
