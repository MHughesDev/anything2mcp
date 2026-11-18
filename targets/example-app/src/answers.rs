//! Answers a reader of HTTP answers must refuse, and a search behind a
//! method HTTP does not mark safe (decision 0249).
//!
//! - `GET /views/hostile/namespace`: an XML document whose default
//!   namespace has an address of 4 MB, over 99,999 empty elements; under
//!   the default ceiling of an answer, and once hundreds of gigabytes of
//!   copying for a reader that copied the address for every element.
//! - `GET /views/hostile/control`: an order as XML whose customer holds an
//!   escape character, written as a character reference, and a bell.
//! - `GET /views/hostile/control-text`: a status line as plain text with
//!   the same characters in it.
//! - `GET /views/hostile/secret`: a binary label that carries
//!   [`ECHOED_SECRET`], as a system that echoes a credential would.
//! - `POST /views/search/orders?status=draft`: the orders of one status,
//!   as JSON. It reads only, and stores nothing; it is the search a service
//!   puts behind `POST`.

use axum::Router;
use axum::extract::{Path, RawQuery, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::json;

use crate::AppState;
use crate::store::OrderStatus;
use crate::views::ORDERS_NAMESPACE;

/// The made-up credential the label of `/views/hostile/secret` carries.
pub const ECHOED_SECRET: &str = "secret-for-tests-echoed-0249";

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/views/hostile/{name}", get(hostile))
        .route("/views/search/orders", post(search))
}

/// The audit's document: a namespace address of 4 MB, then 99,999 empty
/// elements in it.
#[must_use]
pub fn long_namespace_document() -> String {
    format!(
        "<a xmlns=\"urn:{}\">{}</a>",
        "x".repeat(4_000_000),
        "<b/>".repeat(99_999)
    )
}

async fn hostile(Path(name): Path<String>) -> Response {
    let xml = "application/xml; charset=utf-8";
    match name.as_str() {
        "namespace" => ([(header::CONTENT_TYPE, xml)], long_namespace_document()).into_response(),
        "control" => (
            [(header::CONTENT_TYPE, xml)],
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<order xmlns=\"{ORDERS_NAMESPACE}\" id=\"A-100\" status=\"draft\"><customer>Example &#x1B;[31mCustomer\u{7}</customer><items/></order>\n"
            ),
        )
            .into_response(),
        "control-text" => (
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "example order entry: 3 orders, 2 draft, 1 submitted; newest A-102 for Example \u{1b}[31mCustomer\n",
        )
            .into_response(),
        "secret" => {
            let mut bytes = b"EXL1\x00".to_vec();
            bytes.extend_from_slice(ECHOED_SECRET.as_bytes());
            bytes.push(0xFF);
            ([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response()
        }
        _ => (StatusCode::NOT_FOUND, "No such answer.\n").into_response(),
    }
}

/// The orders of one status. A query that is not `status=` and a status
/// the store knows is refused with 422.
async fn search(State(state): State<AppState>, RawQuery(query): RawQuery) -> Response {
    let status = match query.as_deref() {
        Some("status=draft") => OrderStatus::Draft,
        Some("status=submitted") => OrderStatus::Submitted,
        _ => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                axum::Json(json!({"error": "validation"})),
            )
                .into_response();
        }
    };
    let inner = state.lock().await;
    let orders: Vec<&str> = inner
        .store
        .orders()
        .filter(|order| order.status == status)
        .map(|order| order.order_id.as_str())
        .collect();
    axum::Json(json!({"orders": orders})).into_response()
}
