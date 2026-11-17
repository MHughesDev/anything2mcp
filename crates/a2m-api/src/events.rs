//! A run's events over REST: a page of them from a sequence number, and
//! the same as a stream of server-sent events that ends when the run
//! settles.
//!
//! This is the one route beside the document's own that no action
//! generates. Reading a run's events is not an action and records no
//! decision: `Kernel::events` shows a caller only a run that the same
//! caller started, which the decision about the call that started the run
//! already covered (decision 0151). The route is fixed, takes no body, and
//! nothing it is sent chooses what is dispatched, because nothing is.
//!
//! One path serves both forms. A request that accepts `text/event-stream`
//! gets the stream; any other gets one page as JSON, in the shape of every
//! answer of this interface.

use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use a2m_actions::{Caller, EVENTS_PAGE, ExitClass, Kernel, Response, RunEventView, RunEvents};
use a2m_core::{Failure, FailureClass, RunId};
use axum::body::{Body, Bytes};
use axum::extract::{FromRequestParts, RawPathParams};
use axum::http::{HeaderValue, StatusCode, header, request::Parts};
use serde_json::{Value as Json, json};
use tokio::sync::mpsc;

use crate::codes;
use crate::status::{refused, render};

/// The path of a run's events. Served by this crate, and not an action.
pub const RUN_EVENTS_PATH: &str = "/api/v1/runs/{run_id}/events";

/// The header a reconnecting client names the last event it saw with.
const LAST_EVENT_ID: &str = "last-event-id";

/// How long a stream stays silent before a comment line is sent, so that
/// whatever carries the connection does not take it for dead.
pub const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// What a request for a run's events asks for.
struct Asked {
    run: RunId,
    after: u64,
    limit: u32,
    stream: bool,
}

fn invalid(message: impl Into<String>, fix: &str) -> Failure {
    Failure::new(FailureClass::Input, codes::QUERY_INVALID, message, fix)
}

/// Reads the request: the run from the path, `after` and `limit` from the
/// query, and the last event a reconnecting client saw from its header,
/// which then stands in place of `after`.
async fn asked(parts: &mut Parts) -> Result<Asked, Failure> {
    let params = RawPathParams::from_request_parts(parts, &())
        .await
        .map_err(|_| {
            invalid(
                "The path's fields are not readable text.",
                "Write the run's identifier in the path.",
            )
        })?;
    let run = params
        .iter()
        .find(|(name, _)| *name == "run_id")
        .and_then(|(_, value)| value.parse::<RunId>().ok())
        .ok_or_else(|| {
            Failure::new(
                FailureClass::Input,
                codes::SCHEMA_INVALID,
                "That is not a run's identifier.",
                "Use the `run_id` an earlier result gave.",
            )
        })?;
    let mut after = 0;
    let mut limit = EVENTS_PAGE;
    let query = parts.uri.query().unwrap_or("");
    let mut seen = Vec::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        if seen.contains(&name) {
            return Err(invalid(
                "The query names one field twice.",
                "Give `after` and `limit` once each.",
            ));
        }
        seen.push(name);
        let number = || {
            value.parse::<u64>().map_err(|_| {
                invalid(
                    format!("`{name}` is not a whole number."),
                    "Give `after` as the number of the last event seen, and `limit` as a count.",
                )
            })
        };
        match name {
            "after" => after = number()?,
            "limit" => {
                limit = u32::try_from(number()?)
                    .ok()
                    .filter(|count| (1..=EVENTS_PAGE).contains(count))
                    .ok_or_else(|| {
                        invalid(
                            format!("`limit` is not between 1 and {EVENTS_PAGE}."),
                            "Ask for fewer events, and for the next page after the last one.",
                        )
                    })?;
            }
            _ => {
                return Err(invalid(
                    "This route takes `after` and `limit`, and the query names another field.",
                    "Give `after`, `limit`, or neither.",
                ));
            }
        }
    }
    if let Some(value) = parts.headers.get(LAST_EVENT_ID) {
        after = value
            .to_str()
            .ok()
            .and_then(|text| text.trim().parse::<u64>().ok())
            .ok_or_else(|| {
                invalid(
                    "The Last-Event-ID header is not an event's number.",
                    "Send the `id` of the last event the stream gave.",
                )
            })?;
    }
    let stream = parts
        .headers
        .get_all(header::ACCEPT)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|part| {
            part.split(';')
                .next()
                .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("text/event-stream"))
        });
    Ok(Asked {
        run,
        after,
        limit,
        stream,
    })
}

fn event_json(event: &RunEventView) -> Json {
    json!({
        "seq": event.seq,
        "lease": event.lease,
        "at": event.at.as_unix_millis(),
        "kind": event.kind,
        "detail": event.detail,
    })
}

/// Where the run stood when a page was read.
fn standing(page: &RunEvents) -> Json {
    json!({
        "run_id": page.run.to_string(),
        "last": page.last,
        "status": page.state.as_str(),
        "working": page.working,
        "ended": page.ended,
        "settled": page.settled(),
    })
}

/// Answers a request for a run's events. The caller is the one the mount
/// put on the request; the kernel shows it only a run it started.
pub(crate) async fn serve(
    kernel: Kernel,
    caller: Result<Caller, Failure>,
    mut parts: Parts,
) -> axum::response::Response {
    let read = async {
        let caller = caller?;
        let asked = asked(&mut parts).await?;
        // The first page is read before anything is answered, so that a
        // refusal is an ordinary answer and never the start of a stream.
        let limit = if asked.stream {
            EVENTS_PAGE
        } else {
            asked.limit
        };
        let page = kernel
            .events(&caller.principal, asked.run, asked.after, limit)
            .await?;
        Ok::<_, Failure>((caller, asked, page))
    };
    let (caller, asked, page) = match read.await {
        Ok(read) => read,
        Err(failure) => return render(&refused("", &failure, true)),
    };
    if !asked.stream {
        let mut result = standing(&page);
        result["events"] = page.events.iter().map(event_json).collect();
        return render(&Response {
            ok: true,
            action: String::new(),
            result: Some(result),
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

/// Sends the run's events as they come, each with its sequence number as
/// its `id`, and ends with a `settled` event once the run has ended or
/// rests with every event sent. It stops when the caller has gone. It only
/// reads, so stopping it leaves nothing half done.
async fn follow(kernel: Kernel, caller: Caller, first: RunEvents, lines: mpsc::Sender<Bytes>) {
    let mut page = first;
    loop {
        for event in &page.events {
            let line = format!("id: {}\ndata: {}\n\n", event.seq, event_json(event));
            if lines.send(Bytes::from(line)).await.is_err() {
                return;
            }
        }
        // A full page may have more behind it, whatever the run's state.
        let whole = page.events.len() < EVENTS_PAGE as usize;
        if whole && page.settled() {
            let line = format!("event: settled\ndata: {}\n\n", standing(&page));
            let _ = lines.send(Bytes::from(line)).await;
            return;
        }
        if page.events.is_empty() && lines.send(Bytes::from(": waiting\n\n")).await.is_err() {
            return;
        }
        let principal = &caller.principal;
        let next = if whole {
            tokio::select! {
                () = lines.closed() => return,
                next = kernel.next_events(principal, page.run, page.last, EVENTS_PAGE, KEEP_ALIVE) => next,
            }
        } else {
            // More is stored already: read on without a wait.
            kernel
                .events(principal, page.run, page.last, EVENTS_PAGE)
                .await
        };
        page = match next {
            Ok(next) => next,
            Err(failure) => {
                // The stream has begun, so the failure is its last event.
                let failed = refused("", &failure, true);
                let error = failed.error.map_or(Json::Null, |error| error.to_json());
                let line = format!("event: failed\ndata: {error}\n\n");
                let _ = lines.send(Bytes::from(line)).await;
                return;
            }
        };
    }
}

/// The lines of a stream as a body.
struct Lines(mpsc::Receiver<Bytes>);

impl futures_core::Stream for Lines {
    type Item = Result<Bytes, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.poll_recv(cx).map(|line| line.map(Ok))
    }
}

/// The route as the OpenAPI document describes it.
pub(crate) fn operation(envelope: impl Fn(Option<Json>) -> Json) -> Json {
    let event = json!({
        "type": "object",
        "required": ["seq", "lease", "at", "kind", "detail"],
        "properties": {
            "seq": {"type": "integer", "description": "The event's number within the run, from 1, with no gap."},
            "lease": {"type": "integer"},
            "at": {"type": "integer", "description": "When it was recorded, in milliseconds since 1970."},
            "kind": {"type": "string"},
            "detail": {"description": "Small JSON about the event, redacted."},
        },
    });
    let page = json!({
        "type": "object",
        "required": ["run_id", "events", "last", "status", "working", "ended", "settled"],
        "properties": {
            "run_id": {"type": "string"},
            "events": {"type": "array", "items": event},
            "last": {"type": "integer", "description": "The number to ask after next."},
            "status": {"type": "string"},
            "working": {"type": "boolean"},
            "ended": {"type": "boolean"},
            "settled": {"type": "boolean", "description": "Nothing more comes until someone acts."},
        },
    });
    json!({"get": {
        "operationId": "run_events",
        "summary": "The events of a run the caller started, after a sequence number: one page as JSON, or, for a caller that accepts text/event-stream, a stream of them that ends when the run settles.",
        "parameters": [
            {"name": "run_id", "in": "path", "required": true, "schema": {"type": "string"}},
            {"name": "after", "in": "query", "required": false,
             "description": "The number of the last event seen. 0, the default, starts at the first.",
             "schema": {"type": "integer", "minimum": 0}},
            {"name": "limit", "in": "query", "required": false,
             "description": "The most events in a page. A stream ignores it.",
             "schema": {"type": "integer", "minimum": 1, "maximum": EVENTS_PAGE}},
            {"name": "Last-Event-ID", "in": "header", "required": false,
             "description": "What a reconnecting stream sends: the id of the last event it saw. It stands in place of `after`.",
             "schema": {"type": "integer", "minimum": 0}},
        ],
        "responses": {
            "200": {
                "description": "A page of events; or a stream of them, each with its sequence number as its id, ended by one event named `settled`.",
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
