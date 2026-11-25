//! The built-in test application: an imaginary order-entry system named `example`.
//!
//! Anything2MCP is never tested against a real business application. Everything is
//! proven against this one, which imitates the hard cases and has a control
//! interface that reports what it actually stored. `docs/plan/test-application.md`
//! lists the cases; this crate covers cases 1 to 5, and the parts its README names
//! of cases 10, 11, 13 to 15, 17, 23, and 29 to 34, and the pages of cases 6 to 12 and
//! 22 to 27 under `/cases`, with the pages of the input commits of platform
//! profiles under `/cases/commit`.
//!
//! It is test equipment, not part of the product, so it sits outside the crate
//! layering. A product crate may use it in tests only.

pub mod answers;
mod api;
pub mod auth;
pub mod cases;
mod commits;
mod connection;
mod control;
pub mod details;
mod edges;
mod escapes;
pub mod faults;
pub mod forgery;
pub mod harness;
mod identity;
mod later;
pub use later::EXPORT_MARKER;
mod leaving;
mod openapi;
pub mod own_requests;
mod pages;
pub mod person;
pub mod releases;
pub mod store;
#[cfg(feature = "tls")]
pub mod tls;
mod traps;
pub mod views;

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
    /// Case 35: how the shipment's optional `carrier` is answered.
    pub(crate) carrier: answers::CarrierAnswer,
    /// Case 38: whether the served description is its revision.
    pub(crate) description_revised: bool,
    /// Cases 6 to 9, 11, and 12: what the pages told the application.
    pub(crate) case_events: Vec<String>,
    /// Case 24: when the export was started.
    pub(crate) export_started: Option<std::time::Instant>,
    /// Package 9.1: how the form of a line item is changed, as a release
    /// would change it.
    pub(crate) line_item_change: releases::ScreenChange,
    /// Case 39: the rows a page asks for with `POST`, and what their route
    /// was sent.
    pub(crate) case_rows: own_requests::CaseRows,
    /// The walk of a person a test set, which every page then carries
    /// ([`person`]).
    pub(crate) walk: Option<Vec<person::WalkStep>>,
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
            carrier: answers::CarrierAnswer::default(),
            description_revised: false,
            case_events: Vec::new(),
            export_started: None,
            line_item_change: releases::ScreenChange::default(),
            case_rows: own_requests::CaseRows::default(),
            walk: None,
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
    router_over(AppState::default())
}

/// The application over a state that another server, the identity
/// provider's, shares.
pub(crate) fn router_over(state: AppState) -> Router {
    let application = Router::new()
        .merge(api::routes())
        .merge(pages::routes())
        .merge(details::routes())
        .merge(openapi::routes())
        .merge(traps::routes())
        .merge(edges::routes())
        .merge(escapes::routes())
        .merge(leaving::routes())
        .merge(cases::routes())
        .merge(commits::routes())
        .merge(later::routes())
        .merge(own_requests::routes())
        .merge(forgery::routes())
        .merge(views::routes())
        .merge(answers::routes())
        .merge(auth::routes(&state))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            person::walks,
        ))
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

/// Serves the identity provider, over the application's state, until the
/// task is stopped.
pub(crate) async fn serve_provider(listener: TcpListener, state: AppState) -> std::io::Result<()> {
    axum::serve(listener, identity::provider_routes().with_state(state)).await
}

/// Serves a new application on the given listener until the task is stopped.
///
/// Served this way, the fault "lost after commit" can cut a connection.
///
/// # Errors
///
/// Returns the error that stopped the server.
pub async fn serve(listener: TcpListener) -> std::io::Result<()> {
    serve_over(listener, AppState::default()).await
}

/// Serves the application over a state that the identity provider shares.
pub(crate) async fn serve_over(listener: TcpListener, state: AppState) -> std::io::Result<()> {
    axum::serve(
        CuttableListener(listener),
        router_over(state).into_make_service_with_connect_info::<ConnectionHandle>(),
    )
    .await
}
