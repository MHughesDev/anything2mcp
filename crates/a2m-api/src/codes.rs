//! The failure codes this crate adds to the dispatcher's. Each is refused
//! before the dispatcher is reached, except the two about time, which are
//! said in place of an answer that did not come. The status each is served
//! with is decided in `crate::status`.
//!
//! Whether a `busy` code passes by itself, and why:
//!
//! - `request_timed_out` does: the engine was slower than the time allowed,
//!   and may answer the next call in time. Whether it is safe to send the
//!   same request again is a separate answer, and the failure's next step
//!   gives it.

use a2m_core::FailureCode;

/// The body is not JSON, or not one JSON object. `a2m-core` declares it.
pub const SCHEMA_INVALID: FailureCode = FailureCode::SCHEMA_INVALID;

a2m_core::failure_codes! {
    CALLER_MISSING = Policy, "caller_missing",
        meaning "A request came with nothing that names its caller: whoever mounts the router did not put one on it.",
        next "Call through the local channel, or with a credential the engine accepts.";
    BODY_TOO_LARGE = Input, "body_too_large",
        meaning "The body is larger than the limit, or ended early. Served as 413.",
        next "Send a smaller input, whole.";
    CONTENT_TYPE_UNSUPPORTED = Input, "content_type_unsupported",
        meaning "A body was sent with a type other than JSON. Served as 415.",
        next "Send the input as JSON, with the content type application/json.";
    QUERY_INVALID = Input, "query_invalid",
        meaning "The query of a request is not well formed, names a field twice, or was sent on a route that reads its input from the body.",
        next "Give each field once, as name=value, percent-encoded, on a route that reads its input from the query; otherwise send the input as a JSON object in the body.";
    PATH_FIELD_CONFLICT = Input, "path_field_conflict",
        meaning "A field given in the path was given again, differently, in the body.",
        next "Give the field once, in the path.";
    IDEMPOTENCY_KEY_INVALID = Input, "idempotency_key_invalid",
        meaning "The `Idempotency-Key` header is empty, too long, or not printable text.",
        next "Send a short printable key, the same for every retry of one write.";
    ROUTE_NOT_FOUND = Input, "route_not_found",
        meaning "No route has this path. Served as 404.",
        next "Read /api/v1/openapi.json for the routes.";
    METHOD_NOT_ALLOWED = Input, "method_not_allowed",
        meaning "The route exists and does not take this method. Served as 405.",
        next "Read /api/v1/openapi.json for the method of each route.";
    BODY_TIMED_OUT = Input, "body_timed_out",
        meaning "The body did not arrive within the time allowed. Served as 408.",
        next "Send the whole body at once.";
    REQUEST_TIMED_OUT = Busy, "request_timed_out",
        meaning "The dispatcher did not answer within the time allowed. The action may still finish. Served as 503.",
        next "Find the request's run with `run.list` before sending it again, unless it carried an Idempotency-Key header: then send it again with the same key.",
        passes by_itself;
    ENGINE_FAULT = Transient, "engine_fault",
        meaning "The dispatcher stopped without an answer. Served as 500.",
        next "Find the request's run with `run.list` before sending it again, unless it carried an Idempotency-Key header: then send it again with the same key.";
    ROUTE_CONFLICT = Input, "route_conflict",
        meaning "Two routes the registry describes cannot both be served: the same method and path, or a path field named differently at the same place. Returned when the router is made, never to a caller.",
        next "Give each action's alias a path no other route has.";
}
