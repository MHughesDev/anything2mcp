//! Pages whose own script reads from the JSON interface as it loads, each in
//! a way a read made again from inside the page can or cannot repeat, for the
//! web pack's recording and drafting of a page's own requests (decision 0195).
//!
//! - `/orders/{order_id}/summary`: the totals, asked for with no query and no
//!   header of the page's own, as a read from inside the page would ask.
//!   With `?picture=slow` the page also shows a picture,
//!   `/pictures/slow`, that is answered only after [`SLOW_PICTURE`]: the
//!   page says `Loaded.` as soon as its totals are read, and its network
//!   stays busy for as long as a person reads it, as a page with a slow
//!   picture or a long-lived request does.
//! - `/orders/{order_id}/lengths-by-query`: the total length, asked for in
//!   centimetres with a query, `?unit=cm`.
//! - `/orders/{order_id}/lengths-by-header`: the same, asked for with a header
//!   the script sets, `X-Unit: cm`.
//! - `/api/orders/{order_id}/lengths`: answers in inches unless one of those
//!   asks for centimetres, with the same names and kinds either way, so that a
//!   read that drops the query or the header gets another variant with the
//!   same shape.
//! - `/notes/{note_id}`: one screen at two addresses, `/notes/N-1` and
//!   `/notes/N-2`; the page of `N-2` alone sends a write as it loads, to
//!   `/traps/write/notes`, which counts it.
//! - `/invitations/{token}`: a page whose address holds a long random text, and whose
//!   script sends that text in a request's path and as a bare entry of a
//!   query, to `/api/invitations/{token}/check` and `/api/feed?{token}`.
//! - `/cases/rows-by-post`: a page whose script asks for its rows with
//!   `POST /api/cases/rows/search` and the JSON body
//!   `{"status": "open", "op": "search"}`, and shows them in a table, or
//!   says `Failed.` when the request is failed. The
//!   route reads with `op: "search"` and, with `op: "archive"`, archives the
//!   first row of that status: one address that reads and writes by one
//!   method, told apart by one field of its body (decision 0394).
//!   `GET /__control/case-rows` reports the rows as stored and every body
//!   the route was sent, by its `op`.

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::AppState;
use crate::pages::escape;
use crate::store::format_length_in;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/orders/{order_id}/summary", get(summary))
        .route("/orders/{order_id}/lengths-by-query", get(by_query))
        .route("/orders/{order_id}/lengths-by-header", get(by_header))
        .route("/api/orders/{order_id}/lengths", get(lengths))
        .route("/notes/{note_id}", get(note))
        .route("/invitations/{token}", get(invitation))
        .route("/api/invitations/{token}/check", get(invitation_check))
        .route("/api/feed", get(feed))
        .route("/cases/rows-by-post", get(rows_by_post))
        .route("/api/cases/rows/search", post(rows_search))
        .route("/pictures/slow", get(slow_picture))
}

/// How long `/pictures/slow` takes to answer: longer than a test's person
/// reads a page.
pub const SLOW_PICTURE: std::time::Duration = std::time::Duration::from_secs(8);

/// The smallest picture: one transparent pixel, in GIF.
const PIXEL: &[u8] = &[
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xff, 0xff, 0xff, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
];

/// A picture answered after [`SLOW_PICTURE`].
async fn slow_picture() -> Response {
    tokio::time::sleep(SLOW_PICTURE).await;
    ([(axum::http::header::CONTENT_TYPE, "image/gif")], PIXEL).into_response()
}

fn page(title: &str, body: &str) -> Html<String> {
    Html(format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>{} - Example Orders</title>\n</head>\n<body>\n<main>\n{body}\n</main>\n</body>\n</html>\n",
        escape(title)
    ))
}

/// A page that shows one value its script reads, and says `Loaded.` once it
/// has. `call` is the script's `fetch` call, with the order's id as `id`;
/// `more` is put after it, as it is.
fn reading_page(title: &str, order_id: &str, call: &str, shown: &str, more: &str) -> Response {
    let quoted = serde_json::to_string(order_id)
        .unwrap_or_default()
        .replace('<', "\\u003c");
    page(
        title,
        &format!(
            "<h1>{}</h1>\n<p><output id=\"value\"></output></p>\n<p id=\"state\">Loading.</p>\n\
             <script>\n\
             (async () => {{\n\
               const id = encodeURIComponent({quoted});\n\
               const response = await {call};\n\
               const answer = await response.json();\n\
               document.getElementById(\"value\").textContent = String(answer.{shown});\n\
               document.getElementById(\"state\").textContent = \"Loaded.\";\n\
             }})();\n\
             </script>{more}",
            escape(title)
        ),
    )
    .into_response()
}

async fn summary(Path(order_id): Path<String>, uri: Uri) -> Response {
    let slow = uri
        .query()
        .is_some_and(|query| query.split('&').any(|entry| entry == "picture=slow"));
    reading_page(
        &format!("Summary of order {order_id}"),
        &order_id,
        "fetch(\"/api/orders/\" + id + \"/totals\")",
        "total_quantity",
        if slow {
            "\n<p><img src=\"/pictures/slow\" alt=\"Picture of the order\"></p>"
        } else {
            ""
        },
    )
}

async fn by_query(Path(order_id): Path<String>) -> Response {
    reading_page(
        &format!("Lengths of order {order_id}"),
        &order_id,
        "fetch(\"/api/orders/\" + id + \"/lengths?unit=cm\")",
        "total_length",
        "",
    )
}

async fn by_header(Path(order_id): Path<String>) -> Response {
    reading_page(
        &format!("Lengths of order {order_id}"),
        &order_id,
        "fetch(\"/api/orders/\" + id + \"/lengths\", {headers: {\"X-Unit\": \"cm\"}})",
        "total_length",
        "",
    )
}

/// The total length of an order's lines, in inches, or in whole centimetres
/// when the query or the header asks for them.
async fn lengths(
    State(state): State<AppState>,
    Path(order_id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let centimetres = uri
        .query()
        .is_some_and(|query| query.split('&').any(|entry| entry == "unit=cm"))
        || headers.get("x-unit").and_then(|value| value.to_str().ok()) == Some("cm");
    let inner = state.lock().await;
    let Some(order) = inner.store.order(&order_id) else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            axum::Json(json!({"error": "not_found"})),
        )
            .into_response();
    };
    let sixteenths: u64 = order
        .items
        .values()
        .map(|item| u64::from(item.quantity) * u64::from(item.length_sixteenths))
        .sum();
    let total = if centimetres {
        // 2.54 centimetres to the inch, sixteen sixteenths to the inch.
        (sixteenths * 254 / 1600).to_string()
    } else {
        u32::try_from(sixteenths).map_or_else(|_| "0".to_owned(), format_length_in)
    };
    axum::Json(json!({"order_id": order.order_id, "total_length": total})).into_response()
}

async fn note(Path(note_id): Path<String>) -> Response {
    let writes = if note_id == "N-2" {
        "<script>\nfetch(\"/traps/write/notes\", {method: \"POST\"});\n</script>"
    } else {
        ""
    };
    page(
        &format!("Note {note_id}"),
        &format!(
            "<h1>Note {}</h1>\n\
             <table aria-label=\"Entries\">\n\
             <tr><th>When</th><th>Text</th></tr>\n\
             <tr><td>Today</td><td>Written by the stand</td></tr>\n\
             </table>\n{writes}",
            escape(&note_id)
        ),
    )
    .into_response()
}

async fn invitation(Path(token): Path<String>) -> Response {
    let quoted = serde_json::to_string(&token)
        .unwrap_or_default()
        .replace('<', "\\u003c");
    page(
        "Accept your invitation",
        &format!(
            "<h1>Accept your invitation</h1>\n\
             <table aria-label=\"Recent requests\">\n\
             <tr><th>When</th><th>From</th></tr>\n\
             <tr><td>Today</td><td>This device</td></tr>\n\
             </table>\n<p id=\"state\">Loading.</p>\n\
             <script>\n\
             (async () => {{\n\
               const token = {quoted};\n\
               await fetch(\"/api/invitations/\" + encodeURIComponent(token) + \"/check\");\n\
               await fetch(\"/api/feed?\" + encodeURIComponent(token));\n\
               document.getElementById(\"state\").textContent = \"Loaded.\";\n\
             }})();\n\
             </script>"
        ),
    )
    .into_response()
}

async fn invitation_check(Path(_token): Path<String>) -> Response {
    axum::Json(json!({"valid": true})).into_response()
}

async fn feed() -> Response {
    axum::Json(json!({"items": 0})).into_response()
}

/// One row of the page of case 39.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaseRow {
    /// The row's key, such as `C-1`.
    pub case_id: String,
    /// What the case is about.
    pub title: String,
    /// `open`, `closed`, or `archived`.
    pub status: String,
}

/// What the control interface reports of case 39.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CaseRowsReport {
    /// The rows as stored, in key order.
    pub rows: Vec<CaseRow>,
    /// The `op` of every body `POST /api/cases/rows/search` was sent since
    /// the last reset, oldest first; `unreadable` for a body that was not
    /// the one the route takes.
    pub received: Vec<String>,
}

/// The rows of case 39 and what their route was sent.
#[derive(Debug, Clone)]
pub(crate) struct CaseRows {
    rows: Vec<CaseRow>,
    received: Vec<String>,
}

/// How many bodies the route remembers.
const RECEIVED_KEPT: usize = 10_000;

impl Default for CaseRows {
    fn default() -> Self {
        let row = |case_id: &str, title: &str, status: &str| CaseRow {
            case_id: case_id.to_owned(),
            title: title.to_owned(),
            status: status.to_owned(),
        };
        Self {
            rows: vec![
                row("C-1", "Leaking valve", "open"),
                row("C-2", "Late delivery", "open"),
                row("C-3", "Wrong colour", "closed"),
            ],
            received: Vec::new(),
        }
    }
}

impl CaseRows {
    fn heard(&mut self, op: &str) {
        if self.received.len() < RECEIVED_KEPT {
            self.received.push(op.to_owned());
        }
    }
}

pub(crate) async fn case_rows_report(State(state): State<AppState>) -> axum::Json<CaseRowsReport> {
    let inner = state.lock().await;
    axum::Json(CaseRowsReport {
        rows: inner.case_rows.rows.clone(),
        received: inner.case_rows.received.clone(),
    })
}

/// The body the route takes, and nothing more.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RowsAsked {
    status: String,
    op: String,
}

async fn rows_by_post() -> Response {
    page(
        "Open cases",
        "<h1>Open cases</h1>\n\
         <table aria-label=\"Cases\">\n<thead><tr><th>Case</th><th>Title</th><th>Status</th></tr></thead>\n\
         <tbody id=\"rows\"></tbody>\n</table>\n<p id=\"state\">Loading.</p>\n\
         <script>\n\
         (async () => {\n\
           let response;\n\
           try {\n\
             response = await fetch(\"/api/cases/rows/search\", {method: \"POST\", \
             headers: {\"Content-Type\": \"application/json\"}, \
             body: JSON.stringify({status: \"open\", op: \"search\"})});\n\
           } catch (error) {\n\
             document.getElementById(\"state\").textContent = \"Failed.\";\n\
             return;\n\
           }\n\
           const answer = await response.json();\n\
           const rows = document.getElementById(\"rows\");\n\
           for (const row of answer.rows) {\n\
             const line = document.createElement(\"tr\");\n\
             for (const value of [row.case_id, row.title, row.status]) {\n\
               const cell = document.createElement(\"td\");\n\
               cell.textContent = value;\n\
               line.appendChild(cell);\n\
             }\n\
             rows.appendChild(line);\n\
           }\n\
           document.getElementById(\"state\").textContent = \"Loaded.\";\n\
         })();\n\
         </script>",
    )
    .into_response()
}

/// `op: "search"` answers the rows of a status; `op: "archive"` archives the
/// first of them and answers it. Anything else is refused and changes
/// nothing; every body is counted.
async fn rows_search(State(state): State<AppState>, body: axum::body::Bytes) -> Response {
    let mut inner = state.lock().await;
    let Ok(asked) = serde_json::from_slice::<RowsAsked>(&body) else {
        inner.case_rows.heard("unreadable");
        return (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(json!({"error": "bad_body"})),
        )
            .into_response();
    };
    inner.case_rows.heard(&asked.op);
    let rows = &mut inner.case_rows.rows;
    match asked.op.as_str() {
        "search" => {
            let found: Vec<&CaseRow> = rows
                .iter()
                .filter(|row| row.status == asked.status)
                .collect();
            axum::Json(json!({"rows": found})).into_response()
        }
        "archive" => match rows.iter_mut().find(|row| row.status == asked.status) {
            Some(row) => {
                row.status = "archived".to_owned();
                axum::Json(json!({"rows": [row.clone()]})).into_response()
            }
            None => axum::Json(json!({"rows": []})).into_response(),
        },
        _ => (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(json!({"error": "unknown_op"})),
        )
            .into_response(),
    }
}
