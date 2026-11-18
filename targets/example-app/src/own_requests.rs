//! Pages whose own script reads from the JSON interface as it loads, each in
//! a way a read made again from inside the page can or cannot repeat, for the
//! web pack's recording and drafting of a page's own requests (decision 0195).
//!
//! - `/orders/{order_id}/summary`: the totals, asked for with no query and no
//!   header of the page's own, as a read from inside the page would ask.
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

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
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
}

fn page(title: &str, body: &str) -> Html<String> {
    Html(format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>{} - Example Orders</title>\n</head>\n<body>\n<main>\n{body}\n</main>\n</body>\n</html>\n",
        escape(title)
    ))
}

/// A page that shows one value its script reads, and says `Loaded.` once it
/// has. `call` is the script's `fetch` call, with the order's id as `id`.
fn reading_page(title: &str, order_id: &str, call: &str, shown: &str) -> Response {
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
             </script>",
            escape(title)
        ),
    )
    .into_response()
}

async fn summary(Path(order_id): Path<String>) -> Response {
    reading_page(
        &format!("Summary of order {order_id}"),
        &order_id,
        "fetch(\"/api/orders/\" + id + \"/totals\")",
        "total_quantity",
    )
}

async fn by_query(Path(order_id): Path<String>) -> Response {
    reading_page(
        &format!("Lengths of order {order_id}"),
        &order_id,
        "fetch(\"/api/orders/\" + id + \"/lengths?unit=cm\")",
        "total_length",
    )
}

async fn by_header(Path(order_id): Path<String>) -> Response {
    reading_page(
        &format!("Lengths of order {order_id}"),
        &order_id,
        "fetch(\"/api/orders/\" + id + \"/lengths\", {headers: {\"X-Unit\": \"cm\"}})",
        "total_length",
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
