//! The router: one handler per route face of the registry, each bound to
//! its action when the router is built. Nothing a request carries chooses
//! the action: the path is matched against fixed routes, and the input
//! only fills the action's fields.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use a2m_actions::{Caller, FrontDoor, Kernel, Request, Response, RestRouteFace, rest_routes};
use a2m_core::{Failure, FailureClass, FailureCode};
use axum::Router;
use axum::extract::{FromRequestParts, RawPathParams};
use axum::http::{Method, StatusCode, header, request::Parts};
use axum::routing::{MethodFilter, MethodRouter, on};
use serde_json::{Map, Value as Json};

use crate::codes;
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
    /// The action it dispatches; empty for the document's own route.
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
    let mut router = Router::new();
    for (path, methods) in by_path {
        router = router.route(&path, methods.fallback(not_allowed));
    }
    let doc_route = on(MethodFilter::GET, move |request: axum::extract::Request| {
        let doc = Arc::clone(&doc);
        async move {
            match caller_of(request.extensions()) {
                Ok(_) => json_answer(StatusCode::OK, doc.as_str().to_owned()),
                Err(failure) => render(&refused("", &failure, true)),
            }
        }
    })
    .fallback(not_allowed);
    Ok(router.route(OPENAPI_PATH, doc_route).fallback(not_found))
}

/// Every route the router built from this registry serves, the document's
/// own route last: what the OpenAPI document must describe.
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
    served.push(ServedRoute {
        method: "GET",
        path: OPENAPI_PATH.to_owned(),
        action: String::new(),
    });
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
    let caller = caller_of(&parts.extensions)?;
    let key = idempotency_key(parts)?;
    let mut input = if route.get {
        query_input(route, parts.uri.query().unwrap_or(""))?
    } else {
        if parts.uri.query().is_some_and(|query| !query.is_empty()) {
            return Err(input_failure(
                codes::QUERY_INVALID,
                "This route reads its input from the body, and a query was sent.",
                "Send the input as a JSON object in the body.",
            ));
        }
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
                    format!("The field `{name}` is given in the path and differently in the input."),
                    "Give the field once, in the path.",
                ));
            }
            _ => {
                input.insert(name.to_owned(), value);
            }
        }
    }
    let mut request = Request::new(&route.action, Json::Object(input), caller);
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
    let declared = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if declared.is_some_and(|length| length > settings.max_body_bytes as u64) {
        return Err(too_large());
    }
    let bytes = tokio::time::timeout(
        settings.body_timeout,
        axum::body::to_bytes(body, settings.max_body_bytes),
    )
    .await
    .map_err(|_| {
        input_failure(
            codes::BODY_TIMED_OUT,
            "The body did not arrive in time.",
            "Send the whole body at once.",
        )
    })?
    .map_err(|_| too_large())?;
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
            let entry = input
                .entry(name)
                .or_insert_with(|| Json::Array(Vec::new()));
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
    let task = tokio::spawn(async move { kernel.dispatch(request).await });
    match tokio::time::timeout(settings.request_timeout, task).await {
        Ok(Ok(response)) => response,
        Ok(Err(_)) => refused(
            action,
            &Failure::new(
                FailureClass::Transient,
                codes::ENGINE_FAULT,
                "The engine stopped without answering this request.",
                "Read the caller's recent runs with run.list before calling again.",
            ),
            false,
        ),
        Err(_) => refused(
            action,
            &Failure::new(
                FailureClass::Busy,
                codes::REQUEST_TIMED_OUT,
                "The engine did not answer in time; the action may still finish.",
                "Read the caller's recent runs with run.list before calling again.",
            ),
            false,
        ),
    }
}

async fn not_found() -> axum::response::Response {
    render(&refused(
        "",
        &input_failure(
            codes::ROUTE_NOT_FOUND,
            "No route has this path.",
            "Read /api/v1/openapi.json for the routes.",
        ),
        true,
    ))
}

async fn not_allowed(method: Method) -> axum::response::Response {
    render(&refused(
        "",
        &input_failure(
            codes::METHOD_NOT_ALLOWED,
            format!("This route does not take {}.", method.as_str()),
            "Read /api/v1/openapi.json for the method of each route.",
        ),
        true,
    ))
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
    let mut seen: Vec<(&str, &str)> = vec![("GET", OPENAPI_PATH)];
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
        assert!(fields_clash("/api/v1/runs/{run_id}", "/api/v1/runs/{id}/events"));
        assert!(!fields_clash("/api/v1/runs/{run_id}", "/api/v1/runs/{run_id}/cancel"));
        assert!(!fields_clash("/api/v1/tools", "/api/v1/runs/{id}"));
    }
}
