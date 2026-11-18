//! The macOS back end: the system's accessibility interface (`AXUIElement`
//! and `AXObserver`), through the `objc2` project's bindings, read only
//! (decision 0244).
//!
//! **The permission.** macOS answers a program's accessibility calls only
//! when the person has allowed it under Privacy & Security, Accessibility,
//! in System Settings. Before anything else, the back end asks the system
//! whether this process is allowed (`AXIsProcessTrusted`, which never shows
//! the system's prompt). When it is not, every method stops with
//! `busy`/`accessibility_off`, naming where the person allows it; and when
//! a call answers that the interface is off for this process although the
//! check said yes, the stop is the same. Nothing here grants the
//! permission, asks the system to prompt, or writes the system's records.
//!
//! **Threading.** An accessibility element is a Core Foundation object
//! that this back end does not share between threads. Each [`Look`] owns
//! one thread of its own, which makes every element of a walk, keeps it,
//! and releases it, and runs the jobs the async methods send it over a
//! channel, one at a time. An element is known outside the thread only as
//! an opaque key in an [`El`]. An async method sends a job and waits for its
//! answer with a bound, so an application that never answers costs the step
//! its deadline and never blocks an async thread; the interface's own
//! messaging timeout, set for this process, bounds each call on the thread
//! to the same five seconds.
//!
//! **Events.** A wait listens through an observer of the application's
//! accessibility notifications, made on a thread of the watch's own,
//! whose source runs on that thread's run loop, and through the system's
//! word that the application's process exited, on a second thread that
//! waits on a kernel queue. The listener never looks at what a notification
//! says: it wakes the wait, which reads afresh. Neither thread polls.
//!
//! **Reads only.** Every call to the accessibility interface goes through
//! [`ask`], which makes only the calls of [`READ_CALLS`], and every
//! attribute is copied through [`copy`], which asks only for the attributes
//! of [`READ_ATTRIBUTES`]. None sets an attribute, performs an action, or
//! posts an event. A test holds both lists against the calls and the
//! attributes that act, and a second test reads this file and finds every
//! call into the interface inside an [`ask`] that names it. A secure text
//! field (`AXSecureTextField`) is reported as present and secret, and its
//! value is never asked for.

use std::collections::BTreeMap;
use std::ffi::{c_int, c_void};
use std::future::Future as _;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use a2m_provider::{Deadline, resolved_path};
use objc2_application_services::{AXError as Said, AXIsProcessTrusted, AXObserver, AXUIElement};
use objc2_core_foundation::{
    CFArray, CFRetained, CFRunLoop, CFString, CFType, kCFRunLoopDefaultMode,
};
use tokio::sync::oneshot;

use super::{App, AxError, Limits, Window};
use crate::roles;
use crate::tree::{El, Node, Tree};

/// The longest one call to the application may take, on the thread and
/// as the async side waits for it.
const CALL_LIMIT: Duration = Duration::from_secs(5);
/// The most processes one listing of the process table takes.
const MOST_PROCESSES: usize = 100_000;
/// The most comparisons one walk makes to find what labels what.
const MOST_COMPARISONS: usize = 20_000;
/// The number of window listings and walks whose elements are kept.
const KEPT: u64 = 4;

/// Every call into the accessibility interface this back end may make.
/// Each one reads, or sets up this client's own side: a check of this
/// process's permission that never prompts, an element for a process or
/// for the whole system (made without asking the application anything),
/// this client's own messaging timeout, the copy of one attribute's value,
/// and an observer of notifications with its run-loop source. None sets
/// an attribute, performs an action, or posts a key or a pointer event. A
/// test holds the list against the calls that act.
pub(crate) const READ_CALLS: &[&str] = &[
    "AXIsProcessTrusted",
    "AXUIElementCreateApplication",
    "AXUIElementCreateSystemWide",
    "AXUIElementSetMessagingTimeout",
    "AXUIElementCopyAttributeValue",
    "AXObserverCreate",
    "AXObserverAddNotification",
    "AXObserverGetRunLoopSource",
];

/// Every attribute [`copy`] may ask for. `AXValue` is asked of a text
/// field only when its subrole is not a secure text field's, of a static
/// text, which shows the text it is, and of a table cell's text.
pub(crate) const READ_ATTRIBUTES: &[&str] = &[
    "AXRole",
    "AXSubrole",
    "AXTitle",
    "AXDescription",
    "AXValue",
    "AXTitleUIElement",
    "AXChildren",
    "AXWindows",
    "AXRows",
    "AXColumnHeaderUIElements",
    "AXHeader",
];

/// The notifications a wait listens to, on the application's element and
/// on each of its windows: an element made, destroyed, changed in value or
/// title, a window made, moved between the windows' states, or focused,
/// a layout or a row count changed, and the application hidden or shown.
/// Not moves and resizes, which a drag sends many times a second.
pub(crate) const NOTIFICATIONS: &[&str] = &[
    "AXValueChanged",
    "AXTitleChanged",
    "AXCreated",
    "AXUIElementDestroyed",
    "AXWindowCreated",
    "AXWindowMiniaturized",
    "AXWindowDeminiaturized",
    "AXFocusedWindowChanged",
    "AXMainWindowChanged",
    "AXFocusedUIElementChanged",
    "AXLayoutChanged",
    "AXRowCountChanged",
    "AXSelectedChildrenChanged",
    "AXApplicationHidden",
    "AXApplicationShown",
];

/// The subrole of a text field that holds a secret.
const SECURE: &str = "AXSecureTextField";

/// The interface's roles, mapped to the pack's role words: the one table of
/// this system's roles. A role that is not here is [`roles::OTHER`], shown
/// in a snapshot and never matched by a locator. The refinements, in
/// [`role_word`]: a text field whose subrole is a secure text field's is
/// `password_text`; a window whose subrole is a dialog's is `dialog`; a
/// cell, a static text, or a text field directly under a `table_row` is a
/// `table_cell`; and a table's heading row, and each heading in it, are a
/// `table_row` of `table_column_header`s.
pub(crate) const ROLES: &[(&str, &str)] = &[
    ("AXApplication", "application"),
    ("AXWindow", "frame"),
    ("AXSheet", "dialog"),
    ("AXGroup", "panel"),
    ("AXSplitGroup", "panel"),
    ("AXRadioGroup", "panel"),
    ("AXScrollArea", "scroll_pane"),
    ("AXToolbar", "tool_bar"),
    ("AXMenuBar", "menu_bar"),
    ("AXMenu", "menu"),
    ("AXMenuItem", "menu_item"),
    ("AXMenuBarItem", "menu_item"),
    ("AXTabGroup", "page_tab_list"),
    ("AXHeading", "heading"),
    ("AXStaticText", "label"),
    ("AXImage", "image"),
    ("AXTextField", "entry"),
    ("AXTextArea", "text"),
    ("AXIncrementor", "spin_button"),
    ("AXComboBox", "combo_box"),
    ("AXPopUpButton", "combo_box"),
    ("AXCheckBox", "check_box"),
    ("AXRadioButton", "radio_button"),
    ("AXButton", "push_button"),
    ("AXList", "list"),
    ("AXOutline", "tree"),
    ("AXTable", "table"),
    ("AXRow", "table_row"),
    ("AXCell", "table_cell"),
];

/// The roles whose value is text a read may give.
const TEXT_ROLES: &[&str] = &["AXTextField", "AXTextArea", "AXComboBox"];

/// Where an element stands in a walk, which refines its role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    /// Anywhere but the places below.
    Plain,
    /// Directly under a table's row.
    InRow,
    /// A table's heading row.
    Heading,
    /// One heading in a table's heading row.
    InHeading,
}

/// The pack's word for an element of the role `role` and the subrole
/// `subrole`, at `place`.
pub(crate) fn role_word(role: &str, subrole: &str, place: Place) -> &'static str {
    match place {
        Place::Heading => return "table_row",
        Place::InHeading => return "table_column_header",
        Place::InRow if ["AXCell", "AXStaticText", "AXTextField"].contains(&role) => {
            if subrole == SECURE {
                return "password_text";
            }
            return "table_cell";
        }
        Place::InRow | Place::Plain => {}
    }
    if role == "AXTextField" && subrole == SECURE {
        return "password_text";
    }
    if role == "AXWindow" && ["AXDialog", "AXSystemDialog"].contains(&subrole) {
        return "dialog";
    }
    ROLES
        .iter()
        .find(|(known, _)| *known == role)
        .map_or(roles::OTHER, |(_, word)| word)
}

/// What a failed call says, with no value of the application in it.
fn said(call: &str, error: Said) -> AxError {
    if error == Said::APIDisabled {
        return AxError::NotTrusted;
    }
    if error == Said::InvalidUIElement {
        return AxError::Lost(format!("{call}: the element is no longer there"));
    }
    if error == Said::CannotComplete {
        return AxError::Lost(format!(
            "{call}: the application did not answer in time, or has ended"
        ));
    }
    AxError::Lost(format!("{call}: the interface answered error {}", error.0))
}

/// Makes one call into the accessibility interface, when `call` is on the
/// list of reads, and gives what it returned.
fn ask<T>(call: &'static str, make: impl FnOnce() -> T) -> Result<T, AxError> {
    if !READ_CALLS.contains(&call) {
        return Err(AxError::Lost(format!("`{call}` is not a read")));
    }
    Ok(make())
}

/// The value of one attribute of [`READ_ATTRIBUTES`], or `None` when the
/// element has no such attribute or no value for it.
#[allow(unsafe_code)] // one call into the accessibility interface, explained below
fn copy(
    element: &AXUIElement,
    attribute: &'static str,
) -> Result<Option<CFRetained<CFType>>, AxError> {
    if !READ_ATTRIBUTES.contains(&attribute) {
        return Err(AxError::Lost(format!("`{attribute}` is not read")));
    }
    let name = CFString::from_str(attribute);
    let mut value: *const CFType = std::ptr::null();
    // SAFETY: the element is a valid, retained element of this thread; the
    // name outlives the call; `value` is ours, and the call writes into it
    // one object it has retained for us, or nothing.
    let answer = ask("AXUIElementCopyAttributeValue", || unsafe {
        element.copy_attribute_value(&name, NonNull::from(&mut value))
    })?;
    if answer == Said::Success {
        // SAFETY: on success the value is an object the caller owns (the
        // "copy" rule), or null.
        return Ok(
            NonNull::new(value.cast_mut()).map(|value| unsafe { CFRetained::from_raw(value) })
        );
    }
    if [
        Said::AttributeUnsupported,
        Said::NoValue,
        Said::NotImplemented,
    ]
    .contains(&answer)
    {
        return Ok(None);
    }
    Err(said(attribute, answer))
}

/// An attribute's value as text, when it is text.
fn text_of(element: &AXUIElement, attribute: &'static str) -> Result<Option<String>, AxError> {
    Ok(copy(element, attribute)?
        .and_then(|value| value.downcast::<CFString>().ok())
        .map(|text| text.to_string()))
}

/// An attribute's value as text, or nothing when it is empty.
fn named(element: &AXUIElement, attribute: &'static str) -> Result<Option<String>, AxError> {
    Ok(text_of(element, attribute)?.filter(|text| !text.is_empty()))
}

/// An attribute's value as one element, when it is one.
fn element_of(
    element: &AXUIElement,
    attribute: &'static str,
) -> Result<Option<CFRetained<AXUIElement>>, AxError> {
    Ok(copy(element, attribute)?.and_then(|value| value.downcast::<AXUIElement>().ok()))
}

/// An attribute's value as elements, at most `most`; anything in the array
/// that is not an element is passed over.
#[allow(unsafe_code)] // one conversion, explained below
fn elements_of(
    element: &AXUIElement,
    attribute: &'static str,
    most: usize,
) -> Result<Vec<CFRetained<AXUIElement>>, AxError> {
    let Some(array) = copy(element, attribute)?.and_then(|value| value.downcast::<CFArray>().ok())
    else {
        return Ok(Vec::new());
    };
    // SAFETY: every member of an array the accessibility interface gives
    // is a Core Foundation object, which is what `CFType` stands for; each
    // is then checked to be an element before it is used as one.
    let array = unsafe { CFRetained::cast_unchecked::<CFArray<CFType>>(array) };
    Ok((0..array.len().min(most))
        .filter_map(|at| array.get(at))
        .filter_map(|member| member.downcast::<AXUIElement>().ok())
        .collect())
}

/// What the process table says of one process: its effective user and its
/// start time, in seconds and microseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Facts {
    user: u32,
    started: (u64, u64),
}

/// Every process the system lists, by number.
#[allow(unsafe_code)] // two calls into the system, explained below
fn processes() -> Vec<i32> {
    // SAFETY: with no buffer the call writes nothing and says how many
    // processes there are.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    let room = usize::try_from(count)
        .unwrap_or(0)
        .saturating_add(64)
        .min(MOST_PROCESSES);
    let mut listed: Vec<libc::pid_t> = vec![0; room];
    let bytes = c_int::try_from(room * size_of::<libc::pid_t>()).unwrap_or(c_int::MAX);
    // SAFETY: `listed` is writable for `bytes` bytes and outlives the call,
    // which writes at most that many and says how many processes it wrote.
    let got = unsafe { libc::proc_listallpids(listed.as_mut_ptr().cast::<c_void>(), bytes) };
    listed.truncate(usize::try_from(got).unwrap_or(0).min(room));
    listed.retain(|process| *process > 0);
    listed
}

/// The program a process runs, as the system reports it.
#[allow(unsafe_code)] // one call into the system, explained below
fn program_of(process: i32) -> Option<PathBuf> {
    let mut buffer = vec![0_u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let room = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
    // SAFETY: `buffer` is writable for `room` bytes and outlives the call,
    // which writes the path and says how long it is.
    let length = unsafe { libc::proc_pidpath(process, buffer.as_mut_ptr().cast::<c_void>(), room) };
    let used = buffer.get(..usize::try_from(length).ok().filter(|length| *length > 0)?)?;
    Some(PathBuf::from(String::from_utf8(used.to_vec()).ok()?))
}

/// The user and the start time of a process, from the process table.
#[allow(unsafe_code)] // one call into the system and a zeroed value, explained below
fn facts_of(process: i32) -> Option<Facts> {
    // SAFETY: the structure is plain integers and arrays of them, for which
    // all zeroes is a value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = c_int::try_from(size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: `info` is ours and as large as `size` says; the call writes
    // at most that and says how much it wrote.
    let wrote = unsafe {
        libc::proc_pidinfo(
            process,
            libc::PROC_PIDTBSDINFO,
            0,
            (&raw mut info).cast::<c_void>(),
            size,
        )
    };
    (wrote == size).then_some(Facts {
        user: info.pbi_uid,
        started: (info.pbi_start_tvsec, info.pbi_start_tvusec),
    })
}

/// The effective user this process runs as.
#[allow(unsafe_code)] // one call into the system, explained below
fn my_user() -> u32 {
    // SAFETY: no argument; the call cannot fail.
    unsafe { libc::geteuid() }
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
/// path the system gave for the process's program, taken only when the
/// system still gives that path once the file is read. The system's path
/// follows the program's file, so a file moved away and another put in its
/// place while it was read is not taken for the running one.
fn running_digest(process: i32, want: &Path) -> Option<String> {
    let digest = digest_of(want)?;
    program_of(process)
        .and_then(|running| resolved_path(&running).ok())
        .is_some_and(|running| running == want)
        .then_some(digest)
}

/// The process an element belongs to, from its key.
fn process_of(el: &El) -> Result<i32, AxError> {
    el.bus
        .parse::<i32>()
        .map_err(|_| AxError::Lost("an element this back end did not give".to_owned()))
}

/// An application proven before it was asked anything: what proved it,
/// proved again before each later question, and its element.
struct Proven {
    program: PathBuf,
    started: (u64, u64),
    element: CFRetained<AXUIElement>,
}

/// What the accessibility thread holds: every element of one [`Look`].
struct Ax {
    user: u32,
    proven: BTreeMap<i32, Proven>,
    /// The windows of the last listings, by listing.
    listings: BTreeMap<u64, Vec<CFRetained<AXUIElement>>>,
    /// The elements of the last walks, by walk.
    walks: BTreeMap<u64, Vec<CFRetained<AXUIElement>>>,
    next: u64,
    /// The role and the subrole of each element of the last walk, for a
    /// test that must say what it found.
    #[cfg(test)]
    seen: Vec<String>,
}

type Job = Box<dyn FnOnce(&mut Ax) + Send>;

/// What one element said of itself in a walk.
struct Answered {
    node: Node,
    /// What labels it, when its name is its label's.
    label: Option<CFRetained<AXUIElement>>,
    role: String,
    subrole: String,
}

/// One step of a walk: an element and where it stands, or a table's
/// heading row, made of the table's own list of its headings.
enum Item {
    Element(CFRetained<AXUIElement>, Place),
    Headings(CFRetained<AXUIElement>, Vec<CFRetained<AXUIElement>>),
}

impl Ax {
    #[allow(unsafe_code)] // calls into the accessibility interface, explained below
    fn start() -> Result<Self, AxError> {
        // SAFETY: no argument; the check never shows the system's prompt.
        if !ask("AXIsProcessTrusted", || unsafe { AXIsProcessTrusted() })? {
            return Err(AxError::NotTrusted);
        }
        // SAFETY: makes this client's element for the whole system, which
        // asks no application anything.
        let system = ask("AXUIElementCreateSystemWide", || unsafe {
            AXUIElement::new_system_wide()
        })?;
        let limit = CALL_LIMIT.as_secs_f32();
        // SAFETY: on the system-wide element, the timeout is this client's
        // own, for every element it asks; it reaches no application.
        let set = ask("AXUIElementSetMessagingTimeout", || unsafe {
            system.set_messaging_timeout(limit)
        })?;
        if set != Said::Success {
            return Err(said("AXUIElementSetMessagingTimeout", set));
        }
        Ok(Self {
            user: my_user(),
            proven: BTreeMap::new(),
            listings: BTreeMap::new(),
            walks: BTreeMap::new(),
            next: 0,
            #[cfg(test)]
            seen: Vec::new(),
        })
    }

    /// The one running application that is `program`, run by this
    /// process's user, with the name `name`, and with its digest checked
    /// when `digest` is stated. Only the process table is asked about the
    /// processes that are not it, and the application is asked nothing
    /// until it is proven.
    #[allow(unsafe_code)] // one call into the accessibility interface, explained below
    fn application(
        &mut self,
        program: &str,
        name: &str,
        digest: Option<&str>,
    ) -> Result<App, AxError> {
        let Ok(want) = resolved_path(Path::new(program)) else {
            return Err(AxError::NotRunning);
        };
        // Its name: the name of its program's file. The application's own
        // title is not asked before the process is proven.
        let own_name = want.file_name().and_then(|own| own.to_str());
        let mut found = Vec::new();
        for process in processes() {
            let runs = program_of(process)
                .and_then(|running| resolved_path(&running).ok())
                .is_some_and(|running| running == want);
            if !runs {
                continue;
            }
            // The program is the declared one. Before anything is asked of
            // the application: its user and its start time.
            let Some(facts) = facts_of(process) else {
                continue;
            };
            if facts.user != self.user {
                return Err(AxError::NotOwned);
            }
            if let Some(stated) = digest {
                let stated = stated.strip_prefix("sha256:").unwrap_or(stated);
                if running_digest(process, &want).as_deref() != Some(stated) {
                    return Err(AxError::NotProven);
                }
            }
            if own_name == Some(name) {
                found.push((process, facts));
            }
        }
        let (process, facts) = match found.as_slice() {
            [] => return Err(AxError::NotRunning),
            [one] => *one,
            many => return Err(AxError::Ambiguous(many.len())),
        };
        // SAFETY: makes this client's element for the proven process; the
        // call asks the application nothing.
        let element = ask("AXUIElementCreateApplication", || unsafe {
            AXUIElement::new_application(process)
        })?;
        // One real read: the interface may say a process is allowed and
        // still refuse it, which this names.
        copy(&element, "AXRole")?;
        self.proven.insert(
            process,
            Proven {
                program: want,
                started: facts.started,
                element,
            },
        );
        let process = u32::try_from(process)
            .map_err(|_| AxError::Lost("a process number out of range".to_owned()))?;
        Ok(App {
            root: El {
                bus: process.to_string(),
                path: "application".to_owned(),
            },
            process,
        })
    }

    /// The proven application of `process`, proved again: the same start
    /// time and the same program, so a process that ended and whose number
    /// went to another is never asked.
    fn again(&self, process: i32) -> Result<&Proven, AxError> {
        let proven = self.proven.get(&process).ok_or_else(|| {
            AxError::Lost("an application this back end did not prove".to_owned())
        })?;
        let same = facts_of(process).is_some_and(|facts| facts.started == proven.started)
            && program_of(process)
                .and_then(|running| resolved_path(&running).ok())
                .is_some_and(|running| running == proven.program);
        if same {
            Ok(proven)
        } else {
            Err(AxError::Lost(
                "the application's process has ended, or is no longer the one proven".to_owned(),
            ))
        }
    }

    /// The element a key names, proved again to be the application's.
    fn element(&self, el: &El) -> Result<CFRetained<AXUIElement>, AxError> {
        let proven = self.again(process_of(el)?)?;
        if el.path == "application" {
            return Ok(proven.element.clone());
        }
        let kept = |store: &BTreeMap<u64, Vec<CFRetained<AXUIElement>>>, rest: &str| {
            let (which, at) = rest.split_once('.')?;
            let which = which.parse::<u64>().ok()?;
            let at = at.parse::<usize>().ok()?;
            store.get(&which)?.get(at).cloned()
        };
        let found = if let Some(rest) = el.path.strip_prefix("window:") {
            kept(&self.listings, rest)
        } else if let Some(rest) = el.path.strip_prefix("walk:") {
            kept(&self.walks, rest)
        } else {
            None
        };
        found.ok_or_else(|| {
            AxError::Lost("the element is from a read this back end no longer holds".to_owned())
        })
    }

    /// The application's windows.
    fn windows(&mut self, app: &App) -> Result<Vec<Window>, AxError> {
        let element = self.element(&app.root)?;
        let windows = elements_of(&element, "AXWindows", 1_000)?;
        let listing = self.next;
        self.next += 1;
        let mut out = Vec::new();
        for (at, window) in windows.iter().enumerate() {
            let role = text_of(window, "AXRole")?.unwrap_or_default();
            let subrole = text_of(window, "AXSubrole")?.unwrap_or_default();
            let title = text_of(window, "AXTitle")?.unwrap_or_default();
            out.push(Window {
                el: El {
                    bus: app.root.bus.clone(),
                    path: format!("window:{listing}.{at}"),
                },
                role: role_word(&role, &subrole, Place::Plain),
                title,
            });
        }
        self.listings.insert(listing, windows);
        self.listings.retain(|known, _| known + KEPT > listing);
        Ok(out)
    }

    /// The name of what labels an element: its title, else the text it
    /// shows when it is a static text.
    fn label_name(label: &AXUIElement) -> Result<String, AxError> {
        if let Some(title) = named(label, "AXTitle")? {
            return Ok(title);
        }
        let role = text_of(label, "AXRole")?.unwrap_or_default();
        if role == "AXStaticText" {
            return Ok(text_of(label, "AXValue")?.unwrap_or_default());
        }
        Ok(named(label, "AXDescription")?.unwrap_or_default())
    }

    /// Everything one element says of itself but its value, what labels
    /// it when anything does, and its role and subrole.
    fn node(element: &AXUIElement, depth: u32, place: Place) -> Result<Answered, AxError> {
        let role = text_of(element, "AXRole")?.unwrap_or_default();
        let subrole = text_of(element, "AXSubrole")?.unwrap_or_default();
        let secret = subrole == SECURE;
        let word = role_word(&role, &subrole, place);
        let mut label = None;
        let name = if let Some(title) = named(element, "AXTitle")? {
            title
        } else if let Some(description) = named(element, "AXDescription")? {
            description
        } else if let Some(labelling) = element_of(element, "AXTitleUIElement")? {
            // A screen reader's name: what labels it.
            let name = Self::label_name(&labelling)?;
            label = Some(labelling);
            name
        } else if role == "AXStaticText" || place == Place::InHeading {
            // A static text's name is the text it shows, as a label's is.
            text_of(element, "AXValue")?.unwrap_or_default()
        } else if role == "AXCell" {
            // A cell's name is the text of the static text it holds.
            Self::cell_text(element)?
        } else {
            String::new()
        };
        let node = Node {
            depth,
            role: word,
            name,
            // An element hidden by its application is left out of the
            // tree by the interface; what is listed is what is shown.
            showing: true,
            secret,
            has_text: TEXT_ROLES.contains(&role.as_str()) && !secret,
            labels: false,
            children: Vec::new(),
            el: El {
                bus: String::new(),
                path: String::new(),
            },
        };
        Ok(Answered {
            node,
            label,
            role,
            subrole,
        })
    }

    /// The text of the first static text or text field directly in a cell,
    /// never of one that holds a secret.
    fn cell_text(cell: &AXUIElement) -> Result<String, AxError> {
        for inner in elements_of(cell, "AXChildren", 16)? {
            let role = text_of(&inner, "AXRole")?.unwrap_or_default();
            let subrole = text_of(&inner, "AXSubrole")?.unwrap_or_default();
            if ["AXStaticText", "AXTextField"].contains(&role.as_str()) && subrole != SECURE {
                return Ok(text_of(&inner, "AXValue")?.unwrap_or_default());
            }
        }
        Ok(String::new())
    }

    /// The items directly under an element of `role` at `place`, at most
    /// `most`. A table's are its heading row, then its rows; its columns,
    /// which hold the same cells again, are not walked.
    fn children(
        element: &CFRetained<AXUIElement>,
        role: &str,
        place: Place,
        most: usize,
    ) -> Result<Vec<Item>, AxError> {
        if place == Place::Heading {
            return Ok(elements_of(element, "AXChildren", most)?
                .into_iter()
                .map(|heading| Item::Element(heading, Place::InHeading))
                .collect());
        }
        if role == "AXTable" {
            let mut out = Vec::new();
            let headings = elements_of(element, "AXColumnHeaderUIElements", most)?;
            if !headings.is_empty() {
                out.push(Item::Headings(element.clone(), headings));
            } else if let Some(header) = element_of(element, "AXHeader")? {
                out.push(Item::Element(header, Place::Heading));
            }
            out.extend(
                elements_of(element, "AXRows", most)?
                    .into_iter()
                    .map(|row| Item::Element(row, Place::Plain)),
            );
            return Ok(out);
        }
        let below = if role == "AXRow" {
            Place::InRow
        } else {
            Place::Plain
        };
        Ok(elements_of(element, "AXChildren", most)?
            .into_iter()
            .map(|child| Item::Element(child, below))
            .collect())
    }

    /// Walks the tree under `root`, each element before those under it,
    /// within the bounds and the deadline.
    fn tree(&mut self, root: &El, limits: Limits, deadline: Deadline) -> Result<Tree, AxError> {
        let first = self.element(root)?;
        let walk = self.next;
        self.next += 1;
        let mut tree = Tree::default();
        let mut kept: Vec<CFRetained<AXUIElement>> = Vec::new();
        let mut labelling: Vec<CFRetained<AXUIElement>> = Vec::new();
        let mut stack: Vec<(Item, u32, Option<usize>)> =
            vec![(Item::Element(first, Place::Plain), 0, None)];
        #[cfg(test)]
        self.seen.clear();
        while let Some((item, depth, parent)) = stack.pop() {
            if tree.nodes.len() >= limits.nodes {
                tree.cut = Some("nodes");
                break;
            }
            if deadline.passed() {
                tree.cut = Some("seconds");
                break;
            }
            let (node, element, children) = match item {
                Item::Element(element, place) => {
                    let Answered {
                        node,
                        label,
                        role,
                        subrole,
                    } = Self::node(&element, depth, place)?;
                    // The test says, for each element, whether it named
                    // an element as what labels it.
                    #[cfg(test)]
                    self.seen.push(format!(
                        "{role}/{subrole}{}",
                        if label.is_some() { " titled" } else { "" }
                    ));
                    #[cfg(not(test))]
                    let _ = subrole;
                    labelling.extend(label);
                    let children = Self::children(&element, &role, place, limits.nodes)?;
                    (node, element, children)
                }
                Item::Headings(table, headings) => {
                    #[cfg(test)]
                    self.seen.push("(the table's headings)".to_owned());
                    let node = Node {
                        depth,
                        role: "table_row",
                        name: String::new(),
                        showing: true,
                        secret: false,
                        has_text: false,
                        labels: false,
                        children: Vec::new(),
                        el: El {
                            bus: String::new(),
                            path: String::new(),
                        },
                    };
                    let children = headings
                        .into_iter()
                        .map(|heading| Item::Element(heading, Place::InHeading))
                        .collect();
                    (node, table, children)
                }
            };
            let at = tree.nodes.len();
            let mut node = node;
            node.el = El {
                bus: root.bus.clone(),
                path: format!("walk:{walk}.{at}"),
            };
            tree.nodes.push(node);
            if let Some(parent) = parent {
                tree.nodes[parent].children.push(at);
            }
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
        // What labels something: a static text that an element of the walk
        // names as its title element. Elements are compared as the system
        // compares them, which asks no application.
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
                let theirs: &CFType = label;
                let mine: &CFType = &kept[at];
                if theirs == mine {
                    node.labels = true;
                    break;
                }
            }
        }
        self.walks.insert(walk, kept);
        self.walks.retain(|known, _| known + KEPT > walk);
        Ok(tree)
    }

    /// The text a field shows, at most `limits.chars` characters. A secure
    /// text field is never asked for its value: its subrole is read again
    /// first.
    fn text(&self, el: &El, limits: Limits) -> Result<String, AxError> {
        let element = self.element(el)?;
        if text_of(&element, "AXSubrole")?.as_deref() == Some(SECURE) {
            return Err(AxError::Secret);
        }
        let shown = match copy(&element, "AXValue")? {
            None => String::new(),
            Some(value) => match value.downcast::<CFString>() {
                Ok(text) => text.to_string(),
                Err(_) => {
                    return Err(AxError::Lost("the field's value is not text".to_owned()));
                }
            },
        };
        if shown.chars().count() > limits.chars {
            return Err(AxError::TooLarge);
        }
        Ok(shown)
    }
}

/// The accessibility thread: makes its state, says whether it could, then
/// runs jobs until the handle is gone, and releases every element on this
/// thread when it ends.
fn serve(ready: oneshot::Sender<Result<(), AxError>>, jobs: mpsc::Receiver<Job>) {
    match Ax::start() {
        Ok(mut ax) => {
            let _ = ready.send(Ok(()));
            while let Ok(job) = jobs.recv() {
                job(&mut ax);
            }
            drop(ax);
        }
        Err(error) => {
            let _ = ready.send(Err(error));
        }
    }
}

/// The look-only handle: a thread of its own that holds every element.
pub(crate) struct Look {
    jobs: mpsc::Sender<Job>,
}

/// A run loop of a listener's thread, kept to stop it from the watch.
struct Stopper(CFRetained<CFRunLoop>);

// SAFETY: Core Foundation's run loop functions may be called from any
// thread: the platform's threading guide says a run loop object is
// thread-safe, and stopping one from another thread is what
// `CFRunLoopStop` is for. The value is only stopped, woken, and released
// from the watch's side; everything else of the run loop stays on the
// listener's thread. Retaining and releasing a Core Foundation object is
// thread-safe.
#[allow(unsafe_code)]
unsafe impl Send for Stopper {}

/// The application's notifications, and its exit, from the moment the
/// watch began.
pub(crate) struct Watch {
    changes: tokio::sync::mpsc::Receiver<()>,
    /// The word that the application exited; `None` once its listener
    /// ended without it.
    exited: Option<oneshot::Receiver<()>>,
    run_loop: Stopper,
    queue: Arc<OwnedFd>,
}

impl Watch {
    /// Waits for the application's next notification. `false` when the
    /// deadline passed first; the application's exit is an error.
    pub(crate) async fn next(&mut self, deadline: Deadline) -> Result<bool, AxError> {
        let (changes, exited) = (&mut self.changes, &mut self.exited);
        let heard = std::future::poll_fn(|cx| {
            if let Some(waiting) = exited.as_mut()
                && let std::task::Poll::Ready(said) = std::pin::Pin::new(waiting).poll(cx)
            {
                if said.is_ok() {
                    return std::task::Poll::Ready(None);
                }
                // The listener ended without the word: it can say nothing
                // more, and is not asked again.
                *exited = None;
            }
            changes.poll_recv(cx).map(|change| change.map(|()| true))
        });
        match tokio::time::timeout_at(deadline.at(), heard).await {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Err(AxError::Lost(
                "the application exited, or its listener ended".to_owned(),
            )),
            Err(_) => Ok(false),
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.run_loop.0.stop();
        self.run_loop.0.wake_up();
        trigger(&self.queue);
    }
}

/// What the observer's callback is given: the way to wake its watch.
struct Wake(tokio::sync::mpsc::Sender<()>);

/// Called by the system, on the listener's thread, for each notification
/// of the application. It reads nothing of the notification.
#[allow(unsafe_code)] // one read through the pointer the listener gave, explained below
unsafe extern "C-unwind" fn woke(
    _observer: NonNull<AXObserver>,
    _element: NonNull<AXUIElement>,
    _notification: NonNull<CFString>,
    wake: *mut c_void,
) {
    // SAFETY: `listen` passes a pointer to its own `Wake`, which outlives
    // the observer: the observer is released before the `Wake` is dropped,
    // and both on the thread this is called on.
    if let Some(wake) = unsafe { wake.cast::<Wake>().as_ref() } {
        // A wake already waiting is enough: the wait reads afresh.
        let _ = wake.0.try_send(());
    }
}

/// The listener thread: an observer of one application's notifications on
/// this thread's run loop, which it runs until the watch stops it.
#[allow(unsafe_code)] // calls into the accessibility interface and Core Foundation, explained below
fn listen(
    process: i32,
    wake: tokio::sync::mpsc::Sender<()>,
    ready: oneshot::Sender<Result<Stopper, AxError>>,
) {
    let wake = Box::new(Wake(wake));
    let refcon: *mut c_void = std::ptr::from_ref::<Wake>(&wake).cast_mut().cast();
    let made = (|| {
        // SAFETY: makes this thread's own element for the process, which
        // asks the application nothing.
        let application = ask("AXUIElementCreateApplication", || unsafe {
            AXUIElement::new_application(process)
        })?;
        let mut raw: *mut AXObserver = std::ptr::null_mut();
        // SAFETY: `raw` is ours; on success the call writes an observer we
        // own (the "create" rule) whose callback is `woke`.
        let answer = ask("AXObserverCreate", || unsafe {
            AXObserver::create(process, Some(woke), NonNull::from(&mut raw))
        })?;
        if answer != Said::Success {
            return Err(said("AXObserverCreate", answer));
        }
        let observer = NonNull::new(raw)
            .map(|raw| unsafe { CFRetained::from_raw(raw) })
            .ok_or_else(|| AxError::Lost("AXObserverCreate gave no observer".to_owned()))?;
        // The application's element, and each of its windows, so that a
        // notification is heard however the system sends it.
        let mut watched = vec![application.clone()];
        watched.extend(elements_of(&application, "AXWindows", 1_000)?);
        let mut added = 0_usize;
        for element in &watched {
            for notification in NOTIFICATIONS {
                let name = CFString::from_str(notification);
                // SAFETY: the observer and the element are this thread's;
                // the name outlives the call; `refcon` outlives the
                // observer, as `woke` says.
                let answer = ask("AXObserverAddNotification", || unsafe {
                    observer.add_notification(element, &name, refcon)
                })?;
                if answer == Said::Success || answer == Said::NotificationAlreadyRegistered {
                    added += 1;
                } else if answer == Said::APIDisabled {
                    return Err(AxError::NotTrusted);
                }
            }
        }
        if added == 0 {
            return Err(AxError::Lost(
                "the application takes none of the notifications a wait listens to".to_owned(),
            ));
        }
        // SAFETY: the observer is this thread's; the source it gives is
        // retained for us.
        let source = ask("AXObserverGetRunLoopSource", || unsafe {
            observer.run_loop_source()
        })?;
        let run_loop = CFRunLoop::current()
            .ok_or_else(|| AxError::Lost("this thread has no run loop".to_owned()))?;
        // SAFETY: the default mode is the system's own constant.
        let mode = unsafe { kCFRunLoopDefaultMode };
        run_loop.add_source(Some(&source), mode);
        Ok((observer, source, run_loop, mode))
    })();
    match made {
        Ok((observer, source, run_loop, mode)) => {
            if ready.send(Ok(Stopper(run_loop.clone()))).is_ok() {
                // Runs until the watch stops it. A stop that came before
                // the run began is kept by the run loop, which then returns
                // at once.
                CFRunLoop::run();
            }
            run_loop.remove_source(Some(&source), mode);
            drop(source);
            drop(observer);
        }
        Err(error) => {
            let _ = ready.send(Err(error));
        }
    }
    drop(wake);
}

/// The identifier of the event that ends the exit listener.
const STOP_EVENT: usize = 1;

/// Wakes the exit listener with its stop event.
#[allow(unsafe_code)] // one call into the system, explained below
fn trigger(queue: &OwnedFd) {
    // SAFETY: a zeroed event is a value of plain fields; it is then filled.
    let mut event: libc::kevent = unsafe { std::mem::zeroed() };
    event.ident = STOP_EVENT;
    event.filter = libc::EVFILT_USER;
    event.fflags = libc::NOTE_TRIGGER;
    // SAFETY: the queue is open while this holds it; the call reads one
    // event of ours and writes none.
    let _ = unsafe {
        libc::kevent(
            queue.as_raw_fd(),
            &raw const event,
            1,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    };
}

/// A kernel queue that reports the exit of `process` and the watch's stop.
#[allow(unsafe_code)] // calls into the system, explained below
fn exit_queue(process: i32) -> Result<(Arc<OwnedFd>, bool), AxError> {
    // SAFETY: no argument; a new descriptor or -1.
    let raw = unsafe { libc::kqueue() };
    if raw < 0 {
        return Err(AxError::Lost(format!(
            "kqueue: {}",
            std::io::Error::last_os_error()
        )));
    }
    // SAFETY: `raw` is a descriptor the call just made for us alone.
    let queue = Arc::new(unsafe { OwnedFd::from_raw_fd(raw) });
    // SAFETY: zeroed events are values of plain fields; they are then
    // filled.
    let mut events: [libc::kevent; 2] = unsafe { std::mem::zeroed() };
    events[0].ident = usize::try_from(process).unwrap_or(0);
    events[0].filter = libc::EVFILT_PROC;
    events[0].flags = libc::EV_ADD | libc::EV_ONESHOT;
    events[0].fflags = libc::NOTE_EXIT;
    events[1].ident = STOP_EVENT;
    events[1].filter = libc::EVFILT_USER;
    events[1].flags = libc::EV_ADD | libc::EV_CLEAR;
    let mut answers: [libc::kevent; 2] = unsafe { std::mem::zeroed() };
    for (event, answer) in events.iter().zip(answers.iter_mut()) {
        let mut event = *event;
        event.flags |= libc::EV_RECEIPT;
        // SAFETY: the queue is ours; the call reads one event and writes
        // its receipt into `answer`, which is ours.
        let got = unsafe {
            libc::kevent(
                queue.as_raw_fd(),
                &raw const event,
                1,
                answer,
                1,
                std::ptr::null(),
            )
        };
        if got < 0 {
            return Err(AxError::Lost(format!(
                "kevent: {}",
                std::io::Error::last_os_error()
            )));
        }
    }
    // A process that ended before its exit could be listened to says so in
    // its receipt.
    let gone = answers[0].flags & libc::EV_ERROR != 0 && answers[0].data == libc::ESRCH as isize;
    Ok((queue, gone))
}

/// The exit listener's thread: waits on the queue until the process exits
/// or the watch stops it; says which by the oneshot.
#[allow(unsafe_code)] // one call into the system, explained below
fn await_exit(queue: Arc<OwnedFd>, exited: oneshot::Sender<()>) {
    loop {
        // SAFETY: a zeroed event is a value of plain fields.
        let mut event: libc::kevent = unsafe { std::mem::zeroed() };
        // SAFETY: the queue is open while this holds it; the call writes at
        // most one event into `event`, which is ours, and waits with no
        // timeout, on this thread of its own.
        let got = unsafe {
            libc::kevent(
                queue.as_raw_fd(),
                std::ptr::null(),
                0,
                &raw mut event,
                1,
                std::ptr::null(),
            )
        };
        if got < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        if got == 1 && event.filter == libc::EVFILT_PROC {
            let _ = exited.send(());
        }
        return;
    }
}

impl Look {
    /// Starts the accessibility thread, after the system says this process
    /// may use the interface. The interface reads the session the engine
    /// runs in; it has no bus, so `session` is not used.
    pub(crate) async fn connect(_session: Option<&str>) -> Result<Self, AxError> {
        let (jobs, taken) = mpsc::channel::<Job>();
        let (said_ready, ready) = oneshot::channel();
        std::thread::Builder::new()
            .name("a2m-desktop-ax".to_owned())
            .spawn(move || serve(said_ready, taken))
            .map_err(|error| {
                AxError::Lost(format!("starting the accessibility thread: {error}"))
            })?;
        match tokio::time::timeout(CALL_LIMIT, ready).await {
            Ok(Ok(Ok(()))) => Ok(Self { jobs }),
            Ok(Ok(Err(error))) => Err(error),
            Ok(Err(_)) => Err(AxError::Lost(
                "the accessibility thread ended before it started".to_owned(),
            )),
            Err(_) => Err(AxError::Timeout),
        }
    }

    /// Runs one job on the accessibility thread and waits for its answer
    /// until `within` has passed.
    async fn run<T, F>(&self, within: Duration, job: F) -> Result<T, AxError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Ax) -> Result<T, AxError> + Send + 'static,
    {
        let (answer, answered) = oneshot::channel();
        self.jobs
            .send(Box::new(move |ax: &mut Ax| {
                let _ = answer.send(job(ax));
            }))
            .map_err(|_| AxError::Lost("the accessibility thread has ended".to_owned()))?;
        match tokio::time::timeout(within, answered).await {
            Ok(Ok(answer)) => answer,
            Ok(Err(_)) => Err(AxError::Lost(
                "the accessibility thread ended during a call".to_owned(),
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
        self.run(4 * CALL_LIMIT, move |ax| {
            ax.application(&program, &name, digest.as_deref())
        })
        .await
    }

    pub(crate) async fn windows(&self, app: &App) -> Result<Vec<Window>, AxError> {
        let app = app.clone();
        self.run(2 * CALL_LIMIT, move |ax| ax.windows(&app)).await
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
        self.run(within, move |ax| ax.tree(&root, limits, deadline))
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
        self.run(CALL_LIMIT, move |ax| ax.text(&el, limits)).await
    }

    /// Begins listening to the application's notifications and its exit,
    /// each on a thread of the watch's own; returns once both listen.
    pub(crate) async fn watch(&self, app: &App) -> Result<Watch, AxError> {
        let process = i32::try_from(app.process)
            .map_err(|_| AxError::Lost("a process number out of range".to_owned()))?;
        // The process must still be the one proven before it is listened
        // to.
        let root = app.root.clone();
        self.run(CALL_LIMIT, move |ax| ax.element(&root).map(|_| ()))
            .await?;
        let (queue, gone) = exit_queue(process)?;
        let (said_exit, exited) = oneshot::channel();
        if gone {
            let _ = said_exit.send(());
        } else {
            let waiting = Arc::clone(&queue);
            std::thread::Builder::new()
                .name("a2m-desktop-exit".to_owned())
                .spawn(move || await_exit(waiting, said_exit))
                .map_err(|error| AxError::Lost(format!("starting the exit listener: {error}")))?;
        }
        let (wake, changes) = tokio::sync::mpsc::channel(1);
        let (said_ready, ready) = oneshot::channel();
        let spawned = std::thread::Builder::new()
            .name("a2m-desktop-events".to_owned())
            .spawn(move || listen(process, wake, said_ready));
        if let Err(error) = spawned {
            trigger(&queue);
            return Err(AxError::Lost(format!(
                "starting the event listener: {error}"
            )));
        }
        match tokio::time::timeout(CALL_LIMIT, ready).await {
            Ok(Ok(Ok(run_loop))) => Ok(Watch {
                changes,
                exited: Some(exited),
                run_loop,
                queue,
            }),
            outcome => {
                trigger(&queue);
                Err(match outcome {
                    Ok(Ok(Err(error))) => error,
                    Ok(Err(_)) => {
                        AxError::Lost("the event listener ended before it began".to_owned())
                    }
                    _ => AxError::Timeout,
                })
            }
        }
    }

    /// Ends the accessibility thread once the jobs before this are done.
    pub(crate) async fn close(self) {
        drop(self.jobs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The calls of the accessibility interface, and of the system around
    /// it, that change an application, its windows, the focus, the input,
    /// or the permission: none may be made.
    const ACTING_CALLS: &[&str] = &[
        "AXUIElementSetAttributeValue",
        "AXUIElementPerformAction",
        "AXUIElementPostKeyboardEvent",
        "AXMakeProcessTrusted",
        "AXIsProcessTrustedWithOptions",
        "AXObserverRemoveNotification",
        "CGEventPost",
        "CGEventPostToPid",
    ];

    /// The attributes a client sets to change how an application answers,
    /// or what it shows: none may be asked through [`copy`] by mistake for
    /// the sake of setting it.
    const SET_ATTRIBUTES: &[&str] = &[
        "AXManualAccessibility",
        "AXEnhancedUserInterface",
        "AXFocused",
        "AXMain",
        "AXMinimized",
        "AXSelected",
        "AXSelectedText",
        "AXSelectedTextRange",
    ];

    /// Each method of the bindings this module may name, with the call of
    /// the interface it makes.
    const BOUND: &[(&str, &str)] = &[
        ("AXIsProcessTrusted(", "AXIsProcessTrusted"),
        ("new_application(", "AXUIElementCreateApplication"),
        ("new_system_wide(", "AXUIElementCreateSystemWide"),
        ("set_messaging_timeout(", "AXUIElementSetMessagingTimeout"),
        ("copy_attribute_value(", "AXUIElementCopyAttributeValue"),
        ("AXObserver::create(", "AXObserverCreate"),
        ("add_notification(", "AXObserverAddNotification"),
        ("run_loop_source(", "AXObserverGetRunLoopSource"),
    ];

    /// The methods of the bindings that act, by the names this module would
    /// write: none may appear.
    const ACTING_METHODS: &[&str] = &[
        "set_attribute_value(",
        "perform_action(",
        "post_keyboard_event(",
        "remove_notification(",
        "AXMakeProcessTrusted(",
        "AXIsProcessTrustedWithOptions(",
        "kAXTrustedCheckOptionPrompt",
        "copy_element_at_position(",
        "create_with_info_callback(",
        "copy_multiple_attribute_values(",
        "copy_parameterized_attribute_value(",
    ];

    #[test]
    fn the_back_end_makes_no_call_that_acts_and_asks_no_attribute_to_set() {
        for call in READ_CALLS {
            assert!(!ACTING_CALLS.contains(call), "{call}");
        }
        for attribute in READ_ATTRIBUTES {
            assert!(!SET_ATTRIBUTES.contains(attribute), "{attribute}");
        }
        let mut calls = READ_CALLS.to_vec();
        calls.sort_unstable();
        calls.dedup();
        assert_eq!(calls.len(), READ_CALLS.len());
        assert!(NOTIFICATIONS.iter().all(|name| name.starts_with("AX")));
    }

    /// This file up to its tests, which is all the code that runs.
    fn code() -> &'static str {
        let file = include_str!("macos.rs");
        let end = file.find("#[cfg(test)]\nmod tests").unwrap_or(file.len());
        &file[..end]
    }

    #[test]
    fn every_call_into_the_interface_is_an_ask_that_names_it() {
        let code = code();
        // Where each `ask` begins and ends, and the call it names.
        let mut spans = Vec::new();
        let opening = "ask(\"";
        let mut from = 0;
        while let Some(found) = code[from..].find(opening) {
            let start = from + found;
            let named = start + opening.len();
            let call = &code[named..named + code[named..].find('"').unwrap()];
            let end = start + code[start..].find("})").unwrap();
            spans.push((start, end, call));
            from = end;
        }
        // The scan found every ask the code makes, and the asks make every
        // call of the list and no other.
        assert_eq!(
            spans.len(),
            code.matches(opening).count(),
            "an ask was passed over"
        );
        let mut asked: Vec<&str> = spans.iter().map(|(_, _, call)| *call).collect();
        asked.sort_unstable();
        asked.dedup();
        let mut listed = READ_CALLS.to_vec();
        listed.sort_unstable();
        assert_eq!(asked, listed, "the calls asked are not the list of reads");
        for (start, end, call) in &spans {
            assert!(
                READ_CALLS.contains(call),
                "`{call}` is asked and not on the list"
            );
            let made = &code[*start..*end];
            let (method, _) = BOUND
                .iter()
                .find(|(_, bound)| bound == call)
                .unwrap_or_else(|| panic!("`{call}` has no method of the bindings"));
            assert!(
                made.contains(method),
                "the ask that names `{call}` makes something else: {made}"
            );
        }
        // Every method of the bindings named anywhere is inside an ask that
        // names its call.
        for (method, call) in BOUND {
            for (at, _) in code.match_indices(method) {
                let before = code[..at].chars().next_back().unwrap_or(' ');
                if before.is_ascii_alphanumeric() || before == '_' {
                    continue;
                }
                // The test's own list and the definitions are not calls.
                if code[..at].ends_with("fn ") {
                    continue;
                }
                assert!(
                    spans
                        .iter()
                        .any(|(start, end, named)| *start < at && at < *end && named == call),
                    "`{method}` is called outside an ask that names `{call}`"
                );
            }
        }
        // And nothing that acts is named at all.
        for method in ACTING_METHODS {
            assert!(!code.contains(method), "`{method}` is named");
        }
        for call in ACTING_CALLS {
            assert!(!code.contains(&format!("{call}(")), "`{call}` is called");
        }
    }

    #[test]
    fn every_role_word_is_one_the_pack_knows() {
        let places = [Place::Plain, Place::InRow, Place::Heading, Place::InHeading];
        for (role, _) in ROLES {
            for subrole in ["", SECURE, "AXDialog", "AXStandardWindow"] {
                for place in places {
                    let word = role_word(role, subrole, place);
                    assert!(roles::known(word) || word == roles::OTHER, "{role}");
                }
            }
        }
        assert_eq!(
            role_word("AXTextField", SECURE, Place::Plain),
            "password_text"
        );
        assert_eq!(
            role_word("AXTextField", SECURE, Place::InRow),
            "password_text"
        );
        assert_eq!(role_word("AXTextField", "", Place::Plain), "entry");
        assert_eq!(role_word("AXStaticText", "", Place::InRow), "table_cell");
        assert_eq!(role_word("AXStaticText", "", Place::Plain), "label");
        assert_eq!(
            role_word("AXWindow", "AXStandardWindow", Place::Plain),
            "frame"
        );
        assert_eq!(role_word("AXWindow", "AXDialog", Place::Plain), "dialog");
        assert_eq!(
            role_word("AXButton", "AXSortButton", Place::InHeading),
            "table_column_header"
        );
        // Unknown means stop: a role with no word is never a role a locator
        // can name.
        assert_eq!(role_word("AXWebArea", "", Place::Plain), roles::OTHER);
        assert_eq!(role_word("AXColumn", "", Place::Plain), roles::OTHER);
        let mut names: Vec<&str> = ROLES.iter().map(|(role, _)| *role).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ROLES.len());
    }

    #[test]
    fn the_process_table_gives_this_process_its_user_and_its_program() {
        let me = i32::try_from(std::process::id()).unwrap();
        assert!(processes().contains(&me));
        let facts = facts_of(me).unwrap();
        assert_eq!(facts.user, my_user());
        assert!(facts.started.0 > 0);
        let program = program_of(me).unwrap();
        assert_eq!(
            resolved_path(&program).unwrap(),
            resolved_path(&std::env::current_exe().unwrap()).unwrap()
        );
    }

    example_desk::stand_child!();

    /// The walk as text: depth, role word, the interface's role and
    /// subrole, name, and flags. Never a value read through `AXValue` of a
    /// field; the password field's name is its label's.
    fn shown(tree: &Tree, seen: &[String]) -> String {
        let mut out = String::new();
        for (at, node) in tree.nodes.iter().enumerate() {
            out.push_str(&format!(
                "{}{} [{}] {:?}{}{}{}\n",
                "  ".repeat(node.depth as usize),
                node.role,
                seen.get(at).map_or("", String::as_str),
                node.name,
                if node.secret { " secret" } else { "" },
                if node.has_text { " value" } else { "" },
                if node.labels { " labels" } else { "" },
            ));
        }
        out.push_str(&format!("cut: {:?}", tree.cut));
        out.chars().take(12_000).collect()
    }

    /// Against the stand's window: the permission, the proof of the
    /// application, the roles the table maps, and the walk, said in full
    /// when it fails.
    #[tokio::test]
    async fn the_stand_is_proven_and_walked_as_the_role_table_says() {
        let Some(session) = example_desk::Session::start().unwrap() else {
            eprintln!(
                "skipped: this process may not use the accessibility interface, or there is no window server session"
            );
            return;
        };
        let desk = example_desk::Desk::start(&session, example_desk::Order::example())
            .await
            .unwrap();
        let program = session.program();
        let look = match Look::connect(None).await {
            Ok(look) => look,
            Err(error) => panic!("{}", error.failure(&program)),
        };
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
        // The window may take a moment to be listed: wait for it by the
        // application's notifications, never by a sleep.
        let mut watch = look.watch(&app).await.unwrap();
        let deadline = Deadline::after(Duration::from_secs(20));
        let windows = loop {
            let windows = look.windows(&app).await.unwrap();
            if !windows.is_empty() || !watch.next(deadline).await.unwrap() {
                break windows;
            }
        };
        drop(watch);
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
        let seen = look
            .run(CALL_LIMIT, |ax| Ok(ax.seen.clone()))
            .await
            .unwrap();
        let said = shown(&tree, &seen);
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
        assert_eq!(has("entry", ""), 1, "the unnamed field:\n{said}");
        assert_eq!(has("password_text", "Access code"), 1, "{said}");
        assert!(
            tree.nodes
                .iter()
                .all(|node| (node.role == "password_text") == node.secret),
            "{said}"
        );
        assert_eq!(has("push_button", "Save"), 1, "{said}");
        assert!(
            tree.nodes
                .iter()
                .any(|node| node.role == "label" && node.name == "Order number" && node.labels),
            "a label that labels a field:\n{said}"
        );
        assert_eq!(has("table", "Line items"), 1, "{said}");
        let table = tree.nodes.iter().find(|node| node.role == "table").unwrap();
        let rows: Vec<Vec<(&str, &str)>> = table
            .children
            .iter()
            .map(|row| {
                tree.nodes[*row]
                    .children
                    .iter()
                    .map(|cell| (tree.nodes[*cell].role, tree.nodes[*cell].name.as_str()))
                    .collect()
            })
            .collect();
        assert_eq!(rows.len(), 4, "a row of headings and three lines:\n{said}");
        assert_eq!(
            rows[0],
            [
                ("table_column_header", "Item code"),
                ("table_column_header", "Description"),
                ("table_column_header", "Quantity")
            ],
            "{said}"
        );
        assert!(
            rows[1..]
                .iter()
                .all(|row| row.len() == 3 && row.iter().all(|(role, _)| *role == "table_cell")),
            "three cells a row: {rows:?}\n{said}"
        );
        // A value, through `AXValue`; the password, never.
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
            "no notification from the application within ten seconds of its change"
        );
        drop(watch);
        look.close().await;
        let truth = desk.truth();
        assert_eq!(truth.writes, 0, "{:?}", truth.asked);
        assert!(!truth.acted(), "{:?}", truth.asked);
        assert_eq!(truth.secret_reads, 0, "{:?}", truth.asked);
    }
}
