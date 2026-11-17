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
    DialogOpening,
    DialogClosed,
    TargetCrashed,
    Attached(Attached),
    Detached(Detached),
    RequestPaused(RequestPaused),
    ResponseReceived(ResponseReceived),
    BindingCalled(BindingCalled),
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
            "Page.javascriptDialogOpening" => Self::DialogOpening,
            "Page.javascriptDialogClosed" => Self::DialogClosed,
            "Inspector.targetCrashed" => Self::TargetCrashed,
            "Target.attachedToTarget" => Self::Attached(of(params)?),
            "Target.detachedFromTarget" => Self::Detached(of(params)?),
            "Fetch.requestPaused" => Self::RequestPaused(of(params)?),
            "Network.responseReceived" => Self::ResponseReceived(of(params)?),
            "Runtime.bindingCalled" => Self::BindingCalled(of(params)?),
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
    #[serde(default)]
    pub waiting_for_debugger: bool,
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
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PausedRequest {
    pub url: String,
    pub method: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResponseReceived {
    #[serde(default)]
    pub loader_id: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    pub response: ResponseDescription,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ResponseDescription {
    pub status: u16,
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

// ---- Target ----

#[derive(Debug, Serialize)]
pub(crate) struct CreateTarget<'a> {
    pub url: &'a str,
}
command!(CreateTarget<'_>, "Target.createTarget" => CreatedTarget);

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
pub(crate) struct SetAutoAttach {
    pub auto_attach: bool,
    pub wait_for_debugger_on_start: bool,
    pub flatten: bool,
}
command!(SetAutoAttach, "Target.setAutoAttach" => Nothing);

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

/// One argument of a call: a plain value, or an object the page holds.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CallArgument<'a> {
    Value(Value),
    #[allow(dead_code, reason = "an element is passed as `this`; kept for a second element")]
    ObjectId(&'a str),
}

impl CallArgument<'static> {
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
    pub arguments: Vec<CallArgument<'a>>,
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
pub(crate) struct NetworkEnable {}
command!(NetworkEnable, "Network.enable" => Nothing);

#[derive(Debug, Serialize)]
pub(crate) struct BypassServiceWorker {
    pub bypass: bool,
}
command!(BypassServiceWorker, "Network.setBypassServiceWorker" => Nothing);

// ---- Input ----

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
