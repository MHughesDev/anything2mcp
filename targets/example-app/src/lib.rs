//! The built-in test application: an imaginary order-entry system named `example`.
//!
//! Anything2MCP is never tested against a real business application. Everything is
//! proven against this one, which imitates the hard cases and has a control
//! interface that reports what it actually stored. `docs/plan/test-application.md`
//! lists the cases; this crate covers cases 1 to 4, and the parts its README names
//! of cases 10, 11, 13 to 15, 17, 23, and 29 to 34.
//!
//! It is test equipment, not part of the product, so it sits outside the crate
//! layering. A product crate may use it in tests only.

mod api;
pub mod auth;
mod connection;
mod control;
pub mod details;
mod edges;
pub mod faults;
pub mod harness;
mod openapi;
mod pages;
pub mod store;
mod traps;

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::auth::AuthState;
use crate::connection::{ConnectionHandle, CuttableListener};
use crate::details::Deliveries;
use crate::faults::Fault;
use crate::store::Store;

/// How long the slow endpoint waits unless the control interface says otherwise.
const DEFAULT_SLOW_MS: u64 = 200;

/// Everything one running application holds. Cloning shares it.
#[derive(Clone, Default)]
pub(crate) struct AppState(Arc<Mutex<Inner>>);

pub(crate) struct Inner {
    pub(crate) store: Store,
    pub(crate) faults: Vec<Fault>,
    pub(crate) slow_ms: u64,
    pub(crate) auth: AuthState,
    pub(crate) deliveries: Deliveries,
    pub(crate) trap_writes: BTreeMap<String, u64>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            store: Store::seeded(),
            faults: Vec::new(),
            slow_ms: DEFAULT_SLOW_MS,
            auth: AuthState::default(),
            deliveries: Deliveries::default(),
            trap_writes: BTreeMap::new(),
        }
    }
}

impl AppState {
    pub(crate) async fn lock(&self) -> tokio::sync::MutexGuard<'_, Inner> {
        self.0.lock().await
    }
}

/// A new application with the seed data, ready to be served.
///
/// Faults set through the control interface apply to the pages and the JSON
/// interface, never to the control interface itself.
pub fn router() -> Router {
    let state = AppState::default();
    let application = Router::new()
        .merge(api::routes())
        .merge(pages::routes())
        .merge(details::routes())
        .merge(openapi::routes())
        .merge(traps::routes())
        .merge(edges::routes())
        .merge(auth::routes(&state))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            faults::apply,
        ));
    Router::new()
        .merge(application)
        .nest("/__control", control::routes())
        .with_state(state)
}

/// Serves a new application on the given listener until the task is stopped.
///
/// Served this way, the fault "lost after commit" can cut a connection.
///
/// # Errors
///
/// Returns the error that stopped the server.
pub async fn serve(listener: TcpListener) -> std::io::Result<()> {
    axum::serve(
        CuttableListener(listener),
        router().into_make_service_with_connect_info::<ConnectionHandle>(),
    )
    .await
}
