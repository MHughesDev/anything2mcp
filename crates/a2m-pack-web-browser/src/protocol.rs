//! The DevTools protocol, as far as the engine uses it: one typed request
//! and reply for each command, the events it listens for, and the envelope
//! every message travels in. Nothing outside this module sees untyped JSON
//! (decision 0070).
//!
//! A message is JSON. A request carries an `id` and a `method`; its reply
//! carries the same `id` and a `result` or an `error`. A message with a
//! `method` and no `id` is an event. A message for a tab carries the tab's
//! `sessionId` at the top level.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One command of the protocol: its method, its parameters (the type
/// itself), and its reply.
pub(crate) trait Command: Serialize {
    const METHOD: &'static str;
    type Reply: DeserializeOwned;
}

/// A reply with no fields the engine reads.
#[derive(Debug, Deserialize)]
pub(crate) struct Nothing {}

#[derive(Serialize)]
struct Envelope<'a, P: Serialize> {
    id: u64,
    method: &'static str,
    #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
    session: Option<&'a str>,
    params: &'a P,
}

/// The bytes of one request, with the NUL that ends it.
pub(crate) fn encode<C: Command>(id: u64, session: Option<&str>, command: &C) -> Option<Vec<u8>> {
    let mut bytes = serde_json::to_vec(&Envelope {
        id,
        method: C::METHOD,
        session,
        params: command,
    })
    .ok()?;
    bytes.push(0);
    Some(bytes)
}

/// The browser's own error for a request it received and refused.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct ProtocolError {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub message: String,
}

/// The result of a reply, not yet read as the reply of its command.
#[derive(Debug)]
pub(crate) struct ReplyBody(Value);

impl ReplyBody {
    /// Reads the result as the reply of `C`. `None` when the shape is not
    /// the one the command declares.
    pub(crate) fn read<C: Command>(self) -> Option<C::Reply> {
        serde_json::from_value(self.0).ok()
    }
}

/// One message from the browser.
#[derive(Debug)]
pub(crate) enum Incoming {
    Reply {
        id: u64,
        outcome: Result<ReplyBody, ProtocolError>,
    },
    Event(Event),
    /// An event the engine does not listen for.
    Ignored,
}

#[derive(Deserialize)]
struct RawIncoming {
    id: Option<u64>,
    method: Option<String>,
    #[serde(rename = "sessionId")]
    session: Option<String>,
    #[serde(default)]
    params: Value,
    #[serde(default)]
    result: Value,
    error: Option<ProtocolError>,
}

/// Reads one message, without its NUL. `None` when it is not a protocol
/// message: not JSON, not an object, or neither a reply nor an event.
pub(crate) fn decode(bytes: &[u8]) -> Option<Incoming> {
    let raw: RawIncoming = serde_json::from_slice(bytes).ok()?;
    match (raw.id, raw.method) {
        (Some(id), None) => Some(Incoming::Reply {
            id,
            outcome: match raw.error {
                Some(error) => Err(error),
                None => Ok(ReplyBody(raw.result)),
            },
        }),
        (None, Some(method)) => Some(match EventKind::read(&method, raw.params) {
            Some(kind) => Incoming::Event(Event {
                session: raw.session,
                kind,
            }),
            None => Incoming::Ignored,
        }),
        _ => None,
    }
}

/// An event, with the session it came from.
#[derive(Debug, Clone)]
pub(crate) struct Event {
    pub session: Option<String>,
    pub kind: EventKind,
}

/// The events the engine listens for.
#[derive(Debug, Clone)]
pub(crate) enum EventKind {
    Lifecycle(Lifecycle),
    ContextCreated(ContextCreated),
    ContextDestroyed(ContextDestroyed),
    ContextsCleared,
    DialogOpening(DialogOpened),
    DialogClosed,
    TargetCrashed,
    Attached(Attached),
    Detached(Detached),
    RequestPaused(RequestPaused),
    ResponseReceived(ResponseReceived),
    BindingCalled(BindingCalled),
    /// The page itself asked for a move of one of its frames.
    MoveAsked(MoveAsked),
    /// A move of a frame to another document has started.
    MoveStarted(MoveStarted),
    /// A download is about to begin.
    DownloadBegins(DownloadBegins),
    /// The page registered tools (research note 25).
    ToolsAdded(Value),
    /// The page took tools away.
    ToolsRemoved(Value),
    /// A call of a page's tool ended.
    ToolResponded(Value),
    /// A request's answer has arrived whole.
    LoadingFinished(Loading),
    /// A request failed, or was ended.
    LoadingFailed(Loading),
}

/// What the browser says of a dialog that opened.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DialogOpened {
    /// What the dialog says.
    #[serde(default)]
    pub message: String,
    /// `alert`, `confirm`, `prompt`, or `beforeunload`.
    #[serde(default, rename = "type")]
    pub kind: String,
}

impl EventKind {
    /// `None` for a method the engine does not listen for, and for a known
    /// method whose parameters are not the shape expected: such an event is
    /// dropped, and what waited for it ends at its deadline.
    fn read(method: &str, params: Value) -> Option<Self> {
        fn of<T: DeserializeOwned>(params: Value) -> Option<T> {
            serde_json::from_value(params).ok()
        }
        Some(match method {
            "Page.lifecycleEvent" => Self::Lifecycle(of(params)?),
            "Runtime.executionContextCreated" => Self::ContextCreated(of(params)?),
            "Runtime.executionContextDestroyed" => Self::ContextDestroyed(of(params)?),
            "Runtime.executionContextsCleared" => Self::ContextsCleared,
            "Page.javascriptDialogOpening" => Self::DialogOpening(of(params)?),
            "Page.javascriptDialogClosed" => Self::DialogClosed,
            "Inspector.targetCrashed" => Self::TargetCrashed,
            "Target.attachedToTarget" => Self::Attached(of(params)?),
            "Target.detachedFromTarget" => Self::Detached(of(params)?),
            "Fetch.requestPaused" => Self::RequestPaused(of(params)?),
            "Network.responseReceived" => Self::ResponseReceived(of(params)?),
            "Runtime.bindingCalled" => Self::BindingCalled(of(params)?),
            "Page.frameRequestedNavigation" => Self::MoveAsked(of(params)?),
            "Page.frameStartedNavigating" => Self::MoveStarted(of(params)?),
            "Page.downloadWillBegin" => Self::DownloadBegins(of(params)?),
            "WebMCP.toolsAdded" => Self::ToolsAdded(params),
            "WebMCP.toolsRemoved" => Self::ToolsRemoved(params),
            "WebMCP.toolResponded" => Self::ToolResponded(params),
            "Network.loadingFinished" => Self::LoadingFinished(of(params)?),
            "Network.loadingFailed" => Self::LoadingFailed(of(params)?),
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Lifecycle {
    pub frame_id: String,
    pub loader_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ContextCreated {
    pub context: ContextDescription,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContextDescription {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub aux_data: ContextAux,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContextAux {
    #[serde(default)]
    pub frame_id: String,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContextDestroyed {
    pub execution_context_id: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Attached {
    pub session_id: String,
    pub target_info: TargetInfo,
    #[serde(default)]
    pub waiting_for_debugger: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TargetInfo {
    /// `page`, `iframe`, `worker`, and so on.
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub target_id: String,
    /// The target that opened this one, when one did.
    pub opener_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Detached {
    pub session_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequestPaused {
    pub request_id: String,
    pub request: PausedRequest,
    #[serde(default)]
    pub frame_id: String,
    /// `Document` for the request of a frame's own document.
    #[serde(default)]
    pub resource_type: String,
    /// For a document, the loader of the move it belongs to.
    pub network_id: Option<String>,
    /// The request this one follows, when it follows a redirect.
    pub redirected_request_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PausedRequest {
    pub url: String,
    pub method: String,
    /// The request's headers. Only their names are kept; the values of
    /// the few headers the browser sets by itself are compared with the
    /// page's own document's, and then dropped.
    #[serde(default)]
    pub headers: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResponseReceived {
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub loader_id: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    pub response: ResponseDescription,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResponseDescription {
    pub status: u16,
    #[serde(default)]
    pub mime_type: String,
    /// The response's headers. Only the one that names a challenge is
    /// looked at, and none is kept.
    #[serde(default)]
    pub headers: serde_json::Map<String, Value>,
}

impl ResponseDescription {
    /// Whether the edge says the visitor is held for a challenge.
    pub(crate) fn challenged(&self) -> bool {
        self.headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("cf-mitigated")
                && value
                    .as_str()
                    .is_some_and(|text| text.eq_ignore_ascii_case("challenge"))
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Loading {
    pub request_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MoveAsked {
    pub frame_id: String,
    #[serde(default)]
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MoveStarted {
    pub frame_id: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub loader_id: String,
    /// `differentDocument`, `reload`, `historyDifferentDocument`, and so on.
    #[serde(default)]
    pub navigation_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DownloadBegins {
    #[serde(default)]
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BindingCalled {
    pub name: String,
    #[serde(default)]
    pub payload: String,
}

/// Declares a command whose parameters are the struct's own fields.
macro_rules! command {
    ($name:ty, $method:literal => $reply:ty) => {
        impl Command for $name {
            const METHOD: &'static str = $method;
            type Reply = $reply;
        }
    };
}

// ---- Browser ----

#[derive(Debug, Serialize)]
pub(crate) struct GetVersion {}
command!(GetVersion, "Browser.getVersion" => Version);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Version {
    pub protocol_version: String,
    pub product: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct CloseBrowser {}
command!(CloseBrowser, "Browser.close" => Nothing);

/// What the browser does with every download: `deny` writes no file. With
/// no context named, for the browser's default context.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetDownloadBehavior<'a> {
    pub behavior: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser_context_id: Option<&'a str>,
}
command!(SetDownloadBehavior<'_>, "Browser.setDownloadBehavior" => Nothing);

/// The permissions the browser grants: those listed, for every origin,
/// and no other. An empty list refuses every permission a page asks for.
/// With no context named, for the browser's default context.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GrantPermissions<'a> {
    pub permissions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser_context_id: Option<&'a str>,
}
command!(GrantPermissions<'_>, "Browser.grantPermissions" => Nothing);

// ---- Target ----

/// A browser context of its own, which keeps its cookies and storage in
/// memory and writes none of them to the data directory.
#[derive(Debug, Serialize)]
pub(crate) struct CreateBrowserContext {}
command!(CreateBrowserContext, "Target.createBrowserContext" => CreatedBrowserContext);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreatedBrowserContext {
    pub browser_context_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateTarget<'a> {
    pub url: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser_context_id: Option<&'a str>,
}
command!(CreateTarget<'_>, "Target.createTarget" => CreatedTarget);

// ---- Storage ----

/// One cookie as the browser holds it. Only the parts the engine keeps are
/// read; the value is a credential and is handled as one by the caller.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    /// Seconds since the epoch; negative for a cookie that ends with the
    /// browser. Carried, never compared.
    #[serde(default)]
    pub expires: Option<serde_json::Number>,
    #[serde(default)]
    pub http_only: bool,
    #[serde(default)]
    pub secure: bool,
    #[serde(default)]
    pub session: bool,
    #[serde(default)]
    pub same_site: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GetCookies<'a> {
    pub browser_context_id: &'a str,
}
command!(GetCookies<'_>, "Storage.getCookies" => GotCookies);

#[derive(Debug, Deserialize)]
pub(crate) struct GotCookies {
    #[serde(default)]
    pub cookies: Vec<StoredCookie>,
}

/// One cookie to place, in the form the browser takes it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CookieToSet<'a> {
    pub name: &'a str,
    pub value: &'a str,
    pub domain: &'a str,
    pub path: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires: Option<&'a serde_json::Number>,
    pub http_only: bool,
    pub secure: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub same_site: Option<&'a str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetCookies<'a> {
    pub cookies: Vec<CookieToSet<'a>>,
    pub browser_context_id: &'a str,
}
command!(SetCookies<'_>, "Storage.setCookies" => Nothing);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreatedTarget {
    pub target_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttachToTarget<'a> {
    pub target_id: &'a str,
    pub flatten: bool,
}
command!(AttachToTarget<'_>, "Target.attachToTarget" => AttachedSession);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttachedSession {
    pub session_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloseTarget<'a> {
    pub target_id: &'a str,
}
command!(CloseTarget<'_>, "Target.closeTarget" => Nothing);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GetTargetInfo<'a> {
    pub target_id: &'a str,
}
command!(GetTargetInfo<'_>, "Target.getTargetInfo" => TargetAddress);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TargetAddress {
    pub target_info: TargetUrl,
}

/// Where a target is, as the browser says; its title is not read.
#[derive(Debug, Deserialize)]
pub(crate) struct TargetUrl {
    pub url: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct GetTargets {}
command!(GetTargets, "Target.getTargets" => TargetInfos);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TargetInfos {
    pub target_infos: Vec<ListedTarget>,
}

/// One target as the browser lists it. Only what tells the engine's own
/// pages from the browser's first window is read.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListedTarget {
    pub target_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub browser_context_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetAutoAttach {
    pub auto_attach: bool,
    pub wait_for_debugger_on_start: bool,
    pub flatten: bool,
    /// Which targets attach; the first entry that fits a target decides.
    /// Left out, the browser's own choice applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<Vec<TargetFilter>>,
}
command!(SetAutoAttach, "Target.setAutoAttach" => Nothing);

/// One entry of a target filter: a kind of target, or every kind when it
/// names none, taken in or left out.
#[derive(Debug, Serialize)]
pub(crate) struct TargetFilter {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<&'static str>,
    pub exclude: bool,
}

// ---- Page ----

#[derive(Debug, Serialize)]
pub(crate) struct PageEnable {}
command!(PageEnable, "Page.enable" => Nothing);

#[derive(Debug, Serialize)]
pub(crate) struct SetLifecycleEvents {
    pub enabled: bool,
}
command!(SetLifecycleEvents, "Page.setLifecycleEventsEnabled" => Nothing);

#[derive(Debug, Serialize)]
pub(crate) struct Navigate<'a> {
    pub url: &'a str,
}
command!(Navigate<'_>, "Page.navigate" => Navigated);

#[derive(Debug, Serialize)]
pub(crate) struct GetFrameTree {}
command!(GetFrameTree, "Page.getFrameTree" => FrameTreeReply);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FrameTreeReply {
    pub frame_tree: FrameTree,
}

/// One frame and the frames inside it. Only where each frame is, and on
/// which origin, is read.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FrameTree {
    pub frame: FrameDescription,
    #[serde(default)]
    pub child_frames: Vec<FrameTree>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FrameDescription {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub security_origin: String,
}

/// Puts a document the engine wrote into a frame, with no request.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetDocumentContent<'a> {
    pub frame_id: &'a str,
    pub html: &'a str,
}
command!(SetDocumentContent<'_>, "Page.setDocumentContent" => Nothing);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Navigated {
    pub loader_id: Option<String>,
    pub error_text: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AddScriptOnNewDocument<'a> {
    pub source: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub world_name: Option<&'a str>,
}
command!(AddScriptOnNewDocument<'_>, "Page.addScriptToEvaluateOnNewDocument" => Nothing);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateIsolatedWorld<'a> {
    pub frame_id: &'a str,
    pub world_name: &'a str,
}
command!(CreateIsolatedWorld<'_>, "Page.createIsolatedWorld" => IsolatedWorld);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IsolatedWorld {
    pub execution_context_id: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandleDialog<'a> {
    pub accept: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_text: Option<&'a str>,
}
command!(HandleDialog<'_>, "Page.handleJavaScriptDialog" => Nothing);

// ---- Runtime ----

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeEnable {}
command!(RuntimeEnable, "Runtime.enable" => Nothing);

/// One argument of a call: a plain value. An element is passed as `this`,
/// through `object_id`, not as an argument.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CallArgument {
    Value(Value),
}

impl CallArgument {
    /// Any plain data, by value. `None` when it cannot be written as JSON.
    pub(crate) fn of<T: Serialize>(data: &T) -> Option<Self> {
        serde_json::to_value(data).ok().map(Self::Value)
    }

    pub(crate) fn text(text: &str) -> Self {
        Self::Value(Value::String(text.to_owned()))
    }

    pub(crate) fn number(number: usize) -> Self {
        Self::Value(Value::from(number))
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CallFunctionOn<'a> {
    pub function_declaration: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_context_id: Option<i64>,
    pub arguments: Vec<CallArgument>,
    pub return_by_value: bool,
    pub await_promise: bool,
}
command!(CallFunctionOn<'_>, "Runtime.callFunctionOn" => Evaluated);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Evaluate<'a> {
    pub expression: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_id: Option<i64>,
    pub return_by_value: bool,
    pub await_promise: bool,
}
command!(Evaluate<'_>, "Runtime.evaluate" => Evaluated);

/// An expression run as if a person had just used the page. For this
/// crate's tests only: it stands for a page that has a person's gesture to
/// spend, which a tab that only observes never gives it.
#[cfg(test)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EvaluateAsUser<'a> {
    pub expression: &'a str,
    pub return_by_value: bool,
    pub await_promise: bool,
    pub user_gesture: bool,
}
#[cfg(test)]
command!(EvaluateAsUser<'_>, "Runtime.evaluate" => Evaluated);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Evaluated {
    pub result: RemoteObject,
    pub exception_details: Option<ExceptionDetails>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteObject {
    #[serde(rename = "type")]
    pub kind: String,
    pub subtype: Option<String>,
    #[serde(default)]
    value: Value,
    pub object_id: Option<String>,
    pub description: Option<String>,
}

impl RemoteObject {
    /// What was returned by value, read as plain data.
    pub(crate) fn read<T: DeserializeOwned>(self) -> Option<T> {
        serde_json::from_value(self.value).ok()
    }

    /// What was returned, when it is a count.
    pub(crate) fn count(&self) -> Option<usize> {
        (self.kind == "number")
            .then(|| self.value.as_u64())
            .flatten()
            .and_then(|count| usize::try_from(count).ok())
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct ExceptionDetails {
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GetProperties<'a> {
    pub object_id: &'a str,
    pub own_properties: bool,
}
command!(GetProperties<'_>, "Runtime.getProperties" => Properties);

#[derive(Debug, Deserialize)]
pub(crate) struct Properties {
    pub result: Vec<Property>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Property {
    pub name: String,
    pub value: Option<RemoteObject>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DescribeNode<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<&'a str>,
    /// The browser's own number of the node, where no object is held.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_node_id: Option<i64>,
}
command!(DescribeNode<'_>, "DOM.describeNode" => DescribedNode);

#[derive(Debug, Deserialize)]
pub(crate) struct DescribedNode {
    pub node: NodeDescription,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NodeDescription {
    pub backend_node_id: i64,
    #[serde(default)]
    pub local_name: String,
    /// The node's attributes: a name, then its value, and so on.
    #[serde(default)]
    pub attributes: Vec<String>,
}

impl NodeDescription {
    /// The value of one attribute, when the node has it.
    pub(crate) fn attribute(&self, name: &str) -> Option<&str> {
        let (pairs, _) = self.attributes.as_chunks::<2>();
        pairs
            .iter()
            .find(|[found, _]| found.eq_ignore_ascii_case(name))
            .map(|[_, value]| value.as_str())
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct RunIfWaiting {}
command!(RunIfWaiting, "Runtime.runIfWaitingForDebugger" => Nothing);

#[derive(Debug, Serialize)]
pub(crate) struct AddBinding<'a> {
    pub name: &'a str,
}
command!(AddBinding<'_>, "Runtime.addBinding" => Nothing);

// ---- Accessibility ----

#[derive(Debug, Serialize)]
pub(crate) struct GetFullAxTree {}
command!(GetFullAxTree, "Accessibility.getFullAXTree" => AxNodes);

#[derive(Debug, Deserialize)]
pub(crate) struct AxNodes {
    pub nodes: Vec<AxNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AxNode {
    pub node_id: String,
    #[serde(default)]
    pub ignored: bool,
    pub role: Option<AxValue>,
    pub name: Option<AxValue>,
    pub value: Option<AxValue>,
    #[serde(default)]
    pub properties: Vec<AxProperty>,
    pub parent_id: Option<String>,
    #[serde(default)]
    pub child_ids: Vec<String>,
    #[serde(rename = "backendDOMNodeId")]
    pub backend_dom_node_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AxValue {
    #[serde(default)]
    pub value: Value,
}

impl AxValue {
    /// The value as text: a string as it is, a number or a flag written
    /// out, anything else empty.
    pub(crate) fn text(&self) -> String {
        match &self.value {
            Value::String(text) => text.clone(),
            Value::Bool(flag) => flag.to_string(),
            Value::Number(number) => number.to_string(),
            _ => String::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct AxProperty {
    pub name: String,
    pub value: AxValue,
}

// ---- Emulation ----

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetDeviceMetrics {
    pub width: u32,
    pub height: u32,
    pub device_scale_factor: u32,
    pub mobile: bool,
}
command!(SetDeviceMetrics, "Emulation.setDeviceMetricsOverride" => Nothing);

#[derive(Debug, Serialize)]
pub(crate) struct SetLocale<'a> {
    pub locale: &'a str,
}
command!(SetLocale<'_>, "Emulation.setLocaleOverride" => Nothing);

/// The language the page is told the visitor reads: the header
/// `Accept-Language` and `navigator.language`. An empty `user_agent` leaves
/// the browser's own.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetAcceptLanguage<'a> {
    pub user_agent: &'a str,
    pub accept_language: &'a str,
}
command!(SetAcceptLanguage<'_>, "Emulation.setUserAgentOverride" => Nothing);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetTimezone<'a> {
    pub timezone_id: &'a str,
}
command!(SetTimezone<'_>, "Emulation.setTimezoneOverride" => Nothing);

#[derive(Debug, Serialize)]
pub(crate) struct MediaFeature<'a> {
    pub name: &'a str,
    pub value: &'a str,
}

#[derive(Debug, Serialize)]
pub(crate) struct SetEmulatedMedia<'a> {
    pub features: Vec<MediaFeature<'a>>,
}
command!(SetEmulatedMedia<'_>, "Emulation.setEmulatedMedia" => Nothing);

// ---- Fetch and Network ----

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequestPattern<'a> {
    pub url_pattern: &'a str,
    /// Only requests of this kind, such as `Document`; every kind when
    /// left out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<&'a str>,
    pub request_stage: &'a str,
}

#[derive(Debug, Serialize)]
pub(crate) struct FetchEnable<'a> {
    pub patterns: Vec<RequestPattern<'a>>,
}
command!(FetchEnable<'_>, "Fetch.enable" => Nothing);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContinueRequest<'a> {
    pub request_id: &'a str,
}
command!(ContinueRequest<'_>, "Fetch.continueRequest" => Nothing);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FailRequest<'a> {
    pub request_id: &'a str,
    pub error_reason: &'a str,
}
command!(FailRequest<'_>, "Fetch.failRequest" => Nothing);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NetworkEnable {
    /// The most bytes of one answer the browser keeps for
    /// `Network.getResponseBody`; a longer answer is not kept at all.
    pub max_resource_buffer_size: u64,
    /// The most bytes of answers it keeps for the tab in all.
    pub max_total_buffer_size: u64,
    /// The longest body of a request an event may carry; the browser's own
    /// bound when left out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_post_data_size: Option<u64>,
}
command!(NetworkEnable, "Network.enable" => Nothing);

impl NetworkEnable {
    /// The bounds for a tab that reads at most `max_read` characters of an
    /// answer: four bytes for each, the most one character takes, so that
    /// an answer the tab could read is kept, and one far longer is not, and
    /// never comes near the most a message of the link may hold. The whole
    /// is four answers' worth.
    pub(crate) fn keeping(max_read: usize) -> Self {
        let one = u64::try_from(max_read)
            .unwrap_or(u64::MAX)
            .saturating_mul(4)
            .max(1);
        Self {
            max_resource_buffer_size: one,
            max_total_buffer_size: one.saturating_mul(4),
            max_post_data_size: None,
        }
    }

    /// The same, and no body of a request the page's script makes in any
    /// event: the browser leaves out every body longer than the bound, and
    /// reads 0 as no bound, so the bound is one byte. The body of a form a
    /// document posts is put in the event whatever the bound (both
    /// measured, decision 0254).
    pub(crate) fn without_bodies(self) -> Self {
        Self {
            max_post_data_size: Some(1),
            ..self
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct BypassServiceWorker {
    pub bypass: bool,
}
command!(BypassServiceWorker, "Network.setBypassServiceWorker" => Nothing);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GetResponseBody<'a> {
    pub request_id: &'a str,
}
command!(GetResponseBody<'_>, "Network.getResponseBody" => ResponseBody);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResponseBody {
    pub body: String,
    #[serde(default)]
    pub base64_encoded: bool,
}

// ---- WebMCP: the tools a page publishes ----

#[derive(Debug, Serialize)]
pub(crate) struct WebMcpEnable {}
command!(WebMcpEnable, "WebMCP.enable" => Nothing);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebMcpInvoke<'a> {
    pub frame_id: &'a str,
    pub tool_name: &'a str,
    pub input: &'a Value,
}
command!(WebMcpInvoke<'_>, "WebMCP.invokeTool" => WebMcpInvoked);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebMcpInvoked {
    pub invocation_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebMcpCancel<'a> {
    pub invocation_id: &'a str,
}
command!(WebMcpCancel<'_>, "WebMCP.cancelInvocation" => Nothing);

// ---- Input ----

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetFileInputFiles<'a> {
    pub object_id: &'a str,
    pub files: &'a [String],
}
command!(SetFileInputFiles<'_>, "DOM.setFileInputFiles" => Nothing);

#[derive(Debug, Serialize)]
pub(crate) struct InsertText<'a> {
    pub text: &'a str,
}
command!(InsertText<'_>, "Input.insertText" => Nothing);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DispatchKey<'a> {
    #[serde(rename = "type")]
    pub kind: &'a str,
    pub key: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows_virtual_key_code: Option<u32>,
}
command!(DispatchKey<'_>, "Input.dispatchKeyEvent" => Nothing);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DispatchMouse<'a> {
    #[serde(rename = "type")]
    pub kind: &'a str,
    /// A position in the viewport, in CSS pixels. Whole pixels are enough:
    /// the helper gives the middle of the element, rounded.
    pub x: i64,
    pub y: i64,
    pub button: &'a str,
    pub click_count: u32,
}
command!(DispatchMouse<'_>, "Input.dispatchMouseEvent" => Nothing);
