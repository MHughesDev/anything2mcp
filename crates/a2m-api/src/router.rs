//! The router: one handler per route face of the registry, each bound to
//! its action when the router is built. Nothing a request carries chooses
//! the action: the path is matched against fixed routes, and the input
//! only fills the action's fields.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use a2m_actions::{
    Caller, FrontDoor, Kernel, ProjectFolder, Request, Response, RestRouteFace, rest_routes,
};
use a2m_core::{Failure, FailureClass, FailureCode};
use axum::Router;
use axum::extract::{FromRequestParts, RawPathParams};
use axum::http::{Method, StatusCode, header, request::Parts};
use axum::routing::{MethodFilter, MethodRouter, on};
use serde_json::{Map, Value as Json};

use crate::codes;
use crate::events::{self, RUN_EVENTS_PATH};
use crate::openapi::{self, OPENAPI_PATH};
use crate::status::{json_answer, refused, render};

/// The limits of the REST interface. The product crate sets them from its
/// settings; the defaults suit one machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiSettings {
    /// The largest body accepted, in bytes.
    pub max_body_bytes: usize,
    /// How long the body may take to arrive.
    pub body_timeout: Duration,
    /// How long the dispatcher may take to answer. Longer than the longest
    /// wait an action takes (`wait_seconds` is at most 600), so that a call
    /// that waits as asked is not cut off.
    pub request_timeout: Duration,
}

impl Default for ApiSettings {
    fn default() -> Self {
        Self {
            max_body_bytes: 1024 * 1024,
            body_timeout: Duration::from_secs(30),
            request_timeout: Duration::from_secs(630),
        }
    }
}

/// The longest `Idempotency-Key` accepted.
pub const IDEMPOTENCY_KEY_LONGEST: usize = 200;

/// One route as the router serves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedRoute {
    /// `GET` or `POST`.
    pub method: &'static str,
    /// The path, with `{field}` for a field taken from the path.
    pub path: String,
    /// The action it dispatches; empty for the two routes that dispatch
    /// none: a run's events, and the document's own.
    pub action: String,
}

/// The REST interface, built from the kernel's registry, as an Axum router
/// with no state of its own. Whoever mounts it decides the listener and
/// puts the verified [`Caller`] on each request as an extension (for
/// example with `axum::Extension(caller)` as a layer on the connection's
/// router); a request without one is refused with 401 and reaches nothing.
///
/// # Errors
///
/// `input` / `route_conflict` when two route faces cannot both be served.
pub fn router(kernel: Kernel, settings: ApiSettings) -> Result<Router, Failure> {
    let routes = rest_routes(kernel.actions());
    check_routes(&routes)?;
    let doc = Arc::new(openapi::document(&routes).to_string());
    let mut by_path: BTreeMap<String, MethodRouter> = BTreeMap::new();
    for face in routes {
        let filter = if face.method == "GET" {
            MethodFilter::GET
        } else {
            MethodFilter::POST
        };
        let route = Arc::new(Bound::of(&face));
        let kernel = kernel.clone();
        let handler = move |request: axum::extract::Request| {
            let route = Arc::clone(&route);
            let kernel = kernel.clone();
            async move { serve(&route, kernel, settings, request).await }
        };
        let methods = match by_path.remove(&face.path) {
            Some(existing) => existing.on(filter, handler),
            None => on(filter, handler),
        };
        by_path.insert(face.path, methods);
    }
    // A run's events: not an action, so not a route face. The handler
    // reads no body and dispatches nothing.
    let events_kernel = kernel.clone();
    let events = move |request: axum::extract::Request| {
        let kernel = events_kernel.clone();
        async move {
            let (parts, body) = request.into_parts();
            drain(&settings, &parts, body).await;
            let caller = caller_of(&parts.extensions);
            events::serve(kernel, caller, parts).await
        }
    };
    let methods = match by_path.remove(RUN_EVENTS_PATH) {
        Some(existing) => existing.on(MethodFilter::GET, events),
        None => on(MethodFilter::GET, events),
    };
    by_path.insert(RUN_EVENTS_PATH.to_owned(), methods);
    let mut router = Router::new();
    for (path, methods) in by_path {
        router = router.route(&path, methods.fallback(not_allowed(settings)));
    }
    let doc_route = on(MethodFilter::GET, move |request: axum::extract::Request| {
        let doc = Arc::clone(&doc);
        async move {
            let (parts, body) = request.into_parts();
            drain(&settings, &parts, body).await;
            match caller_of(&parts.extensions) {
                Ok(_) => json_answer(StatusCode::OK, doc.as_str().to_owned()),
                Err(failure) => render(&refused("", &failure, true)),
            }
        }
    })
    .fallback(not_allowed(settings));
    Ok(router
        .route(OPENAPI_PATH, doc_route)
        .fallback(not_found(settings)))
}

/// Every route the router built from this registry serves: what the
/// OpenAPI document must describe. The two routes that are no action's
/// come last: a run's events, then the document's own.
#[must_use]
pub fn served_routes(registry: &a2m_actions::ActionRegistry) -> Vec<ServedRoute> {
    let mut served: Vec<ServedRoute> = rest_routes(registry)
        .into_iter()
        .map(|face| ServedRoute {
            method: face.method,
            path: face.path,
            action: face.action,
        })
        .collect();
    for path in [RUN_EVENTS_PATH, OPENAPI_PATH] {
        served.push(ServedRoute {
            method: "GET",
            path: path.to_owned(),
            action: String::new(),
        });
    }
    served
}

/// A route face, bound: what its handler needs.
struct Bound {
    action: String,
    get: bool,
    path_fields: Vec<String>,
    /// Each input field's JSON type, for reading a query.
    types: BTreeMap<String, String>,
}

impl Bound {
    fn of(face: &RestRouteFace) -> Self {
        let types = face
            .input_schema
            .get("properties")
            .and_then(Json::as_object)
            .map(|props| {
                props
                    .iter()
                    .map(|(name, schema)| {
                        let ty = schema.get("type").and_then(Json::as_str).unwrap_or("");
                        (name.clone(), ty.to_owned())
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            action: face.action.clone(),
            get: face.method == "GET",
            path_fields: face.path_fields.clone(),
            types,
        }
    }
}

fn input_failure(code: FailureCode, message: impl Into<String>, fix: &str) -> Failure {
    Failure::new(FailureClass::Input, code, message, fix)
}

fn caller_of(extensions: &axum::http::Extensions) -> Result<Caller, Failure> {
    let mut caller = extensions.get::<Caller>().cloned().ok_or_else(|| {
        Failure::new(
            FailureClass::Policy,
            codes::CALLER_MISSING,
            "No verified caller came with the request.",
            "Call through the local channel, or with a credential the engine accepts.",
        )
    })?;
    caller.door = FrontDoor::Rest;
    Ok(caller)
}

async fn serve(
    route: &Bound,
    kernel: Kernel,
    settings: ApiSettings,
    request: axum::extract::Request,
) -> axum::response::Response {
    let (mut parts, body) = request.into_parts();
    let request = match prepare(route, &settings, &mut parts, body).await {
        Ok(request) => request,
        Err(failure) => return render(&refused(&route.action, &failure, true)),
    };
    render(&dispatch(&route.action, kernel, settings, request).await)
}

/// Everything checked before the dispatcher is reached: the caller, the
/// headers, the query or the body, and the path's fields.
async fn prepare(
    route: &Bound,
    settings: &ApiSettings,
    parts: &mut Parts,
    body: axum::body::Body,
) -> Result<Request, Failure> {
    // Whatever is refused before the body is read, the body is still
    // drained, so that the refusal reaches the caller (`drain`).
    let early = (|| {
        let caller = caller_of(&parts.extensions)?;
        let key = idempotency_key(parts)?;
        if !route.get && parts.uri.query().is_some_and(|query| !query.is_empty()) {
            return Err(input_failure(
                codes::QUERY_INVALID,
                "This route reads its input from the body, and a query was sent.",
                "Send the input as a JSON object in the body.",
            ));
        }
        Ok((caller, key))
    })();
    let (caller, key) = match early {
        Ok(checked) => checked,
        Err(failure) => {
            drain(settings, parts, body).await;
            return Err(failure);
        }
    };
    let mut input = if route.get {
        drain(settings, parts, body).await;
        query_input(route, parts.uri.query().unwrap_or(""))?
    } else {
        body_input(settings, parts, body).await?
    };
    let params = RawPathParams::from_request_parts(parts, &())
        .await
        .map_err(|_| {
            input_failure(
                codes::QUERY_INVALID,
                "The path's fields are not readable text.",
                "Write each field of the path as percent-encoded UTF-8.",
            )
        })?;
    for (name, value) in &params {
        if !route.path_fields.iter().any(|field| field == name) {
            continue;
        }
        let value = Json::String(value.to_owned());
        match input.get(name) {
            Some(given) if *given != value => {
                return Err(input_failure(
                    codes::PATH_FIELD_CONFLICT,
                    format!(
                        "The field `{name}` is given in the path and differently in the input."
                    ),
                    "Give the field once, in the path.",
                ));
            }
            _ => {
                input.insert(name.to_owned(), value);
            }
        }
    }
    // The project folder is what the mount stated beside the caller, and
    // nothing in the path, the query, or the body. The dispatcher checks
    // it before any handler sees it.
    let mut request = Request::new(&route.action, Json::Object(input), caller)
        .in_project_folder(parts.extensions.get::<ProjectFolder>());
    request.idempotency_key = key;
    Ok(request)
}

fn idempotency_key(parts: &Parts) -> Result<Option<String>, Failure> {
    let Some(value) = parts.headers.get("idempotency-key") else {
        return Ok(None);
    };
    let bad = || {
        input_failure(
            codes::IDEMPOTENCY_KEY_INVALID,
            format!(
                "The Idempotency-Key header must be 1 to {IDEMPOTENCY_KEY_LONGEST} printable characters."
            ),
            "Send a short printable key, the same for every retry of one write.",
        )
    };
    let text = value.to_str().map_err(|_| bad())?;
    if text.is_empty()
        || text.len() > IDEMPOTENCY_KEY_LONGEST
        || !text.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(bad());
    }
    Ok(Some(text.to_owned()))
}

async fn body_input(
    settings: &ApiSettings,
    parts: &Parts,
    body: axum::body::Body,
) -> Result<Map<String, Json>, Failure> {
    let too_large = || {
        input_failure(
            codes::BODY_TOO_LARGE,
            format!(
                "The body is larger than {} bytes, or ended early.",
                settings.max_body_bytes
            ),
            "Send a smaller input.",
        )
    };
    let bytes = match read_body(settings, parts, body).await {
        Read::Whole(bytes) => bytes,
        Read::TooLarge => return Err(too_large()),
        Read::TimedOut => {
            return Err(input_failure(
                codes::BODY_TIMED_OUT,
                "The body did not arrive in time.",
                "Send the whole body at once.",
            ));
        }
    };
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(Map::new());
    }
    let json_typed = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("application/json"));
    if !json_typed {
        return Err(input_failure(
            codes::CONTENT_TYPE_UNSUPPORTED,
            "A body was sent that is not marked as JSON.",
            "Send the input as JSON, with the content type application/json.",
        ));
    }
    match serde_json::from_slice::<Json>(&bytes) {
        Ok(Json::Object(map)) => Ok(map),
        Ok(_) => Err(input_failure(
            codes::SCHEMA_INVALID,
            "The body is JSON and not one object.",
            "Send the input as one JSON object, its fields by name.",
        )),
        // The parser's position, never its text: the text may quote the
        // body, and the body may hold a secret.
        Err(error) => Err(input_failure(
            codes::SCHEMA_INVALID,
            format!(
                "The body is not JSON (line {}, column {}).",
                error.line(),
                error.column()
            ),
            "Send the input as one JSON object.",
        )),
    }
}

/// A query as input: each name once, unless its field is a list; a number
/// or a flag read as one when its field is.
fn query_input(route: &Bound, query: &str) -> Result<Map<String, Json>, Failure> {
    let bad = |message: &str| {
        input_failure(
            codes::QUERY_INVALID,
            message.to_owned(),
            "Give each field once, as name=value, percent-encoded.",
        )
    };
    let mut input = Map::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let name = decode(name).ok_or_else(|| bad("The query is not well formed."))?;
        let value = decode(value).ok_or_else(|| bad("The query is not well formed."))?;
        let ty = route.types.get(&name).map(String::as_str).unwrap_or("");
        let value = match ty {
            "integer" => value
                .parse::<i64>()
                .map_or(Json::String(value), |n| Json::Number(n.into())),
            "boolean" => match value.as_str() {
                "true" | "" => Json::Bool(true),
                "false" => Json::Bool(false),
                _ => Json::String(value),
            },
            _ => Json::String(value),
        };
        if ty == "array" {
            let entry = input.entry(name).or_insert_with(|| Json::Array(Vec::new()));
            if let Json::Array(list) = entry {
                list.push(value);
            }
        } else if input.insert(name, value).is_some() {
            return Err(bad("The query names one field twice."));
        }
    }
    Ok(input)
}

/// Percent-decoding of one part of a query, `+` as a space. None when an
/// escape is broken or the result is not UTF-8.
fn decode(part: &str) -> Option<String> {
    let bytes = part.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = bytes.get(i + 1..i + 3)?;
                let text = std::str::from_utf8(hex).ok()?;
                out.push(u8::from_str_radix(text, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// The dispatch, on its own task: a request that runs out of time stops
/// waiting, and never stops the dispatch half way.
async fn dispatch(
    action: &str,
    kernel: Kernel,
    settings: ApiSettings,
    request: Request,
) -> Response {
    let fix = lost_answer_fix(
        request.idempotency_key.is_some(),
        &request.action,
        &request.input,
    );
    let task = tokio::spawn(async move { kernel.dispatch(request).await });
    match tokio::time::timeout(settings.request_timeout, task).await {
        Ok(Ok(response)) => response,
        Ok(Err(_)) => refused(
            action,
            &Failure::new(
                FailureClass::Transient,
                codes::ENGINE_FAULT,
                "The engine stopped without answering this request.",
                fix,
            ),
            false,
        ),
        Err(_) => refused(
            action,
            &Failure::new(
                FailureClass::Busy,
                codes::REQUEST_TIMED_OUT,
                "The engine did not answer in time; the action may still finish.",
                fix,
            ),
            false,
        ),
    }
}

/// What a caller whose answer was lost does next, exactly. With an
/// `Idempotency-Key`, the same request again with the same key, which
/// gives back the run the first one started. Without one, the caller's
/// runs since a minute before the request, of the tool it named, by the
/// route that lists them (`run.list`). The moment is this machine's clock,
/// shown and deciding nothing.
fn lost_answer_fix(keyed: bool, action: &str, input: &Json) -> String {
    if keyed {
        return "Send the same request again with the same Idempotency-Key header and nothing changed: it gives back the run the first one started, finished or not, and starts no other.".to_owned();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        });
    let since = (now - 60_000).max(0);
    let tool = (action == "tool.call")
        .then(|| input.get("tool").and_then(Json::as_str))
        .flatten()
        .filter(|tool| {
            !tool.is_empty()
                && tool.len() <= 128
                && tool
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        });
    let query = match tool {
        Some(tool) => format!("since={since}&tool={tool}"),
        None => format!("since={since}"),
    };
    format!(
        "Do not send it again: a second request may start a second run and write twice. Find its run first with `GET {}/runs?{query}` (`run.list`), and do what that run's `next` says. A request that carries an Idempotency-Key header may be sent again with the same key instead.",
        a2m_actions::REST_PREFIX
    )
}

fn not_found(
    settings: ApiSettings,
) -> impl Fn(axum::extract::Request) -> BoxedAnswer + Clone + Send + Sync + 'static {
    move |request: axum::extract::Request| {
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            drain(&settings, &parts, body).await;
            render(&refused(
                "",
                &input_failure(
                    codes::ROUTE_NOT_FOUND,
                    "No route has this path.",
                    "Read /api/v1/openapi.json for the routes.",
                ),
                true,
            ))
        })
    }
}

fn not_allowed(
    settings: ApiSettings,
) -> impl Fn(axum::extract::Request) -> BoxedAnswer + Clone + Send + Sync + 'static {
    move |request: axum::extract::Request| {
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            let method: Method = parts.method.clone();
            drain(&settings, &parts, body).await;
            render(&refused(
                "",
                &input_failure(
                    codes::METHOD_NOT_ALLOWED,
                    format!("This route does not take {}.", method.as_str()),
                    "Read /api/v1/openapi.json for the method of each route.",
                ),
                true,
            ))
        })
    }
}

type BoxedAnswer =
    std::pin::Pin<Box<dyn std::future::Future<Output = axum::response::Response> + Send>>;

/// How many times the body limit the door keeps reading, and discarding,
/// after it has decided to refuse a request, so that the refusal can be
/// delivered: a server that closes a connection with unread data on it
/// makes the operating system reset the connection, and the caller may see
/// the reset instead of the answer. A body declared or found longer than
/// this is not drained: the connection is closed, and the caller sees
/// either the refusal or a closed connection.
pub const DRAIN_FACTOR: usize = 4;

/// The longest the door keeps draining a refused request's body.
pub const DRAIN_TIME: Duration = Duration::from_secs(5);

/// The body as read within the limits.
enum Read {
    Whole(Vec<u8>),
    TooLarge,
    TimedOut,
}

fn declared_length(parts: &Parts) -> Option<u64> {
    parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
}

fn ceiling(settings: &ApiSettings) -> usize {
    settings.max_body_bytes.saturating_mul(DRAIN_FACTOR)
}

/// The next piece of data of a body, skipping trailers. None at the end;
/// an error when the connection failed.
async fn next_data(body: &mut axum::body::Body) -> Option<Result<axum::body::Bytes, ()>> {
    use axum::body::HttpBody;
    loop {
        let frame =
            std::future::poll_fn(|cx| std::pin::Pin::new(&mut *body).poll_frame(cx)).await?;
        match frame {
            Ok(frame) => {
                if let Ok(data) = frame.into_data() {
                    return Some(Ok(data));
                }
            }
            Err(_) => return Some(Err(())),
        }
    }
}

/// Reads and discards the rest of a body, up to the drain ceiling in all
/// and for at most [`DRAIN_TIME`]. Returns whether it reached the end.
async fn discard(settings: &ApiSettings, body: &mut axum::body::Body, mut seen: usize) -> bool {
    let limit = ceiling(settings);
    let rest = async {
        while let Some(data) = next_data(body).await {
            let Ok(data) = data else { return false };
            seen = seen.saturating_add(data.len());
            if seen > limit {
                return false;
            }
        }
        true
    };
    tokio::time::timeout(DRAIN_TIME.min(settings.body_timeout), rest)
        .await
        .unwrap_or(false)
}

/// Drains a body the door will not use, unless it is declared longer than
/// the drain ceiling.
async fn drain(settings: &ApiSettings, parts: &Parts, mut body: axum::body::Body) {
    if declared_length(parts).is_some_and(|length| length > ceiling(settings) as u64) {
        return;
    }
    discard(settings, &mut body, 0).await;
}

/// Reads a body up to the limit within the body's time. A body over the
/// limit is drained up to the ceiling before the refusal, so that the
/// refusal is delivered.
async fn read_body(settings: &ApiSettings, parts: &Parts, mut body: axum::body::Body) -> Read {
    let limit = settings.max_body_bytes;
    if let Some(length) = declared_length(parts) {
        if length > ceiling(settings) as u64 {
            return Read::TooLarge;
        }
        if length > limit as u64 {
            discard(settings, &mut body, 0).await;
            return Read::TooLarge;
        }
    }
    let mut bytes = Vec::new();
    let mut over: Option<usize> = None;
    let read = tokio::time::timeout(settings.body_timeout, async {
        while let Some(data) = next_data(&mut body).await {
            let Ok(data) = data else { return false };
            if bytes.len() + data.len() > limit {
                over = Some(bytes.len() + data.len());
                return false;
            }
            bytes.extend_from_slice(&data);
        }
        true
    })
    .await;
    match (read, over) {
        (Ok(true), _) => Read::Whole(bytes),
        // Over the limit: drained, with its own time, then refused.
        (_, Some(seen)) => {
            drop(bytes);
            discard(settings, &mut body, seen).await;
            Read::TooLarge
        }
        // A body that broke off is reported as too large or ended early.
        (Ok(false), None) => Read::TooLarge,
        (Err(_), None) => Read::TimedOut,
    }
}

/// Refuses route faces that cannot be served together, instead of letting
/// the router builder stop the process.
fn check_routes(routes: &[RestRouteFace]) -> Result<(), Failure> {
    let conflict = |message: String| {
        Failure::new(
            FailureClass::Input,
            codes::ROUTE_CONFLICT,
            message,
            "Give each action's alias a path no other route has.",
        )
    };
    let mut seen: Vec<(&str, &str)> = vec![("GET", OPENAPI_PATH), ("GET", RUN_EVENTS_PATH)];
    for route in routes {
        if !openapi::under_prefix(&route.path) || route.method != "GET" && route.method != "POST" {
            return Err(conflict(format!(
                "The route of `{}` is not a GET or POST under the prefix.",
                route.action
            )));
        }
        if route.path == OPENAPI_PATH {
            return Err(conflict(format!(
                "The route of `{}` takes the document's own path.",
                route.action
            )));
        }
        for (method, path) in &seen {
            if *method == route.method && *path == route.path {
                return Err(conflict(format!(
                    "Two routes are {} {}.",
                    route.method, route.path
                )));
            }
            if fields_clash(path, &route.path) {
                return Err(conflict(format!(
                    "The route of `{}` names a path field differently from another route at the same place.",
                    route.action
                )));
            }
        }
        seen.push((route.method, &route.path));
    }
    Ok(())
}

/// Whether two paths, equal up to some segment, both take a field there
/// under different names, which the router cannot hold.
fn fields_clash(a: &str, b: &str) -> bool {
    for (left, right) in a.split('/').zip(b.split('/')) {
        let left_field = left.starts_with('{');
        let right_field = right.starts_with('{');
        if left_field && right_field {
            if left != right {
                return true;
            }
        } else if left != right {
            return false;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lost_answer_names_the_same_request_with_its_key_or_the_listing_without_one() {
        let input = serde_json::json!({"tool": "example_item_upsert"});
        let fix = lost_answer_fix(false, "tool.call", &input);
        assert!(fix.starts_with("Do not send it again"), "{fix}");
        assert!(fix.contains("GET /api/v1/runs?since="), "{fix}");
        assert!(fix.contains("&tool=example_item_upsert`"), "{fix}");
        let fix = lost_answer_fix(true, "tool.call", &input);
        assert!(fix.starts_with("Send the same request again with the same Idempotency-Key"));
    }

    fn bound(types: &[(&str, &str)]) -> Bound {
        Bound {
            action: "run.list".to_owned(),
            get: true,
            path_fields: Vec::new(),
            types: types
                .iter()
                .map(|(name, ty)| ((*name).to_owned(), (*ty).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn a_query_is_read_by_the_fields_types_and_a_repeated_name_is_refused() {
        let route = bound(&[("limit", "integer"), ("tags", "array"), ("all", "boolean")]);
        let input = query_input(&route, "limit=5&tags=a&tags=b%20c&all&target=ex+ample").unwrap();
        assert_eq!(input["limit"], 5);
        assert_eq!(input["tags"], serde_json::json!(["a", "b c"]));
        assert_eq!(input["all"], true);
        assert_eq!(input["target"], "ex ample");
        let twice = query_input(&route, "limit=1&limit=2").unwrap_err();
        assert_eq!(twice.code(), &codes::QUERY_INVALID);
        let broken = query_input(&route, "target=%zz").unwrap_err();
        assert_eq!(broken.code(), &codes::QUERY_INVALID);
        assert!(query_input(&route, "target=%ff").is_err());
    }

    #[test]
    fn routes_that_cannot_be_served_together_are_refused_before_the_router_is_built() {
        assert!(fields_clash(
            "/api/v1/runs/{run_id}",
            "/api/v1/runs/{id}/events"
        ));
        assert!(!fields_clash(
            "/api/v1/runs/{run_id}",
            "/api/v1/runs/{run_id}/cancel"
        ));
        assert!(!fields_clash("/api/v1/tools", "/api/v1/runs/{id}"));
    }
}
