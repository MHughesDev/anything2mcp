//! Case 5: a write the application refuses without the anti-forgery token the
//! page was given, for the web pack's `page_http` writes (decision 0206).
//!
//! - `GET /orders/{order_id}/note`: the order's note on a page that holds a
//!   fresh anti-forgery token, the same one in three places: a meta element
//!   named `csrf-token`, a hidden input named `authenticity_token`, and a
//!   cookie its script may read, `example_xsrf`. Every load of the page gives
//!   a new token; an order that does not exist is a 404 page with none.
//! - `GET /api/orders/{order_id}/note`: the note, as JSON.
//! - `PUT /api/orders/{order_id}/note`: writes the note. It takes the token in
//!   the header `X-CSRF-Token`, or as the field `authenticity_token` of a JSON
//!   or form body, once: a token is used up by the write it lets through. A
//!   write with no token, or with one the application did not give or has
//!   used, is refused with 403 and changes nothing.
//!
//! A refused note is answered with 422, quoting the body it was sent, the
//! token too when it came in the body, as some applications do; its
//! message then quotes that token again in capitals.
//!
//! The signed-in part of the application serves the same page and interface
//! behind its session, at `/secure/orders/{order_id}/note` and
//! `/secure/api/orders/{order_id}/note`, over the same notes: once the
//! session has ended, the page is answered by the sign-in page and the
//! interface by 401.
//!
//! What was stored, how often, and how many writes were refused for their
//! token are reported under `/__control/notes`. Every token is made up and
//! looks it: `example-forgery-token-...`.

use std::collections::BTreeMap;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::AppState;
use crate::pages::escape;

/// The header that carries the token.
pub const TOKEN_HEADER: &str = "x-csrf-token";
/// The body field that carries the token.
pub const TOKEN_FIELD: &str = "authenticity_token";
/// The name of the meta element that holds the token.
pub const TOKEN_META: &str = "csrf-token";
/// The cookie that holds the token, which the page's script may read.
pub const TOKEN_COOKIE: &str = "example_xsrf";
/// What every token begins with.
pub const TOKEN_PREFIX: &str = "example-forgery-token-";
/// The longest note the application takes, in characters.
pub const LONGEST_NOTE: usize = 200;

/// How many tokens the application remembers having given.
const TOKENS_KEPT: usize = 10_000;

/// What the anti-forgery case holds.
#[derive(Debug, Clone, Default)]
pub(crate) struct Notes {
    /// Every token given, and whether a write used it.
    tokens: BTreeMap<String, bool>,
    /// How many tokens were given since the last reset.
    issued: u64,
    /// The note of each order that has one.
    notes: BTreeMap<String, String>,
    /// How many writes changed each order's note.
    writes: BTreeMap<String, u64>,
    /// How many writes were refused for their token.
    refused: u64,
}

impl Notes {
    fn seeded() -> Self {
        let mut notes = Self::default();
        notes
            .notes
            .insert("A-100".to_owned(), "Leave at the side door.".to_owned());
        notes
    }

    /// A new token, remembered as given and unused.
    fn issue(&mut self) -> String {
        self.issued += 1;
        // A different text each time, from the count alone: the stand needs
        // tokens that change, not tokens nobody could guess.
        let mixed = self.issued.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32;
        let token = format!("{TOKEN_PREFIX}{:04}-{mixed:08x}", self.issued);
        if self.tokens.len() < TOKENS_KEPT {
            self.tokens.insert(token.clone(), false);
        }
        token
    }
}

/// What the control interface reports of the anti-forgery case.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NotesReport {
    /// The note of each order that has one.
    pub notes: BTreeMap<String, String>,
    /// How many writes changed each order's note since the last reset.
    pub writes: BTreeMap<String, u64>,
    /// How many writes were refused for a missing, unknown, or used token.
    pub forgery_refused: u64,
    /// How many tokens the pages gave.
    pub tokens_issued: u64,
    /// How many of them a write used.
    pub tokens_used: u64,
}

pub(crate) fn seeded() -> Notes {
    Notes::seeded()
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/orders/{order_id}/note", get(page))
        .merge(api_routes())
}

/// The note's interface, which the signed-in part of the application
/// serves under `/secure` too, behind its session.
pub(crate) fn api_routes() -> Router<AppState> {
    Router::new().route("/api/orders/{order_id}/note", get(read).put(write))
}

/// The note's page in the signed-in part of the application, behind its
/// session: `/secure/orders/{order_id}/note`, whose script writes to
/// `/secure/api/orders/{order_id}/note`.
pub(crate) fn secure_pages() -> Router<AppState> {
    Router::new().route("/secure/orders/{order_id}/note", get(secure_page))
}

pub(crate) async fn report(State(state): State<AppState>) -> axum::Json<NotesReport> {
    let inner = state.lock().await;
    let notes = &inner.notes;
    axum::Json(NotesReport {
        notes: notes.notes.clone(),
        writes: notes.writes.clone(),
        forgery_refused: notes.refused,
        tokens_issued: notes.issued,
        tokens_used: notes.tokens.values().filter(|used| **used).count() as u64,
    })
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        axum::Json(json!({"error": "not_found"})),
    )
        .into_response()
}

async fn page(State(state): State<AppState>, Path(order_id): Path<String>) -> Response {
    note_page(&state, &order_id, "").await
}

async fn secure_page(State(state): State<AppState>, Path(order_id): Path<String>) -> Response {
    note_page(&state, &order_id, "/secure").await
}

/// The note's page, whose script writes under `prefix`.
async fn note_page(state: &AppState, order_id: &str, prefix: &str) -> Response {
    let mut inner = state.lock().await;
    let Some(order) = inner.store.order(order_id) else {
        return (
            StatusCode::NOT_FOUND,
            Html("<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>Not found - Example Orders</title></head><body><main><h1>No such order</h1></main></body></html>\n".to_owned()),
        )
            .into_response();
    };
    let order_id = order.order_id.clone();
    let note = inner
        .notes
        .notes
        .get(&order_id)
        .cloned()
        .unwrap_or_default();
    let token = inner.notes.issue();
    let quoted = serde_json::to_string(&order_id)
        .unwrap_or_default()
        .replace('<', "\\u003c");
    let body = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"{TOKEN_META}\" content=\"{token}\">\n\
         <title>Note of order {id} - Example Orders</title>\n</head>\n<body>\n<main>\n\
         <h1>Note of order {id}</h1>\n\
         <form id=\"note-form\">\n\
         <input type=\"hidden\" name=\"{TOKEN_FIELD}\" value=\"{token}\">\n\
         <label>Note <textarea name=\"note\">{note}</textarea></label>\n\
         <button type=\"submit\">Save</button>\n</form>\n\
         <p id=\"state\">Ready.</p>\n\
         <script>\n\
         document.getElementById(\"note-form\").addEventListener(\"submit\", async (event) => {{\n\
           event.preventDefault();\n\
           const token = document.querySelector('meta[name=\"{TOKEN_META}\"]').content;\n\
           const note = event.target.elements.note.value;\n\
           const response = await fetch(\"{prefix}/api/orders/\" + encodeURIComponent({quoted}) + \"/note\", {{\n\
             method: \"PUT\", headers: {{\"Content-Type\": \"application/json\", \"X-CSRF-Token\": token}},\n\
             body: JSON.stringify({{note}}),\n\
           }});\n\
           document.getElementById(\"state\").textContent = response.ok ? \"Saved.\" : \"Refused.\";\n\
         }});\n\
         </script>\n</main>\n</body>\n</html>\n",
        id = escape(&order_id),
        note = escape(&note),
    );
    let mut response = Html(body).into_response();
    if let Ok(cookie) =
        HeaderValue::from_str(&format!("{TOKEN_COOKIE}={token}; Path=/; SameSite=Strict"))
    {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    response
}

async fn read(State(state): State<AppState>, Path(order_id): Path<String>) -> Response {
    let inner = state.lock().await;
    if inner.store.order(&order_id).is_none() {
        return not_found();
    }
    let note = inner
        .notes
        .notes
        .get(&order_id)
        .cloned()
        .unwrap_or_default();
    axum::Json(json!({"order_id": order_id, "note": note})).into_response()
}

/// The fields of a body, as JSON or as a form; `None` when it is neither.
fn fields_of(headers: &HeaderMap, body: &Bytes) -> Option<BTreeMap<String, Value>> {
    let media = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match media.as_str() {
        "application/json" => match serde_json::from_slice::<Value>(body).ok()? {
            Value::Object(fields) => Some(fields.into_iter().collect()),
            _ => None,
        },
        "application/x-www-form-urlencoded" => {
            let text = std::str::from_utf8(body).ok()?;
            let mut fields = BTreeMap::new();
            for pair in text.split('&').filter(|pair| !pair.is_empty()) {
                let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
                fields.insert(decoded(name)?, Value::String(decoded(value)?));
            }
            Some(fields)
        }
        _ => None,
    }
}

/// One part of a form body, decoded; `None` when it is not text.
fn decoded(part: &str) -> Option<String> {
    let bytes = part.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'+' => out.push(b' '),
            b'%' => {
                let hex = std::str::from_utf8(bytes.get(at + 1..at + 3)?).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                at += 2;
            }
            other => out.push(other),
        }
        at += 1;
    }
    String::from_utf8(out).ok()
}

fn refused(error: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(json!({
            "error": error,
            "message": "This write needs the anti-forgery token of the page it was made from.",
        })),
    )
        .into_response()
}

async fn write(
    State(state): State<AppState>,
    Path(order_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut inner = state.lock().await;
    if inner.store.order(&order_id).is_none() {
        return not_found();
    }
    let Some(mut fields) = fields_of(&headers, &body) else {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            axum::Json(json!({"error": "unsupported_media_type"})),
        )
            .into_response();
    };
    // The token, from the header or the body, never from both.
    let in_header = headers
        .get(TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    // What it was sent, as an application that quotes a refused body
    // back does: a page's token sent in the body comes back in the answer.
    let received = Value::Object(fields.clone().into_iter().collect());
    let in_body = fields.remove(TOKEN_FIELD);
    let from_body = in_body.is_some();
    let token = match (in_header, in_body) {
        (Some(token), None) => Some(token),
        (None, Some(Value::String(token))) => Some(token),
        _ => None,
    };
    let Some(token) = token else {
        inner.notes.refused += 1;
        return refused("forgery_token_missing");
    };
    match inner.notes.tokens.get_mut(&token) {
        Some(used) if !*used => *used = true,
        _ => {
            inner.notes.refused += 1;
            return refused("forgery_token_invalid");
        }
    }
    // The token is used up whatever the note is: a page gives one write.
    let note = match fields.get("note") {
        Some(Value::String(note)) => note.clone(),
        _ => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                axum::Json(json!({"error": "invalid", "field": "note", "message": "A note is text.", "received": received})),
            )
                .into_response();
        }
    };
    let fits = !note.trim().is_empty()
        && note.chars().count() <= LONGEST_NOTE
        && !note.chars().any(char::is_control)
        && fields.len() == 1;
    if !fits {
        // A token that came in the body is quoted in the message too, in
        // capitals, as an application that echoes what it refused in a
        // spelling of its own does (decision 0245, audit finding S4).
        let mut message =
            "A note is 1 to 200 characters of text on one line, and the body holds nothing else."
                .to_owned();
        if from_body {
            message.push_str(&format!(" The page's token was {}.", token.to_uppercase()));
        }
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            axum::Json(json!({
                "error": "invalid",
                "field": "note",
                "message": message,
                "received": received,
            })),
        )
            .into_response();
    }
    inner.notes.notes.insert(order_id.clone(), note.clone());
    *inner.notes.writes.entry(order_id.clone()).or_insert(0) += 1;
    axum::Json(json!({"order_id": order_id, "note": note})).into_response()
}
