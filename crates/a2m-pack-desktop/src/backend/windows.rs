//! The Windows back end: UI Automation, through the workspace's `windows`
//! crate, read only (decision 0209).
//!
//! **Threading.** UI Automation is a COM interface, and a COM object may be
//! used only in the apartment that made it. Each [`Look`] owns one thread
//! of its own, which joins the multithreaded apartment, makes the
//! automation object, and then runs, one after another, the jobs the async
//! methods send it over a channel. Every COM object (the automation object,
//! its tree walker, and every element of a walk) is made on that thread,
//! kept on it, and released on it before the thread leaves the apartment;
//! nothing of COM crosses to Tokio. An element is known outside the thread
//! only as an opaque key in an [`El`]. An async method sends a job and
//! waits for its answer with a bound, so a call the application never
//! answers costs the step its deadline and never blocks an async thread;
//! UI Automation's own connection and transaction timeouts bound each call
//! on the thread as well. The process checks before anything is asked
//! (the window's process, its program, its owner, its digest) run on the
//! same thread, as plain system calls.
//!
//! **Events.** A wait listens to the system's accessibility events, filtered
//! by the system to the declared application's process, on a second thread
//! of its own that owns the hook and pumps its messages: an out-of-context
//! event hook loads nothing into the application and calls back on the
//! thread that set it. Standard controls send these events themselves, and
//! UI Automation sends its own events through the same mechanism. The
//! listener never looks at what an event says: it wakes the wait, which
//! reads afresh.
//!
//! **Reads only.** Every call to UI Automation goes through [`ask`], which
//! makes only the members of [`READ_MEMBERS`]; each one reads, and none
//! sets, invokes, selects, toggles, expands, scrolls, or focuses. A test
//! holds the list against the members that act, and a second test reads
//! this file and finds every method called on a UI Automation object
//! inside an [`ask`] that names it. A password edit (`IsPassword`) is
//! reported as present and secret, and its value is never asked for.

use std::collections::BTreeMap;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use a2m_provider::{Deadline, resolved_path};
use tokio::sync::oneshot;
use windows::Win32::Foundation::{CloseHandle, E_ACCESSDENIED, HANDLE, HWND, LPARAM};
use windows::Win32::Security::{
    EqualSid, GetTokenInformation, PSID, TOKEN_ELEVATION, TOKEN_INFORMATION_CLASS, TOKEN_QUERY,
    TOKEN_USER, TokenElevation, TokenUser,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, OpenProcess, OpenProcessToken,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::System::Variant::{VARIANT, VT_BOOL, VariantClear};
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, HWINEVENTHOOK, IUIAutomation, IUIAutomation2, IUIAutomationElement,
    IUIAutomationTreeWalker, IUIAutomationValuePattern, SetWinEventHook, UIA_CONTROLTYPE_ID,
    UIA_E_ELEMENTNOTAVAILABLE, UIA_E_TIMEOUT, UIA_IsValuePatternAvailablePropertyId,
    UIA_PATTERN_ID, UIA_PROPERTY_ID, UIA_ValuePatternId, UnhookWinEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumWindows, GetMessageW, GetWindowThreadProcessId, IsWindow,
    IsWindowVisible, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, TranslateMessage,
    WINEVENT_OUTOFCONTEXT, WM_QUIT,
};
use windows::core::{BOOL, HRESULT, Interface, PWSTR};

use super::{App, AxError, Limits, Window};
use crate::roles;
use crate::tree::{El, Node, Tree};

/// The longest one call to the application may take, on the thread and
/// as the async side waits for it.
const CALL_LIMIT: Duration = Duration::from_secs(5);
/// The most top-level windows one listing takes.
const MOST_WINDOWS: usize = 100_000;
/// The most comparisons one walk makes to find what labels what.
const MOST_COMPARISONS: usize = 20_000;

/// Every UI Automation member this back end may call. Each reads; none
/// sets a value, invokes, selects, toggles, expands, scrolls, moves,
/// focuses, or sends input. `SetConnectionTimeout` and
/// `SetTransactionTimeout` set how long this client waits, on the client's
/// own object, and reach no application. A test holds the list against
/// the members of UI Automation that act.
pub(crate) const READ_MEMBERS: &[&str] = &[
    "SetConnectionTimeout",
    "SetTransactionTimeout",
    "ControlViewWalker",
    "ElementFromHandle",
    "CompareElements",
    "GetFirstChildElement",
    "GetNextSiblingElement",
    "CurrentProcessId",
    "CurrentControlType",
    "CurrentName",
    "CurrentIsPassword",
    "CurrentIsOffscreen",
    "CurrentLabeledBy",
    "GetCurrentPropertyValue",
    "GetCurrentPatternAs",
    "CurrentValue",
];

/// The properties `GetCurrentPropertyValue` may ask for.
const READ_PROPERTIES: &[UIA_PROPERTY_ID] = &[UIA_IsValuePatternAvailablePropertyId];

/// The patterns `GetCurrentPatternAs` may get: the value pattern, of which
/// only `CurrentValue` is read.
const READ_PATTERNS: &[UIA_PATTERN_ID] = &[UIA_ValuePatternId];

/// UI Automation's control types, by number, mapped to the pack's role
/// words: the one table of this system's roles. A control type that is
/// not here is [`roles::OTHER`], shown in a snapshot and never matched by a
/// locator. Two refinements, in [`role_word`]: an edit that says it holds
/// a password is `password_text`, and a text, an edit, or a custom element
/// directly under a `table_row` is a `table_cell`. A list view in report
/// view is a data grid of data items, with a header of header items as its
/// first row.
pub(crate) const CONTROL_TYPES: &[(i32, &str)] = &[
    (50000, "push_button"),         // Button
    (50002, "check_box"),           // CheckBox
    (50003, "combo_box"),           // ComboBox
    (50004, "entry"),               // Edit
    (50006, "image"),               // Image
    (50007, "list_item"),           // ListItem
    (50008, "list"),                // List
    (50009, "menu"),                // Menu
    (50010, "menu_bar"),            // MenuBar
    (50011, "menu_item"),           // MenuItem
    (50013, "radio_button"),        // RadioButton
    (50016, "spin_button"),         // Spinner
    (50017, "status_bar"),          // StatusBar
    (50018, "page_tab_list"),       // Tab
    (50019, "page_tab"),            // TabItem
    (50020, "label"),               // Text
    (50021, "tool_bar"),            // ToolBar
    (50023, "tree"),                // Tree
    (50024, "tree_item"),           // TreeItem
    (50026, "panel"),               // Group
    (50028, "table"),               // DataGrid
    (50029, "table_row"),           // DataItem
    (50032, "frame"),               // Window
    (50033, "panel"),               // Pane
    (50034, "table_row"),           // Header: the row of a grid's headings
    (50035, "table_column_header"), // HeaderItem
    (50036, "table"),               // Table
];

const EDIT: i32 = 50004;
const TEXT: i32 = 50020;
const CUSTOM: i32 = 50025;

/// The pack's word for an element of control type `control`.
pub(crate) fn role_word(control: i32, password: bool, in_row: bool) -> &'static str {
    if control == EDIT && password {
        return "password_text";
    }
    if in_row && [TEXT, EDIT, CUSTOM].contains(&control) {
        return "table_cell";
    }
    CONTROL_TYPES
        .iter()
        .find(|(known, _)| *known == control)
        .map_or(roles::OTHER, |(_, word)| word)
}

/// The system's accessibility events a wait listens to, as ranges: objects
/// made, destroyed, shown, hidden, reordered, focused, selected, changed in
/// state, name, value, or parent; text and live regions; and UI
/// Automation's own events and property changes. Not the location changes,
/// which a caret sends many times a second.
const EVENT_RANGES: &[(u32, u32)] = &[
    (0x8000, 0x800A),
    (0x800C, 0x8019),
    (0x4E00, 0x4EFF),
    (0x7500, 0x75FF),
];

/// What a failed call to the system says, in words, with its code.
fn said(what: &str, error: &windows::core::Error) -> AxError {
    if error.code() == HRESULT(UIA_E_TIMEOUT as i32) {
        return AxError::Timeout;
    }
    let message = error.message();
    let message = if message.trim().is_empty() {
        "no message".to_owned()
    } else {
        message.trim().to_owned()
    };
    if error.code() == HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32) {
        return AxError::Lost(format!(
            "{what}: the element is no longer there ({message})"
        ));
    }
    AxError::Lost(format!("{what}: {message} ({})", error.code()))
}

/// Makes one call to UI Automation, when `member` is on the list of reads,
/// and gives its answer or what the system said.
fn ask<T>(
    member: &'static str,
    call: impl FnOnce() -> windows::core::Result<T>,
) -> Result<T, AxError> {
    if !READ_MEMBERS.contains(&member) {
        return Err(AxError::Lost(format!("`{member}` is not a read")));
    }
    call().map_err(|error| said(member, &error))
}

/// The same, for a member whose answer may be no element: the system then
/// answers with success and nothing, which the `windows` crate gives as an
/// error with a success code.
fn ask_optional<T>(
    member: &'static str,
    call: impl FnOnce() -> windows::core::Result<T>,
) -> Result<Option<T>, AxError> {
    if !READ_MEMBERS.contains(&member) {
        return Err(AxError::Lost(format!("`{member}` is not a read")));
    }
    match call() {
        Ok(found) => Ok(Some(found)),
        Err(error) if error.code().is_ok() => Ok(None),
        Err(error) => Err(said(member, &error)),
    }
}

/// A handle the system opened for this module, closed when dropped.
struct Opened(HANDLE);

impl Drop for Opened {
    #[allow(unsafe_code)] // one call into the system, explained below
    fn drop(&mut self) {
        // SAFETY: the handle was opened for this value alone and is closed
        // once, here.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// A process's user and whether it runs elevated, from its token.
struct Token {
    /// The token's `TOKEN_USER`, in whole 64-bit words so that it is
    /// aligned for the structure, which holds a pointer into itself.
    user: Vec<u64>,
    elevated: bool,
}

/// One item of a token's information, in a buffer of its own.
#[allow(unsafe_code)] // two calls into the system, explained below
fn information(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> windows::core::Result<Vec<u64>> {
    let mut needed = 0_u32;
    // SAFETY: the token is open. With no buffer the call writes only
    // `needed`, which is ours, and fails saying the buffer is too small.
    let _ = unsafe { GetTokenInformation(token, class, None, 0, &raw mut needed) };
    if needed == 0 {
        return Err(windows::core::Error::from_thread());
    }
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(size_of::<u64>())];
    let length = u32::try_from(buffer.len() * size_of::<u64>()).unwrap_or(u32::MAX);
    // SAFETY: `buffer` is writable for `length` bytes and outlives the
    // call; `needed` is ours.
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some(buffer.as_mut_ptr().cast::<c_void>()),
            length,
            &raw mut needed,
        )
    }?;
    Ok(buffer)
}

/// The user and the elevation of the process `process` is a handle to.
#[allow(unsafe_code)] // three calls into the system, explained below
fn token_of(process: HANDLE) -> windows::core::Result<Token> {
    let mut raw = HANDLE::default();
    // SAFETY: the process handle is open; the call writes one handle into
    // `raw`, which is ours and is closed by `Opened`.
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut raw) }?;
    let token = Opened(raw);
    let user = information(token.0, TokenUser)?;
    let mut elevation = TOKEN_ELEVATION::default();
    let mut got = 0_u32;
    // SAFETY: `elevation` is ours and as long as the length says.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some((&raw mut elevation).cast::<c_void>()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &raw mut got,
        )
    }?;
    Ok(Token {
        user,
        elevated: elevation.TokenIsElevated != 0,
    })
}

impl Token {
    /// Whether two tokens are of one user: their security identifiers are
    /// equal, as the system compares them, never as text.
    #[allow(unsafe_code)] // one call into the system and two reads, explained below
    fn same_user(&self, other: &Self) -> bool {
        // SAFETY: each buffer holds a `TOKEN_USER` the system wrote, is
        // aligned for it, and outlives the comparison; the identifier each
        // points to lies inside its own buffer.
        unsafe {
            let mine: PSID = (*self.user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
            let theirs: PSID = (*other.user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
            EqualSid(mine, theirs).is_ok()
        }
    }
}

/// This process: its token and its session.
struct Me {
    token: Token,
    session: u32,
}

#[allow(unsafe_code)] // two calls into the system, explained below
fn me() -> Result<Me, AxError> {
    // SAFETY: the pseudo handle of this process needs no closing.
    let token = token_of(unsafe { GetCurrentProcess() })
        .map_err(|error| said("reading this process's own token", &error))?;
    let mut session = 0_u32;
    // SAFETY: `session` is ours.
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &raw mut session) }
        .map_err(|error| said("reading this process's own session", &error))?;
    Ok(Me { token, session })
}

/// Opens a process to ask the system about it, and nothing more.
#[allow(unsafe_code)] // one call into the system, explained below
fn open(process: u32) -> windows::core::Result<Opened> {
    // SAFETY: three plain values; the handle given is owned by `Opened`.
    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process) }.map(Opened)
}

/// The program a process runs, as the system reports it.
#[allow(unsafe_code)] // one call into the system, explained below
fn image_of(process: u32) -> Option<PathBuf> {
    let opened = open(process).ok()?;
    let mut buffer = vec![0_u16; 32_768];
    let mut length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
    // SAFETY: `buffer` is writable for `length` characters and outlives
    // the call, which writes the length it used into `length`.
    unsafe {
        QueryFullProcessImageNameW(
            opened.0,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &raw mut length,
        )
    }
    .ok()?;
    let used = buffer.get(..length as usize)?;
    Some(PathBuf::from(String::from_utf16(used).ok()?))
}

/// The session a process runs in.
#[allow(unsafe_code)] // one call into the system, explained below
fn session_of(process: u32) -> Option<u32> {
    let mut session = 0_u32;
    // SAFETY: `session` is ours.
    unsafe { ProcessIdToSessionId(process, &raw mut session) }
        .ok()
        .map(|()| session)
}

fn hwnd(number: isize) -> HWND {
    HWND(number as *mut c_void)
}

/// Adds one window to the list a `LPARAM` points to.
#[allow(unsafe_code)] // one read through the pointer the caller gave, explained below
extern "system" fn listed(window: HWND, list: LPARAM) -> BOOL {
    // SAFETY: `top_level_windows` passes a pointer to its own vector,
    // which outlives the enumeration and is touched by nothing else while
    // the system calls this back on the same thread.
    let list = unsafe { &mut *(list.0 as *mut Vec<isize>) };
    list.push(window.0 as isize);
    BOOL::from(list.len() < MOST_WINDOWS)
}

/// The top-level windows of this desktop, by handle. Only the system is
/// asked; no window is sent anything.
#[allow(unsafe_code)] // one call into the system, explained below
fn top_level_windows() -> Result<Vec<isize>, AxError> {
    let mut list: Vec<isize> = Vec::new();
    // SAFETY: the callback reads the pointer as the vector it is, during
    // the call only.
    let done = unsafe { EnumWindows(Some(listed), LPARAM(&raw mut list as isize)) };
    if done.is_err() && list.len() < MOST_WINDOWS {
        return Err(AxError::Lost(
            "EnumWindows: the system did not list the windows".to_owned(),
        ));
    }
    Ok(list)
}

/// The process a window belongs to, and whether it is shown; `None` when
/// it is no longer a window.
#[allow(unsafe_code)] // three calls into the system, explained below
fn window_facts(window: isize) -> Option<(u32, bool)> {
    let window = hwnd(window);
    // SAFETY: each call takes a window handle, which the system checks, and
    // writes only `process`, which is ours.
    unsafe {
        if !IsWindow(Some(window)).as_bool() {
            return None;
        }
        let mut process = 0_u32;
        GetWindowThreadProcessId(window, Some(&raw mut process));
        Some((process, IsWindowVisible(window).as_bool()))
    }
}

/// The digest of a program file, as a surface states one.
fn digest_of(program: &Path) -> Option<String> {
    use sha2::Digest as _;
    use std::io::Read;
    let mut file = std::fs::File::open(program).ok()?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => hasher.update(&buffer[..read]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    Some(
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}

/// The digest of the program a process runs: of the file at `want`, the
/// path the system gave for the process's image, taken only when the
/// system still gives that path once the file is read. The system's path
/// follows the image's file, so a file moved away and another put in its
/// place while it was read is not taken for the running one. A change of
/// the file's content in place is not caught here; that the system refuses
/// it for a running image is from memory, not verified (decision 0244).
fn running_digest(process: u32, want: &Path) -> Option<String> {
    let digest = digest_of(want)?;
    image_of(process)
        .and_then(|image| resolved_path(&image).ok())
        .is_some_and(|image| image == want)
        .then_some(digest)
}

/// The process an element of the walk belongs to, from its key.
fn process_of(el: &El) -> Result<u32, AxError> {
    el.bus
        .parse::<u32>()
        .map_err(|_| AxError::Lost("an element this back end did not give".to_owned()))
}

/// What the UI Automation thread holds: every COM object of one [`Look`].
struct Uia {
    automation: IUIAutomation,
    walker: IUIAutomationTreeWalker,
    me: Me,
    /// The elements of the last two walks, by walk.
    walks: BTreeMap<u64, Vec<IUIAutomationElement>>,
    next: u64,
    /// The control type of each element of the last walk, for a test that
    /// must say what it found.
    #[cfg(test)]
    types: Vec<i32>,
}

type Job = Box<dyn FnOnce(&mut Uia) + Send>;

impl Uia {
    #[allow(unsafe_code)] // calls into COM and UI Automation, explained below
    fn start() -> Result<Self, AxError> {
        // SAFETY: the class identifier is the system's own constant; the
        // object is made in this thread's apartment, which the caller
        // joined, and is owned by the wrapper.
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }
                .map_err(|error| said("making the UI Automation object", &error))?;
        let limit = u32::try_from(CALL_LIMIT.as_millis()).unwrap_or(u32::MAX);
        // SAFETY (each `ask` below): a method of an interface the wrapper
        // holds a counted reference to, on the thread that made it, with
        // plain values as arguments.
        if let Ok(timed) = automation.cast::<IUIAutomation2>() {
            ask("SetConnectionTimeout", || unsafe {
                timed.SetConnectionTimeout(limit)
            })?;
            ask("SetTransactionTimeout", || unsafe {
                timed.SetTransactionTimeout(limit)
            })?;
        }
        let walker = ask("ControlViewWalker", || unsafe {
            automation.ControlViewWalker()
        })?;
        Ok(Self {
            automation,
            walker,
            me: me()?,
            walks: BTreeMap::new(),
            next: 0,
            #[cfg(test)]
            types: Vec::new(),
        })
    }

    /// The one running application that is `program`, run by this
    /// process's user in this session at no higher level, with the name
    /// `name`, and with its digest checked when `digest` is stated. Only
    /// the system is asked about the windows that are not its.
    fn application(&self, program: &str, name: &str, digest: Option<&str>) -> Result<App, AxError> {
        let Ok(want) = resolved_path(Path::new(program)) else {
            return Err(AxError::NotRunning);
        };
        let mut runs: BTreeMap<u32, bool> = BTreeMap::new();
        for window in top_level_windows()? {
            let Some((process, shown)) = window_facts(window) else {
                continue;
            };
            if !shown || process == 0 || runs.contains_key(&process) {
                continue;
            }
            let program = image_of(process).and_then(|image| resolved_path(&image).ok());
            runs.insert(process, program.as_deref() == Some(want.as_path()));
        }
        let own_name = want.file_stem().and_then(|stem| stem.to_str());
        let mut found = Vec::new();
        for (process, _) in runs.into_iter().filter(|(_, runs)| *runs) {
            // The program is the declared one. Before anything is asked of
            // the application: its session, its user, and its level.
            if session_of(process) != Some(self.me.session) {
                return Err(AxError::NotOwned);
            }
            let theirs = match open(process).and_then(|opened| token_of(opened.0)) {
                Ok(token) => token,
                Err(error) if error.code() == E_ACCESSDENIED => return Err(AxError::Elevated),
                Err(error) => return Err(said("reading the application's token", &error)),
            };
            if !theirs.same_user(&self.me.token) {
                return Err(AxError::NotOwned);
            }
            if theirs.elevated && !self.me.token.elevated {
                return Err(AxError::Elevated);
            }
            if let Some(stated) = digest {
                let stated = stated.strip_prefix("sha256:").unwrap_or(stated);
                if running_digest(process, &want).as_deref() != Some(stated) {
                    return Err(AxError::NotProven);
                }
            }
            // Its name: the name of its program's file, without the
            // extension. UI Automation has no element for an application.
            if own_name == Some(name) {
                found.push(process);
            }
        }
        match found.as_slice() {
            [] => Err(AxError::NotRunning),
            [process] => Ok(App {
                root: El {
                    bus: process.to_string(),
                    path: "process".to_owned(),
                },
                process: *process,
            }),
            many => Err(AxError::Ambiguous(many.len())),
        }
    }

    /// The element a key names, made afresh for a window, or kept from a
    /// walk.
    #[allow(unsafe_code)] // one call into UI Automation, explained below
    fn element(&self, el: &El) -> Result<IUIAutomationElement, AxError> {
        let process = process_of(el)?;
        if let Some(window) = el.path.strip_prefix("window:") {
            let window = window
                .parse::<isize>()
                .map_err(|_| AxError::Lost("a window this back end did not give".to_owned()))?;
            // The window must still be the application's before it is
            // asked anything.
            match window_facts(window) {
                Some((owner, _)) if owner == process => {}
                _ => {
                    return Err(AxError::Lost(
                        "the window is gone, or is no longer the application's".to_owned(),
                    ));
                }
            }
            // SAFETY: a method of the automation object, on its thread; the
            // handle is a plain value the system checks.
            return ask("ElementFromHandle", || unsafe {
                self.automation.ElementFromHandle(hwnd(window))
            });
        }
        let kept = el.path.strip_prefix("walk:").and_then(|rest| {
            let (walk, at) = rest.split_once('.')?;
            let walk = walk.parse::<u64>().ok()?;
            let at = at.parse::<usize>().ok()?;
            self.walks.get(&walk)?.get(at).cloned()
        });
        kept.ok_or_else(|| {
            AxError::Lost("the element is from a read this back end no longer holds".to_owned())
        })
    }

    /// The application's windows: its top-level windows that are shown.
    #[allow(unsafe_code)] // calls into UI Automation, explained below
    fn windows(&self, app: &App) -> Result<Vec<Window>, AxError> {
        let mut out = Vec::new();
        for window in top_level_windows()? {
            match window_facts(window) {
                Some((process, true)) if process == app.process => {}
                _ => continue,
            }
            let el = El {
                bus: app.process.to_string(),
                path: format!("window:{window}"),
            };
            let element = match self.element(&el) {
                Ok(element) => element,
                // A window that closed since it was listed is not one.
                Err(_) if window_facts(window).is_none() => continue,
                Err(error) => return Err(error),
            };
            // SAFETY (each `ask`): a method of an element this thread made,
            // with no argument.
            let control = ask("CurrentControlType", || unsafe {
                element.CurrentControlType()
            })?;
            let title = ask("CurrentName", || unsafe { element.CurrentName() })?;
            out.push(Window {
                el,
                role: role_word(control.0, false, false),
                title: title.to_string(),
            });
        }
        Ok(out)
    }

    /// Whether a property of [`READ_PROPERTIES`] that is a truth holds.
    #[allow(unsafe_code)] // one call into UI Automation and reads of its answer, explained below
    fn flag(element: &IUIAutomationElement, property: UIA_PROPERTY_ID) -> Result<bool, AxError> {
        if !READ_PROPERTIES.contains(&property) {
            return Err(AxError::Lost("a property that is not read".to_owned()));
        }
        // SAFETY: a method of an element this thread made, with a plain
        // value as its argument.
        let mut value: VARIANT = ask("GetCurrentPropertyValue", || unsafe {
            element.GetCurrentPropertyValue(property)
        })?;
        // SAFETY: the variant is the system's answer, which says its own
        // type; its truth is read only when that type is a truth. Clearing
        // it frees what it holds, once.
        unsafe {
            let inner = &value.Anonymous.Anonymous;
            let holds = inner.vt == VT_BOOL && inner.Anonymous.boolVal.0 != 0;
            let _ = VariantClear(&raw mut value);
            Ok(holds)
        }
    }

    /// Everything one element says of itself but its value, and what
    /// labels it, when anything does.
    #[allow(unsafe_code)] // calls into UI Automation, explained below
    fn node(
        element: &IUIAutomationElement,
        depth: u32,
        in_row: bool,
    ) -> Result<(Node, Option<IUIAutomationElement>, i32), AxError> {
        // SAFETY (each `ask`): a method of an element this thread made,
        // with no argument; each answer is owned by its wrapper.
        let control: UIA_CONTROLTYPE_ID = ask("CurrentControlType", || unsafe {
            element.CurrentControlType()
        })?;
        let secret = ask("CurrentIsPassword", || unsafe {
            element.CurrentIsPassword()
        })?;
        let offscreen = ask("CurrentIsOffscreen", || unsafe {
            element.CurrentIsOffscreen()
        })?;
        let has_text = Self::flag(element, UIA_IsValuePatternAvailablePropertyId)?;
        let mut name = ask("CurrentName", || unsafe { element.CurrentName() })?.to_string();
        let label = ask_optional("CurrentLabeledBy", || unsafe { element.CurrentLabeledBy() })?;
        if name.is_empty()
            && let Some(label) = &label
        {
            // A screen reader's name: what labels it.
            name = ask("CurrentName", || unsafe { label.CurrentName() })?.to_string();
        }
        let node = Node {
            depth,
            role: role_word(control.0, secret.as_bool(), in_row),
            name,
            showing: !offscreen.as_bool(),
            secret: secret.as_bool(),
            has_text,
            labels: false,
            children: Vec::new(),
            el: El {
                bus: String::new(),
                path: String::new(),
            },
        };
        Ok((node, label, control.0))
    }

    /// The elements directly under `element` that are the application's,
    /// at most `most`.
    #[allow(unsafe_code)] // calls into UI Automation, explained below
    fn children(
        &self,
        element: &IUIAutomationElement,
        process: u32,
        most: usize,
    ) -> Result<Vec<IUIAutomationElement>, AxError> {
        let mut out = Vec::new();
        // SAFETY (each `ask`): a method of the walker or of an element this
        // thread made, with an element this thread holds as its argument.
        let mut next = ask_optional("GetFirstChildElement", || unsafe {
            self.walker.GetFirstChildElement(element)
        })?;
        while let Some(child) = next {
            if out.len() >= most {
                break;
            }
            let owner = ask("CurrentProcessId", || unsafe { child.CurrentProcessId() })?;
            next = ask_optional("GetNextSiblingElement", || unsafe {
                self.walker.GetNextSiblingElement(&child)
            })?;
            // Another program's element inside the window is not read.
            if u32::try_from(owner).ok() == Some(process) {
                out.push(child);
            }
        }
        Ok(out)
    }

    /// Walks the tree under `root`, each element before those under it,
    /// within the bounds and the deadline.
    #[allow(unsafe_code)] // one call into UI Automation, explained below
    fn tree(&mut self, root: &El, limits: Limits, deadline: Deadline) -> Result<Tree, AxError> {
        let process = process_of(root)?;
        let first = self.element(root)?;
        let walk = self.next;
        self.next += 1;
        let mut tree = Tree::default();
        let mut kept: Vec<IUIAutomationElement> = Vec::new();
        let mut labelling: Vec<IUIAutomationElement> = Vec::new();
        let mut stack: Vec<(IUIAutomationElement, u32, Option<usize>)> = vec![(first, 0, None)];
        #[cfg(test)]
        self.types.clear();
        while let Some((element, depth, parent)) = stack.pop() {
            if tree.nodes.len() >= limits.nodes {
                tree.cut = Some("nodes");
                break;
            }
            if deadline.passed() {
                tree.cut = Some("seconds");
                break;
            }
            let in_row = parent.is_some_and(|at| tree.nodes[at].role == "table_row");
            let (mut node, label, _control) = Self::node(&element, depth, in_row)?;
            #[cfg(test)]
            self.types.push(_control);
            let at = tree.nodes.len();
            node.el = El {
                bus: process.to_string(),
                path: format!("walk:{walk}.{at}"),
            };
            labelling.extend(label);
            tree.nodes.push(node);
            if let Some(parent) = parent {
                tree.nodes[parent].children.push(at);
            }
            let children = self.children(&element, process, limits.nodes)?;
            kept.push(element);
            if depth >= limits.depth {
                if !children.is_empty() && tree.cut.is_none() {
                    tree.cut = Some("depth");
                }
                continue;
            }
            for child in children.into_iter().rev() {
                stack.push((child, depth + 1, Some(at)));
            }
        }
        // What labels something: a label that an element of the walk names
        // as its label.
        let mut compared = 0_usize;
        'labels: for label in &labelling {
            for (at, node) in tree.nodes.iter_mut().enumerate() {
                if node.role != "label" || node.labels {
                    continue;
                }
                compared += 1;
                if compared > MOST_COMPARISONS || deadline.passed() {
                    break 'labels;
                }
                // SAFETY: a method of the automation object, with two
                // elements this thread holds.
                let same = ask("CompareElements", || unsafe {
                    self.automation.CompareElements(label, &kept[at])
                })?;
                if same.as_bool() {
                    node.labels = true;
                    break;
                }
            }
        }
        self.walks.insert(walk, kept);
        self.walks.retain(|known, _| known + 1 >= walk);
        Ok(tree)
    }

    /// The value an element shows, through the value pattern, at most
    /// `limits.chars` characters. A password edit is never asked.
    #[allow(unsafe_code)] // calls into UI Automation, explained below
    fn text(&self, el: &El, limits: Limits) -> Result<String, AxError> {
        let element = self.element(el)?;
        // SAFETY (each `ask`): a method of an element this thread holds, or
        // of the pattern it gave, with plain values as arguments.
        if ask("CurrentIsPassword", || unsafe {
            element.CurrentIsPassword()
        })?
        .as_bool()
        {
            return Err(AxError::Secret);
        }
        let pattern = UIA_ValuePatternId;
        if !READ_PATTERNS.contains(&pattern) {
            return Err(AxError::Lost("a pattern that is not read".to_owned()));
        }
        let value: IUIAutomationValuePattern = ask("GetCurrentPatternAs", || unsafe {
            element.GetCurrentPatternAs::<IUIAutomationValuePattern>(pattern)
        })?;
        let shown = ask("CurrentValue", || unsafe { value.CurrentValue() })?.to_string();
        if shown.chars().count() > limits.chars {
            return Err(AxError::TooLarge);
        }
        Ok(shown)
    }
}

/// The UI Automation thread: joins the multithreaded apartment, makes the
/// automation object, says whether it could, then runs jobs until the
/// handle is gone, and leaves the apartment after every object is
/// released.
#[allow(unsafe_code)] // two calls into COM, explained below
fn serve(ready: oneshot::Sender<Result<(), AxError>>, jobs: mpsc::Receiver<Job>) {
    // SAFETY: joins this new thread to the multithreaded apartment; no
    // argument is a pointer.
    let joined = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if joined.is_err() {
        let _ = ready.send(Err(said(
            "joining the multithreaded apartment",
            &windows::core::Error::from(joined),
        )));
        return;
    }
    match Uia::start() {
        Ok(mut uia) => {
            let _ = ready.send(Ok(()));
            while let Ok(job) = jobs.recv() {
                job(&mut uia);
            }
            drop(uia);
        }
        Err(error) => {
            let _ = ready.send(Err(error));
        }
    }
    // SAFETY: balances the join above, on the same thread, after every COM
    // object this thread made has been released.
    unsafe { CoUninitialize() };
}

/// The look-only handle: a thread of its own that holds every COM object.
pub(crate) struct Look {
    jobs: mpsc::Sender<Job>,
}

/// The application's events, from the moment the watch began.
pub(crate) struct Watch {
    events: tokio::sync::mpsc::Receiver<()>,
    thread: u32,
}

impl Watch {
    /// Waits for the application's next event. `false` when the deadline
    /// passed first.
    pub(crate) async fn next(&mut self, deadline: Deadline) -> Result<bool, AxError> {
        match tokio::time::timeout_at(deadline.at(), self.events.recv()).await {
            Ok(Some(())) => Ok(true),
            Ok(None) => Err(AxError::Lost(
                "the listener for the application's events ended".to_owned(),
            )),
            Err(_) => Ok(false),
        }
    }
}

impl Drop for Watch {
    #[allow(unsafe_code)] // one call into the system, explained below
    fn drop(&mut self) {
        // SAFETY: posts a plain message to the listener's thread, which
        // ends its loop, removes its hooks, and returns. A thread that has
        // already ended makes the call fail, which changes nothing.
        let _ = unsafe {
            PostThreadMessageW(
                self.thread,
                WM_QUIT,
                windows::Win32::Foundation::WPARAM(0),
                LPARAM(0),
            )
        };
    }
}

thread_local! {
    /// The listener thread's way to wake its watch.
    static WAKE: std::cell::RefCell<Option<tokio::sync::mpsc::Sender<()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Called by the system, on the listener's thread, for each event of the
/// application. It reads nothing of the event.
extern "system" fn woke(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _window: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    WAKE.with(|wake| {
        if let Some(wake) = wake.borrow().as_ref() {
            // A wake already waiting is enough: the wait reads afresh.
            let _ = wake.try_send(());
        }
    });
}

/// The listener thread: hooks the events of one process, says so, and
/// pumps its messages until told to quit.
#[allow(unsafe_code)] // calls into the system, explained below
fn listen(
    process: u32,
    wake: tokio::sync::mpsc::Sender<()>,
    ready: oneshot::Sender<Result<u32, AxError>>,
) {
    WAKE.with(|slot| *slot.borrow_mut() = Some(wake));
    let mut message = MSG::default();
    // SAFETY: `message` is ours. Peeking makes this thread's message
    // queue, so that a quit posted to it is never lost.
    let _ = unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_NOREMOVE) };
    let mut hooks = Vec::new();
    for (low, high) in EVENT_RANGES {
        // SAFETY: an out-of-context hook loads nothing into the application
        // and calls `woke`, a function of this program, on this thread,
        // for the events of `process` only.
        let hook = unsafe {
            SetWinEventHook(
                *low,
                *high,
                None,
                Some(woke),
                process,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if hook.is_invalid() {
            for made in hooks {
                // SAFETY: a hook this thread set, removed once.
                let _ = unsafe { UnhookWinEvent(made) };
            }
            let _ = ready.send(Err(said(
                "SetWinEventHook",
                &windows::core::Error::from_thread(),
            )));
            return;
        }
        hooks.push(hook);
    }
    // SAFETY: no argument.
    let _ = ready.send(Ok(unsafe { GetCurrentThreadId() }));
    loop {
        // SAFETY: `message` is ours; this is the thread the hooks call back
        // on, which must pump its messages for them.
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
    for hook in hooks {
        // SAFETY: a hook this thread set, removed once, on this thread.
        let _ = unsafe { UnhookWinEvent(hook) };
    }
}

impl Look {
    /// Starts the UI Automation thread. UI Automation reads the session
    /// the engine runs in; it has no bus, so `session` is not used.
    pub(crate) async fn connect(_session: Option<&str>) -> Result<Self, AxError> {
        let (jobs, taken) = mpsc::channel::<Job>();
        let (said_ready, ready) = oneshot::channel();
        std::thread::Builder::new()
            .name("a2m-desktop-uia".to_owned())
            .spawn(move || serve(said_ready, taken))
            .map_err(|error| {
                AxError::Lost(format!("starting the UI Automation thread: {error}"))
            })?;
        match tokio::time::timeout(CALL_LIMIT, ready).await {
            Ok(Ok(Ok(()))) => Ok(Self { jobs }),
            Ok(Ok(Err(error))) => Err(error),
            Ok(Err(_)) => Err(AxError::Lost(
                "the UI Automation thread ended before it started".to_owned(),
            )),
            Err(_) => Err(AxError::Timeout),
        }
    }

    /// Runs one job on the UI Automation thread and waits for its answer
    /// until `within` has passed.
    async fn run<T, F>(&self, within: Duration, job: F) -> Result<T, AxError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Uia) -> Result<T, AxError> + Send + 'static,
    {
        let (answer, answered) = oneshot::channel();
        self.jobs
            .send(Box::new(move |uia: &mut Uia| {
                let _ = answer.send(job(uia));
            }))
            .map_err(|_| AxError::Lost("the UI Automation thread has ended".to_owned()))?;
        match tokio::time::timeout(within, answered).await {
            Ok(Ok(answer)) => answer,
            Ok(Err(_)) => Err(AxError::Lost(
                "the UI Automation thread ended during a call".to_owned(),
            )),
            Err(_) => Err(AxError::Timeout),
        }
    }

    pub(crate) async fn application(
        &self,
        program: &str,
        name: &str,
        digest: Option<&str>,
    ) -> Result<App, AxError> {
        let (program, name, digest) = (
            program.to_owned(),
            name.to_owned(),
            digest.map(str::to_owned),
        );
        self.run(4 * CALL_LIMIT, move |uia| {
            uia.application(&program, &name, digest.as_deref())
        })
        .await
    }

    pub(crate) async fn windows(&self, app: &App) -> Result<Vec<Window>, AxError> {
        let app = app.clone();
        self.run(2 * CALL_LIMIT, move |uia| uia.windows(&app)).await
    }

    pub(crate) async fn tree(
        &self,
        root: &El,
        limits: &Limits,
        deadline: Deadline,
    ) -> Result<Tree, AxError> {
        let (root, limits) = (root.clone(), *limits);
        // The walk stops itself at the deadline; one call may run past it
        // by its own bound.
        let within = deadline.remaining() + CALL_LIMIT;
        self.run(within, move |uia| uia.tree(&root, limits, deadline))
            .await
    }

    pub(crate) async fn text(&self, node: &Node, limits: &Limits) -> Result<String, AxError> {
        if node.secret {
            return Err(AxError::Secret);
        }
        if !node.has_text {
            return Ok(node.name.clone());
        }
        let (el, limits) = (node.el.clone(), *limits);
        self.run(CALL_LIMIT, move |uia| uia.text(&el, limits)).await
    }

    /// Begins listening to the application's events, on a thread of the
    /// watch's own; returns once the hooks are set.
    pub(crate) async fn watch(&self, app: &App) -> Result<Watch, AxError> {
        let process = app.process;
        let (wake, events) = tokio::sync::mpsc::channel(1);
        let (said_ready, ready) = oneshot::channel();
        std::thread::Builder::new()
            .name("a2m-desktop-events".to_owned())
            .spawn(move || listen(process, wake, said_ready))
            .map_err(|error| AxError::Lost(format!("starting the event listener: {error}")))?;
        match tokio::time::timeout(CALL_LIMIT, ready).await {
            Ok(Ok(Ok(thread))) => Ok(Watch { events, thread }),
            Ok(Ok(Err(error))) => Err(error),
            Ok(Err(_)) => Err(AxError::Lost(
                "the event listener ended before it began".to_owned(),
            )),
            Err(_) => Err(AxError::Timeout),
        }
    }

    /// Ends the UI Automation thread once the jobs before this are done.
    pub(crate) async fn close(self) {
        drop(self.jobs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The members of UI Automation's interfaces and patterns that change
    /// an application, its windows, the focus, or what is shown.
    const ACTING: &[&str] = &[
        "Invoke",
        "SetValue",
        "Toggle",
        "Select",
        "AddToSelection",
        "RemoveFromSelection",
        "Expand",
        "Collapse",
        "SetFocus",
        "DoDefaultAction",
        "ScrollIntoView",
        "Scroll",
        "SetScrollPercent",
        "Realize",
        "SetWindowVisualState",
        "Close",
        "WaitForInputIdle",
        "Move",
        "Resize",
        "Rotate",
        "Zoom",
        "ZoomByUnit",
        "SetDockPosition",
        "SetView",
        "ShowContextMenu",
        "Navigate",
        "Activate",
        "SetSelection",
        "AddToSelection",
        "Select",
        "SetCurrentValue",
        "PutValue",
        "SetDefaultAction",
        "SendKeys",
        "SendInput",
        "PostMessageW",
        "SendMessageW",
        "SetForegroundWindow",
    ];

    #[test]
    fn the_back_end_calls_no_member_that_acts() {
        for member in READ_MEMBERS {
            assert!(!ACTING.contains(member), "{member}");
        }
        assert_eq!(READ_PATTERNS, &[UIA_ValuePatternId]);
        assert_eq!(READ_PROPERTIES, &[UIA_IsValuePatternAvailablePropertyId]);
    }

    /// This file up to its tests, which is all the code that runs.
    fn code() -> &'static str {
        let file = include_str!("windows.rs");
        let end = file.find("#[cfg(test)]\nmod tests").unwrap_or(file.len());
        &file[..end]
    }

    #[test]
    fn every_call_to_ui_automation_is_an_ask_that_names_it() {
        let code = code();
        // Where each `ask` and `ask_optional` begins and ends, and the
        // member it names.
        let mut spans = Vec::new();
        for opening in ["ask(\"", "ask_optional(\""] {
            let mut from = 0;
            while let Some(found) = code[from..].find(opening) {
                let start = from + found;
                let named = start + opening.len();
                let member = &code[named..named + code[named..].find('"').unwrap()];
                let end = start + code[start..].find("})").unwrap();
                spans.push((start, end, member));
                from = end;
            }
        }
        assert!(spans.len() >= 20, "{} asks found", spans.len());
        for (start, end, member) in &spans {
            assert!(
                READ_MEMBERS.contains(member),
                "`{member}` is asked and not on the list"
            );
            let call = &code[*start..*end];
            assert!(
                call.contains(&format!(".{member}(")) || call.contains(&format!(".{member}::<")),
                "the ask that names `{member}` calls something else: {call}"
            );
        }
        // Every method of a UI Automation object called anywhere is inside
        // one of them.
        let bytes = code.as_bytes();
        for (at, _) in code.match_indices('.') {
            let rest = &code[at + 1..];
            let method: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            let looks = ["Current", "Get", "Set", "Element", "Compare", "Control"]
                .iter()
                .any(|start| method.starts_with(start));
            let called =
                rest[method.len()..].starts_with('(') || rest[method.len()..].starts_with("::<");
            if !looks || !called || at == 0 || !bytes[at - 1].is_ascii_alphanumeric() {
                continue;
            }
            assert!(
                spans.iter().any(|(start, end, _)| *start < at && at < *end),
                "`.{method}(` is called outside an ask"
            );
        }
        // And no method that acts is called at all.
        for member in ACTING {
            assert!(
                !code.contains(&format!(".{member}(")),
                "`.{member}(` is called"
            );
        }
    }

    example_desk::stand_child!();

    /// The walk as text: depth, role, control type, name, and flags. Never
    /// a value read through the value pattern; the password edit's name
    /// is its label's.
    fn shown(tree: &Tree, types: &[i32]) -> String {
        let mut out = String::new();
        for (at, node) in tree.nodes.iter().enumerate() {
            out.push_str(&format!(
                "{}{} [{}] {:?}{}{}{}{}\n",
                "  ".repeat(node.depth as usize),
                node.role,
                types.get(at).copied().unwrap_or_default(),
                node.name,
                if node.showing { "" } else { " offscreen" },
                if node.secret { " secret" } else { "" },
                if node.has_text { " value" } else { "" },
                if node.labels { " labels" } else { "" },
            ));
        }
        out.push_str(&format!("cut: {:?}", tree.cut));
        out.chars().take(12_000).collect()
    }

    /// Against the stand's window: the proof of the application, the
    /// roles the table maps, and the walk, said in full when it fails.
    #[tokio::test]
    async fn the_stand_is_proven_and_walked_as_the_role_table_says() {
        let Some(session) = example_desk::Session::start().unwrap() else {
            eprintln!("skipped: this machine has no interactive desktop for the stand");
            return;
        };
        let desk = example_desk::Desk::start(&session, example_desk::Order::example())
            .await
            .unwrap();
        let program = session.program();
        let look = Look::connect(None).await.unwrap();
        // Another name, another digest: named stops, before anything is
        // asked of the window.
        assert_eq!(
            look.application(&program, "another-name", None).await,
            Err(AxError::NotRunning)
        );
        assert_eq!(
            look.application(&program, example_desk::APPLICATION, Some("sha256:00"))
                .await,
            Err(AxError::NotProven)
        );
        let digest = digest_of(Path::new(&program)).unwrap();
        let app = look
            .application(&program, example_desk::APPLICATION, Some(&digest))
            .await
            .unwrap();
        let windows = look.windows(&app).await.unwrap();
        let titles: Vec<(&str, &str)> = windows
            .iter()
            .map(|window| (window.role, window.title.as_str()))
            .collect();
        assert_eq!(titles, [("frame", example_desk::WINDOW)], "{titles:?}");
        let tree = look
            .tree(
                &windows[0].el,
                &Limits::default(),
                Deadline::after(Duration::from_secs(20)),
            )
            .await
            .unwrap();
        let types = look
            .run(CALL_LIMIT, |uia| Ok(uia.types.clone()))
            .await
            .unwrap();
        let said = shown(&tree, &types);
        let has = |role: &str, name: &str| {
            tree.nodes
                .iter()
                .filter(|node| node.role == role && node.name == name)
                .count()
        };
        assert_eq!(tree.cut, None, "{said}");
        for name in ["Order number", "Customer", "Status", "Tracking"] {
            assert_eq!(has("entry", name), 1, "entry {name}:\n{said}");
        }
        assert_eq!(has("entry", "Reference"), 2, "{said}");
        assert_eq!(has("entry", ""), 1, "the unnamed edit:\n{said}");
        assert_eq!(has("password_text", "Access code"), 1, "{said}");
        assert!(
            tree.nodes
                .iter()
                .all(|node| (node.role == "password_text") == node.secret),
            "{said}"
        );
        assert_eq!(has("table", "Line items"), 1, "{said}");
        let table = tree.nodes.iter().find(|node| node.role == "table").unwrap();
        let rows: Vec<Vec<&str>> = table
            .children
            .iter()
            .map(|row| {
                tree.nodes[*row]
                    .children
                    .iter()
                    .map(|cell| tree.nodes[*cell].role)
                    .collect()
            })
            .collect();
        assert_eq!(rows.len(), 4, "a row of headings and three lines:\n{said}");
        assert!(
            rows.iter().all(|row| row.len() == 3),
            "three cells a row: {rows:?}\n{said}"
        );
        // A value, through the value pattern; the password, never.
        let number = tree
            .nodes
            .iter()
            .find(|node| node.role == "entry" && node.name == "Order number")
            .unwrap();
        assert_eq!(
            look.text(number, &Limits::default()).await.unwrap(),
            "A-100"
        );
        let secret = tree.nodes.iter().find(|node| node.secret).unwrap();
        assert_eq!(
            look.text(secret, &Limits::default()).await,
            Err(AxError::Secret)
        );
        // A watch hears the late value.
        let mut watch = look.watch(&app).await.unwrap();
        desk.show_late(Duration::from_millis(100));
        assert!(
            watch
                .next(Deadline::after(Duration::from_secs(10)))
                .await
                .unwrap(),
            "no event from the application within ten seconds of its change"
        );
        drop(watch);
        look.close().await;
        let truth = desk.truth();
        assert_eq!(truth.writes, 0, "{:?}", truth.asked);
        assert!(!truth.acted(), "{:?}", truth.asked);
        assert_eq!(truth.secret_reads, 0, "{:?}", truth.asked);
    }

    #[test]
    fn every_role_word_is_one_the_pack_knows() {
        for number in 49_990..50_050 {
            for (password, in_row) in [(false, false), (true, false), (false, true)] {
                let word = role_word(number, password, in_row);
                assert!(roles::known(word) || word == roles::OTHER, "{number}");
            }
        }
        assert_eq!(role_word(EDIT, true, false), "password_text");
        assert_eq!(role_word(EDIT, false, false), "entry");
        assert_eq!(role_word(TEXT, false, true), "table_cell");
        assert_eq!(role_word(TEXT, false, false), "label");
        // Unknown means stop: a control type with no word is never a role
        // a locator can name.
        assert_eq!(role_word(50005, false, false), roles::OTHER);
        let mut numbers: Vec<i32> = CONTROL_TYPES.iter().map(|(number, _)| *number).collect();
        numbers.sort_unstable();
        numbers.dedup();
        assert_eq!(numbers.len(), CONTROL_TYPES.len());
    }
}
