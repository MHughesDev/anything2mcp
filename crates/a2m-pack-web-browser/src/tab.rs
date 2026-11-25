//! Tabs: one page each, in one of two postures. A tab that may act, and a
//! tab that may only observe. Everything that only looks is the [`Look`]
//! trait, which both have; only [`ActTab`] has a method that acts.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::Value as Json;
use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use a2m_core::Redact;
use a2m_provider::Deadline;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use crate::browsers::{Address, BrowserInner, Shared};
use crate::client::{Client, ClientError};
use crate::error::{BrowserError, Dispatch, REFUSED_WHEN_MADE, Reason, RequestShape};
use crate::origin::{Origins, origin_of, path_of, shown_origin};
use crate::page_read::{PageAnswer, PagePath};
use crate::page_write::{PageWrite, TokenProblem};
use crate::protocol::{
    AddBinding, AddScriptOnNewDocument, AttachToTarget, Attached, BypassServiceWorker,
    CallArgument, CallFunctionOn, CloseTarget, Command, ContinueRequest, CreateIsolatedWorld,
    CreateTarget, DescribeNode, DispatchKey, DispatchMouse, Evaluate, Evaluated, Event, EventKind,
    FailRequest, FetchEnable, GetFrameTree, GetFullAxTree, GetProperties, GetResponseBody,
    GetTargetInfo, HandleDialog, InsertText, MediaFeature, Navigate, NetworkEnable,
    NodeDescription, PageEnable, RequestPattern, RequestPaused, RunIfWaiting, RuntimeEnable,
    SetAcceptLanguage, SetAutoAttach, SetDeviceMetrics, SetDocumentContent, SetEmulatedMedia,
    SetFileInputFiles, SetLifecycleEvents, SetLocale, SetTimezone, TerminateExecution,
};
use crate::safe::SafeRequests;
use crate::snapshot::{Snapshot, SnapshotLimits};

/// The helper that runs in the page, in a world of its own.
const HELPER: &str = include_str!("helper.js");

/// Where the helper's source holds the origins its tab was opened for.
const HELPER_ORIGINS: &str = "/*a2m:origins*/[]";

/// How the helper says that the page is on an origin the tab was not
/// opened for, and that a selector would tell a password.
const OUTSIDE_ORIGINS: &str = "a2m-outside-origins ";
const PASSWORD_REFUSED: &str = "a2m-password-refused";

/// What a tab reports for a move of the tab that was refused, a new window
/// that was closed before it ran, and a download that was refused.
const NAVIGATION: &str = "NAVIGATION";
const POPUP: &str = "POPUP";
const DOWNLOAD: &str = "DOWNLOAD";

/// What a tab reports for a request of its page, a frame, or a worker that
/// was refused because its address is on none of the origins the tab may
/// reach (decision 0292).
pub const OUTSIDE: &str = "OUTSIDE";

/// How the browser is told to end a request. A document that is ended as
/// aborted leaves the page that asked for it in place; one that is ended
/// as blocked is replaced by the browser's own error page (measured,
/// decision 0143).
const ABORTED: &str = "Aborted";
const BLOCKED: &str = "BlockedByClient";

/// The helper's source for a tab opened for these origins.
fn helper_for(origins: &Origins) -> String {
    let list: Vec<&str> = origins.iter().collect();
    // A list of plain texts is always written; an empty one refuses all.
    let list = serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_owned());
    HELPER.replacen(HELPER_ORIGINS, &list, 1)
}

/// The name of the helper's isolated world.
const WORLD: &str = "anything2mcp";

/// The binding through which an observing tab's page reports a socket that
/// was refused.
const REFUSED_BINDING: &str = "__anything2mcpRefused";

/// Run in the page's own world, and in every worker, of an observing tab.
/// What request interception cannot see is refused when it is made: a
/// socket, a peer connection, a shared worker, and a service worker, each
/// of which could carry a write past the interception. This lock is a
/// script in the page, so a page that sets out to defeat it can; the
/// interception of requests is in the browser, and cannot be reached from
/// the page.
///
/// A service worker outlives the page that woke it, and any page of its
/// origin may wake it again, so it is held to this script in both postures
/// (decision 0292). A name already refused by it is passed over, so the
/// script can be placed again in a worker an earlier tab locked.
const REFUSE_SOCKETS: &str = r#"(() => {
  // The address as the page would have reached it: a relative one is
  // resolved against the page's own, so the refusal names where it led.
  const report = (what, address) => {
    let whole = String(address);
    try { whole = new URL(whole, globalThis.location ? globalThis.location.href : undefined).href; } catch (_) {}
    try { globalThis.__anything2mcpRefused(what + " " + whole); } catch (_) {}
  };
  const mark = Symbol.for("anything2mcp.refused");
  const refusal = () => new DOMException("Refused while the page is only observed.", "SecurityError");
  // The prefixed name is a second property holding the same constructor:
  // refusing the plain name alone leaves it open.
  for (const name of ["WebSocket", "WebTransport", "SharedWorker", "RTCPeerConnection", "webkitRTCPeerConnection"]) {
    if (!(name in globalThis)) continue;
    if (globalThis[name] && globalThis[name][mark]) continue;
    const refused = function (address) {
      report(name === "webkitRTCPeerConnection" ? "RTCPeerConnection" : name, address);
      throw refusal();
    };
    refused[mark] = true;
    Object.defineProperty(globalThis, name, { value: refused, writable: false, configurable: false });
  }
  if (globalThis.ServiceWorkerContainer && !ServiceWorkerContainer.prototype.register[mark]) {
    const refused = function (address) {
      report("ServiceWorker", address);
      return Promise.reject(refusal());
    };
    refused[mark] = true;
    Object.defineProperty(ServiceWorkerContainer.prototype, "register", {
      value: refused,
      writable: false,
      configurable: false,
    });
  }
})()"#;

/// Where the socket script of an acting tab holds the origins it may reach.
const SOCKET_REACH: &str = "/*a2m:reach*/[]";

/// Run in the page's own world, and in every worker, of an acting tab
/// (decision 0292). What request interception cannot see is held to the
/// tab's reach when it is made: a WebSocket or a WebTransport to an origin
/// outside it is refused, and so is every peer connection, shared worker,
/// and service worker, whose requests could go anywhere. Like the
/// observing tab's, this lock is a script in the page.
const HOLD_SOCKETS: &str = r#"(() => {
  const reach = /*a2m:reach*/[];
  // The address as the page would have reached it: a relative one is
  // resolved against the page's own, so the refusal names where it led.
  const report = (what, address) => {
    let whole = String(address);
    try { whole = new URL(whole, globalThis.location ? globalThis.location.href : undefined).href; } catch (_) {}
    try { globalThis.__anything2mcpRefused(what + " " + whole); } catch (_) {}
  };
  const refusal = () => new DOMException("Refused: outside what this tab may reach.", "SecurityError");
  const inside = (address) => {
    try {
      const url = new URL(String(address), globalThis.location ? globalThis.location.href : undefined);
      const scheme = url.protocol === "ws:" ? "http:" : url.protocol === "wss:" ? "https:" : url.protocol;
      return reach.includes(scheme + "//" + url.host);
    } catch (_) {
      return false;
    }
  };
  for (const name of ["WebSocket", "WebTransport"]) {
    const real = globalThis[name];
    if (typeof real !== "function") continue;
    const held = new Proxy(real, {
      apply() { throw new TypeError(name + " must be called with new."); },
      construct(target, args, made) {
        if (!inside(args[0])) {
          report(name, args[0]);
          throw refusal();
        }
        return Reflect.construct(target, args, made);
      },
    });
    Object.defineProperty(globalThis, name, { value: held, writable: false, configurable: false });
  }
  for (const name of ["SharedWorker", "RTCPeerConnection", "webkitRTCPeerConnection"]) {
    if (!(name in globalThis)) continue;
    const refused = function (address) {
      report(name === "webkitRTCPeerConnection" ? "RTCPeerConnection" : name, address);
      throw refusal();
    };
    Object.defineProperty(globalThis, name, { value: refused, writable: false, configurable: false });
  }
  if (globalThis.ServiceWorkerContainer) {
    Object.defineProperty(ServiceWorkerContainer.prototype, "register", {
      value: function (address) {
        report("ServiceWorker", address);
        return Promise.reject(refusal());
      },
      writable: false,
      configurable: false,
    });
  }
})()"#;

/// The socket script of an acting tab that may reach these origins.
fn sockets_held_to(origins: &Origins) -> String {
    let list = origins.reach();
    // A list of plain texts is always written; an empty one refuses all.
    let list = serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_owned());
    HOLD_SOCKETS.replacen(SOCKET_REACH, &list, 1)
}

/// The fixed environment of a tab. Set per target and never varied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageEnvironment {
    /// The viewport's width and height, in CSS pixels.
    pub viewport: (u32, u32),
    /// The locale, such as `en-US`.
    pub locale: String,
    /// The time zone, as the zone database names it, such as `UTC`.
    pub time_zone: String,
    /// Whether the page is told the person prefers a dark scheme.
    pub dark: bool,
    /// Whether the page is told the person prefers reduced motion.
    pub reduced_motion: bool,
}

impl Default for PageEnvironment {
    fn default() -> Self {
        Self {
            viewport: (1280, 800),
            locale: "en-US".to_owned(),
            time_zone: "UTC".to_owned(),
            dark: false,
            reduced_motion: true,
        }
    }
}

pub(crate) enum Posture {
    Act,
    Observe(SafeRequests),
    /// A tab a person signs in in, in a window (decision 0254): held to its
    /// origins as an acting tab is, and with no network event on, since
    /// nothing in it writes and what the person types would cross in them.
    SignIn,
    /// A tab a person uses while the engine watches, in a window (decision
    /// 0392, point 1): held to its origins as a sign-in tab is, with no
    /// interception of a request by its method, and its network events on,
    /// so that every request of its page is recorded by structure.
    Watch,
}

/// How one element is matched. Every variant must match exactly one
/// rendered element; the first of several is never picked.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum By {
    /// Accessibility role and accessible name. The first choice.
    RoleName {
        /// The role, such as `textbox` or `button`.
        role: String,
        /// The accessible name, exact after white space is collapsed.
        name: String,
    },
    /// The text of the label associated with a control.
    Label {
        /// The label's text.
        text: String,
    },
    /// Placeholder text.
    Placeholder {
        /// The placeholder.
        text: String,
    },
    /// Visible text: the deepest element that carries it.
    Text {
        /// The text.
        text: String,
        /// Whether the element's text only has to contain it.
        contains: bool,
    },
    /// The `data-testid` attribute.
    TestId {
        /// The identifier.
        id: String,
    },
    /// Title or tooltip text.
    Title {
        /// The title.
        text: String,
    },
    /// The one heading of this level, whatever it is named: a record's
    /// page whose heading is the record's own value is found by it, never
    /// by the value (decision 0473).
    Heading {
        /// The heading's level, 1 to 6.
        level: u32,
    },
    /// A selector. The last resort.
    Css {
        /// The selector.
        selector: String,
    },
    /// The row of a grid or a table whose first cell shows this text
    /// exactly: a row is known by its key, never by its place.
    RowKeyed {
        /// The text of the row's first cell.
        key: String,
    },
    /// One cell of the row whose first cell shows this text exactly: a
    /// cell is known by its row's key and its place among the row's cells.
    CellKeyed {
        /// The text of the row's first cell.
        key: String,
        /// The cell's place in its row, counted from zero.
        column: usize,
    },
}

/// A query for exactly one element, inside another element when `within`
/// names one. Open shadow roots are looked through.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Query {
    /// How the element is matched.
    pub by: By,
    /// The element it must be inside, itself matched to exactly one.
    pub within: Option<Box<Query>>,
}

impl Query {
    /// A query over the whole page.
    #[must_use]
    pub fn new(by: By) -> Self {
        Self { by, within: None }
    }

    /// A query by role and accessible name.
    #[must_use]
    pub fn role(role: &str, name: &str) -> Self {
        Self::new(By::RoleName {
            role: role.to_owned(),
            name: name.to_owned(),
        })
    }

    /// The same query, inside the one element `scope` matches.
    #[must_use]
    pub fn within(mut self, scope: Query) -> Self {
        self.within = Some(Box::new(scope));
        self
    }
}

/// A text to compare with.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct TextMatch {
    /// The text, compared after white space is collapsed.
    pub text: String,
    /// Whether containing it is enough.
    pub contains: bool,
}

/// The conditions the base can wait on. Each ends on an observer or an
/// event of the page; none asks again on a timer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Primitive {
    /// At least one rendered element matches.
    Visible(Query),
    /// No rendered element matches.
    Hidden(Query),
    /// Exactly one matches and it is enabled.
    Enabled(Query),
    /// Exactly one matches and its value, or its text, matches.
    Text(Query, TextMatch),
    /// The page's address matches.
    Url(TextMatch),
    /// Every one holds.
    All(Vec<Primitive>),
    /// At least one holds.
    Any(Vec<Primitive>),
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Condition<'a> {
    Visible {
        query: &'a Query,
    },
    Hidden {
        query: &'a Query,
    },
    Enabled {
        query: &'a Query,
    },
    Text {
        query: &'a Query,
        r#match: &'a TextMatch,
    },
    Url {
        text: &'a str,
        contains: bool,
    },
    All {
        of: Vec<Condition<'a>>,
    },
    Any {
        of: Vec<Condition<'a>>,
    },
}

impl<'a> From<&'a Primitive> for Condition<'a> {
    fn from(primitive: &'a Primitive) -> Self {
        match primitive {
            Primitive::Visible(query) => Self::Visible { query },
            Primitive::Hidden(query) => Self::Hidden { query },
            Primitive::Enabled(query) => Self::Enabled { query },
            Primitive::Text(query, r#match) => Self::Text { query, r#match },
            Primitive::Url(pattern) => Self::Url {
                text: &pattern.text,
                contains: pattern.contains,
            },
            Primitive::All(of) => Self::All {
                of: of.iter().map(Self::from).collect(),
            },
            Primitive::Any(of) => Self::Any {
                of: of.iter().map(Self::from).collect(),
            },
        }
    }
}

/// What to read of an element.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReadWhat {
    /// The value of a control.
    Value,
    /// The text a person sees.
    Text,
    /// Whether a check box, a radio button, or a switch is on.
    Checked,
    /// The text of the option a select shows.
    SelectedText,
    /// One attribute.
    Attribute {
        /// The attribute's name.
        name: String,
    },
    /// The text of each element of one role inside the element, and not
    /// inside another of that role below it, in the page's order, each with
    /// its white space collapsed, one to a line: the items of a list, or
    /// the paragraphs of a part of a page (decision 0473). One item never
    /// spans two lines.
    Items {
        /// The role of each item: `listitem` or `paragraph`.
        role: String,
    },
    /// The text of one cell of a row, counted from zero among its cells.
    Cell {
        /// The cell's place in its row.
        index: usize,
    },
    /// The labels of the selected item of a tree and of the items it is
    /// under, outermost first, joined by `/`.
    TreePath,
    /// The names of the files a file input holds, joined by `/`.
    Files,
}

/// What a read gave, as the page holds it: not trimmed, not normalized.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RawValue {
    /// A text.
    Text(String),
    /// On or off.
    Flag(bool),
    /// The element has no such value.
    Absent,
}

/// One element of one page generation, and the query that found it. After
/// a navigation, acting on it or reading it is [`BrowserError::NotFound`].
/// After a re-render that removes it, acting on it is
/// [`BrowserError::NotFound`] and it is never looked for again; a read
/// alone finds it again by its query, in the same document and the same
/// call that reads it, when that query still matches exactly one element.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Element {
    object: String,
    tab: u64,
    query: Query,
}

/// A page that has loaded as far as its document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    /// The address the page ended at, after redirects.
    pub address: String,
    /// The page's title.
    pub title: String,
    /// The status of the document's response, when there was one. `None`
    /// for a move within the same document.
    pub status: Option<u16>,
    /// Whether the document's response says the edge holds the visitor for
    /// a challenge: the header `cf-mitigated` with the value `challenge`.
    /// Never passed: the engine does not answer a challenge (rule 5).
    pub challenged: bool,
}

/// A key to press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// Enter.
    Enter,
    /// Tab.
    Tab,
    /// Escape.
    Escape,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// The up arrow.
    ArrowUp,
    /// The down arrow.
    ArrowDown,
    /// The left arrow.
    ArrowLeft,
    /// The right arrow.
    ArrowRight,
    /// Home.
    Home,
    /// End.
    End,
    /// The space bar.
    Space,
}

impl Key {
    /// The key's name, its code, its virtual key code, and the text it
    /// types when it types one.
    fn parts(self) -> (&'static str, &'static str, u32, Option<&'static str>) {
        match self {
            Self::Enter => ("Enter", "Enter", 13, Some("\r")),
            Self::Tab => ("Tab", "Tab", 9, None),
            Self::Escape => ("Escape", "Escape", 27, None),
            Self::Backspace => ("Backspace", "Backspace", 8, None),
            Self::Delete => ("Delete", "Delete", 46, None),
            Self::ArrowUp => ("ArrowUp", "ArrowUp", 38, None),
            Self::ArrowDown => ("ArrowDown", "ArrowDown", 40, None),
            Self::ArrowLeft => ("ArrowLeft", "ArrowLeft", 37, None),
            Self::ArrowRight => ("ArrowRight", "ArrowRight", 39, None),
            Self::Home => ("Home", "Home", 36, None),
            Self::End => ("End", "End", 35, None),
            Self::Space => (" ", "Space", 32, Some(" ")),
        }
    }
}

/// One input to one element. Each is one strategy; none falls back to
/// another. A text to enter may be a password, so it is never printed:
/// the debug form says how long it is.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Input {
    /// Focus, select what is there, and insert the text as one edit.
    Fill(String),
    /// Focus, select what is there, and type the text key by key.
    Type(String),
    /// Set the value through the element's own setter and tell the page.
    NativeSet(String),
    /// Take the focus from the element, as a person's moving on would, so
    /// that a page that commits a value when its control loses the focus is
    /// told; nothing is done to an element that does not hold the focus.
    Blur,
    /// Focus and press one key.
    Press(Key),
    /// A click in the middle of the element.
    Click,
    /// Bring a check box or a radio button to this state, by a click when
    /// it is not there already.
    Check(bool),
    /// Choose the option of a select that has this value.
    SelectByValue(String),
    /// Give a file input this one file, by its path, as a person's choice
    /// in the browser's file chooser would. The path is a checked path
    /// from the pack: this crate opens nothing.
    Attach(std::path::PathBuf),
}

impl fmt::Debug for Input {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let length = |text: &str| text.chars().count();
        match self {
            Self::Fill(text) => write!(f, "Fill({} characters)", length(text)),
            Self::Type(text) => write!(f, "Type({} characters)", length(text)),
            Self::NativeSet(text) => write!(f, "NativeSet({} characters)", length(text)),
            Self::Blur => f.write_str("Blur"),
            Self::Press(key) => write!(f, "Press({key:?})"),
            Self::Click => f.write_str("Click"),
            Self::Check(on) => write!(f, "Check({on})"),
            Self::SelectByValue(value) => write!(f, "SelectByValue({value:?})"),
            Self::Attach(_) => f.write_str("Attach(a file)"),
        }
    }
}

/// A dialog of the browser that is open on a tab: what kind it is and what
/// it says, as the page gave it, redacted and bounded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenDialog {
    /// `alert`, `confirm`, `prompt`, or `beforeunload`.
    pub kind: String,
    /// What the dialog says.
    pub message: String,
}

/// How to answer a dialog of the page.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DialogAnswer {
    /// Accept it.
    Accept,
    /// Accept a prompt with this text.
    AcceptWith(String),
    /// Dismiss it.
    Dismiss,
}

/// A request sent from inside the page, with the page's own session. Its
/// headers and its body may carry a credential, so they are never printed:
/// the debug form gives the names of the headers and the length of the
/// body, and the address without its query.
#[derive(Clone, PartialEq, Eq)]
pub struct PageRequest {
    /// The method, in capitals.
    pub method: String,
    /// Where it goes: an address on one of the tab's origins. The tab
    /// refuses any other, and follows no redirect.
    pub address: Address,
    /// Its headers.
    pub headers: Vec<(String, String)>,
    /// Its body.
    pub body: Option<String>,
}

impl fmt::Debug for PageRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(name, _)| name.as_str()).collect();
        f.debug_struct("PageRequest")
            .field("method", &self.method)
            .field("address", &without_query(self.address.as_str()))
            .field("headers", &names)
            .field(
                "body_characters",
                &self.body.as_ref().map(|body| body.chars().count()),
            )
            .finish()
    }
}

/// The answer to a [`PageRequest`], with credentials removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageResponse {
    /// The status.
    pub status: u16,
    /// The address that answered, without its query.
    pub address: String,
    /// The headers, without cookies and authorization, in order of name.
    pub headers: Vec<(String, String)>,
    /// The body, with every known secret replaced.
    pub body: String,
}

/// One request the page of an observing tab made by itself, with GET, to
/// one of the tab's origins, and its answer, by structure alone: no value
/// of the address's query, of a header, of a cookie, or of the answer is
/// kept (decisions 0187 and 0195).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageExchange {
    /// The method, in capitals: `GET`.
    pub method: String,
    /// The origin, as the browser writes it.
    pub origin: String,
    /// The path, without its query, with every known secret replaced and
    /// every segment that is not a plain word kept as
    /// [`crate::PLACEHOLDER`] ([`crate::path_by_structure`]).
    pub path: String,
    /// The names of the query's parameters, in order, each once: only
    /// those that are plain names ([`crate::query_names`]).
    pub query: Vec<String>,
    /// Whether the query held an entry that is not kept: one with no `=`,
    /// whose whole text is a value, or one whose name is not a plain name.
    pub query_left_out: bool,
    /// The names, in lowercase and in order, of the request's headers that
    /// carry a credential or an anti-forgery token. Never their values.
    pub credential_headers: Vec<String>,
    /// The names, in lowercase and in order, of the request's headers that
    /// the page's own script set: every header but those the browser sends
    /// with any request of a script by itself ([`BROWSER_HEADERS`]), and
    /// `accept` when it is not the browser's own `*/*`. Never their values.
    /// A read made again from inside the page sends none of them.
    pub set_headers: Vec<String>,
    /// The status of the answer.
    pub status: u16,
    /// The answer's media type, in lowercase, without its parameters.
    pub content_type: String,
    /// For an answer that is a JSON document: each of its top-level names,
    /// and each name one level inside an object, as `outer.inner`, or
    /// `[]` and `[].name` for a list's items, with the kind of value it held:
    /// `string`, `integer`, `number`, `boolean`, `null`, `object`, or
    /// `array`; `mixed` when the items of a list held more than one.
    /// `None` when the answer is not JSON, could not be read, or is longer
    /// than the tab reads.
    pub fields: Option<BTreeMap<String, String>>,
    /// Whether the names of an object of the answer were withheld: it had
    /// more names than [`crate::MOST_NAMES`], or one that is not a plain
    /// name, and was read as a dictionary, whose names are values. Only its
    /// own name and kind are in `fields`.
    pub names_withheld: bool,
    /// The media type of the body the request sent, in lowercase, without
    /// its parameters; empty when it sent none. Recorded by a watching tab
    /// alone: an observing tab records only `GET`, which sends no body.
    pub body_media_type: String,
    /// For a body sent as JSON, its names and kinds by the rule of
    /// `fields`; for a form, its entries' names, each a `string`, by the
    /// rule of [`crate::query_names`] and [`crate::names_are_structure`].
    /// `None` for no body, a body of another media type, or one the
    /// browser did not give whole up to [`crate::LONGEST_WATCHED_BODY`].
    /// Never a value.
    pub body_fields: Option<BTreeMap<String, String>>,
    /// Whether names of the body were withheld, as `names_withheld`.
    pub body_names_withheld: bool,
}

/// What [`ObserveTab::exchanges`] gives: the page's own requests answered
/// whole, and what was not kept, said.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PageExchanges {
    /// The exchanges, oldest first.
    pub exchanges: Vec<PageExchange>,
    /// How many requests of the page were left out because the tab already
    /// held [`EXCHANGES_KEPT`].
    pub dropped: usize,
    /// How many requests of the page were still awaiting their answer when
    /// the deadline came.
    pub awaited: usize,
}

/// The request headers the browser sends by itself with a request a script
/// of the page makes, and that the script cannot set: a read made again
/// from inside the page sends each the same. Data, found by looking at what
/// the browser reports for a `fetch` with no header of its own (decision
/// 0195).
pub const BROWSER_HEADERS: &[&str] = &[
    "cookie",
    "sec-ch-ua",
    "sec-ch-ua-mobile",
    "sec-ch-ua-platform",
];

/// The request headers the browser also sends by itself with every such
/// request, and that a script may set too. Each is the browser's own when
/// it holds what the request of the page's own document held; `referer`
/// when it is the page's own address; `accept` when it is `*/*`. Otherwise
/// the page set it. Data.
pub const BROWSER_DEFAULT_HEADERS: &[&str] = &["accept-language", "user-agent"];

/// What the request of the tab's own document held of the headers in
/// [`BROWSER_DEFAULT_HEADERS`], and its address: kept in memory to tell the
/// browser's own headers from the page's, and never recorded.
#[derive(Debug, Clone, Default)]
struct DocumentHeaders {
    address: String,
    defaults: BTreeMap<String, String>,
}

/// The words in a request header's name that say it carries a credential
/// or an anti-forgery token. Data.
pub const CREDENTIAL_HEADER_WORDS: &[&str] = &[
    "authorization",
    "cookie",
    "token",
    "csrf",
    "xsrf",
    "auth",
    "session",
    "key",
    "secret",
];

/// A request of the page, heard and not yet answered whole.
#[derive(Debug, Clone)]
struct Pending {
    exchange: PageExchange,
    answered: bool,
    /// The loader of the document it was sent under, where the browser
    /// said: a watching tab closes what a document left behind.
    loader: Option<String>,
    /// Whether its answer has arrived whole and is being read: then the
    /// read alone makes it an exchange, and nothing else closes it.
    reading: bool,
}

/// The most exchanges an observing tab keeps before they are taken.
pub const EXCHANGES_KEPT: usize = 256;

fn json_kind(value: &Json) -> &'static str {
    match value {
        Json::Null => "null",
        Json::Bool(_) => "boolean",
        Json::Number(number) if number.is_i64() || number.is_u64() => "integer",
        Json::Number(_) => "number",
        Json::String(_) => "string",
        Json::Array(_) => "array",
        Json::Object(_) => "object",
    }
}

/// The names of a JSON answer and the kinds of what they held, to one
/// level inside an object or a list's items, and whether the names of an
/// object were withheld. No value is kept, and the names of an object that
/// reads as a dictionary are not either ([`crate::names_are_structure`]):
/// only the object's own name and kind are.
pub(crate) fn fields_of(json: &Json) -> (BTreeMap<String, String>, bool) {
    fn note(out: &mut BTreeMap<String, String>, name: String, kind: &str) {
        let entry = out.entry(name).or_insert_with(|| kind.to_owned());
        if entry != kind {
            "mixed".clone_into(entry);
        }
    }
    fn object(
        out: &mut BTreeMap<String, String>,
        prefix: &str,
        map: &serde_json::Map<String, Json>,
        inner: bool,
    ) -> bool {
        if !crate::names_are_structure(map.keys()) {
            return true;
        }
        let mut withheld = false;
        for (name, value) in map {
            let at = format!("{prefix}{name}");
            note(out, at.clone(), json_kind(value));
            if inner && let Json::Object(nested) = value {
                withheld |= object(out, &format!("{at}."), nested, false);
            }
        }
        withheld
    }
    let mut out = BTreeMap::new();
    let mut withheld = false;
    match json {
        Json::Object(map) => withheld = object(&mut out, "", map, true),
        Json::Array(items) => {
            for item in items {
                note(&mut out, "[]".to_owned(), json_kind(item));
                if let Json::Object(map) = item {
                    withheld |= object(&mut out, "[].", map, false);
                }
            }
        }
        _ => {}
    }
    (out, withheld)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    Open,
    Dialog,
    Crashed,
    Gone,
    /// The page gave no answer to a call by its deadline, nor to the probe
    /// after it: the tab was closed, and every call on it since is a
    /// timeout.
    Hung,
}

#[derive(Debug, Clone, Default)]
struct Nav {
    /// The loader whose document has been parsed.
    loaded: Option<String>,
    /// The loader whose document the browser last said is idle on the
    /// network.
    idle: Option<String>,
    /// The loader of the last document response, and its status.
    document: Option<(String, u16, bool)>,
}

/// Who is moving the tab to another document: the `goto` under way, or the
/// page itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mover {
    Goto,
    Page,
}

/// What the tab knows about the moves of its own frame.
#[derive(Debug, Default)]
struct Moves {
    /// Whether a `goto` is under way.
    going: bool,
    /// The loader of the move that is the `goto`'s, once its request has
    /// been seen, and that request's latest number: a redirect keeps the
    /// loader and takes a new number.
    goto_loader: Option<String>,
    goto_request: Option<String>,
    /// The origin the `goto` under way was refused for.
    refused: Option<String>,
    /// The addresses of moves the page has asked for and that have not
    /// started, without their fragments.
    asked: VecDeque<String>,
    /// The loaders of the moves the page started.
    by_page: VecDeque<String>,
}

impl Moves {
    /// A page that keeps asking must not fill the engine's memory.
    const KEPT: usize = 64;

    fn remember(list: &mut VecDeque<String>, one: String) {
        if list.len() >= Self::KEPT {
            list.pop_front();
        }
        list.push_back(one);
    }

    /// The page asked for a move to this address.
    fn asked(&mut self, address: &str) {
        Self::remember(&mut self.asked, without_fragment(address).to_owned());
    }

    /// A move has started. It is the page's when the page asked for it, or
    /// when it is a move through the tab's history, which `goto` never is.
    fn started(&mut self, address: &str, loader: &str, kind: &str) {
        let address = without_fragment(address);
        let asked = self.asked.iter().position(|one| one == address);
        if let Some(at) = asked {
            self.asked.remove(at);
        }
        let history = kind.starts_with("history") || kind.starts_with("restore");
        if (asked.is_some() || history) && !loader.is_empty() {
            Self::remember(&mut self.by_page, loader.to_owned());
        }
    }

    /// Whose the request of the tab's own document is. The `goto`'s is the
    /// first one, since the `goto` began, that the page did not ask for,
    /// and every redirect of it.
    fn mover(&mut self, paused: &RequestPaused) -> Mover {
        let loader = paused
            .network_id
            .as_deref()
            .filter(|loader| !loader.is_empty());
        let follows = paused.redirected_request_id.as_deref();
        let of_goto = (follows.is_some() && follows == self.goto_request.as_deref())
            || (loader.is_some() && loader == self.goto_loader.as_deref());
        if of_goto {
            self.goto_request = Some(paused.request_id.clone());
            return Mover::Goto;
        }
        if loader.is_some_and(|loader| self.by_page.iter().any(|one| one == loader)) {
            return Mover::Page;
        }
        let address = without_fragment(&paused.request.url);
        if let Some(at) = self.asked.iter().position(|one| one == address) {
            self.asked.remove(at);
            if let Some(loader) = loader {
                Self::remember(&mut self.by_page, loader.to_owned());
            }
            return Mover::Page;
        }
        if follows.is_none() && self.going && self.goto_request.is_none() {
            self.goto_loader = loader.map(str::to_owned);
            self.goto_request = Some(paused.request_id.clone());
            return Mover::Goto;
        }
        Mover::Page
    }
}

/// What becomes of a request the browser holds.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict {
    /// It goes on.
    Pass,
    /// It is ended before it leaves the browser, and reported under this
    /// word when there is one.
    Refuse(Option<String>),
}

pub(crate) struct Page {
    client: Client,
    pub(crate) session: String,
    pub(crate) target: String,
    serial: u64,
    call_deadline: Duration,
    max_read: usize,
    redact: Arc<dyn Redact>,
    world: watch::Sender<Option<i64>>,
    pub(crate) status: watch::Sender<Status>,
    nav: watch::Sender<Nav>,
    /// How many dialogs of the page have closed.
    dialogs_closed: watch::Sender<u64>,
    /// A download a step declared, and how it stands.
    download: Mutex<DownloadState>,
    /// How many times what the page's download stands at changed.
    download_changes: watch::Sender<u64>,
    /// The tools the page registered, and the answers of calls of them.
    site: Mutex<SiteState>,
    /// How many times what the page announced of its tools changed.
    site_changes: watch::Sender<u64>,
    /// The dialog that is open, as the browser says it: its kind and what it
    /// says, redacted.
    open_dialog: Mutex<Option<OpenDialog>>,
    blocked: Mutex<Vec<RequestShape>>,
    /// How many documents of the tab's own frame have loaded, and how many
    /// things the tab refused, together: what a wait for the next change of
    /// the tab waits on.
    changes: watch::Sender<u64>,
    /// The page's own requests heard and not yet answered whole, by the
    /// browser's number for them, and how many there are.
    pending: Mutex<BTreeMap<String, Pending>>,
    waiting: watch::Sender<usize>,
    /// The page's own requests answered whole, oldest first.
    exchanges: Mutex<Vec<PageExchange>>,
    /// Whether the page's own requests are recorded: only once
    /// [`ObserveTab::record_exchanges`] asked, so that a tab whose
    /// exchanges nobody takes fetches no answer (decision 0195).
    recording: AtomicBool,
    /// How many requests of the page were left out at [`EXCHANGES_KEPT`]
    /// since the exchanges were last taken.
    dropped: AtomicUsize,
    /// What the request of the page's own document held, for telling the
    /// browser's headers from the page's.
    document: Mutex<Option<DocumentHeaders>>,
    safe: Option<SafeRequests>,
    /// Whether the tab hears no network event (a sign-in tab).
    quiet: bool,
    /// Whether the tab is a watching tab: it records every request of its
    /// page from the browser's network events, and the screens it shows.
    watching: bool,
    /// The screens a watching tab's page showed, by structure.
    screens: Mutex<watching::Screens>,
    /// How many addresses a watching tab has looked at: a snapshot taken,
    /// missed, or past the bound.
    looked: watch::Sender<usize>,
    /// The origins the tab was opened for.
    origins: Origins,
    /// The helper's source, with those origins written in.
    helper: String,
    /// The script that holds what interception cannot see: every socket
    /// refused in an observing tab, and in an acting tab every socket
    /// outside the tab's reach.
    sockets: String,
    moves: Mutex<Moves>,
    /// Whether the tab has been released. Kept apart from its status,
    /// which the tab's events also end: the release is done once, whatever
    /// the status says by then.
    released: AtomicBool,
    /// Whether the page is being probed after a call it did not answer.
    probing: AtomicBool,
    /// How long the probe waits for the page's answer.
    hang_probe: Duration,
    /// Whether the browser has been asked to close the page of a hung tab,
    /// and has answered or not by its deadline.
    pub(crate) hang_closed: watch::Sender<bool>,
    /// Whether the task that hears the tab's events has ended: the browser
    /// detached the tab, or the link closed.
    ended: watch::Sender<bool>,
    browser: Arc<BrowserInner>,
}

mod downloads;
mod site_tools;
mod watching;
use downloads::DownloadState;
pub use downloads::{Download, DownloadError};
use site_tools::SiteState;
pub use site_tools::{SiteAnswer, SiteTool, digest_of};
pub use watching::{
    LONGEST_WATCHED_BODY, SCREENS_KEPT, SETTLE_BOUND, WatchStart, WatchTab, WatchedScreens,
};

/// A call to the page that has not been answered. Dropped unanswered, at
/// its deadline or because its caller was dropped, it has the page probed
/// ([`Page::suspect`]).
struct Unanswered<'a> {
    page: &'a Page,
    armed: bool,
}

impl<'a> Unanswered<'a> {
    fn new(page: &'a Page) -> Self {
        Self { page, armed: true }
    }

    /// The page answered, or the call ended for a reason with a name.
    fn answered(mut self) {
        self.armed = false;
    }
}

impl Drop for Unanswered<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.page.suspect();
        }
    }
}

/// Clears the mark of a `goto` under way when the `goto` ends, however it
/// ends.
struct Going<'a>(&'a Page);

impl Drop for Going<'_> {
    fn drop(&mut self) {
        self.0.moves().going = false;
    }
}

/// A call that could not be made as asked.
enum CallError {
    /// The execution context, or the object, is of an earlier page.
    Gone,
    /// The helper is not in the context.
    NoHelper,
    Browser(BrowserError),
}

impl From<BrowserError> for CallError {
    fn from(error: BrowserError) -> Self {
        Self::Browser(error)
    }
}

/// Where a helper function runs.
#[derive(Clone, Copy)]
enum On<'a> {
    World(i64),
    Element(&'a str),
}

/// A text of the page or of the browser, as far as it may leave the crate
/// in a message: every known secret replaced, and only then cut. Cut
/// first, a secret that the cut divides would leave its first part behind
/// (decision 0143).
pub(crate) fn shown(redact: &dyn Redact, text: &str) -> String {
    cut(&redact.redact(text))
}

fn cut(text: &str) -> String {
    const SHOWN: usize = 200;
    let clean: String = text
        .chars()
        .filter(|c| !c.is_control())
        .take(SHOWN)
        .collect();
    if text.chars().count() > SHOWN {
        format!("{clean}…")
    } else {
        clean
    }
}

/// An address without its query and fragment: a query is where tokens
/// travel.
fn without_query(address: &str) -> &str {
    address.split(['?', '#']).next().unwrap_or(address)
}

fn without_fragment(address: &str) -> &str {
    address.split('#').next().unwrap_or(address)
}

/// Whether a node the browser described is a password input: by its type,
/// or by an `autocomplete` that names a password.
fn is_password(node: &NodeDescription) -> bool {
    node.local_name.eq_ignore_ascii_case("input")
        && (node
            .attribute("type")
            .is_some_and(|kind| kind.eq_ignore_ascii_case("password"))
            || node
                .attribute("autocomplete")
                .is_some_and(|hint| hint.to_ascii_lowercase().contains("password")))
}

impl Page {
    fn shown(&self, text: &str) -> String {
        shown(self.redact.as_ref(), text)
    }

    /// The tab's moves, also after a panic elsewhere poisoned the lock:
    /// the state is plain data and every change to it is whole.
    fn moves(&self) -> MutexGuard<'_, Moves> {
        self.moves
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The error for a page, or an address, outside the tab's origins.
    fn outside(&self, origin: &str) -> BrowserError {
        BrowserError::ForeignOrigin(self.shown(origin))
    }

    fn client_error(&self, error: ClientError) -> BrowserError {
        match error {
            ClientError::Protocol { message, .. } => BrowserError::Unexpected(self.shown(&message)),
            ClientError::Closed { dispatch, .. } => BrowserError::Disconnected { dispatch },
            ClientError::Timeout { dispatch } => BrowserError::Timeout { dispatch },
            ClientError::Shape { method } => {
                BrowserError::Unexpected(format!("the reply of {method} has another shape"))
            }
        }
    }

    fn interrupted(status: Status, heed_dialog: bool, dispatch: Dispatch) -> Option<BrowserError> {
        match status {
            Status::Crashed => Some(BrowserError::Crashed { dispatch }),
            Status::Gone => Some(BrowserError::Disconnected { dispatch }),
            Status::Hung => Some(BrowserError::Timeout { dispatch }),
            Status::Dialog if heed_dialog => Some(BrowserError::UnexpectedDialog { dispatch }),
            Status::Dialog | Status::Open => None,
        }
    }

    /// One protocol call on this tab's session. It ends at its reply, at
    /// the deadline, or when the page crashes, the browser goes, or a
    /// dialog opens; each ending says whether the request was written.
    async fn raw<C: Command>(
        &self,
        command: &C,
        deadline: Deadline,
        heed_dialog: bool,
    ) -> Result<C::Reply, Ended> {
        let mut status = self.status.subscribe();
        let now = *status.borrow_and_update();
        if let Some(error) = Self::interrupted(now, heed_dialog, Dispatch::NotSent) {
            return Err(Ended::Browser(error));
        }
        let mut flight = self
            .client
            .begin(Some(&self.session), command)
            .map_err(Ended::Client)?;
        // From here, a call that ends at its deadline, or is dropped before
        // its reply, has the page probed: a page that does not answer is
        // hung, and its tab is closed.
        let unanswered = Unanswered::new(self);
        loop {
            tokio::select! {
                biased;
                reply = flight.finish(deadline) => {
                    if !matches!(reply, Err(ClientError::Timeout { .. })) {
                        unanswered.answered();
                    }
                    return reply.map_err(Ended::Client);
                }
                changed = status.changed() => {
                    let now = if changed.is_ok() { *status.borrow_and_update() } else { Status::Gone };
                    if let Some(error) = Self::interrupted(now, heed_dialog, flight.dispatch()) {
                        unanswered.answered();
                        return Err(Ended::Browser(error));
                    }
                }
            }
        }
    }

    /// The page did not answer a call by its deadline, or the call was
    /// dropped before its reply. In a tab the engine drives, the page is
    /// asked for nothing but an answer, within the settings' probe time,
    /// on a task of its own, so that the caller's step ends at its own
    /// deadline. A page that answers is slow, not hung, and its tab goes
    /// on. A tab a person uses is never probed or closed by the engine.
    fn suspect(&self) {
        if self.quiet || self.watching || self.released.load(Ordering::SeqCst) {
            return;
        }
        if *self.status.borrow() != Status::Open || self.probing.swap(true, Ordering::SeqCst) {
            return;
        }
        let page = self
            .browser
            .pages()
            .get(&self.target)
            .and_then(Weak::upgrade);
        match (page, tokio::runtime::Handle::try_current()) {
            (Some(page), Ok(runtime)) => {
                runtime.spawn(async move { page.probe().await });
            }
            _ => self.probing.store(false, Ordering::SeqCst),
        }
    }

    /// Asks the page for a constant, in the helper's world where there is
    /// one: it runs nothing of the page's and reads nothing. Only a page
    /// whose own script holds it gives no answer.
    async fn probe(&self) {
        let world = *self.world.borrow();
        let answered = self
            .client
            .call(
                Some(&self.session),
                &Evaluate {
                    expression: "0",
                    context_id: world,
                    return_by_value: true,
                    await_promise: false,
                },
                Deadline::after(self.hang_probe),
            )
            .await;
        match answered {
            Err(ClientError::Timeout { .. }) => self.hang().await,
            _ => self.probing.store(false, Ordering::SeqCst),
        }
    }

    /// Closes a hung tab: every call on it is a timeout from now on, its
    /// events stop, the browser is asked to close its page and the windows
    /// it opened, and the hang is counted against the browser. The handle
    /// stays held by whoever opened the tab until it is dropped, so that
    /// the browser is not recycled under a run that is still going.
    async fn hang(&self) {
        let fresh = self.status.send_if_modified(|status| {
            let open = *status == Status::Open;
            if open {
                *status = Status::Hung;
            }
            open
        });
        if !fresh {
            return;
        }
        self.browser.hangs.fetch_add(1, Ordering::SeqCst);
        let deadline = Deadline::after(self.call_deadline);
        // The page's script is ended first: a renderer held by its own
        // script does not end with its last tab, and would go on running.
        let _ = self
            .client
            .call(Some(&self.session), &TerminateExecution {}, deadline)
            .await;
        // The tab's events stop: this listener takes the session's place,
        // and its end is the browser's word that the session is detached,
        // which it says once the page is gone. Until then it fails every
        // request the page still makes: one left paused is sent when the
        // session is detached (decision 0461).
        let mut session = self.client.subscribe(Some(&self.session));
        let mut closing = vec![self.target.clone()];
        closing.extend(self.browser.windows_of(&self.target));
        let close = async {
            for target in closing {
                let _ = self
                    .client
                    .call(None, &CloseTarget { target_id: &target }, deadline)
                    .await;
            }
        };
        let refuse = async {
            while let Some(event) = session.recv().await {
                if let EventKind::RequestPaused(paused) = event.kind {
                    let _ = self
                        .client
                        .call(
                            event.session.as_deref(),
                            &FailRequest {
                                request_id: &paused.request_id,
                                error_reason: BLOCKED,
                            },
                            deadline,
                        )
                        .await;
                }
            }
        };
        let _ = tokio::time::timeout_at(deadline.at(), async { tokio::join!(close, refuse) }).await;
        self.client.unsubscribe(&self.session);
        self.hang_closed.send_replace(true);
    }

    pub(crate) async fn call<C: Command>(
        &self,
        command: &C,
        deadline: Deadline,
    ) -> Result<C::Reply, BrowserError> {
        self.raw(command, deadline, true)
            .await
            .map_err(|error| match error {
                Ended::Browser(error) => error,
                Ended::Client(error) => self.client_error(error),
            })
    }

    /// Runs one helper function and gives what it returned.
    async fn evaluate(
        &self,
        on: On<'_>,
        function: &str,
        arguments: Vec<CallArgument>,
        by_value: bool,
        await_promise: bool,
        deadline: Deadline,
    ) -> Result<Evaluated, CallError> {
        let (object_id, execution_context_id) = match on {
            On::World(world) => (None, Some(world)),
            On::Element(object) => (Some(object), None),
        };
        let command = CallFunctionOn {
            function_declaration: function,
            object_id,
            execution_context_id,
            arguments,
            return_by_value: by_value,
            await_promise,
        };
        let evaluated = match self.raw(&command, deadline, true).await {
            Ok(evaluated) => evaluated,
            Err(Ended::Browser(error)) => return Err(error.into()),
            Err(Ended::Client(ClientError::Protocol { message, .. })) if gone(&message) => {
                return Err(CallError::Gone);
            }
            Err(Ended::Client(error)) => return Err(self.client_error(error).into()),
        };
        match &evaluated.exception_details {
            None => Ok(evaluated),
            Some(details) => {
                let text = format!(
                    "{} {}",
                    details.text,
                    evaluated.result.description.as_deref().unwrap_or_default()
                );
                if text.contains(OUTSIDE_ORIGINS) {
                    // The helper found the page on an origin the tab was
                    // not opened for, and did nothing. The whole text is
                    // redacted before the origin is taken out of it.
                    let whole = self.redact.redact(&text);
                    let origin = whole
                        .split_once(OUTSIDE_ORIGINS)
                        .and_then(|(_, after)| after.split_whitespace().next())
                        .unwrap_or_default();
                    Err(BrowserError::ForeignOrigin(cut(origin)).into())
                } else if text.contains(PASSWORD_REFUSED) {
                    Err(BrowserError::PasswordField.into())
                } else if text.contains("__a2m") {
                    Err(CallError::NoHelper)
                } else if gone(&text) {
                    Err(CallError::Gone)
                } else {
                    Err(BrowserError::Unexpected(self.shown(&text)).into())
                }
            }
        }
    }

    /// The helper's world of the present document, waiting for it while a
    /// new document is on its way.
    async fn world(&self, deadline: Deadline) -> Result<i64, BrowserError> {
        let mut world = self.world.subscribe();
        let mut status = self.status.subscribe();
        loop {
            if let Some(error) =
                Self::interrupted(*status.borrow_and_update(), true, Dispatch::NotSent)
            {
                return Err(error);
            }
            if let Some(found) = *world.borrow_and_update() {
                return Ok(found);
            }
            tokio::select! {
                changed = world.changed() => {
                    if changed.is_err() {
                        return Err(BrowserError::Disconnected { dispatch: Dispatch::NotSent });
                    }
                }
                _ = status.changed() => {}
                () = tokio::time::sleep_until(deadline.at()) => {
                    return Err(BrowserError::Timeout { dispatch: Dispatch::NotSent });
                }
            }
        }
    }

    /// Runs a helper function in the helper's world. A read: when the
    /// document is replaced under it, it is asked again in the new one.
    async fn in_world(
        &self,
        function: &str,
        arguments: impl Fn() -> Option<Vec<CallArgument>>,
        by_value: bool,
        await_promise: bool,
        deadline: Deadline,
    ) -> Result<Evaluated, BrowserError> {
        let shape =
            || BrowserError::Unexpected("a helper argument could not be written".to_owned());
        loop {
            let world = self.world(deadline).await?;
            let outcome = self
                .evaluate(
                    On::World(world),
                    function,
                    arguments().ok_or_else(shape)?,
                    by_value,
                    await_promise,
                    deadline,
                )
                .await;
            match outcome {
                Ok(evaluated) => return Ok(evaluated),
                Err(CallError::Browser(error)) => return Err(error),
                Err(CallError::NoHelper) => {
                    // The world was made before the helper reached it.
                    let install = Evaluate {
                        expression: &self.helper,
                        context_id: Some(world),
                        return_by_value: true,
                        await_promise: false,
                    };
                    match self.raw(&install, deadline, true).await {
                        Ok(installed) if installed.exception_details.is_none() => {}
                        Ok(_) => {
                            return Err(BrowserError::Unexpected(
                                "the helper could not be placed in the page".to_owned(),
                            ));
                        }
                        Err(Ended::Browser(error)) => return Err(error),
                        Err(Ended::Client(ClientError::Protocol { message, .. }))
                            if gone(&message) =>
                        {
                            self.forget_world(world);
                        }
                        Err(Ended::Client(error)) => return Err(self.client_error(error)),
                    }
                }
                Err(CallError::Gone) => self.forget_world(world),
            }
        }
    }

    /// Marks a world as gone, unless a newer one has already replaced it.
    fn forget_world(&self, world: i64) {
        self.world.send_if_modified(|current| {
            if *current == Some(world) {
                *current = None;
                true
            } else {
                false
            }
        });
    }

    /// Runs a helper function on an element. A stale element is not found.
    async fn on_element(
        &self,
        element: &Element,
        function: &str,
        arguments: Vec<CallArgument>,
        deadline: Deadline,
    ) -> Result<Evaluated, BrowserError> {
        if element.tab != self.serial {
            return Err(BrowserError::NotFound);
        }
        match self
            .evaluate(
                On::Element(&element.object),
                function,
                arguments,
                true,
                false,
                deadline,
            )
            .await
        {
            Ok(evaluated) => Ok(evaluated),
            Err(CallError::Gone | CallError::NoHelper) => Err(BrowserError::NotFound),
            Err(CallError::Browser(error)) => Err(error),
        }
    }

    /// Whether the element left its document within `within`: `true` once
    /// it has, or once its document was replaced under the wait; `false`
    /// when the time ends first. The page waits, and is asked once.
    async fn gone(
        &self,
        element: &Element,
        within: std::time::Duration,
        deadline: Deadline,
    ) -> Result<bool, BrowserError> {
        if element.tab != self.serial {
            return Ok(true);
        }
        let millis = usize::try_from(within.as_millis()).unwrap_or(usize::MAX);
        match self
            .evaluate(
                On::Element(&element.object),
                "function (timeout) { return __a2m.gone(this, timeout); }",
                vec![CallArgument::number(millis)],
                true,
                true,
                deadline,
            )
            .await
        {
            Ok(evaluated) => Self::answer(evaluated),
            Err(CallError::Gone | CallError::NoHelper) => Ok(true),
            Err(CallError::Browser(error)) => Err(error),
        }
    }

    fn answer<T: DeserializeOwned>(evaluated: Evaluated) -> Result<T, BrowserError> {
        evaluated.result.read().ok_or_else(|| {
            BrowserError::Unexpected("the helper answered in a shape no rule covers".to_owned())
        })
    }

    // ---- looking ----

    async fn goto(&self, address: &Address, deadline: Deadline) -> Result<Loaded, BrowserError> {
        // Refused before the browser hears of it.
        if !self.origins.allows(address) {
            return Err(self.outside(&address.origin()));
        }
        self.goto_text(address.as_str(), deadline).await
    }

    pub(crate) async fn goto_text(
        &self,
        address: &str,
        deadline: Deadline,
    ) -> Result<Loaded, BrowserError> {
        let mut nav = self.nav.subscribe();
        let mut status = self.status.subscribe();
        {
            let mut moves = self.moves();
            moves.going = true;
            moves.goto_loader = None;
            moves.goto_request = None;
            moves.refused = None;
        }
        let _going = Going(self);
        let navigated = self.call(&Navigate { url: address }, deadline).await?;
        if let Some(why) = navigated.error_text.filter(|why| !why.is_empty()) {
            // The tab itself ended the move, at a redirect to an origin it
            // was not opened for: that is the reason, not the browser's.
            let refused = self.moves().refused.take();
            return Err(match refused {
                Some(origin) => self.outside(&origin),
                None => BrowserError::NotLoaded(self.shown(&why)),
            });
        }
        let mut document_status = None;
        if let Some(loader) = navigated.loader_id {
            // A document that is not parsed by the deadline may be held by
            // its own script: the page is probed then.
            let unanswered = Unanswered::new(self);
            // The document of this navigation has been parsed: a protocol
            // event, not a guess at how long a page takes.
            loop {
                if nav.borrow_and_update().loaded.as_deref() == Some(loader.as_str()) {
                    unanswered.answered();
                    break;
                }
                if let Some(error) =
                    Self::interrupted(*status.borrow_and_update(), true, Dispatch::Sent)
                {
                    unanswered.answered();
                    return Err(error);
                }
                tokio::select! {
                    changed = nav.changed() => {
                        if changed.is_err() {
                            unanswered.answered();
                            return Err(BrowserError::Disconnected { dispatch: Dispatch::Sent });
                        }
                    }
                    _ = status.changed() => {}
                    () = tokio::time::sleep_until(deadline.at()) => {
                        return Err(BrowserError::Timeout { dispatch: Dispatch::Sent });
                    }
                }
            }
            document_status = nav
                .borrow()
                .document
                .as_ref()
                .filter(|(of, _, _)| *of == loader)
                .map(|(_, status, challenged)| (*status, *challenged));
        }
        // A tab a person signs in in may be on a page whose origin its
        // helper does not look at: the browser says where it is, and the
        // page's title is not read.
        let (address, title) = if self.quiet {
            let info = self
                .client
                .call(
                    None,
                    &GetTargetInfo {
                        target_id: &self.target,
                    },
                    deadline,
                )
                .await
                .map_err(|error| self.client_error(error))?;
            (self.redact.redact(&info.target_info.url), String::new())
        } else {
            self.address_and_title(deadline).await?
        };
        Ok(Loaded {
            address,
            title,
            status: document_status.map(|(status, _)| status),
            challenged: document_status.is_some_and(|(_, challenged)| challenged),
        })
    }

    pub(crate) async fn address_and_title(
        &self,
        deadline: Deadline,
    ) -> Result<(String, String), BrowserError> {
        #[derive(Deserialize)]
        struct Now {
            address: String,
            title: String,
        }
        let evaluated = self
            .in_world(
                "function () { return __a2m.page(); }",
                || Some(Vec::new()),
                true,
                false,
                deadline,
            )
            .await?;
        let now: Now = Self::answer(evaluated)?;
        Ok((
            self.redact.redact(&now.address),
            self.redact.redact(&now.title),
        ))
    }

    async fn locate(&self, query: &Query, deadline: Deadline) -> Result<Element, BrowserError> {
        let evaluated = self
            .in_world(
                "function (query) { return __a2m.locate(query); }",
                || Some(vec![CallArgument::of(query)?]),
                false,
                false,
                deadline,
            )
            .await?;
        let found = evaluated.result;
        if let (Some("node"), Some(object)) = (found.subtype.as_deref(), found.object_id.clone()) {
            return Ok(Element {
                object,
                tab: self.serial,
                query: query.clone(),
            });
        }
        match found.count() {
            Some(0) => Err(BrowserError::NotFound),
            Some(count) => Err(BrowserError::Ambiguous { count }),
            None => Err(BrowserError::Unexpected(
                "the helper answered a locate with neither an element nor a count".to_owned(),
            )),
        }
    }

    async fn read(
        &self,
        element: &Element,
        what: &ReadWhat,
        deadline: Deadline,
    ) -> Result<RawValue, BrowserError> {
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Read {
            stale: bool,
            password: bool,
            absent: bool,
            too_large: bool,
            flag: Option<bool>,
            text: Option<String>,
        }
        let arguments = vec![
            CallArgument::of(what).ok_or_else(|| {
                BrowserError::Unexpected("a helper argument could not be written".to_owned())
            })?,
            CallArgument::number(self.max_read),
            // A page that drew the element again since it was found is read
            // in the element its query finds now (decision 0379).
            CallArgument::of(&element.query).ok_or_else(|| {
                BrowserError::Unexpected("a helper argument could not be written".to_owned())
            })?,
        ];
        let evaluated = self
            .on_element(
                element,
                "function (what, limit, query) { return __a2m.read(this, what, limit, query); }",
                arguments,
                deadline,
            )
            .await?;
        let read: Read = Self::answer(evaluated)?;
        match read {
            Read { stale: true, .. } => Err(BrowserError::NotFound),
            Read { password: true, .. } => Err(BrowserError::PasswordField),
            Read {
                too_large: true, ..
            } => Err(BrowserError::TooLarge {
                limit: self.max_read,
            }),
            Read { absent: true, .. } => Ok(RawValue::Absent),
            Read {
                flag: Some(flag), ..
            } => Ok(RawValue::Flag(flag)),
            Read {
                text: Some(text), ..
            } => Ok(RawValue::Text(self.redact.redact(&text))),
            Read { .. } => Err(BrowserError::Unexpected(
                "the helper answered a read with nothing".to_owned(),
            )),
        }
    }

    async fn wait(&self, until: &Primitive, deadline: Deadline) -> Result<(), BrowserError> {
        if self.until(until, true, deadline).await? {
            Ok(())
        } else {
            Err(BrowserError::Timeout {
                dispatch: Dispatch::NotSent,
            })
        }
    }

    /// Whether the condition holds now, on the page's own word: the page
    /// looks once and answers at once. The deadline bounds only the
    /// browser's answer, so a slow machine is never read as "does not
    /// hold".
    async fn holds(&self, condition: &Primitive, deadline: Deadline) -> Result<bool, BrowserError> {
        self.until(condition, false, deadline).await
    }

    /// The page's answer to a condition: `true` once it holds. When
    /// `page_waits`, the page waits for it until the deadline; otherwise
    /// it looks once, and `false` is its word that the condition does not
    /// hold now.
    async fn until(
        &self,
        until: &Primitive,
        page_waits: bool,
        deadline: Deadline,
    ) -> Result<bool, BrowserError> {
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Waited {
            ok: bool,
            error: Option<String>,
        }
        let condition = Condition::from(until);
        // `in_world` asks again when the document is replaced under the
        // wait: the wait is re-armed by that event, in the new document.
        let evaluated = self
            .in_world(
                "function (condition, timeout) { return __a2m.until(condition, timeout); }",
                || {
                    let left = if page_waits {
                        u64::try_from(deadline.remaining().as_millis()).unwrap_or(u64::MAX)
                    } else {
                        0
                    };
                    Some(vec![
                        CallArgument::of(&condition)?,
                        CallArgument::number(usize::try_from(left).unwrap_or(usize::MAX)),
                    ])
                },
                true,
                true,
                deadline,
            )
            .await?;
        let waited: Waited = Self::answer(evaluated)?;
        match waited {
            Waited { ok: true, .. } => Ok(true),
            Waited {
                error: Some(error), ..
            } if error.contains(PASSWORD_REFUSED) => Err(BrowserError::PasswordField),
            Waited {
                error: Some(error), ..
            } => Err(BrowserError::Unexpected(self.shown(&error))),
            Waited { .. } => Ok(false),
        }
    }

    async fn snapshot(
        &self,
        limits: &SnapshotLimits,
        deadline: Deadline,
    ) -> Result<Snapshot, BrowserError> {
        let (address, title) = self.address_and_title(deadline).await?;
        // Before the tree is taken: the password inputs the helper sees.
        let mut masked = self.password_nodes(deadline).await?;
        let tree = self.call(&GetFullAxTree {}, deadline).await?;
        // After it: every node that carries a value is asked of the
        // browser itself, which also sees inside a closed shadow root. A
        // node that is a password input at either moment is masked, and
        // so is one the browser can no longer describe.
        for node in &tree.nodes {
            let valued = node
                .value
                .as_ref()
                .is_some_and(|value| !value.text().is_empty());
            let Some(number) = node.backend_dom_node_id.filter(|_| valued) else {
                continue;
            };
            if masked.contains(&number) {
                continue;
            }
            let described = self
                .call(
                    &DescribeNode {
                        object_id: None,
                        backend_node_id: Some(number),
                    },
                    deadline,
                )
                .await;
            match described {
                Ok(described) if !is_password(&described.node) => {}
                Ok(_) | Err(BrowserError::Unexpected(_)) => {
                    masked.insert(number);
                }
                Err(error) => return Err(error),
            }
        }
        Ok(Snapshot::build(
            // A query is where tokens travel, and a snapshot is kept.
            without_query(&address).to_owned(),
            title,
            &tree.nodes,
            &masked,
            limits,
            self.redact.as_ref(),
        ))
    }

    /// The nodes of the page's password inputs, by the browser's own
    /// number for each, so that a snapshot masks exactly those. Their
    /// values are not read.
    async fn password_nodes(&self, deadline: Deadline) -> Result<BTreeSet<i64>, BrowserError> {
        let listed = self
            .in_world(
                "function () { return __a2m.passwords(); }",
                || Some(Vec::new()),
                false,
                false,
                deadline,
            )
            .await?;
        let mut nodes = BTreeSet::new();
        let Some(list) = listed.result.object_id else {
            return Err(BrowserError::Unexpected(
                "the helper did not list the password inputs".to_owned(),
            ));
        };
        if listed.result.description.as_deref() == Some("Array(0)") {
            return Ok(nodes);
        }
        let entries = self
            .call(
                &GetProperties {
                    object_id: &list,
                    own_properties: true,
                },
                deadline,
            )
            .await?;
        for entry in entries.result {
            let element = entry
                .value
                .filter(|_| entry.name.parse::<usize>().is_ok())
                .and_then(|value| value.object_id);
            if let Some(element) = element {
                let described = self
                    .call(
                        &DescribeNode {
                            object_id: Some(&element),
                            backend_node_id: None,
                        },
                        deadline,
                    )
                    .await?;
                nodes.insert(described.node.backend_node_id);
            }
        }
        Ok(nodes)
    }

    // ---- acting ----

    async fn checked(
        &self,
        element: &Element,
        function: &str,
        arguments: Vec<CallArgument>,
        deadline: Deadline,
    ) -> Result<Checked, BrowserError> {
        let evaluated = self
            .on_element(element, function, arguments, deadline)
            .await?;
        let checked: Checked = Self::answer(evaluated)?;
        if checked.stale {
            return Err(BrowserError::NotFound);
        }
        match checked.no.as_deref() {
            None if checked.ok => Ok(checked),
            Some("not_visible") => Err(BrowserError::NotActionable(Reason::NotVisible)),
            Some("disabled") => Err(BrowserError::NotActionable(Reason::Disabled)),
            Some("not_editable") => Err(BrowserError::NotActionable(Reason::NotEditable)),
            Some("obscured") => Err(BrowserError::NotActionable(Reason::Obscured)),
            Some("wrong_control") => Err(BrowserError::NotActionable(Reason::WrongControl)),
            Some("no_such_option") => Err(BrowserError::NotActionable(Reason::NoSuchOption)),
            _ => Err(BrowserError::Unexpected(
                "the helper answered a check with a reason no rule covers".to_owned(),
            )),
        }
    }

    async fn key(&self, key: Key, deadline: Deadline) -> Result<(), BrowserError> {
        let (name, code, virtual_key, text) = key.parts();
        self.call(
            &DispatchKey {
                kind: if text.is_some() {
                    "keyDown"
                } else {
                    "rawKeyDown"
                },
                key: name,
                code: Some(code),
                text,
                windows_virtual_key_code: Some(virtual_key),
            },
            deadline,
        )
        .await?;
        self.call(
            &DispatchKey {
                kind: "keyUp",
                key: name,
                code: Some(code),
                text: None,
                windows_virtual_key_code: Some(virtual_key),
            },
            deadline,
        )
        .await
        .map_err(after_sent)?;
        Ok(())
    }

    async fn click_at(&self, x: i64, y: i64, deadline: Deadline) -> Result<(), BrowserError> {
        let mouse = |kind: &'static str, button: &'static str, click_count| DispatchMouse {
            kind,
            x,
            y,
            button,
            click_count,
        };
        self.call(&mouse("mouseMoved", "none", 0), deadline).await?;
        self.call(&mouse("mousePressed", "left", 1), deadline)
            .await
            .map_err(after_sent)?;
        self.call(&mouse("mouseReleased", "left", 1), deadline)
            .await
            .map_err(after_sent)?;
        Ok(())
    }

    async fn input(
        &self,
        element: &Element,
        how: &Input,
        deadline: Deadline,
    ) -> Result<(), BrowserError> {
        const CHECK: &str = "function (need) { return __a2m.check(this, need); }";
        const PREPARE: &str = "function () { return __a2m.prepareFill(this); }";
        // Checking and focusing send nothing a system could store, so a
        // failure there says the input was not sent.
        match how {
            Input::Fill(text) => {
                self.checked(element, PREPARE, Vec::new(), deadline)
                    .await
                    .map_err(looked)?;
                if text.is_empty() {
                    self.key(Key::Delete, deadline).await
                } else {
                    self.call(&InsertText { text }, deadline).await.map(|_| ())
                }
            }
            Input::Type(text) => {
                self.checked(element, PREPARE, Vec::new(), deadline)
                    .await
                    .map_err(looked)?;
                if text.is_empty() {
                    return self.key(Key::Delete, deadline).await;
                }
                let mut sent = false;
                let mut buffer = [0_u8; 4];
                for character in text.chars() {
                    let typed: &str = character.encode_utf8(&mut buffer);
                    let stroke = |kind: &'static str, text| DispatchKey {
                        kind,
                        key: typed,
                        code: None,
                        text,
                        windows_virtual_key_code: None,
                    };
                    let down = self.call(&stroke("keyDown", Some(typed)), deadline).await;
                    down.map_err(|error| if sent { after_sent(error) } else { error })?;
                    sent = true;
                    self.call(&stroke("keyUp", None), deadline)
                        .await
                        .map_err(after_sent)?;
                }
                Ok(())
            }
            Input::NativeSet(text) => self
                .checked(
                    element,
                    "function (text) { return __a2m.nativeSet(this, text); }",
                    vec![CallArgument::text(text)],
                    deadline,
                )
                .await
                .map(|_| ()),
            Input::Blur => self
                .checked(
                    element,
                    "function () { return __a2m.blur(this); }",
                    Vec::new(),
                    deadline,
                )
                .await
                .map(|_| ()),
            Input::SelectByValue(value) => self
                .checked(
                    element,
                    "function (value) { return __a2m.selectByValue(this, value); }",
                    vec![CallArgument::text(value)],
                    deadline,
                )
                .await
                .map(|_| ()),
            Input::Attach(path) => {
                // Only a file input takes a file; nothing is sent to any
                // other element.
                self.checked(
                    element,
                    "function () { return __a2m.fileInput(this); }",
                    Vec::new(),
                    deadline,
                )
                .await
                .map_err(looked)?;
                let files = [path.to_string_lossy().into_owned()];
                self.call(
                    &SetFileInputFiles {
                        object_id: &element.object,
                        files: &files,
                    },
                    deadline,
                )
                .await
                .map(|_| ())
                .map_err(after_sent)
            }
            Input::Press(key) => {
                self.checked(element, PREPARE_FOCUS, Vec::new(), deadline)
                    .await
                    .map_err(looked)?;
                self.key(*key, deadline).await
            }
            Input::Click => {
                let at = self
                    .checked(element, CHECK, vec![CallArgument::text("click")], deadline)
                    .await
                    .map_err(looked)?;
                self.click_at(at.x, at.y, deadline).await
            }
            Input::Check(want) => {
                let at = self
                    .checked(element, CHECK, vec![CallArgument::text("check")], deadline)
                    .await
                    .map_err(looked)?;
                if at.checked == Some(*want) {
                    return Ok(());
                }
                self.click_at(at.x, at.y, deadline).await
            }
        }
    }

    pub(crate) async fn fetch(
        &self,
        request: &PageRequest,
        deadline: Deadline,
    ) -> Result<PageResponse, BrowserError> {
        #[derive(Serialize)]
        struct Spec<'a> {
            method: &'a str,
            address: &'a str,
            headers: &'a [(String, String)],
            body: Option<&'a str>,
        }
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Answer {
            too_large: bool,
            redirected: bool,
            status: Option<u16>,
            address: String,
            headers: Vec<(String, String)>,
            body: String,
        }
        let shape =
            || BrowserError::Unexpected("a helper argument could not be written".to_owned());
        // Refused before anything is sent: the request would carry the
        // page's session to an origin the tab was not opened for.
        if !self.origins.allows(&request.address) {
            return Err(self.outside(&request.address.origin()));
        }
        let spec = Spec {
            method: &request.method,
            address: request.address.as_str(),
            headers: &request.headers,
            body: request.body.as_deref(),
        };
        // Sent once. A document replaced under it is not a reason to send
        // it again: what became of the request is then not known.
        let world = self.world(deadline).await?;
        let outcome = self
            .evaluate(
                On::World(world),
                "function (spec, limit) { return __a2m.request(spec, limit); }",
                vec![
                    CallArgument::of(&spec).ok_or_else(shape)?,
                    CallArgument::number(self.max_read),
                ],
                true,
                true,
                deadline,
            )
            .await;
        let evaluated = match outcome {
            Ok(evaluated) => evaluated,
            Err(CallError::Browser(BrowserError::Unexpected(why))) => {
                return Err(BrowserError::RequestFailed(why));
            }
            Err(CallError::Browser(error)) => return Err(error),
            Err(CallError::Gone | CallError::NoHelper) => {
                return Err(BrowserError::RequestFailed(
                    "the page was replaced while the request was under way".to_owned(),
                ));
            }
        };
        let answer: Answer = Self::answer(evaluated)?;
        if answer.redirected {
            // The request was answered, by a redirect. Its next address
            // was checked by nobody, so it is not followed; what the first
            // request did is for the crate above to find out.
            return Err(BrowserError::RequestFailed(
                "the answer was a redirect, and a request from the page follows none".to_owned(),
            ));
        }
        let Some(status) = answer.status else {
            return Err(if answer.too_large {
                BrowserError::TooLarge {
                    limit: self.max_read,
                }
            } else {
                BrowserError::Unexpected("the helper answered a request with no status".to_owned())
            });
        };
        let mut headers: Vec<(String, String)> = answer
            .headers
            .into_iter()
            .filter(|(name, _)| !credential_header(name))
            .map(|(name, value)| (self.redact.redact(&name), self.redact.redact(&value)))
            .collect();
        headers.sort();
        Ok(PageResponse {
            status,
            address: without_query(&self.redact.redact(&answer.address)).to_owned(),
            headers,
            body: self.redact.redact(&answer.body),
        })
    }

    /// A read from inside the page, with GET, to a checked path on the
    /// page's own origin (decision 0190). The page's origin is checked
    /// against the tab's in the same call that sends it.
    async fn read_from_page(
        &self,
        path: &PagePath,
        deadline: Deadline,
    ) -> Result<PageAnswer, BrowserError> {
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Answer {
            too_large: bool,
            redirected: bool,
            status: Option<u16>,
            media_type: String,
            body: String,
        }
        let world = self.world(deadline).await?;
        let outcome = self
            .evaluate(
                On::World(world),
                "function (path, limit) { return __a2m.readFromPage(path, limit); }",
                vec![
                    CallArgument::text(path.as_str()),
                    CallArgument::number(self.max_read),
                ],
                true,
                true,
                deadline,
            )
            .await;
        let evaluated = match outcome {
            Ok(evaluated) => evaluated,
            Err(CallError::Browser(BrowserError::Unexpected(why))) => {
                return Err(BrowserError::RequestFailed(self.shown(&why)));
            }
            Err(CallError::Browser(error)) => return Err(error),
            Err(CallError::Gone | CallError::NoHelper) => {
                return Err(BrowserError::RequestFailed(
                    "the page was replaced while the read was under way".to_owned(),
                ));
            }
        };
        let answer: Answer = Self::answer(evaluated)?;
        if answer.redirected {
            return Err(BrowserError::Redirected);
        }
        let Some(status) = answer.status else {
            return Err(if answer.too_large {
                BrowserError::TooLarge {
                    limit: self.max_read,
                }
            } else {
                BrowserError::Unexpected("the helper answered a read with no status".to_owned())
            });
        };
        Ok(PageAnswer {
            status,
            media_type: self.redact.redact(&answer.media_type),
            body: self.redact.redact(&answer.body),
            withheld: false,
        })
    }

    /// A write from inside the page, with the page's own anti-forgery
    /// token, which the helper reads and places in the same call that
    /// sends it, and never hands back (decision 0206). Only an acting tab
    /// calls it; in an observing tab the browser fails the request.
    pub(crate) async fn write_from_page(
        &self,
        write: &PageWrite,
        deadline: Deadline,
    ) -> Result<PageAnswer, BrowserError> {
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Answer {
            refused: Option<String>,
            failed: bool,
            too_large: bool,
            redirected: bool,
            status: Option<u16>,
            media_type: String,
            body: String,
            withheld: bool,
        }
        let shape =
            || BrowserError::Unexpected("a helper argument could not be written".to_owned());
        let spec = write.spec();
        let argument = CallArgument::of(&spec).ok_or_else(shape)?;
        let world = self.world(deadline).await?;
        // Sent once. From here, a failure leaves the write's fate unknown,
        // unless the helper says it refused before anything left.
        let outcome = self
            .evaluate(
                On::World(world),
                "function (spec, limit) { return __a2m.writeFromPage(spec, limit); }",
                vec![argument, CallArgument::number(self.max_read)],
                true,
                true,
                deadline,
            )
            .await;
        let lost = |why: &str| BrowserError::RequestFailed(why.to_owned());
        let evaluated = match outcome {
            Ok(evaluated) => evaluated,
            // The helper's own text is not quoted: nothing it says while a
            // write is under way is handed out.
            Err(CallError::Browser(BrowserError::Unexpected(_))) => {
                return Err(lost(
                    "the helper in the page failed while the write was under way",
                ));
            }
            Err(CallError::Browser(error)) => return Err(error),
            Err(CallError::Gone | CallError::NoHelper) => {
                return Err(lost("the page was replaced while the write was under way"));
            }
        };
        let answer: Answer = Self::answer(evaluated)?;
        if let Some(refused) = answer.refused {
            return Err(match refused.as_str() {
                "token_missing" => BrowserError::Token(TokenProblem::Missing),
                "token_several" => BrowserError::Token(TokenProblem::Several),
                "token_unusable" | "token_place" => BrowserError::Token(TokenProblem::Unusable),
                "path" => BrowserError::WriteRefused(
                    "the page has no origin of its own, or the path is not one on it".to_owned(),
                ),
                _ => BrowserError::WriteRefused("the helper does not send such a write".to_owned()),
            });
        }
        if answer.failed {
            return Err(lost("the page could not complete the write"));
        }
        if answer.redirected {
            return Err(BrowserError::Redirected);
        }
        let Some(status) = answer.status else {
            return Err(if answer.too_large {
                BrowserError::TooLarge {
                    limit: self.max_read,
                }
            } else {
                BrowserError::Unexpected("the helper answered a write with no status".to_owned())
            });
        };
        Ok(PageAnswer {
            status,
            media_type: self.redact.redact(&answer.media_type),
            body: if answer.withheld {
                String::new()
            } else {
                self.redact.redact(&answer.body)
            },
            withheld: answer.withheld,
        })
    }

    async fn answer_dialog(
        &self,
        how: &DialogAnswer,
        deadline: Deadline,
    ) -> Result<(), BrowserError> {
        let command = match how {
            DialogAnswer::Accept => HandleDialog {
                accept: true,
                prompt_text: None,
            },
            DialogAnswer::AcceptWith(text) => HandleDialog {
                accept: true,
                prompt_text: Some(text),
            },
            DialogAnswer::Dismiss => HandleDialog {
                accept: false,
                prompt_text: None,
            },
        };
        let mut closed = self.dialogs_closed.subscribe();
        let before = *closed.borrow_and_update();
        self.raw(&command, deadline, false)
            .await
            .map_err(|error| match error {
                Ended::Browser(error) => error,
                Ended::Client(error) => self.client_error(error),
            })?;
        // The browser's answer and its event that the dialog closed travel
        // separately. The tab is usable again when the event has been
        // heard, so the answer is given only then: a call made right after
        // it is not told that a dialog is still open.
        let heard = tokio::time::timeout_at(deadline.at(), closed.wait_for(|now| *now > before));
        match heard.await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(_)) => Err(BrowserError::Disconnected {
                dispatch: Dispatch::Sent,
            }),
            Err(_) => Err(BrowserError::Timeout {
                dispatch: Dispatch::Sent,
            }),
        }
    }

    /// Runs an expression as one of the page's own scripts would run, and
    /// gives its result as text. For this crate's tests only: it stands
    /// for what a page does by itself.
    #[cfg(test)]
    pub(crate) async fn as_the_page(
        &self,
        expression: &str,
        deadline: Deadline,
    ) -> Result<String, BrowserError> {
        let evaluated = self
            .call(
                &Evaluate {
                    expression,
                    context_id: None,
                    return_by_value: true,
                    await_promise: true,
                },
                deadline,
            )
            .await?;
        if let Some(details) = &evaluated.exception_details {
            return Err(BrowserError::Unexpected(format!(
                "{} {}",
                details.text,
                evaluated.result.description.as_deref().unwrap_or_default()
            )));
        }
        Ok(evaluated.result.read::<String>().unwrap_or_default())
    }

    /// The same, run as if a person had just used the page: what a page
    /// needs before the browser lets it open a window. For this crate's
    /// tests only.
    #[cfg(test)]
    pub(crate) async fn as_the_page_with_a_gesture(
        &self,
        expression: &str,
        deadline: Deadline,
    ) -> Result<String, BrowserError> {
        let evaluated = self
            .call(
                &crate::protocol::EvaluateAsUser {
                    expression,
                    return_by_value: true,
                    await_promise: true,
                    user_gesture: true,
                },
                deadline,
            )
            .await?;
        Ok(evaluated.result.read::<String>().unwrap_or_default())
    }

    /// One request that never answers, for the test that kills the browser
    /// while a call is pending.
    #[cfg(test)]
    pub(crate) fn begin_endless(
        &self,
    ) -> Result<crate::client::InFlight<Evaluate<'static>>, ClientError> {
        self.client.begin(
            Some(&self.session),
            &Evaluate {
                expression: "new Promise(() => {})",
                context_id: None,
                return_by_value: true,
                await_promise: true,
            },
        )
    }

    /// Waits until the count of changes grows past `seen`.
    pub(crate) async fn changed_since(
        &self,
        seen: u64,
        deadline: Deadline,
    ) -> Result<(), BrowserError> {
        let mut changes = self.changes.subscribe();
        let mut status = self.status.subscribe();
        loop {
            if *changes.borrow_and_update() > seen {
                return Ok(());
            }
            if let Some(error) =
                Page::interrupted(*status.borrow_and_update(), false, Dispatch::NotSent)
            {
                return Err(error);
            }
            tokio::select! {
                changed = changes.changed() => {
                    if changed.is_err() {
                        return Err(BrowserError::Disconnected { dispatch: Dispatch::NotSent });
                    }
                }
                _ = status.changed() => {}
                () = tokio::time::sleep_until(deadline.at()) => {
                    return Err(BrowserError::Timeout { dispatch: Dispatch::NotSent });
                }
            }
        }
    }

    // ---- the tab's own task ----

    pub(crate) fn block(&self, method: &str, address: &str) {
        let shape = RequestShape {
            method: method.to_owned(),
            // Redacted whole, then cut at the query, then to length.
            address: cut(without_query(&self.redact.redact(address))),
        };
        if let Ok(mut blocked) = self.blocked.lock() {
            // A page that keeps trying must not fill the engine's memory.
            if blocked.len() < 1024 {
                blocked.push(shape);
            }
        }
        self.changes.send_modify(|count| *count += 1);
    }

    /// Puts the lock on a worker or a frame that attached under an
    /// observing tab, then lets it run. When the lock cannot be put on,
    /// it is not let run, and that is reported.
    ///
    /// A dedicated worker has no interception of its own to switch on: its
    /// requests pass through its page's, which is already on (measured
    /// against the browser of decision 0111). Every other kind must take
    /// its own, or it stays stopped.
    ///
    /// The refusal of sockets is placed where the child can take it. A
    /// worker has one global, which is there while it waits: the script is
    /// run in it once. A frame in another process has no document while it
    /// waits, and loads a new one whenever it moves: the script is placed
    /// on every document it will load, which the browser allows once the
    /// frame's page events are switched on (measured, decision 0143).
    async fn guard_child(&self, session: &str, kind: &str, waiting: bool) {
        let deadline = Deadline::after(self.call_deadline);
        let client = &self.client;
        let intercepted = match client
            .call(Some(session), &fetch_everything(), deadline)
            .await
        {
            Ok(_) => true,
            Err(ClientError::Protocol { code, .. }) => kind == "worker" && code == METHOD_NOT_FOUND,
            Err(_) => false,
        };
        // Workers of this one, and frames inside it, are locked the same way.
        let nested = client
            .call(
                Some(session),
                &SetAutoAttach {
                    auto_attach: true,
                    wait_for_debugger_on_start: true,
                    flatten: true,
                    filter: None,
                },
                deadline,
            )
            .await;
        let nested = match nested {
            Ok(_) => true,
            Err(ClientError::Protocol { code, .. }) => code == METHOD_NOT_FOUND,
            Err(_) => false,
        };
        // The report of a refused socket is an event of the child's own
        // scripts, which the browser sends only once they are switched on.
        let _ = client
            .call(Some(session), &RuntimeEnable {}, deadline)
            .await;
        let _ = client
            .call(
                Some(session),
                &AddBinding {
                    name: REFUSED_BINDING,
                },
                deadline,
            )
            .await;
        // A service worker is held to the strictest script whatever the
        // tab: it outlives the page and serves every page of its origin.
        let sockets = if kind == "service_worker" {
            REFUSE_SOCKETS
        } else {
            self.sockets.as_str()
        };
        let now = Evaluate {
            expression: sockets,
            context_id: None,
            return_by_value: true,
            await_promise: false,
        };
        let sockets_refused = match client.call(Some(session), &PageEnable {}, deadline).await {
            Ok(_) => {
                let on_every_document = client
                    .call(
                        Some(session),
                        &AddScriptOnNewDocument {
                            source: sockets,
                            world_name: None,
                        },
                        deadline,
                    )
                    .await
                    .is_ok();
                if !waiting {
                    // It already runs: the document it has is locked too.
                    let _ = client.call(Some(session), &now, deadline).await;
                }
                on_every_document
            }
            // Not a frame: it has no page events to switch on.
            Err(ClientError::Protocol { code, .. }) if code == METHOD_NOT_FOUND => client
                .call(Some(session), &now, deadline)
                .await
                .is_ok_and(|evaluated| evaluated.exception_details.is_none()),
            Err(_) => false,
        };
        if !(intercepted && nested && sockets_refused) {
            self.block("WORKER", "");
            return;
        }
        if waiting {
            let _ = client.call(Some(session), &RunIfWaiting {}, deadline).await;
        }
    }

    /// What becomes of a request that is not a move of the tab itself.
    ///
    /// A request whose address is on none of the origins the tab may reach
    /// is refused first, whatever its method and whichever tab it is in
    /// (decision 0292). Then an observing tab refuses what could write.
    fn judge_request(&self, paused: &RequestPaused) -> Verdict {
        let request = &paused.request;
        // An origin the tool declares as refused is failed all the same, and
        // reports nothing: the run goes on without it.
        if self.origins.refused_quietly_reported(&request.url) {
            return Verdict::Refuse(None);
        }
        if !self.origins.reaches_reported(&request.url) {
            return Verdict::Refuse(Some(OUTSIDE.to_owned()));
        }
        match &self.safe {
            Some(safe)
                if !safe.allows(
                    &request.method,
                    &request.url,
                    &request.headers,
                    &request.body,
                ) =>
            {
                Verdict::Refuse(Some(request.method.clone()))
            }
            _ => Verdict::Pass,
        }
    }

    /// What becomes of the request of the tab's own next document.
    ///
    /// A tab goes only to the origins it was opened for, whoever moves it
    /// and through however many redirects: each hop is asked here. A tab
    /// that only observes is moved by `goto` alone: a move its page starts
    /// is ended before its request leaves the browser, whatever its
    /// method, because a system may treat a GET as an action.
    fn judge_move(&self, paused: &RequestPaused) -> Verdict {
        let request = &paused.request;
        let inside = self.origins.allows_reported(&request.url);
        let mut moves = self.moves();
        let mover = moves.mover(paused);
        let reads = matches!(request.method.as_str(), "GET" | "HEAD" | "OPTIONS");
        match (mover, &self.safe) {
            (Mover::Goto, _) if !inside => {
                // The `goto` reports it, as its own error.
                moves.refused = Some(shown_origin(&request.url));
                Verdict::Refuse(None)
            }
            (Mover::Goto, _) => {
                drop(moves);
                self.judge_request(paused)
            }
            (Mover::Page, Some(_)) if reads => Verdict::Refuse(Some(NAVIGATION.to_owned())),
            (Mover::Page, Some(_)) => Verdict::Refuse(Some(request.method.clone())),
            (Mover::Page, None) if inside => Verdict::Pass,
            (Mover::Page, None) => Verdict::Refuse(Some(NAVIGATION.to_owned())),
        }
    }

    async fn heard(&self, event: Event) {
        let own = event.session.as_deref() == Some(self.session.as_str());
        match event.kind {
            EventKind::Lifecycle(lifecycle) if own => {
                if lifecycle.frame_id == self.target && lifecycle.name == "DOMContentLoaded" {
                    if self.watching {
                        self.left_behind(&lifecycle.loader_id);
                    }
                    // Counted before the document is said to be parsed: a
                    // `goto` returns once it is, and a count taken then must
                    // already hold it, or the next wait for a change ends at
                    // this document's own load.
                    self.changes.send_modify(|count| *count += 1);
                    self.nav
                        .send_modify(|nav| nav.loaded = Some(lifecycle.loader_id));
                } else if lifecycle.frame_id == self.target && lifecycle.name == "networkIdle" {
                    self.nav
                        .send_modify(|nav| nav.idle = Some(lifecycle.loader_id));
                }
            }
            EventKind::ResponseReceived(received) if own => {
                self.answered(&received.request_id, &received.response);
                if received.kind == "Document" && !received.loader_id.is_empty() {
                    self.nav.send_modify(|nav| {
                        let challenged = received.response.challenged();
                        nav.document =
                            Some((received.loader_id, received.response.status, challenged));
                    });
                }
            }
            EventKind::ToolsAdded(params) if own => self.tools_added(&params),
            EventKind::ToolsRemoved(params) if own => self.tools_removed(&params),
            EventKind::ToolResponded(params) if own => self.tool_responded(&params),
            EventKind::ContextCreated(created) if own => {
                let context = created.context;
                if context.name == WORLD
                    && !context.aux_data.is_default
                    && context.aux_data.frame_id == self.target
                {
                    self.world.send_replace(Some(context.id));
                }
            }
            EventKind::ContextDestroyed(destroyed) if own => {
                self.forget_world(destroyed.execution_context_id);
            }
            EventKind::ContextsCleared if own => {
                self.world.send_replace(None);
            }
            EventKind::DialogOpening(opened) if own => {
                if let Ok(mut open) = self.open_dialog.lock() {
                    *open = Some(OpenDialog {
                        kind: opened.kind,
                        message: shown(self.redact.as_ref(), &opened.message),
                    });
                }
                self.status.send_if_modified(|status| {
                    let open = *status == Status::Open;
                    if open {
                        *status = Status::Dialog;
                    }
                    open
                });
            }
            EventKind::DialogClosed if own => {
                if let Ok(mut open) = self.open_dialog.lock() {
                    *open = None;
                }
                self.status.send_if_modified(|status| {
                    let dialog = *status == Status::Dialog;
                    if dialog {
                        *status = Status::Open;
                    }
                    dialog
                });
                self.dialogs_closed.send_modify(|closed| *closed += 1);
            }
            EventKind::TargetCrashed if own => {
                // A tab closed as hung stays hung, whatever its renderer
                // does as it goes.
                self.status.send_if_modified(|status| {
                    let hung = *status == Status::Hung;
                    if !hung {
                        *status = Status::Crashed;
                    }
                    !hung
                });
            }
            EventKind::MoveAsked(asked) if own => {
                if asked.frame_id == self.target {
                    self.moves().asked(&asked.url);
                }
            }
            EventKind::MoveStarted(started) if own => {
                if started.frame_id == self.target {
                    self.moves().started(
                        &started.url,
                        &started.loader_id,
                        &started.navigation_type,
                    );
                }
            }
            // The browser writes no file for it: every download is denied
            // when the browser starts. This is the report.
            EventKind::DownloadBegins(download) => {
                // A download a step declared is the one, kept; any other is
                // refused as ever, and a second one cancelled.
                let declared = self.download_begins(&serde_json::json!({
                    "guid": download.guid,
                    "suggestedFilename": download.suggested_filename,
                }));
                if !declared {
                    self.block(DOWNLOAD, &download.url);
                }
            }
            EventKind::DownloadProgress(params) if own => self.download_progress(&params),
            EventKind::Attached(attached) => {
                // Every child of a tab a tool runs in is locked before it
                // runs, in both postures (decision 0292); a sign-in tab
                // attaches none. One that comes while the tab is being
                // closed is left waiting, and never runs.
                if self.released.load(Ordering::SeqCst) {
                    return;
                }
                if !self.quiet {
                    self.guard_child(
                        &attached.session_id,
                        &attached.target_info.kind,
                        attached.waiting_for_debugger,
                    )
                    .await;
                } else if attached.waiting_for_debugger {
                    let deadline = Deadline::after(self.call_deadline);
                    let _ = self
                        .client
                        .call(Some(&attached.session_id), &RunIfWaiting {}, deadline)
                        .await;
                }
            }
            EventKind::RequestPaused(paused) => {
                let deadline = Deadline::after(self.call_deadline);
                let session = event.session.as_deref();
                let document = paused.resource_type == "Document";
                let verdict = if self.released.load(Ordering::SeqCst) {
                    // A tab being closed sends nothing more.
                    Verdict::Refuse(None)
                } else if own && document && paused.frame_id == self.target {
                    self.judge_move(&paused)
                } else {
                    self.judge_request(&paused)
                };
                match verdict {
                    Verdict::Pass => {
                        if own && !document {
                            self.heard_request(&paused);
                        } else if own && paused.frame_id == self.target {
                            self.heard_document(&paused);
                        }
                        let _ = self
                            .client
                            .call(
                                session,
                                &ContinueRequest {
                                    request_id: &paused.request_id,
                                },
                                deadline,
                            )
                            .await;
                    }
                    Verdict::Refuse(report) => {
                        // Recorded first, so that whoever sees the request
                        // fail can already read why.
                        if let Some(word) = report {
                            self.block(&word, &paused.request.url);
                        }
                        // Ended before it leaves the browser. A request left
                        // paused is sent when the tab is detached, so a tab
                        // hears its events until then (`release`).
                        let _ = self
                            .client
                            .call(
                                session,
                                &FailRequest {
                                    request_id: &paused.request_id,
                                    error_reason: if document { ABORTED } else { BLOCKED },
                                },
                                deadline,
                            )
                            .await;
                    }
                }
            }
            EventKind::RequestSent(sent) if own && self.watching => self.heard_sent(&sent),
            EventKind::SameDocument(moved) if own && self.watching => {
                if moved.frame_id == self.target {
                    self.changes.send_modify(|count| *count += 1);
                }
            }
            EventKind::LoadingFinished(done) if own => self.finished(&done.request_id).await,
            EventKind::LoadingFailed(done) if own => self.forget(&done.request_id),
            EventKind::BindingCalled(called) if called.name == REFUSED_BINDING => {
                // The payload is the page's: only a known word is taken as
                // the method, and the rest is an address like any other.
                let (what, address) = called.payload.split_once(' ').unwrap_or(("", ""));
                let [socket, transport, peer, shared, service] = REFUSED_WHEN_MADE;
                let method = match what {
                    "WebSocket" => socket,
                    "WebTransport" => transport,
                    "SharedWorker" => shared,
                    "RTCPeerConnection" => peer,
                    "ServiceWorker" => service,
                    _ => "REFUSED",
                };
                self.block(method, address);
            }
            _ => {}
        }
    }

    /// Notes a request the page of an observing tab made by itself, with
    /// GET, to one of the tab's origins, from a script: the start of a
    /// [`PageExchange`]. Only names are kept.
    fn heard_request(&self, paused: &RequestPaused) {
        let request = &paused.request;
        let scripted = matches!(paused.resource_type.as_str(), "XHR" | "Fetch");
        let Some(number) = paused.network_id.as_deref() else {
            return;
        };
        if self.safe.is_none()
            || !self.recording.load(Ordering::SeqCst)
            || !scripted
            || request.method != "GET"
            || !self.origins.allows_reported(&request.url)
        {
            return;
        }
        if let Some(exchange) = self.exchange_of(request) {
            self.note_pending(number, exchange, None);
        }
    }

    /// The start of a [`PageExchange`] for a request: its method, origin,
    /// path, and the names of its query and headers, by structure. No
    /// value is kept. `None` for an address with no origin.
    fn exchange_of(&self, request: &crate::protocol::PausedRequest) -> Option<PageExchange> {
        let origin = origin_of(&request.url)?;
        let address = without_fragment(&request.url);
        let (query, query_left_out) = crate::query_names(
            address
                .split_once('?')
                .map(|(_, query)| query)
                .unwrap_or_default(),
        );
        let names: BTreeSet<String> = request
            .headers
            .keys()
            .map(|name| name.to_ascii_lowercase())
            .collect();
        let credential_headers: BTreeSet<String> = names
            .iter()
            .filter(|name| {
                CREDENTIAL_HEADER_WORDS
                    .iter()
                    .any(|word| name.contains(word))
            })
            .cloned()
            .collect();
        // A header the browser also sets is the page's when it holds what
        // the browser would not have sent. The values are compared here and
        // not kept.
        let document = self.document.lock().ok().and_then(|held| held.clone());
        let value = |name: &str| {
            request
                .headers
                .iter()
                .find(|(own, _)| own.eq_ignore_ascii_case(name))
                .and_then(|(_, value)| value.as_str())
        };
        let browsers_own = |name: &str| match name {
            "accept" => value(name) == Some("*/*"),
            "referer" => document
                .as_ref()
                .is_some_and(|document| value(name) == Some(document.address.as_str())),
            _ if BROWSER_DEFAULT_HEADERS.contains(&name) => {
                document.as_ref().is_some_and(|document| {
                    value(name).is_some()
                        && value(name) == document.defaults.get(name).map(String::as_str)
                })
            }
            _ => BROWSER_HEADERS.contains(&name),
        };
        let set_headers: BTreeSet<String> = names
            .into_iter()
            .filter(|name| !browsers_own(name))
            .collect();
        Some(PageExchange {
            method: request.method.clone(),
            origin,
            path: crate::path_by_structure(&self.redact.redact(path_of(address))),
            query: query.iter().map(|name| self.redact.redact(name)).collect(),
            query_left_out,
            credential_headers: credential_headers.into_iter().collect(),
            set_headers: set_headers.into_iter().collect(),
            status: 0,
            content_type: String::new(),
            fields: None,
            names_withheld: false,
            body_media_type: String::new(),
            body_fields: None,
            body_names_withheld: false,
        })
    }

    /// Notes a request of the page heard and not yet answered, or counts it
    /// as left out when the tab already holds its most.
    fn note_pending(&self, number: &str, exchange: PageExchange, loader: Option<String>) {
        if let Ok(mut pending) = self.pending.lock() {
            if pending.len() < EXCHANGES_KEPT {
                pending.insert(
                    number.to_owned(),
                    Pending {
                        exchange,
                        answered: false,
                        loader,
                        reading: false,
                    },
                );
                self.waiting.send_replace(pending.len());
            } else {
                self.dropped.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    /// Notes what the request of the tab's own document held of the
    /// headers the browser sets by itself, and its address.
    fn heard_document(&self, paused: &RequestPaused) {
        if (self.safe.is_none() && !self.watching) || !self.recording.load(Ordering::SeqCst) {
            return;
        }
        let defaults = paused
            .request
            .headers
            .iter()
            .filter_map(|(name, value)| {
                let name = name.to_ascii_lowercase();
                BROWSER_DEFAULT_HEADERS
                    .contains(&name.as_str())
                    .then(|| Some((name, value.as_str()?.to_owned())))
                    .flatten()
            })
            .collect();
        if let Ok(mut held) = self.document.lock() {
            *held = Some(DocumentHeaders {
                address: without_fragment(&paused.request.url).to_owned(),
                defaults,
            });
        }
    }

    /// The head of the answer to a request of the page that is noted.
    fn answered(&self, number: &str, response: &crate::protocol::ResponseDescription) {
        if let Ok(mut pending) = self.pending.lock()
            && let Some(one) = pending.get_mut(number)
        {
            one.exchange.status = response.status;
            one.exchange.content_type = response
                .mime_type
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase();
            one.answered = true;
        }
    }

    /// Forgets a request of the page that failed: it has no answer.
    fn forget(&self, number: &str) {
        if let Ok(mut pending) = self.pending.lock()
            && pending.remove(number).is_some()
        {
            self.waiting.send_replace(pending.len());
        }
    }

    /// The answer to a request of the page has arrived whole: its names
    /// and kinds are read, and it is an exchange.
    async fn finished(&self, number: &str) {
        let taken = self
            .pending
            .lock()
            .ok()
            .and_then(|pending| pending.get(number).cloned());
        let Some(Pending {
            mut exchange,
            answered: true,
            ..
        }) = taken
        else {
            self.forget(number);
            return;
        };
        let json =
            exchange.content_type == "application/json" || exchange.content_type.ends_with("+json");
        if json {
            // The browser keeps no answer longer than the tab reads
            // (`Network.enable`, below), so a longer one is not fetched at
            // all, and the length is checked again here.
            let deadline = Deadline::after(self.call_deadline);
            let body = self
                .client
                .call(
                    Some(&self.session),
                    &GetResponseBody { request_id: number },
                    deadline,
                )
                .await;
            if let Some((fields, withheld)) = body
                .ok()
                .filter(|body| !body.base64_encoded && body.body.chars().count() <= self.max_read)
                .and_then(|body| serde_json::from_str::<Json>(&body.body).ok())
                .map(|json| fields_of(&json))
            {
                exchange.fields = Some(
                    fields
                        .into_iter()
                        .map(|(name, kind)| (self.redact.redact(&name), kind))
                        .collect(),
                );
                exchange.names_withheld = withheld;
            }
        }
        if let Ok(mut exchanges) = self.exchanges.lock() {
            if exchanges.len() < EXCHANGES_KEPT {
                exchanges.push(exchange);
            } else {
                self.dropped.fetch_add(1, Ordering::SeqCst);
            }
        }
        self.forget(number);
    }

    /// The page's own requests answered whole, oldest first, taken out,
    /// once none is awaiting its answer or the deadline has passed, with
    /// how many were left out and how many were still awaited.
    async fn take_exchanges(&self, deadline: Deadline) -> PageExchanges {
        let mut waiting = self.waiting.subscribe();
        let _ = tokio::time::timeout_at(deadline.at(), waiting.wait_for(|count| *count == 0)).await;
        let awaited = *self.waiting.borrow();
        PageExchanges {
            exchanges: self
                .exchanges
                .lock()
                .map(|mut exchanges| std::mem::take(&mut *exchanges))
                .unwrap_or_default(),
            dropped: self.dropped.swap(0, Ordering::SeqCst),
            awaited,
        }
    }

    /// The number of a request of the page whose answer has arrived whole,
    /// when its body is to be read: the read is made off the tab's one
    /// stream of events, so that the events after it are not held. The
    /// request is marked as being read, so that the read alone closes it:
    /// a document loaded while the read is on its way leaves it be
    /// ([`Self::left_behind`]), and it is recorded once.
    fn answer_to_read(&self, event: &Event) -> Option<String> {
        match &event.kind {
            EventKind::LoadingFinished(done)
                if event.session.as_deref() == Some(self.session.as_str()) =>
            {
                let mut pending = self.pending.lock().ok()?;
                let one = pending.get_mut(&done.request_id)?;
                one.reading = true;
                Some(done.request_id.clone())
            }
            _ => None,
        }
    }

    /// The requests this observing tab refused, oldest first, taken out.
    fn take_blocked(&self) -> Vec<RequestShape> {
        self.blocked
            .lock()
            .map(|mut blocked| std::mem::take(&mut *blocked))
            .unwrap_or_default()
    }

    /// Ends the tab: it is done with, and the browser is asked to close
    /// its page. Safe to call more than once.
    ///
    /// A request the browser holds for the tab is sent as it was when the
    /// tab's session is detached, unless it was answered first (measured,
    /// decision 0461), and a page makes requests while it is closed: a
    /// handler of its leaving, or a load that was on its way. So the tab
    /// goes on hearing its events until the browser has detached it, and
    /// from the release on fails every request of its page, its frames,
    /// and its workers. Before the page is closed, its document is replaced
    /// with an empty one the engine writes, with no request: that erases
    /// every handler the page set on its window and its document, so its
    /// leaving runs nothing of the page's.
    fn release(&self) {
        self.status.send_replace(Status::Gone);
        if self.released.swap(true, Ordering::SeqCst) {
            return;
        }
        self.browser.tabs.fetch_sub(1, Ordering::SeqCst);
        self.browser.pages().remove(&self.target);
        // The tab's own page first, then the windows it opened that are
        // still there: with the page gone, nothing drives them.
        let mut closing = vec![self.target.clone()];
        closing.extend(self.browser.windows_of(&self.target));
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            // With no runtime, no task hears the tab either.
            self.client.unsubscribe(&self.session);
            return;
        };
        let client = self.client.clone();
        let session = self.session.clone();
        let target = self.target.clone();
        let mut ended = self.ended.subscribe();
        let browser = Arc::clone(&self.browser);
        let call_deadline = self.call_deadline;
        browser.closing.fetch_add(1, Ordering::SeqCst);
        runtime.spawn(async move {
            let deadline = Deadline::after(call_deadline);
            let _ = client
                .call(
                    Some(&session),
                    &SetDocumentContent {
                        frame_id: &target,
                        html: "",
                    },
                    deadline,
                )
                .await;
            let deadline = Deadline::after(call_deadline);
            for target in closing {
                let _ = client
                    .call(None, &CloseTarget { target_id: &target }, deadline)
                    .await;
            }
            // The browser's word that the session is detached ends the task
            // that hears the tab: until then it fails every request.
            let _ = tokio::time::timeout_at(deadline.at(), ended.wait_for(|ended| *ended)).await;
            client.unsubscribe(&session);
            browser.closing.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

/// The protocol's code for a method the target does not have.
const METHOD_NOT_FOUND: i64 = -32601;

const PREPARE_FOCUS: &str = "function () { const checked = __a2m.check(this, 'focus'); if (checked.ok) this.focus(); return checked; }";

/// The two ways a tab's call ends without a reply.
enum Ended {
    Browser(BrowserError),
    Client(ClientError),
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Checked {
    stale: bool,
    ok: bool,
    no: Option<String>,
    x: i64,
    y: i64,
    checked: Option<bool>,
}

/// Whether the browser's text says the context or the object is gone.
fn gone(message: &str) -> bool {
    [
        "Cannot find context with specified id",
        "Execution context was destroyed",
        "Could not find object with given id",
        "Invalid remote object id",
        "Cannot find default execution context",
        "Inspected target navigated or closed",
        "Promise was collected",
    ]
    .iter()
    .any(|known| message.contains(known))
}

fn credential_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "cookie" | "set-cookie" | "authorization" | "proxy-authorization"
    )
}

/// A method that only looks sent no input, whatever was written to the
/// browser on its way.
fn looked(error: BrowserError) -> BrowserError {
    let dispatch = Dispatch::NotSent;
    match error {
        BrowserError::Timeout { .. } => BrowserError::Timeout { dispatch },
        BrowserError::Crashed { .. } => BrowserError::Crashed { dispatch },
        BrowserError::Disconnected { .. } => BrowserError::Disconnected { dispatch },
        BrowserError::UnexpectedDialog { .. } => BrowserError::UnexpectedDialog { dispatch },
        other => other,
    }
}

/// An error after part of an input had already been sent.
fn after_sent(error: BrowserError) -> BrowserError {
    let dispatch = Dispatch::Sent;
    match error {
        BrowserError::Timeout { .. } => BrowserError::Timeout { dispatch },
        BrowserError::Crashed { .. } => BrowserError::Crashed { dispatch },
        BrowserError::Disconnected { .. } => BrowserError::Disconnected { dispatch },
        BrowserError::UnexpectedDialog { .. } => BrowserError::UnexpectedDialog { dispatch },
        other => other,
    }
}

fn fetch_everything() -> FetchEnable<'static> {
    FetchEnable {
        patterns: vec![RequestPattern {
            url_pattern: "*",
            resource_type: None,
            request_stage: "Request",
        }],
    }
}

/// The requests of documents, and of what a page loads and no body goes
/// with: what a sign-in tab holds (decision 0335). A style, an image, a
/// media file, a font, a script, and the rest the browser files as `Other`
/// carry no body of the person's. A `fetch`, an `XHR`, and a beacon do, and
/// are not held, because the password a person types would cross into the
/// engine's process in them (decision 0254). The kinds this browser's
/// interception filter does not name (measured: `TextTrack`, `Prefetch`,
/// `Manifest`) are not held either.
fn fetch_documents_and_loads() -> FetchEnable<'static> {
    const LOADS: [&str; 7] = [
        "Document",
        "Stylesheet",
        "Image",
        "Media",
        "Font",
        "Script",
        "Other",
    ];
    FetchEnable {
        patterns: LOADS
            .iter()
            .map(|kind| RequestPattern {
                url_pattern: "*",
                resource_type: Some(kind),
                request_stage: "Request",
            })
            .collect(),
    }
}

/// Watches the browser itself for windows its pages open, for as long as
/// the browser's link lasts.
///
/// The browser is told, when it starts, to attach every new page and hold
/// it before it runs. A page with no opener is let run when it is one the
/// engine made, and never otherwise. A page with an opener is a window
/// some page opened, also when the page asked for a window with no opener.
/// The tab that opened it reports it at once. What becomes of it follows
/// the tab's posture, and each way was measured (decision 0143):
///
/// - Opened from a tab that only observes, or from nothing the engine
///   knows: it is closed while it is held, and never runs. It fetches
///   nothing and gives its opener no window to use. The page's own scripts
///   are left waiting, and the tab can no longer be sent anywhere: a
///   `goto` ends at its deadline. The engine's reads of the page still
///   answer, and the tab has reported `POPUP`. Only a person's gesture
///   opens a window, and a tab that only observes never gives one.
/// - Opened from a tab that may act: every request of it is set to be
///   ended in the browser, and it is let run, so that the page that opened
///   it goes on. The first page it asks for is reported and ended, and
///   only then is it closed: a window closed while its first request was
///   on its way let one in eight through. One that asks for nothing stays,
///   with that lock on, until the tab that opened it goes.
///
/// Only the browser's own side can be set on a window that is held: its
/// scripts answer nothing until it runs. The browser answers the call that
/// lets a window run only once the window's first request has been decided,
/// so that answer is never awaited on the one stream of events that decides
/// it.
pub(crate) async fn watch_windows(
    browser: Weak<BrowserInner>,
    mut events: mpsc::UnboundedReceiver<Event>,
    call_deadline: Duration,
) {
    while let Some(event) = events.recv().await {
        let Some(browser) = browser.upgrade() else {
            return;
        };
        let client = &browser.client;
        let deadline = Deadline::after(call_deadline);
        match event.kind {
            EventKind::Attached(Attached {
                session_id,
                target_info,
                waiting_for_debugger,
            }) if event.session.is_none() => {
                let Some(opener) = target_info.opener_id else {
                    // The browser also announces a session the engine
                    // attached itself; only one it holds a page on counts.
                    if waiting_for_debugger && browser.holds(&target_info.target_id, &session_id) {
                        let _ = client
                            .call(Some(&session_id), &RunIfWaiting {}, deadline)
                            .await;
                    }
                    continue;
                };
                let session = Some(session_id.as_str());
                let close = CloseTarget {
                    target_id: &target_info.target_id,
                };
                browser.report(&opener, POPUP, "");
                let may_act = browser
                    .pages()
                    .get(&opener)
                    .and_then(Weak::upgrade)
                    .is_some_and(|page| page.safe.is_none());
                let locked = may_act
                    && client
                        .call(session, &fetch_everything(), deadline)
                        .await
                        .is_ok();
                if !locked {
                    let _ = client.call(None, &close, deadline).await;
                    continue;
                }
                browser.windows().insert(
                    session_id.clone(),
                    crate::browsers::Window {
                        opener,
                        target: target_info.target_id.clone(),
                        asked: false,
                    },
                );
                // The browser answers this only once the window's first
                // request has been decided, and deciding it is this loop's
                // next event: awaited here, the two waited on each other
                // until the deadline. So the answer is awaited apart, and
                // the loop goes on to the window's first request.
                let client = client.clone();
                tokio::spawn(async move {
                    let _ = client
                        .call(Some(&session_id), &RunIfWaiting {}, deadline)
                        .await;
                });
            }
            EventKind::RequestPaused(paused) => {
                let session = event.session.as_deref();
                // The first page a window asks for is reported, and the
                // window is closed once that request has been ended.
                let first = session.and_then(|session| {
                    let mut windows = browser.windows();
                    let window = windows.get_mut(session)?;
                    (!std::mem::replace(&mut window.asked, true))
                        .then(|| (window.opener.clone(), window.target.clone()))
                });
                if let Some((opener, _)) = &first {
                    browser.report(opener, POPUP, &paused.request.url);
                }
                let _ = client
                    .call(
                        session,
                        &FailRequest {
                            request_id: &paused.request_id,
                            error_reason: ABORTED,
                        },
                        deadline,
                    )
                    .await;
                if let Some((_, target)) = &first {
                    let _ = client
                        .call(None, &CloseTarget { target_id: target }, deadline)
                        .await;
                }
            }
            EventKind::Detached(detached) => {
                browser.windows().remove(&detached.session_id);
            }
            _ => {}
        }
    }
}

async fn pump(page: Arc<Page>, mut events: mpsc::UnboundedReceiver<Event>) {
    while let Some(event) = events.recv().await {
        // An answer's body is read off this stream, each within the tab's
        // own time limit for a call, so that the events after it wait for
        // nothing.
        if let Some(number) = page.answer_to_read(&event) {
            let page = Arc::clone(&page);
            tokio::spawn(async move { page.finished(&number).await });
            continue;
        }
        page.heard(event).await;
    }
    // The subscription ended: the tab was detached, or the link closed.
    page.status.send_if_modified(|status| {
        let open = !matches!(*status, Status::Gone | Status::Crashed | Status::Hung);
        if open {
            *status = Status::Gone;
        }
        open
    });
    page.ended.send_replace(true);
}

/// Opens a tab on `about:blank` in the browser, in the posture given, for
/// the origins given.
pub(crate) async fn open_page(
    shared: &Arc<Shared>,
    browser: &Arc<BrowserInner>,
    env: &PageEnvironment,
    posture: Posture,
    origins: &Origins,
    look_on: Option<&Origins>,
) -> Result<Arc<Page>, BrowserError> {
    let client = browser.client.clone();
    let settings = &shared.settings;
    let deadline = Deadline::after(settings.call_deadline);
    let lost = |error: ClientError| match error {
        ClientError::Closed { dispatch, .. } => BrowserError::Disconnected { dispatch },
        ClientError::Timeout { dispatch } => BrowserError::Timeout { dispatch },
        ClientError::Protocol { message, .. } => {
            BrowserError::Unexpected(shown(shared.redact.as_ref(), &message))
        }
        ClientError::Shape { method } => {
            BrowserError::Unexpected(format!("the reply of {method} has another shape"))
        }
    };
    let created = client
        .call(
            None,
            &CreateTarget {
                url: "about:blank",
                browser_context_id: Some(&browser.context),
            },
            deadline,
        )
        .await
        .map_err(lost)?;
    // The browser holds every new page before it runs. This one is the
    // engine's own, on `about:blank`: it is let run, and is locked below
    // before it is sent anywhere.
    if let Some(held_on) = browser.made(&created.target_id) {
        client
            .call(Some(&held_on), &RunIfWaiting {}, deadline)
            .await
            .map_err(lost)?;
    }
    let attached = client
        .call(
            None,
            &AttachToTarget {
                target_id: &created.target_id,
                flatten: true,
            },
            deadline,
        )
        .await
        .map_err(lost)?;
    let sockets = match &posture {
        Posture::Observe(_) => REFUSE_SOCKETS.to_owned(),
        Posture::Act | Posture::SignIn | Posture::Watch => sockets_held_to(origins),
    };
    let page = Arc::new(Page {
        client: client.clone(),
        session: attached.session_id,
        target: created.target_id,
        serial: shared.next_tab.fetch_add(1, Ordering::SeqCst),
        call_deadline: settings.call_deadline,
        max_read: settings.max_read_chars,
        redact: Arc::clone(&shared.redact),
        world: watch::Sender::new(None),
        status: watch::Sender::new(Status::Open),
        nav: watch::Sender::new(Nav::default()),
        dialogs_closed: watch::Sender::new(0),
        download: Mutex::new(DownloadState::default()),
        download_changes: watch::Sender::new(0),
        site: Mutex::new(SiteState::default()),
        site_changes: watch::Sender::new(0),
        open_dialog: Mutex::new(None),
        blocked: Mutex::new(Vec::new()),
        changes: watch::Sender::new(0),
        pending: Mutex::new(BTreeMap::new()),
        waiting: watch::Sender::new(0),
        exchanges: Mutex::new(Vec::new()),
        // A watching tab is opened for a capture alone: it records from
        // its first request.
        recording: AtomicBool::new(matches!(posture, Posture::Watch)),
        dropped: AtomicUsize::new(0),
        document: Mutex::new(None),
        quiet: matches!(posture, Posture::SignIn),
        watching: matches!(posture, Posture::Watch),
        screens: Mutex::new(watching::Screens::default()),
        looked: watch::Sender::new(0),
        safe: match posture {
            Posture::Act | Posture::SignIn | Posture::Watch => None,
            Posture::Observe(safe) => Some(safe),
        },
        origins: origins.clone(),
        helper: helper_for(look_on.unwrap_or(origins)),
        sockets,
        moves: Mutex::new(Moves::default()),
        released: AtomicBool::new(false),
        probing: AtomicBool::new(false),
        hang_probe: settings.hang_probe,
        hang_closed: watch::Sender::new(false),
        ended: watch::Sender::new(false),
        browser: Arc::clone(browser),
    });
    browser.tabs.fetch_add(1, Ordering::SeqCst);
    browser
        .pages()
        .insert(page.target.clone(), Arc::downgrade(&page));
    // Listening starts before anything is switched on, so no event is missed.
    let events = client.subscribe(Some(&page.session));
    tokio::spawn(pump(Arc::clone(&page), events));

    let prepared = prepare(&page, env, deadline).await;
    match prepared {
        Ok(()) => Ok(page),
        Err(error) => {
            page.release();
            Err(looked(error))
        }
    }
}

async fn prepare(
    page: &Page,
    env: &PageEnvironment,
    deadline: Deadline,
) -> Result<(), BrowserError> {
    if page.safe.is_some() {
        // The lock goes on before the page can do anything: requests are
        // intercepted, workers wait until they are locked too, service
        // workers are passed by, and sockets are refused.
        page.call(&fetch_everything(), deadline).await?;
        page.call(
            &SetAutoAttach {
                auto_attach: true,
                wait_for_debugger_on_start: true,
                flatten: true,
                filter: None,
            },
            deadline,
        )
        .await?;
        page.call(&NetworkEnable::keeping(page.max_read), deadline)
            .await?;
        page.call(&BypassServiceWorker { bypass: true }, deadline)
            .await?;
        page.call(&RuntimeEnable {}, deadline).await?;
        page.call(
            &AddBinding {
                name: REFUSED_BINDING,
            },
            deadline,
        )
        .await?;
        page.call(
            &AddScriptOnNewDocument {
                source: &page.sockets,
                world_name: None,
            },
            deadline,
        )
        .await?;
    } else if page.watching {
        // A tab a person uses while the engine watches holds the requests
        // of documents and of what the page loads with no body to its
        // origins, as a sign-in tab does, and fails none for its method:
        // every request in it is the person's (decision 0392, point 1). Its
        // network events are on, with a body up to a bound, so that each
        // request of the page is recorded by structure; the page's service
        // worker is passed by, so that its requests are seen. Nothing is
        // attached to its workers or frames.
        page.call(&fetch_documents_and_loads(), deadline).await?;
        page.call(
            &NetworkEnable::keeping(page.max_read).with_bodies_up_to(LONGEST_WATCHED_BODY),
            deadline,
        )
        .await?;
        page.call(&BypassServiceWorker { bypass: true }, deadline)
            .await?;
        page.call(&RuntimeEnable {}, deadline).await?;
        page.call(
            &AddBinding {
                name: REFUSED_BINDING,
            },
            deadline,
        )
        .await?;
        page.call(
            &AddScriptOnNewDocument {
                source: &page.sockets,
                world_name: None,
            },
            deadline,
        )
        .await?;
    } else if page.quiet {
        // A tab a person signs in in holds the requests of documents and
        // of what the page loads with no body, to its origins and the
        // providers' (decision 0335), and hears no network event at all. A
        // `fetch`, an `XHR`, or a beacon of its page is not held: what the
        // person types would cross in it.
        page.call(&fetch_documents_and_loads(), deadline).await?;
        page.call(&RuntimeEnable {}, deadline).await?;
    } else {
        // A tab that may act holds every request, as an observing tab does,
        // so that none leaves for an origin outside its reach (decision
        // 0292), and locks each worker and frame in another process the
        // same way before it runs; what interception cannot see is held by
        // the socket script. The page's service worker is passed by here
        // too, so that a write from inside the page reaches the application
        // as sent, once, and no worker answers or changes it (decision
        // 0245, audit finding M6).
        //
        // Its network events stay on: the browser passes the service
        // worker by only while they are, and switching them off undoes it
        // (measured, decision 0254). What they would carry of a posted body
        // is cut: no request's body is put in an event, so the password a
        // form posts and a token sent in a write's body never cross into
        // the engine's process by them (audit finding M7). A write's "sent"
        // and "answered" rest on none of them: they are the page's own
        // request and its answer, through the helper. The interception's
        // own event carries a held request's body as the browser holds it;
        // an acting tab never decodes it (decision 0292), and only an
        // observing tab's verdict on a request one safe request could match
        // does (decision 0394).
        page.call(&fetch_everything(), deadline).await?;
        page.call(
            &SetAutoAttach {
                auto_attach: true,
                wait_for_debugger_on_start: true,
                flatten: true,
                filter: None,
            },
            deadline,
        )
        .await?;
        page.call(
            &NetworkEnable::keeping(page.max_read).without_bodies(),
            deadline,
        )
        .await?;
        page.call(&BypassServiceWorker { bypass: true }, deadline)
            .await?;
        page.call(&RuntimeEnable {}, deadline).await?;
        page.call(
            &AddBinding {
                name: REFUSED_BINDING,
            },
            deadline,
        )
        .await?;
        page.call(
            &AddScriptOnNewDocument {
                source: &page.sockets,
                world_name: None,
            },
            deadline,
        )
        .await?;
    }
    page.call(&PageEnable {}, deadline).await?;
    page.call(&SetLifecycleEvents { enabled: true }, deadline)
        .await?;

    // The fixed environment.
    let (width, height) = env.viewport;
    page.call(
        &SetDeviceMetrics {
            width,
            height,
            device_scale_factor: 1,
            mobile: false,
        },
        deadline,
    )
    .await?;
    page.call(
        &SetLocale {
            locale: &env.locale,
        },
        deadline,
    )
    .await?;
    // A page reads its language from `navigator.language` and from the
    // header the browser sends, not from the number format alone: both are
    // the target's locale too.
    page.call(
        &SetAcceptLanguage {
            user_agent: "",
            accept_language: &env.locale,
        },
        deadline,
    )
    .await?;
    page.call(
        &SetTimezone {
            timezone_id: &env.time_zone,
        },
        deadline,
    )
    .await?;
    page.call(
        &SetEmulatedMedia {
            features: vec![
                MediaFeature {
                    name: "prefers-color-scheme",
                    value: if env.dark { "dark" } else { "light" },
                },
                MediaFeature {
                    name: "prefers-reduced-motion",
                    value: if env.reduced_motion {
                        "reduce"
                    } else {
                        "no-preference"
                    },
                },
            ],
        },
        deadline,
    )
    .await?;

    // The helper: in every document from now on, and in this one.
    page.call(
        &AddScriptOnNewDocument {
            source: &page.helper,
            world_name: Some(WORLD),
        },
        deadline,
    )
    .await?;
    let world = page
        .call(
            &CreateIsolatedWorld {
                frame_id: &page.target,
                world_name: WORLD,
            },
            deadline,
        )
        .await?;
    let installed = page
        .call(
            &Evaluate {
                expression: &page.helper,
                context_id: Some(world.execution_context_id),
                return_by_value: true,
                await_promise: false,
            },
            deadline,
        )
        .await?;
    if installed.exception_details.is_some() {
        return Err(BrowserError::Unexpected(
            "the helper could not be placed in the page".to_owned(),
        ));
    }
    page.world.send_if_modified(|current| {
        let unset = current.is_none();
        if unset {
            *current = Some(world.execution_context_id);
        }
        unset
    });
    Ok(())
}

/// Everything that only looks. Both kinds of tab have it. A method here
/// sends no input to the page, so each of its errors says `NotSent`.
pub trait Look {
    /// Sends the tab to an address and waits until its document has been
    /// parsed, which the browser reports as an event. An address on an
    /// origin the tab was not opened for is refused before the browser
    /// hears of it, and a redirect to one is ended in the browser: both
    /// are [`BrowserError::ForeignOrigin`].
    fn goto(
        &self,
        address: &Address,
        deadline: Deadline,
    ) -> impl Future<Output = Result<Loaded, BrowserError>> + Send;

    /// The one rendered element the query matches, now. It does not wait:
    /// wait for [`Primitive::Visible`] first where the element may be
    /// late. This, and every method below, looks only at a page of an
    /// origin the tab was opened for: on any other page it is
    /// [`BrowserError::ForeignOrigin`].
    fn locate(
        &self,
        query: &Query,
        deadline: Deadline,
    ) -> impl Future<Output = Result<Element, BrowserError>> + Send;

    /// Reads one thing of an element. A password input is never read,
    /// whatever is asked.
    fn read(
        &self,
        element: &Element,
        what: &ReadWhat,
        deadline: Deadline,
    ) -> impl Future<Output = Result<RawValue, BrowserError>> + Send;

    /// Waits until the condition holds. An observer in the page ends the
    /// wait; a new document re-arms it.
    fn wait(
        &self,
        until: &Primitive,
        deadline: Deadline,
    ) -> impl Future<Output = Result<(), BrowserError>> + Send;

    /// Whether the condition holds now: the page looks once and answers
    /// at once, and `false` is only its own word that the condition does
    /// not hold. The deadline bounds the browser's answer alone; a page
    /// that gives none by then is [`BrowserError::Timeout`], never `false`.
    fn holds(
        &self,
        condition: &Primitive,
        deadline: Deadline,
    ) -> impl Future<Output = Result<bool, BrowserError>> + Send;

    /// The snapshot of the page, from the accessibility tree, without
    /// screen matches.
    fn snapshot(
        &self,
        limits: &SnapshotLimits,
        deadline: Deadline,
    ) -> impl Future<Output = Result<Snapshot, BrowserError>> + Send;
}

macro_rules! looks {
    ($tab:ty) => {
        impl Look for $tab {
            async fn goto(
                &self,
                address: &Address,
                deadline: Deadline,
            ) -> Result<Loaded, BrowserError> {
                self.page.goto(address, deadline).await.map_err(looked)
            }

            async fn locate(
                &self,
                query: &Query,
                deadline: Deadline,
            ) -> Result<Element, BrowserError> {
                self.page.locate(query, deadline).await.map_err(looked)
            }

            async fn read(
                &self,
                element: &Element,
                what: &ReadWhat,
                deadline: Deadline,
            ) -> Result<RawValue, BrowserError> {
                self.page
                    .read(element, what, deadline)
                    .await
                    .map_err(looked)
            }

            async fn wait(
                &self,
                until: &Primitive,
                deadline: Deadline,
            ) -> Result<(), BrowserError> {
                self.page.wait(until, deadline).await.map_err(looked)
            }

            async fn holds(
                &self,
                condition: &Primitive,
                deadline: Deadline,
            ) -> Result<bool, BrowserError> {
                self.page.holds(condition, deadline).await.map_err(looked)
            }

            async fn snapshot(
                &self,
                limits: &SnapshotLimits,
                deadline: Deadline,
            ) -> Result<Snapshot, BrowserError> {
                self.page.snapshot(limits, deadline).await.map_err(looked)
            }
        }

        impl Drop for $tab {
            fn drop(&mut self) {
                self.page.release();
            }
        }

        impl std::fmt::Debug for $tab {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct(stringify!($tab))
                    .field("tab", &self.page.serial)
                    .finish_non_exhaustive()
            }
        }
    };
}

/// A tab that may act, on the origins it was opened for. Closed when
/// dropped.
pub struct ActTab {
    pub(crate) page: Arc<Page>,
}

/// A tab that can read and cannot act: it has no method that acts, and the
/// browser fails every request of its page that could change the system
/// and every move of the tab that its page starts. Closed when dropped.
pub struct ObserveTab {
    pub(crate) page: Arc<Page>,
}

looks!(ActTab);
looks!(ObserveTab);

impl ActTab {
    pub(crate) fn new(page: Arc<Page>) -> Self {
        Self { page }
    }

    /// One input to one element, after checking that it can take it:
    /// rendered, enabled, fit for the input, and, for a click, the element
    /// that would receive it. Nothing is retried and no other strategy is
    /// tried.
    ///
    /// # Errors
    ///
    /// [`BrowserError::NotActionable`] and [`BrowserError::NotFound`]
    /// before anything was sent, and [`BrowserError::ForeignOrigin`] when
    /// the page is of an origin the tab was not opened for: the origin is
    /// checked in the same call as the element, after it was located and
    /// before any input is sent. Every other error says through
    /// [`BrowserError::dispatch`] whether input reached the page.
    pub async fn input(
        &self,
        element: &Element,
        how: &Input,
        deadline: Deadline,
    ) -> Result<(), BrowserError> {
        self.page.input(element, how, deadline).await
    }

    /// Sends one request from inside the page, with the page's own
    /// session, once, to an address on one of the tab's origins. A
    /// redirect is not followed.
    ///
    /// # Errors
    ///
    /// [`BrowserError::ForeignOrigin`] for an address on another origin;
    /// nothing was sent. [`BrowserError::RequestFailed`] when the page
    /// could not complete it, or the answer was a redirect; whether it was
    /// sent is then not known.
    pub async fn fetch(
        &self,
        request: &PageRequest,
        deadline: Deadline,
    ) -> Result<PageResponse, BrowserError> {
        self.page.fetch(request, deadline).await
    }

    /// Writes from inside the page the tab is on, with the page's own
    /// session and the page's own anti-forgery token, once (decision
    /// 0206): the write's method from the closed list, to its checked path
    /// on the page's own origin, which must be one of the tab's, with its
    /// body, and with the token read from the page by the place the write
    /// names and sent where it names, in the same call. No other header is
    /// sent, and no redirect is followed. The token never leaves the page:
    /// it is not returned, and every copy of it in the answer is replaced
    /// in the page before the answer is handed back, which then passes the
    /// redactor.
    ///
    /// Only an acting tab has this call. An observing tab has none, and its
    /// browser fails every request of its page whose method could write:
    ///
    /// ```compile_fail
    /// # async fn no(tab: a2m_pack_web_browser::ObserveTab, write: a2m_pack_web_browser::PageWrite, deadline: a2m_pack_web_browser::Deadline) {
    /// let _ = tab.write_from_page(&write, deadline).await;
    /// # }
    /// ```
    ///
    /// ```no_run
    /// # async fn yes(tab: a2m_pack_web_browser::ActTab, write: a2m_pack_web_browser::PageWrite, deadline: a2m_pack_web_browser::Deadline) {
    /// let _ = tab.write_from_page(&write, deadline).await;
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Before anything was sent ([`Dispatch::NotSent`]):
    /// [`BrowserError::ForeignOrigin`] for a page on an origin the tab was
    /// not opened for; [`BrowserError::Token`] when the page holds no token
    /// where the write says, two different ones, or one of a form that is
    /// not sent; [`BrowserError::WriteRefused`] for a page with no origin
    /// of its own. After the write may have left: [`BrowserError::Redirected`]
    /// and [`BrowserError::TooLarge`] for an answer that was given and is not
    /// read; [`BrowserError::RequestFailed`] when the page could not
    /// complete it, or was replaced under it, and whether it was sent is
    /// not known; a lost browser or a passed deadline as their dispatch
    /// says.
    pub async fn write_from_page(
        &self,
        write: &PageWrite,
        deadline: Deadline,
    ) -> Result<PageAnswer, BrowserError> {
        self.page.write_from_page(write, deadline).await
    }

    /// The dialog that is open, if one is: nothing is answered.
    #[must_use]
    pub fn open_dialog(&self) -> Option<OpenDialog> {
        self.page
            .open_dialog
            .lock()
            .ok()
            .and_then(|open| open.clone())
    }

    /// Answers the dialog that is open.
    ///
    /// # Errors
    ///
    /// [`BrowserError::Unexpected`] when no dialog is open.
    pub async fn answer_dialog(
        &self,
        how: &DialogAnswer,
        deadline: Deadline,
    ) -> Result<(), BrowserError> {
        self.page.answer_dialog(how, deadline).await
    }
}

impl ActTab {
    /// How many documents of the tab have loaded and how many things it
    /// refused, together, so far. [`ActTab::changed_since`] waits for it to
    /// grow.
    #[must_use]
    pub fn changes(&self) -> u64 {
        *self.page.changes.borrow()
    }

    /// Waits until a document of the tab loads, or the tab refuses
    /// something, after the count `seen` that [`ActTab::changes`] gave: a
    /// protocol event, never a guess at how long a page takes.
    ///
    /// # Errors
    ///
    /// [`BrowserError::Timeout`] at the deadline; [`BrowserError::Crashed`]
    /// or [`BrowserError::Disconnected`] when the tab goes.
    pub async fn changed_since(&self, seen: u64, deadline: Deadline) -> Result<(), BrowserError> {
        self.page.changed_since(seen, deadline).await
    }

    /// Waits, at most `within`, for the page to draw again the part that
    /// holds `element`: `true` once the element has left its document, or
    /// a document of the tab has loaded after the count `seen` that
    /// [`ActTab::changes`] gave; `false` when the time ends with the
    /// element still there. A page whose platform commits a value by
    /// sending it and drawing the form again is waited for so, on the
    /// page's own word, never for a fixed time.
    ///
    /// # Errors
    ///
    /// [`BrowserError::Crashed`], [`BrowserError::Disconnected`], or
    /// [`BrowserError::Timeout`] when the tab goes or does not answer by
    /// `deadline`.
    pub async fn wait_redrawn(
        &self,
        element: &Element,
        seen: u64,
        within: std::time::Duration,
        deadline: Deadline,
    ) -> Result<bool, BrowserError> {
        if self.changes() > seen {
            return Ok(true);
        }
        let soon = Deadline::after(within.min(deadline.remaining()));
        tokio::select! {
            gone = self.page.gone(element, within.min(deadline.remaining()), deadline) => gone,
            changed = self.changed_since(seen, soon) => match changed {
                Ok(()) => Ok(true),
                Err(BrowserError::Timeout { .. }) => Ok(false),
                Err(error) => Err(error),
            },
        }
    }

    /// Where the tab's own document is, as the browser says, without asking
    /// the page.
    ///
    /// # Errors
    ///
    /// [`BrowserError::Disconnected`] or [`BrowserError::Timeout`] when the
    /// browser is gone or does not answer.
    pub async fn address(&self, deadline: Deadline) -> Result<String, BrowserError> {
        let page = &self.page;
        let info = page
            .client
            .call(
                None,
                &GetTargetInfo {
                    target_id: &page.target,
                },
                deadline,
            )
            .await
            .map_err(|error| page.client_error(error))?;
        Ok(page.redact.redact(&info.target_info.url))
    }

    /// The origin of every frame inside the tab's own document, as the
    /// browser says, without asking the page; a frame that inherits its
    /// document's origin (`about:blank`, `about:srcdoc`) is left out.
    ///
    /// # Errors
    ///
    /// As [`ActTab::address`].
    pub async fn frame_origins(&self, deadline: Deadline) -> Result<Vec<String>, BrowserError> {
        fn walk(tree: &crate::protocol::FrameTree, out: &mut Vec<String>) {
            for child in &tree.child_frames {
                if !child.frame.url.starts_with("about:") {
                    out.push(child.frame.security_origin.clone());
                }
                walk(child, out);
            }
        }
        let tree = self.page.call(&GetFrameTree {}, deadline).await?;
        let mut out = Vec::new();
        walk(&tree.frame_tree, &mut out);
        Ok(out
            .into_iter()
            .map(|origin| self.page.redact.redact(&origin))
            .collect())
    }

    /// Waits for `until`, and ends early when the tab refuses something or
    /// a document of it loads after the count `seen`: `Ok(true)` when the
    /// condition held, `Ok(false)` when the tab changed first. On a page
    /// whose origin the tab's helper does not look at, nothing is looked
    /// at: the wait is for the next change alone.
    ///
    /// # Errors
    ///
    /// As [`Look::wait`] and [`ActTab::changed_since`].
    pub async fn wait_unless_changed(
        &self,
        until: &Primitive,
        seen: u64,
        deadline: Deadline,
    ) -> Result<bool, BrowserError> {
        tokio::select! {
            waited = self.page.wait(until, deadline) => match waited {
                Ok(()) => Ok(true),
                Err(BrowserError::ForeignOrigin(_)) => {
                    self.changed_since(seen, deadline).await.map(|()| false)
                }
                Err(error) => Err(looked(error)),
            },
            changed = self.changed_since(seen, deadline) => changed.map(|()| false),
        }
    }

    /// What this tab refused since the last call, oldest first: a move of
    /// the tab to an origin it was not opened for (`NAVIGATION`), a window
    /// its page opened, which was closed before it fetched anything
    /// (`POPUP`), and a download, for which no file was written
    /// (`DOWNLOAD`); each with the address it was for, cut at the query. A
    /// step that expected none of these has met something no rule covers.
    #[must_use]
    pub fn blocked(&self) -> Vec<RequestShape> {
        self.page.take_blocked()
    }
}

impl ObserveTab {
    pub(crate) fn new(page: Arc<Page>) -> Self {
        Self { page }
    }

    /// Whether the browser this tab is in is lost: its link is closed,
    /// because its process ended or the engine closed it, or this tab has
    /// itself seen its page crash or its target go while the engine had not
    /// released it. Every call on the tab fails from then on, and a tab
    /// opened again for the profile is in a new browser started from the
    /// same data directory.
    ///
    /// A tab can see its loss before the link ends: on Windows, a browser
    /// killed with its children ends its renderers and its pipe in no set
    /// order, and the pipe can end after the step that saw the loss. So a
    /// tab that saw itself lost closes the link here, and the next open of
    /// the profile finds the browser exited and starts a new one, whatever
    /// the order in which the pipe and the process end. A page that
    /// crashed in a browser that still runs costs that browser: a step
    /// cannot tell the two apart without waiting, and the tab can only
    /// read.
    #[must_use]
    pub fn browser_lost(&self) -> bool {
        if self.page.client.closed().is_some() {
            return true;
        }
        let seen = !self.page.released.load(Ordering::SeqCst)
            && matches!(*self.page.status.borrow(), Status::Crashed | Status::Gone);
        if seen {
            self.page.client.close();
        }
        seen
    }

    /// Whether the browser this tab is in still answers: asked for its
    /// version, on the link, within the settings' probe time and the
    /// deadline. A browser that is lost, whose link closes under the
    /// question, or that gives no answer in that time, is lost: the link is
    /// closed, as [`Self::browser_lost`] closes it, and every call on the
    /// tab fails from then on. A refusal is an answer.
    ///
    /// A browser can be dead before the engine sees it: on Windows the
    /// engine's end of the pipe can stay open after the browser's process
    /// has ended, and the tab may have heard nothing, so that its first
    /// request after the loss is the one that finds the pipe broken. A
    /// step that lost its page for a reason the tab cannot tell from a
    /// browser that is going asks this before the next step uses the tab.
    pub async fn browser_answers(&self, deadline: Deadline) -> bool {
        if self.browser_lost() {
            return false;
        }
        let page = &self.page;
        let within = Deadline::after(page.hang_probe.min(deadline.remaining()));
        match page
            .client
            .call(None, &crate::protocol::GetVersion {}, within)
            .await
        {
            Ok(_) | Err(ClientError::Protocol { .. } | ClientError::Shape { .. }) => true,
            Err(ClientError::Closed { .. } | ClientError::Timeout { .. }) => {
                page.client.close();
                false
            }
        }
    }

    /// What this tab refused since the last call, oldest first: a request
    /// whose method could change the system, by its method; a move of the
    /// tab that its page started (`NAVIGATION`); a window its page opened,
    /// closed before it fetched anything (`POPUP`); a download, for which
    /// no file was written (`DOWNLOAD`); a socket
    /// (`WEBSOCKET`, `WEBTRANSPORT`, `PEERCONNECTION`), a shared worker
    /// (`SHAREDWORKER`), or a service worker (`SERVICEWORKER`) refused when
    /// the page made it; and a worker or frame that could not be locked
    /// and was not let run (`WORKER`). Each is a limitation of what was
    /// observed. A refusal is listed once the tab has heard of it, which
    /// can be a moment after the page did: one that is not in this answer
    /// is in the next. After a `goto` or a `wait` that followed it, it is
    /// there.
    #[must_use]
    pub fn blocked(&self) -> Vec<RequestShape> {
        self.page.take_blocked()
    }

    /// The requests its page made by itself since the last call, with GET,
    /// from a script, to one of the tab's origins, and their answers, by
    /// structure alone: oldest first, once no such request is awaiting its
    /// answer, or when the deadline passes with what was answered by then.
    /// A request of any other method is refused, and is in [`Self::blocked`].
    ///
    /// Nothing is recorded until [`Self::record_exchanges`] is called.
    pub async fn exchanges(&self, deadline: Deadline) -> PageExchanges {
        self.page.take_exchanges(deadline).await
    }

    /// Starts recording the requests its page makes by itself, for
    /// [`Self::exchanges`]. A tab whose exchanges nobody takes does not
    /// record them, and fetches no answer of its page.
    pub fn record_exchanges(&self) {
        self.page.recording.store(true, Ordering::SeqCst);
    }

    /// Reads from inside the page the tab is on, with the page's own
    /// session, once: a GET to `path` on the page's own origin, which must
    /// be one of the tab's, with no header a caller chose and no body
    /// (decision 0190). It takes no method: it cannot send another. A
    /// redirect is not followed, and the answer is read as text up to the
    /// tab's limit. The tab's guard still fails any request of the page
    /// that could write, beneath this. No credential, cookie, or token of
    /// the page is read: the browser sends the page's cookies itself, and
    /// nothing else of the page is sent. What comes back passes the
    /// redactor; no header of it is returned.
    ///
    /// # Errors
    ///
    /// [`BrowserError::ForeignOrigin`] when the page is on an origin the
    /// tab was not opened for; nothing was sent.
    /// [`BrowserError::TooLarge`] for an answer longer than the limit.
    /// [`BrowserError::Redirected`] when the answer was a redirect.
    /// [`BrowserError::RequestFailed`] when the page could not complete
    /// the read. A read changes nothing, so
    /// whether it was sent matters to no one: every error is `NotSent` or
    /// as the base reports it.
    pub async fn read_from_page(
        &self,
        path: &PagePath,
        deadline: Deadline,
    ) -> Result<PageAnswer, BrowserError> {
        self.page.read_from_page(path, deadline).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::PausedRequest;

    fn held(
        address: &str,
        number: &str,
        loader: Option<&str>,
        follows: Option<&str>,
    ) -> RequestPaused {
        RequestPaused {
            request_id: number.to_owned(),
            request: PausedRequest {
                url: address.to_owned(),
                method: "GET".to_owned(),
                headers: serde_json::Map::new(),
                body: crate::protocol::HeldBody::default(),
            },
            frame_id: "frame".to_owned(),
            resource_type: "Document".to_owned(),
            network_id: loader.map(str::to_owned),
            redirected_request_id: follows.map(str::to_owned),
        }
    }

    const ASKED: &str = "https://app.example.com/orders";
    const BY_PAGE: &str = "https://app.example.com/orders/A-100/delete";

    #[test]
    fn the_goto_s_request_is_the_first_the_page_did_not_ask_for_and_its_redirects() {
        // With no `goto` under way, every move is the page's.
        let mut moves = Moves::default();
        assert_eq!(
            moves.mover(&held(ASKED, "1.0", Some("L1"), None)),
            Mover::Page
        );

        // A `goto` begins. The page announces a move of its own first, and
        // its request comes before the `goto`'s: it is still the page's.
        let mut moves = Moves {
            going: true,
            ..Moves::default()
        };
        moves.asked(&format!("{BY_PAGE}#now"));
        moves.started(BY_PAGE, "L1", "differentDocument");
        moves.started(ASKED, "L2", "differentDocument");
        assert_eq!(
            moves.mover(&held(BY_PAGE, "1.0", Some("L1"), None)),
            Mover::Page
        );
        assert_eq!(
            moves.mover(&held(ASKED, "2.0", Some("L2"), None)),
            Mover::Goto
        );
        // A redirect of the `goto` is the `goto`'s, by its loader or by
        // the request it follows; a redirect of the page's move is not.
        let hop = "https://app.example.com/orders/";
        assert_eq!(
            moves.mover(&held(hop, "2.1", Some("L2"), Some("2.0"))),
            Mover::Goto
        );
        assert_eq!(
            moves.mover(&held(hop, "2.2", None, Some("2.1"))),
            Mover::Goto
        );
        assert_eq!(
            moves.mover(&held(hop, "1.1", Some("L1"), Some("1.0"))),
            Mover::Page
        );
        // Only one move is the `goto`'s: a second that nobody announced is
        // not let through on its account.
        assert_eq!(
            moves.mover(&held(ASKED, "3.0", Some("L3"), None)),
            Mover::Page
        );

        // The browser's word of where a move started can be missing: the
        // page's announcement alone still marks its request.
        let mut moves = Moves {
            going: true,
            ..Moves::default()
        };
        moves.asked(BY_PAGE);
        assert_eq!(moves.mover(&held(BY_PAGE, "1.0", None, None)), Mover::Page);
        assert_eq!(moves.mover(&held(ASKED, "2.0", None, None)), Mover::Goto);

        // A move through the tab's history is never the `goto`'s.
        let mut moves = Moves {
            going: true,
            ..Moves::default()
        };
        moves.started(ASKED, "L1", "historyDifferentDocument");
        assert_eq!(
            moves.mover(&held(ASKED, "1.0", Some("L1"), None)),
            Mover::Page
        );

        // What the page asks for is remembered up to a bound.
        let mut moves = Moves::default();
        for number in 0..1000 {
            moves.asked(&format!("{ASKED}/{number}"));
            moves.started(
                &format!("{ASKED}/x{number}"),
                &format!("L{number}"),
                "restore",
            );
        }
        assert_eq!(
            (moves.asked.len(), moves.by_page.len()),
            (Moves::KEPT, Moves::KEPT)
        );
    }

    #[test]
    fn the_helper_carries_the_tab_s_origins_and_as_it_stands_refuses_everything() {
        assert_eq!(HELPER.matches(HELPER_ORIGINS).count(), 1);
        let origins = Origins::of([&Address::new("https://app.example.com/orders").unwrap()]);
        let helper = helper_for(&origins);
        assert!(helper.contains(r#"const ALLOWED = ["https://app.example.com"];"#));
        assert!(helper_for(&Origins::default()).contains("const ALLOWED = [];"));
    }
}
