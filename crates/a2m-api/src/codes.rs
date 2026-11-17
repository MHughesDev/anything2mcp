//! The failure codes this crate adds to the dispatcher's. Each is refused
//! before the dispatcher is reached, except the two about time, which are
//! said in place of an answer that did not come.

use a2m_core::FailureCode;

/// No caller came with the request: whoever mounts the router did not put
/// one on it. Class `policy`; status 401.
pub const CALLER_MISSING: FailureCode = FailureCode::fixed("caller_missing");
/// The body is not JSON, or not one JSON object. Class `input`.
pub const SCHEMA_INVALID: FailureCode = FailureCode::SCHEMA_INVALID;
/// The body is larger than the limit, or ended early. Class `input`; 413.
pub const BODY_TOO_LARGE: FailureCode = FailureCode::fixed("body_too_large");
/// A body was sent with a type other than JSON. Class `input`; 415.
pub const CONTENT_TYPE_UNSUPPORTED: FailureCode = FailureCode::fixed("content_type_unsupported");
/// The query of a request is not well formed, names a field twice, or was
/// sent on a route that reads its input from the body. Class `input`.
pub const QUERY_INVALID: FailureCode = FailureCode::fixed("query_invalid");
/// A field given in the path was given again, differently, in the body.
/// Class `input`.
pub const PATH_FIELD_CONFLICT: FailureCode = FailureCode::fixed("path_field_conflict");
/// The `Idempotency-Key` header is empty, too long, or not printable text.
/// Class `input`.
pub const IDEMPOTENCY_KEY_INVALID: FailureCode = FailureCode::fixed("idempotency_key_invalid");
/// No route has this path. Class `input`; 404.
pub const ROUTE_NOT_FOUND: FailureCode = FailureCode::fixed("route_not_found");
/// The route exists and does not take this method. Class `input`; 405.
pub const METHOD_NOT_ALLOWED: FailureCode = FailureCode::fixed("method_not_allowed");
/// The body did not arrive within the time allowed. Class `input`; 408.
pub const BODY_TIMED_OUT: FailureCode = FailureCode::fixed("body_timed_out");
/// The dispatcher did not answer within the time allowed. The action may
/// still finish. Class `busy`; 503.
pub const REQUEST_TIMED_OUT: FailureCode = FailureCode::fixed("request_timed_out");
/// The dispatcher stopped without an answer. Class `transient`; 500.
pub const ENGINE_FAULT: FailureCode = FailureCode::fixed("engine_fault");
/// Two routes the registry describes cannot both be served: the same
/// method and path, or a path field named differently at the same place.
/// Class `input`. Returned by [`crate::router`], never to a caller.
pub const ROUTE_CONFLICT: FailureCode = FailureCode::fixed("route_conflict");
