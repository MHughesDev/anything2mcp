//! Faults a test can switch on, so that a failure is reproduced on demand.

use std::time::Duration;

use axum::Json;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::AppState;

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
    async fn take_fault(&self, path: &str) -> Option<FaultKind> {
        let mut inner = self.lock().await;
        let index = inner.faults.iter().position(|f| {
            f.path_prefix
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
    match state.take_fault(request.uri().path()).await {
        Some(FaultKind::Delay { ms }) => {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            next.run(request).await
        }
        Some(FaultKind::Status { status }) => {
            let status = StatusCode::from_u16(status).unwrap_or(StatusCode::SERVICE_UNAVAILABLE);
            (status, Json(json!({"error": "fault", "message": "A fault was switched on for this request."})))
                .into_response()
        }
        None => next.run(request).await,
    }
}
