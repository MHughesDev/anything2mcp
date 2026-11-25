//! Pages drawn on the server that a person reads and a capture of their use
//! drafts from (field test 11, findings 61 and 62; decisions 0999 and 0998).
//! Each imitates only the part of such a page that a drafting rule finds
//! hard.
//!
//! - `/orders/listed`: the orders as a list of links, one item to an order,
//!   each link showing the order's id; the list has no accessible name.
//! - `/orders/{order_id}/record`: one order, headed by its customer, with
//!   its id and its status below as paragraphs with no label.
//! - `/cases/rows-kept`: the orders in a table with no accessible name,
//!   whose headings carry a table library's sort words for a screen reader
//!   (`Order Order: Activate to invert sorting`), filled by its script with
//!   `POST /api/cases/rows/kept`. The script keeps its state in the
//!   browser's local storage: the first visit sends `{"draw", "start",
//!   "length"}`, and every later one adds the `order` it kept. The page
//!   says `Loaded.` once its rows are in. The route reads only.

use axum::Router;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::json;

use crate::AppState;
use crate::pages::escape;
use crate::store::OrderStatus;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/orders/listed", get(listed))
        .route("/orders/{order_id}/record", get(record))
        .route("/cases/rows-kept", get(rows_kept))
        .route("/api/cases/rows/kept", post(rows_kept_search))
}

fn page(title: &str, body: &str) -> Response {
    axum::response::Html(format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>{} - Example Orders</title>\n\
         <style>.hint{{position:absolute;width:1px;height:1px;overflow:hidden;clip:rect(0 0 0 0);white-space:nowrap}}</style>\n\
         </head>\n<body>\n<main>\n{body}\n</main>\n</body>\n</html>\n",
        escape(title)
    ))
    .into_response()
}

/// The words a status shows on a record's page.
pub fn status_words(status: OrderStatus) -> &'static str {
    match status {
        OrderStatus::Draft => "Draft",
        OrderStatus::Submitted => "Submitted",
    }
}

async fn listed(State(state): State<AppState>) -> Response {
    let inner = state.lock().await;
    let items: String = inner
        .store
        .orders()
        .map(|order| {
            format!(
                "<li><a href=\"/orders/{0}/record\">{0}</a></li>\n",
                escape(&order.order_id)
            )
        })
        .collect();
    page("Orders", &format!("<h1>Orders</h1>\n<ul>\n{items}</ul>"))
}

async fn record(State(state): State<AppState>, Path(order_id): Path<String>) -> Response {
    let inner = state.lock().await;
    let Some(order) = inner.store.order(&order_id) else {
        return (axum::http::StatusCode::NOT_FOUND, "No such order.").into_response();
    };
    page(
        &order.customer,
        &format!(
            "<h1>{}</h1>\n<p>{}</p>\n<p>{}</p>",
            escape(&order.customer),
            escape(&order.order_id),
            status_words(order.status)
        ),
    )
}

async fn rows_kept() -> Response {
    let heading = |words: &str| {
        format!("<th>{words}<span class=\"hint\"> {words}: Activate to invert sorting</span></th>")
    };
    page(
        "Kept orders",
        &format!(
            "<h1>Kept orders</h1>\n\
             <table>\n<thead><tr>{}{}</tr></thead>\n<tbody id=\"rows\"></tbody>\n</table>\n\
             <p id=\"state\">Loading.</p>\n\
             <script>\n\
             (async () => {{\n\
               const key = \"example-rows-kept\";\n\
               const kept = localStorage.getItem(key);\n\
               const asked = {{draw: 1, start: 0, length: 10}};\n\
               if (kept !== null) asked.order = kept;\n\
               const response = await fetch(\"/api/cases/rows/kept\", {{method: \"POST\", \
               headers: {{\"Content-Type\": \"application/json\"}}, body: JSON.stringify(asked)}});\n\
               const answer = await response.json();\n\
               const rows = document.getElementById(\"rows\");\n\
               for (const row of answer.rows) {{\n\
                 const line = document.createElement(\"tr\");\n\
                 for (const value of [row.order_id, row.customer]) {{\n\
                   const cell = document.createElement(\"td\");\n\
                   cell.textContent = value;\n\
                   line.appendChild(cell);\n\
                 }}\n\
                 rows.appendChild(line);\n\
               }}\n\
               localStorage.setItem(key, \"order_id\");\n\
               document.getElementById(\"state\").textContent = \"Loaded.\";\n\
             }})();\n\
             </script>",
            heading("Order"),
            heading("Customer")
        ),
    )
}

async fn rows_kept_search(State(state): State<AppState>) -> Response {
    let inner = state.lock().await;
    let rows: Vec<serde_json::Value> = inner
        .store
        .orders()
        .map(|order| json!({"order_id": order.order_id, "customer": order.customer}))
        .collect();
    axum::Json(json!({"draw": 1, "rows": rows})).into_response()
}
