//! Faults a test can switch on, so that a failure is reproduced on demand.

use std::time::Duration;

use axum::Json;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::AppState;
use crate::connection::ConnectionHandle;

/// What a fault does to a request it applies to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FaultKind {
    /// Answer correctly, after waiting this long.
    Delay {
        /// Milliseconds to wait before answering.
        ms: u64,
    },
    /// Answer with this status and do nothing else. Nothing is stored.
    Status {
        /// The HTTP status to answer with.
        status: u16,
    },
    /// Case 17: carry the request out as usual, so a write is stored, then close
    /// the connection without sending any answer. It applies to requests that may
    /// write (any method but `GET` and `HEAD`); a read passes by without using it
    /// up. It needs the application served by [`crate::serve`] or the harness; on
    /// a router served some other way the answer is a 500 instead.
    LostAfterCommit,
    /// Case 31: answer as usual, but with one field of a JSON answer renamed,
    /// wherever it appears in the answer, as if the application had changed shape.
    ChangedShape {
        /// The field to rename.
        field: String,
        /// Its new name.
        renamed_to: String,
    },
    /// A write that is made and then answered with a redirect, as a posted form
    /// often is: carry the request out as usual, so a write is stored, then answer
    /// `303 See Other` with this `Location` in place of the application's own
    /// answer. Like "lost after commit" it applies to requests that may write, and
    /// a read passes by without using it up.
    RedirectAfterCommit {
        /// The `Location` header to answer with, sent as it is written here.
        location: String,
    },
}

impl FaultKind {
    /// Whether this kind of fault applies to a request with this method.
    fn applies_to(&self, method: &Method) -> bool {
        match self {
            Self::LostAfterCommit | Self::RedirectAfterCommit { .. } => {
                method != Method::GET && method != Method::HEAD
            }
            _ => true,
        }
    }
}

/// One fault and the requests it applies to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fault {
    /// What happens to a matching request.
    #[serde(flatten)]
    pub kind: FaultKind,
    /// Only requests whose path starts with this are affected. All are, if absent.
    #[serde(default)]
    pub path_prefix: Option<String>,
    /// How many requests it applies to before it clears itself. Until cleared, if absent.
    #[serde(default)]
    pub times: Option<u32>,
}

impl Fault {
    /// A fault that affects the next request only.
    #[must_use]
    pub fn once(kind: FaultKind) -> Self {
        Self {
            kind,
            path_prefix: None,
            times: Some(1),
        }
    }

    /// The same fault, limited to paths that start with `prefix`.
    #[must_use]
    pub fn on_paths(mut self, prefix: &str) -> Self {
        self.path_prefix = Some(prefix.to_owned());
        self
    }
}

impl AppState {
    /// The first fault that applies to this path, used up by one.
    async fn take_fault(&self, method: &Method, path: &str) -> Option<FaultKind> {
        let mut inner = self.lock().await;
        let index = inner.faults.iter().position(|f| {
            f.kind.applies_to(method)
                && f.path_prefix
                    .as_deref()
                    .is_none_or(|prefix| path.starts_with(prefix))
        })?;
        let fault = &mut inner.faults[index];
        let kind = fault.kind.clone();
        if let Some(times) = fault.times.as_mut() {
            *times = times.saturating_sub(1);
            if *times == 0 {
                inner.faults.remove(index);
            }
        }
        Some(kind)
    }
}

/// Applies the first matching fault to a request, or lets it through.
pub(crate) async fn apply(State(state): State<AppState>, request: Request, next: Next) -> Response {
    match state
        .take_fault(request.method(), request.uri().path())
        .await
    {
        Some(FaultKind::Delay { ms }) => {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            next.run(request).await
        }
        Some(FaultKind::Status { status }) => {
            let status = StatusCode::from_u16(status).unwrap_or(StatusCode::SERVICE_UNAVAILABLE);
            (status, Json(json!({"error": "fault", "message": "A fault was switched on for this request."})))
                .into_response()
        }
        Some(FaultKind::LostAfterCommit) => {
            let connection = request
                .extensions()
                .get::<ConnectInfo<ConnectionHandle>>()
                .map(|info| info.0.clone());
            let response = next.run(request).await;
            match connection {
                Some(connection) => {
                    connection.cut();
                    response
                }
                None => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": "fault_not_possible",
                        "message": "The connection cannot be cut: serve the application with example_app::serve.",
                    })),
                )
                    .into_response(),
            }
        }
        Some(FaultKind::ChangedShape { field, renamed_to }) => {
            changed_shape(next.run(request).await, &field, &renamed_to).await
        }
        Some(FaultKind::RedirectAfterCommit { location }) => {
            // The request is carried out first: what it wrote is stored.
            drop(next.run(request).await);
            match HeaderValue::from_str(&location) {
                Ok(location) => (StatusCode::SEE_OTHER, [(header::LOCATION, location)]).into_response(),
                Err(_) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": "fault_not_possible",
                        "message": "The fault's location cannot be sent as a header.",
                    })),
                )
                    .into_response(),
            }
        }
        None => next.run(request).await,
    }
}

/// The largest answer the shape fault will rewrite.
const MAX_REWRITTEN_BYTES: usize = 4 * 1024 * 1024;

/// The same answer with `field` renamed to `renamed_to` wherever it appears, if the
/// answer is JSON; any other answer is passed on unchanged.
async fn changed_shape(response: Response, field: &str, renamed_to: &str) -> Response {
    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX_REWRITTEN_BYTES).await else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "the answer could not be read",
        )
            .into_response();
    };
    let Ok(mut value) = serde_json::from_slice::<Value>(&bytes) else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    rename(&mut value, field, renamed_to);
    let rewritten = value.to_string();
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(rewritten))
}

fn rename(value: &mut Value, field: &str, renamed_to: &str) {
    match value {
        Value::Object(map) => {
            if let Some(taken) = map.remove(field) {
                map.insert(renamed_to.to_owned(), taken);
            }
            for child in map.values_mut() {
                rename(child, field, renamed_to);
            }
        }
        Value::Array(items) => {
            for child in items {
                rename(child, field, renamed_to);
            }
        }
        _ => {}
    }
}
