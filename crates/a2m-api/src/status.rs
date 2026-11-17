//! The one table from a response to an HTTP status, and the rendering of a
//! response as an HTTP answer. The body is always the whole response, so a
//! client never needs the status to know what happened.

use a2m_actions::{ErrorBody, ExitClass, Response};
use a2m_core::{Failure, FailureClass};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::IntoResponse;

use crate::codes;

/// The status of a response: chosen from its exit class and, for a
/// failure, its class; a few codes of this crate's own name the precise
/// HTTP condition.
#[must_use]
pub fn status_of(response: &Response) -> StatusCode {
    match (response.exit, &response.error) {
        (ExitClass::Succeeded, _) => StatusCode::OK,
        (ExitClass::PersonNeeded, _) => StatusCode::ACCEPTED,
        (ExitClass::Malformed, None) => StatusCode::BAD_REQUEST,
        (ExitClass::Failed, None) => StatusCode::INTERNAL_SERVER_ERROR,
        (exit, Some(error)) => of_failure(exit, error),
    }
}

fn of_failure(exit: ExitClass, error: &ErrorBody) -> StatusCode {
    let code = error.code.as_str();
    match error.class {
        FailureClass::Policy
            if code == codes::CALLER_MISSING.as_str()
                || code == a2m_actions::codes::CALLER_NOT_VERIFIED.as_str() =>
        {
            StatusCode::UNAUTHORIZED
        }
        FailureClass::Policy => StatusCode::FORBIDDEN,
        FailureClass::Input => match code {
            _ if code == codes::BODY_TOO_LARGE.as_str() => StatusCode::PAYLOAD_TOO_LARGE,
            _ if code == codes::CONTENT_TYPE_UNSUPPORTED.as_str() => {
                StatusCode::UNSUPPORTED_MEDIA_TYPE
            }
            _ if code == codes::ROUTE_NOT_FOUND.as_str() => StatusCode::NOT_FOUND,
            _ if code == codes::METHOD_NOT_ALLOWED.as_str() => StatusCode::METHOD_NOT_ALLOWED,
            _ if code == codes::BODY_TIMED_OUT.as_str() => StatusCode::REQUEST_TIMEOUT,
            _ => StatusCode::BAD_REQUEST,
        },
        FailureClass::Stale => StatusCode::CONFLICT,
        FailureClass::Busy => StatusCode::SERVICE_UNAVAILABLE,
        _ if exit == ExitClass::Malformed => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// A failure this crate found before the dispatcher was reached, or in
/// place of its answer, as a response of the same shape as the
/// dispatcher's. `retry_safe` is the caller's to say: only the front door
/// knows whether anything was handed on.
pub(crate) fn refused(action: &str, failure: &Failure, retry_safe: bool) -> Response {
    let exit = match failure.class() {
        FailureClass::Input => ExitClass::Malformed,
        _ => ExitClass::Failed,
    };
    Response {
        ok: false,
        action: action.to_owned(),
        result: None,
        error: Some(ErrorBody {
            class: failure.class(),
            code: failure.code().to_string(),
            message: failure.message().to_owned(),
            fix: failure.next_step().to_owned(),
            retry_safe,
        }),
        warnings: Vec::new(),
        next: None,
        exit,
        decision: None,
    }
}

/// A response as an HTTP answer: its status, and its JSON as the body.
pub(crate) fn render(response: &Response) -> axum::response::Response {
    json_answer(status_of(response), response.to_json().to_string())
}

/// A JSON body with a status. Never cached: a result is the caller's.
pub(crate) fn json_answer(status: StatusCode, body: String) -> axum::response::Response {
    let mut answer = (status, body).into_response();
    let headers = answer.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    answer
}
