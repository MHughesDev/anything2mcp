//! Cases 23 and 34: answers that come from the edge of a site rather than the
//! application, and a redirect to another host name.
//!
//! - `/edge/elsewhere` redirects to the order list on the same machine and port
//!   under the other loopback name: `localhost` when asked as `127.0.0.1`, and
//!   `127.0.0.1` otherwise. To a client it is another host.
//! - `/edge/blocked` answers 403 with a page, as a protective edge would.
//! - `/edge/limited` answers 429 with `Retry-After: 30`.

use axum::Router;
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

async fn blocked() -> Response {
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

async fn limited() -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, RETRY_AFTER_SECONDS.to_string())],
        axum::Json(json!({"error": "rate_limited", "message": "Too many requests. Try again later."})),
    )
        .into_response()
}
