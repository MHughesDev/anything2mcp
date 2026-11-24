//! Cases 23 and 34: answers that come from the edge of a site rather than the
//! application, and a redirect to another host name.
//!
//! - `/edge/elsewhere` redirects to the order list on the same machine and port
//!   under the other loopback name: `localhost` when asked as `127.0.0.1`, and
//!   `127.0.0.1` otherwise. To a client it is another host.
//! - `/edge/blocked` answers 403 with a page, as a protective edge would.
//! - `/edge/limited` answers 429 with `Retry-After: 30`.
//! - `/edge/challenge` answers 403 with a page that asks the visitor to prove
//!   it is not a program, and the header `cf-mitigated: challenge`, as a
//!   protective edge does.
//! - `/edge/maintenance` answers 503 with `Retry-After: 120` and a page that
//!   says the site is being worked on.
//! - `/edge/ok` answers 200, so a test has a page that is not blocked.
//!
//! Each tells the application's cases list (`GET /__control/cases`) that it
//! was asked, as `edge: <name>`.

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use serde_json::json;

use crate::AppState;

/// The seconds `/edge/limited` asks a client to wait.
pub(crate) const RETRY_AFTER_SECONDS: u64 = 30;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/edge/elsewhere", get(elsewhere))
        .route("/edge/blocked", get(blocked))
        .route("/edge/limited", get(limited))
        .route("/edge/challenge", get(challenge))
        .route("/edge/maintenance", get(maintenance))
        .route("/edge/ok", get(ok))
}

/// Records that an edge page was asked for.
async fn asked(state: &AppState, name: &str) {
    state.lock().await.case_events.push(format!("edge: {name}"));
}

fn edge_page(title: &str, text: &str) -> Html<String> {
    Html(format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>{title}</title>\n</head>\n<body>\n<main>\n<h1>{title}</h1>\n<p>{text}</p>\n</main>\n</body>\n</html>\n"
    ))
}

async fn challenge(State(state): State<AppState>) -> Response {
    asked(&state, "challenge").await;
    (
        StatusCode::FORBIDDEN,
        [("cf-mitigated", "challenge")],
        edge_page("Just a moment", "Prove that you are a person to go on."),
    )
        .into_response()
}

async fn maintenance(State(state): State<AppState>) -> Response {
    asked(&state, "maintenance").await;
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(header::RETRY_AFTER, "120")],
        edge_page("Back soon", "The site is being worked on."),
    )
        .into_response()
}

async fn ok(State(state): State<AppState>) -> Response {
    asked(&state, "ok").await;
    edge_page("Edge ok", "Nothing is in the way.").into_response()
}

async fn elsewhere(headers: HeaderMap) -> Response {
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let (name, port) = host.rsplit_once(':').unwrap_or((host, "80"));
    let Ok(port) = port.parse::<u16>() else {
        return (StatusCode::BAD_REQUEST, "no usable Host header").into_response();
    };
    let other = if name == "127.0.0.1" {
        "localhost"
    } else {
        "127.0.0.1"
    };
    let to = format!("http://{other}:{port}/orders");
    (StatusCode::FOUND, [(header::LOCATION, to)]).into_response()
}

async fn blocked(State(state): State<AppState>) -> Response {
    asked(&state, "blocked").await;
    (
        StatusCode::FORBIDDEN,
        [("x-example-edge", "blocked")],
        Html(
            "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
             <title>Access denied</title>\n</head>\n<body>\n<h1>Access denied</h1>\n\
             <p>The request was blocked by the site's edge.</p>\n</body>\n</html>\n",
        ),
    )
        .into_response()
}

async fn limited(State(state): State<AppState>) -> Response {
    asked(&state, "limited").await;
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, RETRY_AFTER_SECONDS.to_string())],
        axum::Json(
            json!({"error": "rate_limited", "message": "Too many requests. Try again later."}),
        ),
    )
        .into_response()
}
