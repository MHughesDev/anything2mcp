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
//! - `GET /api/orders/{order_id}/shipment` (case 35): a shipment whose optional
//!   `carrier` object the control interface makes the application answer as
//!   an object, as `null`, as missing, or as a text. Its description lists
//!   the property as not required and does not mark it nullable.
//! - `GET /api/orders/{order_id}/handoff` (case 36): an answer with a
//!   property named `client_secret`, whose invented value is
//!   [`HANDOFF_SECRET`], beside ordinary values; it takes an optional
//!   `access_token` query value. `GET /api/orders/{order_id}/pickup` requires
//!   an `api_key` query value.
//! - `POST /views/search/orders?status=draft`: the orders of one status,
//!   as JSON. It reads only, and stores nothing; it is the search a service
//!   puts behind `POST`.

use axum::Router;
use axum::extract::{Path, RawQuery, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
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
        .route("/api/orders/{order_id}/shipment", get(shipment))
        .route("/api/orders/{order_id}/handoff", get(handoff))
        .route("/api/orders/{order_id}/pickup", get(pickup))
}

/// The invented value of the `client_secret` property of case 36. It is
/// plainly not a credential, and no part of a tool's result may hold it.
pub const HANDOFF_SECRET: &str = "example-not-a-secret-0001";

/// Case 36: an answer that holds a property named as a secret beside
/// ordinary values. An optional `access_token` query value is accepted and
/// ignored.
async fn handoff(State(state): State<AppState>, Path(order_id): Path<String>) -> Response {
    let inner = state.lock().await;
    if inner.store.order(&order_id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"error": "not_found", "message": "No such order."})),
        )
            .into_response();
    }
    axum::Json(json!({
        "order_id": order_id,
        "carrier_code": "EXF",
        "packages": 2,
        "client_secret": HANDOFF_SECRET,
    }))
    .into_response()
}

/// Case 36: a read that needs an `api_key` query value, any value.
async fn pickup(
    State(state): State<AppState>,
    Path(order_id): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let inner = state.lock().await;
    if inner.store.order(&order_id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"error": "not_found", "message": "No such order."})),
        )
            .into_response();
    }
    let keyed = query
        .as_deref()
        .is_some_and(|query| query.split('&').any(|pair| pair.starts_with("api_key=")));
    if !keyed {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"error": "no_key", "message": "An api key is required."})),
        )
            .into_response();
    }
    axum::Json(json!({"order_id": order_id, "window": "09:00-12:00"})).into_response()
}

/// How the shipment's `carrier` is answered; set through
/// `POST /__control/shipment` with `{"answer": "null"}`, and `object`
/// again at every reset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum CarrierAnswer {
    /// An object with a name and a tracking code.
    #[default]
    Object,
    /// The property is there and is `null`.
    Null,
    /// The property is left out.
    Missing,
    /// The property is a text, which is not what the description says.
    Text,
}

async fn shipment(State(state): State<AppState>, Path(order_id): Path<String>) -> Response {
    let inner = state.lock().await;
    if inner.store.order(&order_id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"error": "not_found", "message": "No such order."})),
        )
            .into_response();
    }
    let mut answer = json!({"order_id": order_id});
    match inner.carrier {
        CarrierAnswer::Object => {
            answer["carrier"] = json!({"name": "Example Freight", "tracking": "T-100"});
        }
        CarrierAnswer::Null => answer["carrier"] = json!(null),
        CarrierAnswer::Missing => {}
        CarrierAnswer::Text => answer["carrier"] = json!("Example Freight"),
    }
    axum::Json(answer).into_response()
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
