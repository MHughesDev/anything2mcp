//! The control interface: what tests use to reset the application, switch on
//! faults, and ask what it actually stored. It exists only in this test
//! application, under `/__control`, and no fault ever applies to it.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::{AuthSettings, Credential};
use crate::faults::Fault;
use crate::{AppState, Inner};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Revoke {
    credential: Credential,
}

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
        .route("/shipment", post(set_carrier))
        .route("/orders/{order_id}/delivery", get(stored_delivery))
        .route("/auth", get(auth_report))
        .route("/auth/settings", post(auth_settings))
        .route("/auth/sessions/expire", post(expire_sessions))
        .route("/auth/sessions/revoke", post(revoke_sessions))
        .route("/auth/revoke", post(revoke))
        .route("/traps", get(trap_writes))
        .route("/seen", get(seen))
        .route("/notes", get(crate::forgery::report))
}

/// Exactly what is stored for an order's delivery details, or `null`, and how many
/// times they were written since the last reset.
async fn stored_delivery(
    State(state): State<AppState>,
    Path(order_id): Path<String>,
) -> Json<Value> {
    let inner = state.lock().await;
    Json(json!({
        "stored": inner.deliveries.get(&order_id),
        "writes": inner.deliveries.write_count(&order_id),
    }))
}

/// What the application counted about signing in.
async fn auth_report(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.lock().await.auth.report()))
}

async fn auth_settings(
    State(state): State<AppState>,
    Json(settings): Json<AuthSettings>,
) -> StatusCode {
    state.lock().await.auth.set_settings(settings);
    StatusCode::NO_CONTENT
}

/// Every usable session ends as if it had timed out.
async fn expire_sessions(State(state): State<AppState>) -> StatusCode {
    state.lock().await.auth.expire_sessions();
    StatusCode::NO_CONTENT
}

/// Every usable session ends as if an administrator had ended it.
async fn revoke_sessions(State(state): State<AppState>) -> StatusCode {
    state.lock().await.auth.revoke_sessions();
    StatusCode::NO_CONTENT
}

/// The bearer token or the API key stops working until the next reset.
async fn revoke(State(state): State<AppState>, Json(revoke): Json<Revoke>) -> StatusCode {
    state.lock().await.auth.revoke(revoke.credential);
    StatusCode::NO_CONTENT
}

/// How many times each trap page's write arrived, by name.
async fn trap_writes(State(state): State<AppState>) -> Json<Value> {
    Json(json!({"writes": state.lock().await.trap_writes}))
}

/// The method and path of every request the application received since the
/// last reset, outside this interface, oldest first.
async fn seen(State(state): State<AppState>) -> Json<Value> {
    Json(json!({"seen": state.lock().await.seen}))
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

/// Case 35: how the shipment's optional `carrier` is answered from now on,
/// until the next reset.
async fn set_carrier(
    State(state): State<AppState>,
    Json(answer): Json<crate::answers::CarrierAnswer>,
) -> StatusCode {
    state.lock().await.carrier = answer;
    StatusCode::NO_CONTENT
}
