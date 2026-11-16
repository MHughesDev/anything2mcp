//! The control interface: what tests use to reset the application, switch on
//! faults, and ask what it actually stored. It exists only in this test
//! application, under `/__control`, and no fault ever applies to it.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::faults::Fault;
use crate::{AppState, Inner};

/// Settings a test can change.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    /// How long the slow endpoint waits, in milliseconds.
    slow_ms: u64,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/reset", post(reset))
        .route("/orders/{order_id}", get(stored_order))
        .route("/orders/{order_id}/items/{item_code}", get(stored_item))
        .route("/writes", get(writes))
        .route("/faults", get(faults).post(add_fault).delete(clear_faults))
        .route("/settings", post(settings))
}

/// Restores the seed data and clears every fault, counter, and setting.
async fn reset(State(state): State<AppState>) -> StatusCode {
    *state.lock().await = Inner::default();
    StatusCode::NO_CONTENT
}

/// Exactly what is stored for an order, or `null`.
async fn stored_order(State(state): State<AppState>, Path(order_id): Path<String>) -> Json<Value> {
    let inner = state.lock().await;
    Json(json!({"stored": inner.store.order(&order_id)}))
}

/// Exactly what is stored for a line item, or `null`.
async fn stored_item(
    State(state): State<AppState>,
    Path((order_id, item_code)): Path<(String, String)>,
) -> Json<Value> {
    let inner = state.lock().await;
    Json(json!({"stored": inner.store.item(&order_id, &item_code)}))
}

/// Every write since the last reset: which request changed which record.
async fn writes(State(state): State<AppState>) -> Json<Value> {
    let inner = state.lock().await;
    Json(json!({"writes": inner.store.writes()}))
}

/// The faults that are switched on now.
async fn faults(State(state): State<AppState>) -> Json<Value> {
    let inner = state.lock().await;
    Json(json!({"faults": inner.faults}))
}

async fn add_fault(State(state): State<AppState>, Json(fault): Json<Fault>) -> StatusCode {
    state.lock().await.faults.push(fault);
    StatusCode::NO_CONTENT
}

async fn clear_faults(State(state): State<AppState>) -> StatusCode {
    state.lock().await.faults.clear();
    StatusCode::NO_CONTENT
}

async fn settings(State(state): State<AppState>, Json(settings): Json<Settings>) -> StatusCode {
    state.lock().await.slow_ms = settings.slow_ms;
    StatusCode::NO_CONTENT
}
