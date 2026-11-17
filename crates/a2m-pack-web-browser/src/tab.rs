//! Tabs: one page each, in one of two postures. A tab that may act, and a
//! tab that may only observe. Everything that only looks is the [`Look`]
//! trait, which both have; only [`ActTab`] has a method that acts.

use std::collections::BTreeSet;
use std::future::Future;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_core::Redact;
use a2m_provider::Deadline;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use crate::browsers::{Address, BrowserInner, Shared};
use crate::client::{Client, ClientError};
use crate::error::{BrowserError, Dispatch, Reason, RequestShape};
use crate::protocol::{
    AddBinding, AddScriptOnNewDocument, AttachToTarget, BypassServiceWorker, CallArgument,
    CallFunctionOn, CloseTarget, Command, ContinueRequest, CreateIsolatedWorld, CreateTarget,
    DescribeNode, DispatchKey, DispatchMouse, Evaluate, Evaluated, Event, EventKind, FailRequest,
    FetchEnable, GetFullAxTree, GetProperties, HandleDialog, InsertText, MediaFeature, Navigate,
    NetworkEnable, PageEnable, RequestPattern, RunIfWaiting, RuntimeEnable, SetAutoAttach,
    SetDeviceMetrics, SetEmulatedMedia, SetLifecycleEvents, SetLocale, SetTimezone,
};
use crate::snapshot::{Snapshot, SnapshotLimits};

/// The helper that runs in the page, in a world of its own.
const HELPER: &str = include_str!("helper.js");

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
const REFUSE_SOCKETS: &str = r#"(() => {
  const report = (what, address) => {
    try { globalThis.__anything2mcpRefused(what + " " + String(address)); } catch (_) {}
  };
  const refusal = () => new DOMException("Refused while the page is only observed.", "SecurityError");
  for (const name of ["WebSocket", "WebTransport", "SharedWorker", "RTCPeerConnection"]) {
    if (!(name in globalThis)) continue;
    const refused = function (address) {
      report(name, address);
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

/// One request an observing tab lets through although its method could
/// change the system. Only a person marks a request safe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeRequest {
    /// The method, in capitals, such as `POST`.
    pub method: String,
    /// What the address must start with, such as the address of a search
    /// endpoint that is called with POST.
    pub address_prefix: String,
}

/// The requests an observing tab lets through beside GET, HEAD, and
/// OPTIONS. Empty by default.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SafeRequests {
    /// Each request marked safe.
    pub allowed: Vec<SafeRequest>,
}

impl SafeRequests {
    fn allows(&self, method: &str, address: &str) -> bool {
        matches!(method, "GET" | "HEAD" | "OPTIONS")
            || self.allowed.iter().any(|safe| {
                safe.method == method
                    && !safe.address_prefix.is_empty()
                    && address.starts_with(&safe.address_prefix)
            })
    }
}

pub(crate) enum Posture {
    Act,
    Observe(SafeRequests),
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
    /// A selector. The last resort.
    Css {
        /// The selector.
        selector: String,
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

/// One element of one page generation. After a navigation, or a re-render
/// that removes it, using it is [`BrowserError::NotFound`]; it is never
/// looked for again silently.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Element {
    object: String,
    tab: u64,
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
/// another.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Input {
    /// Focus, select what is there, and insert the text as one edit.
    Fill(String),
    /// Focus, select what is there, and type the text key by key.
    Type(String),
    /// Set the value through the element's own setter and tell the page.
    NativeSet(String),
    /// Focus and press one key.
    Press(Key),
    /// A click in the middle of the element.
    Click,
    /// Bring a check box or a radio button to this state, by a click when
    /// it is not there already.
    Check(bool),
    /// Choose the option of a select that has this value.
    SelectByValue(String),
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

/// A request sent from inside the page, with the page's own session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRequest {
    /// The method, in capitals.
    pub method: String,
    /// Where it goes. The crate above has checked the origin.
    pub address: Address,
    /// Its headers.
    pub headers: Vec<(String, String)>,
    /// Its body.
    pub body: Option<String>,
}

/// The answer to a [`PageRequest`], with credentials removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageResponse {
    /// The status.
    pub status: u16,
    /// The address that answered, after redirects.
    pub address: String,
    /// The headers, without cookies and authorization, in order of name.
    pub headers: Vec<(String, String)>,
    /// The body, with every known secret replaced.
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Open,
    Dialog,
    Crashed,
    Gone,
}

#[derive(Debug, Clone, Default)]
struct Nav {
    /// The loader whose document has been parsed.
    loaded: Option<String>,
    /// The loader of the last document response, and its status.
    document: Option<(String, u16)>,
}

pub(crate) struct Page {
    client: Client,
    session: String,
    target: String,
    serial: u64,
    call_deadline: Duration,
    max_read: usize,
    redact: Arc<dyn Redact>,
    world: watch::Sender<Option<i64>>,
    status: watch::Sender<Status>,
    nav: watch::Sender<Nav>,
    blocked: Mutex<Vec<RequestShape>>,
    safe: Option<SafeRequests>,
    browser: Arc<BrowserInner>,
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

impl Page {
    fn shown(&self, text: &str) -> String {
        self.redact.redact(&cut(text))
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
        loop {
            tokio::select! {
                biased;
                reply = flight.finish(deadline) => return reply.map_err(Ended::Client),
                changed = status.changed() => {
                    let now = if changed.is_ok() { *status.borrow_and_update() } else { Status::Gone };
                    if let Some(error) = Self::interrupted(now, heed_dialog, flight.dispatch()) {
                        return Err(Ended::Browser(error));
                    }
                }
            }
        }
    }

    async fn call<C: Command>(
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
        arguments: Vec<CallArgument<'_>>,
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
                if text.contains("__a2m") {
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
        arguments: impl Fn() -> Option<Vec<CallArgument<'static>>>,
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
                        expression: HELPER,
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
        arguments: Vec<CallArgument<'_>>,
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

    fn answer<T: DeserializeOwned>(evaluated: Evaluated) -> Result<T, BrowserError> {
        evaluated.result.read().ok_or_else(|| {
            BrowserError::Unexpected("the helper answered in a shape no rule covers".to_owned())
        })
    }

    // ---- looking ----

    async fn goto(&self, address: &Address, deadline: Deadline) -> Result<Loaded, BrowserError> {
        self.goto_text(address.as_str(), deadline).await
    }

    pub(crate) async fn goto_text(
        &self,
        address: &str,
        deadline: Deadline,
    ) -> Result<Loaded, BrowserError> {
        let mut nav = self.nav.subscribe();
        let mut status = self.status.subscribe();
        let navigated = self.call(&Navigate { url: address }, deadline).await?;
        if let Some(why) = navigated.error_text.filter(|why| !why.is_empty()) {
            return Err(BrowserError::NotLoaded(self.shown(&why)));
        }
        let mut document_status = None;
        if let Some(loader) = navigated.loader_id {
            // The document of this navigation has been parsed: a protocol
            // event, not a guess at how long a page takes.
            loop {
                if nav.borrow_and_update().loaded.as_deref() == Some(loader.as_str()) {
                    break;
                }
                if let Some(error) =
                    Self::interrupted(*status.borrow_and_update(), true, Dispatch::Sent)
                {
                    return Err(error);
                }
                tokio::select! {
                    changed = nav.changed() => {
                        if changed.is_err() {
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
                .filter(|(of, _)| *of == loader)
                .map(|(_, status)| *status);
        }
        let (address, title) = self.address_and_title(deadline).await?;
        Ok(Loaded {
            address,
            title,
            status: document_status,
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
        ];
        let evaluated = self
            .on_element(
                element,
                "function (what, limit) { return __a2m.read(this, what, limit); }",
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
                    let left = u64::try_from(deadline.remaining().as_millis()).unwrap_or(u64::MAX);
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
            Waited { ok: true, .. } => Ok(()),
            Waited {
                error: Some(error), ..
            } => Err(BrowserError::Unexpected(self.shown(&error))),
            Waited { .. } => Err(BrowserError::Timeout {
                dispatch: Dispatch::NotSent,
            }),
        }
    }

    async fn snapshot(
        &self,
        limits: &SnapshotLimits,
        deadline: Deadline,
    ) -> Result<Snapshot, BrowserError> {
        let (address, title) = self.address_and_title(deadline).await?;
        let masked = self.password_nodes(deadline).await?;
        let tree = self.call(&GetFullAxTree {}, deadline).await?;
        Ok(Snapshot::build(
            address,
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
                            object_id: &element,
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
        arguments: Vec<CallArgument<'_>>,
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
            Input::SelectByValue(value) => self
                .checked(
                    element,
                    "function (value) { return __a2m.selectByValue(this, value); }",
                    vec![CallArgument::text(value)],
                    deadline,
                )
                .await
                .map(|_| ()),
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

    async fn fetch(
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
            status: Option<u16>,
            address: String,
            headers: Vec<(String, String)>,
            body: String,
        }
        let shape =
            || BrowserError::Unexpected("a helper argument could not be written".to_owned());
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
            .map(|(name, value)| (name, self.redact.redact(&value)))
            .collect();
        headers.sort();
        Ok(PageResponse {
            status,
            address: self.redact.redact(&answer.address),
            headers,
            body: self.redact.redact(&answer.body),
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
        self.raw(&command, deadline, false)
            .await
            .map(|_| ())
            .map_err(|error| match error {
                Ended::Browser(error) => error,
                Ended::Client(error) => self.client_error(error),
            })
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

    // ---- the tab's own task ----

    fn block(&self, method: &str, address: &str) {
        let shape = RequestShape {
            method: method.to_owned(),
            address: self.redact.redact(&cut(without_query(address))),
        };
        if let Ok(mut blocked) = self.blocked.lock() {
            // A page that keeps trying must not fill the engine's memory.
            if blocked.len() < 1024 {
                blocked.push(shape);
            }
        }
    }

    /// Puts the lock on a worker or a frame that attached under an
    /// observing tab, then lets it run. When the lock cannot be put on,
    /// it is not let run, and that is reported.
    ///
    /// A dedicated worker has no interception of its own to switch on: its
    /// requests pass through its page's, which is already on (measured
    /// against the browser of decision 0111). Every other kind must take
    /// its own, or it stays stopped.
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
                },
                deadline,
            )
            .await;
        let nested = match nested {
            Ok(_) => true,
            Err(ClientError::Protocol { code, .. }) => code == METHOD_NOT_FOUND,
            Err(_) => false,
        };
        let _ = client
            .call(
                Some(session),
                &AddBinding {
                    name: REFUSED_BINDING,
                },
                deadline,
            )
            .await;
        let sockets_refused = client
            .call(
                Some(session),
                &Evaluate {
                    expression: REFUSE_SOCKETS,
                    context_id: None,
                    return_by_value: true,
                    await_promise: false,
                },
                deadline,
            )
            .await
            .is_ok_and(|evaluated| evaluated.exception_details.is_none());
        if !(intercepted && nested && sockets_refused) {
            self.block("WORKER", "");
            return;
        }
        if waiting {
            let _ = client.call(Some(session), &RunIfWaiting {}, deadline).await;
        }
    }

    async fn heard(&self, event: Event) {
        let own = event.session.as_deref() == Some(self.session.as_str());
        match event.kind {
            EventKind::Lifecycle(lifecycle) if own => {
                if lifecycle.frame_id == self.target && lifecycle.name == "DOMContentLoaded" {
                    self.nav
                        .send_modify(|nav| nav.loaded = Some(lifecycle.loader_id));
                }
            }
            EventKind::ResponseReceived(received) if own => {
                if received.kind == "Document" && !received.loader_id.is_empty() {
                    self.nav.send_modify(|nav| {
                        nav.document = Some((received.loader_id, received.response.status));
                    });
                }
            }
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
            EventKind::DialogOpening if own => {
                self.status.send_if_modified(|status| {
                    let open = *status == Status::Open;
                    if open {
                        *status = Status::Dialog;
                    }
                    open
                });
            }
            EventKind::DialogClosed if own => {
                self.status.send_if_modified(|status| {
                    let dialog = *status == Status::Dialog;
                    if dialog {
                        *status = Status::Open;
                    }
                    dialog
                });
            }
            EventKind::TargetCrashed if own => {
                self.status.send_replace(Status::Crashed);
            }
            EventKind::Attached(attached) => {
                if self.safe.is_some() {
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
                let Some(safe) = &self.safe else { return };
                let deadline = Deadline::after(self.call_deadline);
                let session = event.session.as_deref();
                let (method, address) = (&paused.request.method, &paused.request.url);
                if safe.allows(method, address) {
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
                } else {
                    // Failed before it leaves the browser. If this call is
                    // lost, the request stays paused: it is never sent.
                    let _ = self
                        .client
                        .call(
                            session,
                            &FailRequest {
                                request_id: &paused.request_id,
                                error_reason: "BlockedByClient",
                            },
                            deadline,
                        )
                        .await;
                    self.block(method, address);
                }
            }
            EventKind::BindingCalled(called) if called.name == REFUSED_BINDING => {
                // The payload is the page's: only a known word is taken as
                // the method, and the rest is an address like any other.
                let (what, address) = called.payload.split_once(' ').unwrap_or(("", ""));
                let method = match what {
                    "WebSocket" => "WEBSOCKET",
                    "WebTransport" => "WEBTRANSPORT",
                    "SharedWorker" => "SHAREDWORKER",
                    "RTCPeerConnection" => "PEERCONNECTION",
                    "ServiceWorker" => "SERVICEWORKER",
                    _ => "REFUSED",
                };
                self.block(method, address);
            }
            _ => {}
        }
    }

    /// The requests this observing tab refused, oldest first, taken out.
    fn take_blocked(&self) -> Vec<RequestShape> {
        self.blocked
            .lock()
            .map(|mut blocked| std::mem::take(&mut *blocked))
            .unwrap_or_default()
    }

    /// Ends the tab: its events stop, and the browser is asked to close
    /// its page. Safe to call more than once.
    fn release(&self) {
        self.client.unsubscribe(&self.session);
        if self.status.send_replace(Status::Gone) == Status::Gone {
            return;
        }
        self.browser.tabs.fetch_sub(1, Ordering::SeqCst);
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let client = self.client.clone();
            let target = self.target.clone();
            let deadline = Deadline::after(self.call_deadline);
            runtime.spawn(async move {
                let _ = client
                    .call(None, &CloseTarget { target_id: &target }, deadline)
                    .await;
            });
        }
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
            request_stage: "Request",
        }],
    }
}

async fn pump(page: Arc<Page>, mut events: mpsc::UnboundedReceiver<Event>) {
    while let Some(event) = events.recv().await {
        page.heard(event).await;
    }
    // The subscription ended: the tab was released, or the link closed.
    page.status.send_if_modified(|status| {
        let open = *status != Status::Gone && *status != Status::Crashed;
        if open {
            *status = Status::Gone;
        }
        open
    });
}

/// Opens a tab on `about:blank` in the browser, in the posture given.
pub(crate) async fn open_page(
    shared: &Arc<Shared>,
    browser: &Arc<BrowserInner>,
    env: &PageEnvironment,
    posture: Posture,
) -> Result<Arc<Page>, BrowserError> {
    let client = browser.client.clone();
    let settings = &shared.settings;
    let deadline = Deadline::after(settings.call_deadline);
    let lost = |error: ClientError| match error {
        ClientError::Closed { dispatch, .. } => BrowserError::Disconnected { dispatch },
        ClientError::Timeout { dispatch } => BrowserError::Timeout { dispatch },
        ClientError::Protocol { message, .. } => {
            BrowserError::Unexpected(shared.redact.redact(&cut(&message)))
        }
        ClientError::Shape { method } => {
            BrowserError::Unexpected(format!("the reply of {method} has another shape"))
        }
    };
    let created = client
        .call(None, &CreateTarget { url: "about:blank" }, deadline)
        .await
        .map_err(lost)?;
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
        blocked: Mutex::new(Vec::new()),
        safe: match posture {
            Posture::Act => None,
            Posture::Observe(safe) => Some(safe),
        },
        browser: Arc::clone(browser),
    });
    browser.tabs.fetch_add(1, Ordering::SeqCst);
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
            },
            deadline,
        )
        .await?;
        page.call(&NetworkEnable {}, deadline).await?;
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
                source: REFUSE_SOCKETS,
                world_name: None,
            },
            deadline,
        )
        .await?;
    } else {
        page.call(&NetworkEnable {}, deadline).await?;
        page.call(&RuntimeEnable {}, deadline).await?;
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
            source: HELPER,
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
                expression: HELPER,
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
    /// parsed, which the browser reports as an event.
    fn goto(
        &self,
        address: &Address,
        deadline: Deadline,
    ) -> impl Future<Output = Result<Loaded, BrowserError>> + Send;

    /// The one rendered element the query matches, now. It does not wait:
    /// wait for [`Primitive::Visible`] first where the element may be
    /// late.
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

/// A tab that may act. Closed when dropped.
pub struct ActTab {
    pub(crate) page: Arc<Page>,
}

/// A tab that can read and cannot act: it has no method that acts, and the
/// browser fails every request of its page that could change the system.
/// Closed when dropped.
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
    /// before anything was sent. Every other error says through
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
    /// session, once.
    ///
    /// # Errors
    ///
    /// [`BrowserError::RequestFailed`] when the page could not complete
    /// it; whether it was sent is then not known.
    pub async fn fetch(
        &self,
        request: &PageRequest,
        deadline: Deadline,
    ) -> Result<PageResponse, BrowserError> {
        self.page.fetch(request, deadline).await
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

impl ObserveTab {
    pub(crate) fn new(page: Arc<Page>) -> Self {
        Self { page }
    }

    /// What this tab refused since the last call, oldest first: a request
    /// whose method could change the system, by its method; a socket
    /// (`WEBSOCKET`, `WEBTRANSPORT`, `PEERCONNECTION`), a shared worker
    /// (`SHAREDWORKER`), or a service worker (`SERVICEWORKER`) refused when
    /// the page made it; and a worker or frame that could not be locked
    /// and was not let run (`WORKER`). Each is a limitation of what was
    /// observed.
    #[must_use]
    pub fn blocked(&self) -> Vec<RequestShape> {
        self.page.take_blocked()
    }
}
