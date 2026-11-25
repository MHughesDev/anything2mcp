//! A watch's changes over REST (decisions 0234 and 0478): a page of them
//! from a number, and the same as a stream of server-sent events that stays
//! open while the watch lasts.
//!
//! Like a run's events, this route is no action's and records no decision:
//! `Kernel::watch_changes` shows a caller only a watch of the person it
//! acts for, which is what `watch.changes` shows it, and the watch was
//! asked for and confirmed by decisions of their own. The route is fixed,
//! takes no body, and dispatches nothing.
//!
//! One path serves both forms. A request that accepts `text/event-stream`
//! gets the stream; any other gets one page as JSON, in the shape
//! `watch.changes` returns.

use std::time::{Duration, Instant};

use a2m_actions::{Caller, ExitClass, Kernel, Response, WatchChanges, WatchId};
use a2m_core::{Failure, FailureClass};
use axum::body::{Body, Bytes};
use axum::extract::{FromRequestParts, RawPathParams};
use axum::http::{HeaderValue, StatusCode, header, request::Parts};
use serde_json::{Value as Json, json};
use tokio::sync::mpsc;

use crate::codes;
use crate::events::{KEEP_ALIVE, Lines, followed, send};
use crate::status::{refused, render};

/// The path of a watch's changes. Served by this crate, and not an action.
pub const WATCH_CHANGES_PATH: &str = "/api/v1/watches/{watch_id}/changes";

/// The most changes one page gives: `watch.changes`'s own.
const CHANGES_PAGE: u32 = 100;

/// The watch named in the path.
async fn watch_of(parts: &mut Parts) -> Result<WatchId, Failure> {
    let params = RawPathParams::from_request_parts(parts, &())
        .await
        .map_err(|_| {
            Failure::new(
                FailureClass::Input,
                codes::QUERY_INVALID,
                "The path's fields are not readable text.",
                "Write the watch's identifier in the path.",
            )
        })?;
    params
        .iter()
        .find(|(name, _)| *name == "watch_id")
        .and_then(|(_, value)| value.parse::<WatchId>().ok())
        .ok_or_else(|| {
            Failure::new(
                FailureClass::Input,
                codes::SCHEMA_INVALID,
                "That is not a watch's identifier.",
                "Use the `watch_id` that `watch.create` gave.",
            )
        })
}

/// Answers a request for a watch's changes. The caller is the one the
/// mount put on the request; the kernel shows it only a watch of the
/// person it acts for.
pub(crate) async fn serve(
    kernel: Kernel,
    caller: Result<Caller, Failure>,
    mut parts: Parts,
) -> axum::response::Response {
    let read = async {
        let caller = caller?;
        let watch = watch_of(&mut parts).await?;
        let (after, limit, stream) = followed(&parts, CHANGES_PAGE)?;
        // The first page is read before anything is answered, so that a
        // refusal is an ordinary answer and never the start of a stream.
        let limit = if stream { CHANGES_PAGE } else { limit };
        let page = kernel
            .watch_changes(&caller.principal, watch, after, limit, Duration::ZERO)
            .await?;
        Ok::<_, Failure>((caller, stream, page))
    };
    let (caller, stream, page) = match read.await {
        Ok(read) => read,
        Err(failure) => return render(&refused("", &failure, true)),
    };
    if !stream {
        return render(&Response {
            ok: true,
            action: String::new(),
            result: Some(page.to_json()),
            error: None,
            warnings: Vec::new(),
            next: None,
            exit: ExitClass::Succeeded,
            decision: None,
        });
    }
    let (lines, stream) = mpsc::channel(8);
    tokio::spawn(follow(kernel, caller, page, lines));
    let mut answer = axum::response::Response::new(Body::from_stream(Lines(stream)));
    *answer.status_mut() = StatusCode::OK;
    let headers = answer.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    answer
}

/// Where the watch stood when the stream ended.
fn standing(page: &WatchChanges) -> Json {
    json!({
        "watch_id": page.watch.to_string(),
        "state": page.state.as_str(),
        "after": page.last,
    })
}

/// Sends the watch's changes as they are recorded, each with its number as
/// its `id`, and ends with one event named `ended` once the watch has ended
/// and every change is sent. A watch lasts until its end, so the stream
/// stays open as long; a comment is sent when it has said nothing for
/// [`KEEP_ALIVE`]. It stops when the caller has gone. It only reads, so
/// stopping it leaves nothing half done.
async fn follow(kernel: Kernel, caller: Caller, first: WatchChanges, lines: mpsc::Sender<Bytes>) {
    let principal = &caller.principal;
    let mut page = first;
    let mut said = Instant::now();
    loop {
        for change in &page.changes {
            let seq = change.get("seq").and_then(Json::as_u64).unwrap_or(0);
            let line = format!("id: {seq}\ndata: {change}\n\n");
            if !send(&lines, line).await {
                return;
            }
            said = Instant::now();
        }
        if page.over() {
            let line = format!("event: ended\ndata: {}\n\n", standing(&page));
            let _ = send(&lines, line).await;
            return;
        }
        if said.elapsed() >= KEEP_ALIVE {
            if !send(&lines, ": waiting\n\n").await {
                return;
            }
            said = Instant::now();
        }
        // A full page may have more behind it: read on without a wait.
        let wait = if page.complete {
            KEEP_ALIVE.saturating_sub(said.elapsed())
        } else {
            Duration::ZERO
        };
        let next = tokio::select! {
            () = lines.closed() => return,
            next = kernel.watch_changes(principal, page.watch, page.last, CHANGES_PAGE, wait) => next,
        };
        page = match next {
            Ok(next) => next,
            Err(failure) => {
                // The stream has begun, so the failure is its last event.
                let failed = refused("", &failure, true);
                let error = failed.error.map_or(Json::Null, |error| error.to_json());
                let line = format!("event: failed\ndata: {error}\n\n");
                let _ = send(&lines, line).await;
                return;
            }
        };
    }
}

/// The route as the OpenAPI document describes it.
pub(crate) fn operation(envelope: impl Fn(Option<Json>) -> Json, page: Json) -> Json {
    json!({"get": {
        "operationId": "watch_changes_stream",
        "summary": "The changes a watch of the caller's person recorded, after a number: one page as JSON in the shape watch.changes returns, or, for a caller that accepts text/event-stream, a stream of them that stays open while the watch lasts and ends with one event named `ended`.",
        "parameters": [
            {"name": "watch_id", "in": "path", "required": true, "schema": {"type": "string"}},
            {"name": "after", "in": "query", "required": false,
             "description": "The `seq` of the last change seen. 0, the default, starts at the first.",
             "schema": {"type": "integer", "minimum": 0}},
            {"name": "limit", "in": "query", "required": false,
             "description": "The most changes in a page. A stream ignores it.",
             "schema": {"type": "integer", "minimum": 1, "maximum": CHANGES_PAGE}},
            {"name": "Last-Event-ID", "in": "header", "required": false,
             "description": "What a reconnecting stream sends: the id of the last change it saw. It stands in place of `after`.",
             "schema": {"type": "integer", "minimum": 0}},
        ],
        "responses": {
            "200": {
                "description": "A page of changes; or a stream of them, each with its `seq` as its id, ended by one event named `ended` once the watch has ended.",
                "content": {
                    "application/json": {"schema": envelope(Some(page))},
                    "text/event-stream": {"schema": {"type": "string"}},
                },
            },
            "default": {
                "description": "A failure: its class, code, message, and fix.",
                "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Failed"}}},
            },
        },
    }})
}
