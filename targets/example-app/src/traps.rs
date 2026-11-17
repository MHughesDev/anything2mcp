//! Case 32: pages that write without anyone pressing a button, for the discoverers'
//! proofs that they never write. Each write lands on `/traps/write/{name}`, which
//! counts it; the control interface reports the counts, so a test can show zero.
//!
//! - `/traps/on-load`: the page's script sends a write as soon as it loads.
//! - `/traps/worker`: the page starts a worker, and the worker sends a write.
//! - `/traps/socket`: the page opens a WebSocket to a route that writes. The
//!   opening handshake is itself counted as the write; the route then refuses the
//!   upgrade, so no socket stays open.
//! - `/traps/links`: links that look like actions, one by its address and one by its
//!   name, each of which writes when followed with `GET`.

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{any, get};

use crate::AppState;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/traps/on-load", get(on_load))
        .route("/traps/worker", get(worker_page))
        .route("/traps/worker.js", get(worker_script))
        .route("/traps/socket", get(socket_page))
        .route("/traps/links", get(links))
        .route("/traps/write/{*name}", any(write))
}

fn page(title: &str, body: &str) -> Html<String> {
    Html(format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>{title} - Example Orders</title>\n</head>\n<body>\n<main>\n{body}\n</main>\n</body>\n</html>\n"
    ))
}

async fn on_load() -> Html<String> {
    page(
        "Recent activity",
        "<h1>Recent activity</h1>\n<p>Nothing to show.</p>\n<script>\n\
         fetch(\"/traps/write/on-load\", {method: \"POST\"});\n</script>",
    )
}

async fn worker_page() -> Html<String> {
    page(
        "Background sync",
        "<h1>Background sync</h1>\n<p>Up to date.</p>\n<script>\n\
         new Worker(\"/traps/worker.js\");\n</script>",
    )
}

async fn worker_script() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        "fetch(\"/traps/write/worker\", {method: \"POST\"});\n",
    )
        .into_response()
}

async fn socket_page() -> Html<String> {
    page(
        "Live updates",
        "<h1>Live updates</h1>\n<p>Waiting for news.</p>\n<script>\n\
         const scheme = location.protocol === \"https:\" ? \"wss://\" : \"ws://\";\n\
         new WebSocket(scheme + location.host + \"/traps/write/socket\");\n</script>",
    )
}

async fn links() -> Html<String> {
    page(
        "Order tools",
        "<h1>Order tools</h1>\n<ul>\n\
         <li><a href=\"/traps/write/orders/A-100/delete\">A-100</a></li>\n\
         <li><a href=\"/traps/write/tool-2\">Delete order A-102</a></li>\n\
         </ul>",
    )
}

async fn write(State(state): State<AppState>, Path(name): Path<String>) -> Response {
    *state.lock().await.trap_writes.entry(name).or_insert(0) += 1;
    (StatusCode::OK, page("Done", "<h1>Done</h1>")).into_response()
}
