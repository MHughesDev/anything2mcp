//! The JSON interface the pages use, and that HTTP bindings will call directly.

use std::time::Duration;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use serde_json::json;

use crate::AppState;
use crate::store::{
    ItemInput, OrderStatus, StoredItem, StoredOrder, WriteAction, WriteRefused, format_length_in,
};

/// A line item as the application shows it to its users.
#[derive(Debug, Serialize)]
pub(crate) struct ItemView {
    order_id: String,
    item_code: String,
    description: String,
    quantity: u32,
    length_in: String,
    version: u64,
}

impl ItemView {
    pub(crate) fn of(order_id: &str, item: &StoredItem) -> Self {
        Self {
            order_id: order_id.to_owned(),
            item_code: item.item_code.clone(),
            description: item.description.clone(),
            quantity: item.quantity,
            length_in: format_length_in(item.length_sixteenths),
            version: item.version,
        }
    }
}

#[derive(Debug, Serialize)]
struct OrderSummary {
    order_id: String,
    customer: String,
    status: OrderStatus,
    lines: usize,
}

#[derive(Debug, Serialize)]
struct OrderView {
    order_id: String,
    customer: String,
    status: OrderStatus,
    items: Vec<ItemView>,
}

impl OrderView {
    fn of(order: &StoredOrder) -> Self {
        Self {
            order_id: order.order_id.clone(),
            customer: order.customer.clone(),
            status: order.status,
            items: order
                .items
                .values()
                .map(|i| ItemView::of(&order.order_id, i))
                .collect(),
        }
    }
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/orders", get(list_orders))
        .route("/api/orders/{order_id}", get(get_order))
        .route(
            "/api/orders/{order_id}/items/{item_code}",
            get(get_item).put(put_item),
        )
        .route("/api/orders/{order_id}/totals", get(order_totals))
        .route("/api/stock/{item_code}", get(stock))
}

fn refusal(status: StatusCode, error: &str, message: &str) -> Response {
    (status, Json(json!({"error": error, "message": message}))).into_response()
}

fn not_found(what: &str) -> Response {
    refusal(
        StatusCode::NOT_FOUND,
        "not_found",
        &format!("No such {what}."),
    )
}

async fn list_orders(State(state): State<AppState>) -> Json<Vec<OrderSummary>> {
    let inner = state.lock().await;
    let orders = inner.store.orders().map(|o| OrderSummary {
        order_id: o.order_id.clone(),
        customer: o.customer.clone(),
        status: o.status,
        lines: o.items.len(),
    });
    Json(orders.collect())
}

async fn get_order(State(state): State<AppState>, Path(order_id): Path<String>) -> Response {
    let inner = state.lock().await;
    match inner.store.order(&order_id) {
        Some(order) => Json(OrderView::of(order)).into_response(),
        None => not_found("order"),
    }
}

async fn get_item(
    State(state): State<AppState>,
    Path((order_id, item_code)): Path<(String, String)>,
) -> Response {
    let inner = state.lock().await;
    match inner.store.item(&order_id, &item_code) {
        Some(item) => with_version(StatusCode::OK, &ItemView::of(&order_id, item)),
        None => not_found("line item"),
    }
}

/// The item, with its version also in the `ETag` header.
fn with_version(status: StatusCode, item: &ItemView) -> Response {
    let etag = format!("\"{}\"", item.version);
    (status, [(header::ETAG, etag)], Json(item)).into_response()
}

async fn put_item(
    State(state): State<AppState>,
    Path((order_id, item_code)): Path<(String, String)>,
    body: Result<Json<ItemInput>, JsonRejection>,
) -> Response {
    let Json(input) = match body {
        Ok(body) => body,
        Err(rejection) => {
            return refusal(
                StatusCode::BAD_REQUEST,
                "bad_request",
                &rejection.body_text(),
            );
        }
    };
    let mut inner = state.lock().await;
    match inner.store.put_item(&order_id, &item_code, &input) {
        Ok((stored, action)) => {
            let status = match action {
                WriteAction::Created => StatusCode::CREATED,
                WriteAction::Updated => StatusCode::OK,
            };
            with_version(status, &ItemView::of(&order_id, &stored))
        }
        Err(WriteRefused::NoSuchOrder) => not_found("order"),
        Err(WriteRefused::OrderNotDraft) => refusal(
            StatusCode::CONFLICT,
            "order_not_draft",
            "The order has been submitted and can no longer be changed.",
        ),
        Err(WriteRefused::Invalid(problems)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "error": "validation",
                "message": "The line item was not saved.",
                "problems": problems,
            })),
        )
            .into_response(),
        Err(WriteRefused::VersionConflict { current }) => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "version_conflict",
                "message": "The line item was changed by someone else. Reload it and try again.",
                "current_version": current,
            })),
        )
            .into_response(),
    }
}

/// Case 4: an endpoint that is slow by nature.
async fn order_totals(State(state): State<AppState>, Path(order_id): Path<String>) -> Response {
    let (slow_ms, totals) = {
        let inner = state.lock().await;
        let totals = inner.store.order(&order_id).map(|order| {
            let quantity: u64 = order.items.values().map(|i| u64::from(i.quantity)).sum();
            let sixteenths: u64 =
                order.items.values().map(|i| u64::from(i.quantity) * u64::from(i.length_sixteenths)).sum();
            json!({
                "order_id": order.order_id,
                "lines": order.items.len(),
                "total_quantity": quantity,
                "total_length_in": u32::try_from(sixteenths).map_or_else(|_| "0".to_owned(), format_length_in),
            })
        });
        (inner.slow_ms, totals)
    };
    tokio::time::sleep(Duration::from_millis(slow_ms)).await;
    match totals {
        Some(totals) => Json(totals).into_response(),
        None => not_found("order"),
    }
}

/// Case 4: an endpoint that fails some of the time. Every third call, counted
/// since the last reset, answers 503, so a test can rely on the pattern.
async fn stock(State(state): State<AppState>, Path(item_code): Path<String>) -> Response {
    let mut inner = state.lock().await;
    if inner.store.stock_call_fails() {
        return refusal(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "The stock service did not answer. Try again.",
        );
    }
    Json(json!({"item_code": item_code, "in_stock": 25})).into_response()
}
