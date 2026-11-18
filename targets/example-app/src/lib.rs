//! The built-in test application: an imaginary order-entry system named `example`.
//!
//! Anything2MCP is never tested against a real business application. Everything is
//! proven against this one, which imitates the hard cases and has a control
//! interface that reports what it actually stored. `docs/plan/test-application.md`
//! lists the cases; this crate covers cases 1 to 5, and the parts its README names
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
mod escapes;
pub mod faults;
pub mod forgery;
pub mod harness;
mod leaving;
mod openapi;
mod own_requests;
mod pages;
pub mod store;
#[cfg(feature = "tls")]
pub mod tls;
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
    /// Case 5: the notes a write needs the page's anti-forgery token for.
    pub(crate) notes: forgery::Notes,
    /// The method and path of every request the application received, outside
    /// the control interface, oldest first, up to [`SEEN_KEPT`].
    pub(crate) seen: Vec<String>,
}

/// How many requests the application remembers having received.
const SEEN_KEPT: usize = 10_000;

impl Default for Inner {
    fn default() -> Self {
        Self {
            store: Store::seeded(),
            faults: Vec::new(),
            slow_ms: DEFAULT_SLOW_MS,
            auth: AuthState::default(),
            deliveries: Deliveries::default(),
            trap_writes: BTreeMap::new(),
            notes: forgery::seeded(),
            seen: Vec::new(),
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
        .merge(escapes::routes())
        .merge(leaving::routes())
        .merge(own_requests::routes())
        .merge(forgery::routes())
        .merge(auth::routes(&state))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            faults::apply,
        ))
        .layer(axum::middleware::from_fn_with_state(state.clone(), seen));
    Router::new()
        .merge(application)
        .nest("/__control", control::routes())
        .with_state(state)
}

/// Remembers the method and path of a request, without its query, before
/// anything else answers it.
async fn seen(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    {
        let mut inner = state.lock().await;
        if inner.seen.len() < SEEN_KEPT {
            let line = format!("{} {}", request.method(), request.uri().path());
            inner.seen.push(line);
        }
    }
    next.run(request).await
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
