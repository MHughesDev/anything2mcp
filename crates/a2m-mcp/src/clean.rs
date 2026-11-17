//! The redactor on the way out of the endpoint.
//!
//! A result is redacted by the dispatcher before this crate sees it. What
//! the dispatcher never sees is what the protocol layer writes itself: an
//! error that quotes a method, a tool's name, a cursor, or a value of the
//! wrong type, and the request id and progress token it echoes. So every
//! message that leaves is parsed, and every text in it passes the
//! redactor. A message with nothing to redact leaves byte for byte as it
//! was written.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, ready};

use a2m_core::Redact;
use axum::body::{Body, Bytes, HttpBody};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use serde_json::Value as Json;

use crate::http::HttpResponse;

/// Redacts every text in `value`, names of fields included. Returns
/// whether anything changed.
fn walk(redact: &dyn Redact, value: &mut Json) -> bool {
    match value {
        Json::String(text) => {
            let clean = redact.redact(text);
            let changed = clean != *text;
            *text = clean;
            changed
        }
        Json::Array(items) => items
            .iter_mut()
            .fold(false, |changed, item| walk(redact, item) | changed),
        Json::Object(map) => {
            let mut changed = false;
            let mut renamed = Vec::new();
            for (name, inner) in map.iter_mut() {
                changed |= walk(redact, inner);
                let clean = redact.redact(name);
                if clean != *name {
                    renamed.push((name.clone(), clean));
                }
            }
            for (name, clean) in renamed {
                if let Some(inner) = map.remove(&name) {
                    map.insert(clean, inner);
                    changed = true;
                }
            }
            changed
        }
        _ => false,
    }
}

/// One message with its secrets taken out, or nothing when it holds none.
/// Bytes that are not JSON are redacted as text.
fn message(redact: &dyn Redact, bytes: &[u8]) -> Option<Vec<u8>> {
    match serde_json::from_slice::<Json>(bytes) {
        Ok(mut value) => walk(redact, &mut value).then(|| value.to_string().into_bytes()),
        Err(_) => {
            let text = String::from_utf8_lossy(bytes);
            let clean = redact.redact(&text);
            (clean != text).then(|| clean.into_bytes())
        }
    }
}

/// One event of a stream with the message in its `data` redacted.
fn event(redact: &dyn Redact, bytes: Vec<u8>) -> Vec<u8> {
    let mut changed = false;
    let lines: Vec<Vec<u8>> = bytes
        .split(|byte| *byte == b'\n')
        .map(|line| {
            let Some(data) = line.strip_prefix(b"data:") else {
                return line.to_vec();
            };
            match message(redact, data.strip_prefix(b" ").unwrap_or(data)) {
                Some(clean) => {
                    changed = true;
                    [b"data: ".as_slice(), &clean].concat()
                }
                None => line.to_vec(),
            }
        })
        .collect();
    if changed { lines.join(&b'\n') } else { bytes }
}

/// A stream of events, each redacted as it passes. Dropping it drops the
/// stream it reads, which is how the service learns its caller has gone.
struct Events {
    inner: Body,
    pending: Vec<u8>,
    ended: bool,
    redact: Arc<dyn Redact>,
}

impl futures_core::Stream for Events {
    type Item = Result<Bytes, axum::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            // An event ends with an empty line.
            if let Some(end) = self.pending.windows(2).position(|pair| pair == b"\n\n") {
                let whole: Vec<u8> = self.pending.drain(..end + 2).collect();
                return Poll::Ready(Some(Ok(event(self.redact.as_ref(), whole).into())));
            }
            if self.ended {
                if self.pending.is_empty() {
                    return Poll::Ready(None);
                }
                let rest = std::mem::take(&mut self.pending);
                return Poll::Ready(Some(Ok(event(self.redact.as_ref(), rest).into())));
            }
            match ready!(Pin::new(&mut self.inner).poll_frame(cx)) {
                Some(Ok(frame)) => {
                    if let Ok(data) = frame.into_data() {
                        self.pending.extend_from_slice(&data);
                    }
                }
                Some(Err(error)) => return Poll::Ready(Some(Err(error))),
                None => self.ended = true,
            }
        }
    }
}

/// The response with every message in it redacted.
pub(crate) async fn outgoing(redact: Arc<dyn Redact>, response: HttpResponse) -> HttpResponse {
    let (mut parts, body) = response.into_parts();
    let stream = parts
        .headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/event-stream"));
    if stream {
        let events = Events {
            inner: body,
            pending: Vec::new(),
            ended: false,
            redact,
        };
        return HttpResponse::from_parts(parts, Body::from_stream(events));
    }
    // A body that is not a stream is one message, already whole.
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .unwrap_or_default();
    let body = match message(redact.as_ref(), &bytes) {
        Some(clean) if !bytes.is_empty() => {
            parts.headers.remove(CONTENT_LENGTH);
            Body::from(clean)
        }
        _ => Body::from(bytes),
    };
    HttpResponse::from_parts(parts, body)
}
