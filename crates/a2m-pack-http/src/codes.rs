//! The failure codes this pack produces. Each belongs to exactly one class,
//! and each is declared in the pack's manifest; a test holds the two lists
//! together. `timeout`, `connection_lost`, and `no_confirmation` are the
//! provider contract's own; the TLS failures are the session keeper's, in
//! `a2m_auth::tls`, and [`PASSED_ON`] names those the driver passes on.
//!
//! Whether a `busy` code passes by itself, and why: `reads_still_running`
//! does, because each read left running is told to stop, reads an answer of
//! bounded size, and gives its place back when its thread ends.

use a2m_core::{Failure, FailureCode};
use a2m_provider::StepFailure;

a2m_core::failure_codes! {
    APP_VALIDATION_ERROR = Rejected, "app_validation_error",
        meaning "The system refused the request in a shape the endpoint declares as a refusal.",
        next "Correct the input the system refused, then run the tool again.";
    VERSION_CONFLICT = Stale, "version_conflict",
        meaning "The version sent with a write is no longer the record's.",
        next "Preview the change again, so that it is made against the record as it is now.";
    VERSION_NOT_READ = Stale, "version_not_read",
        meaning "A write that sends a record's version as a precondition, in a pass that read no version for the record. Nothing is sent.",
        next "Preview the change again, so that the record is read before it is written.";
    RESPONSE_SCHEMA_MISMATCH = Drift, "response_schema_mismatch",
        meaning "The body of an accepted answer does not fit the declared shape.",
        next "Compare the system's answer with the endpoint's `response_schema` and the operation's outputs, and correct the one that is wrong.";
    UNEXPECTED_STATUS = UnknownVariant, "unexpected_status",
        meaning "A status the endpoint does not declare.",
        next "Find out what the answer means, and declare it under the endpoint's `responses`.";
    RESPONSE_TOO_LARGE = UnknownVariant, "response_too_large",
        meaning "An answer larger than the limit.",
        next "Raise `max_response_bytes` on the endpoint, or `max_depth` or `max_elements` under its `answer`, if the answer is meant to be that large.";
    DOCUMENT_TYPE_REFUSED = UnknownVariant, "document_type_refused",
        meaning "An XML answer that declares a document type. None is read, so no entity it defines is expanded and nothing it names is fetched (decision 0217).",
        next "Ask the system's owner for the answer without a document type, or read it another way.";
    ANSWER_HOLDS_SECRET = UnknownVariant, "answer_holds_secret",
        meaning "A `bytes` answer whose body holds a registered secret, so neither its length nor its digest is returned (decision 0249); or an answer that quotes the anti-forgery token the write carried, so no value that holds it is returned (decision 0357).",
        next "Ask the system's owner why the answer carries the credential, or read it another way.";
    UPSTREAM_UNAVAILABLE = Transient, "upstream_unavailable",
        meaning "An answer the endpoint declares as \"not now\".",
        next "Run the tool again later.";
    TOKEN_NOT_READ = Stale, "token_not_read",
        meaning "A write that sends an anti-forgery token, in a pass that fetched none for it, or whose token was already sent. Nothing is sent (decision 0357).",
        next "Run the tool again, so that the page is fetched and its token read before the write.";
    ANTI_FORGERY_NOT_FOUND = Drift, "anti_forgery_not_found",
        meaning "The page that gives a write's anti-forgery token answered without a page, or holds no token where the endpoint's `anti_forgery` says. The write was not sent (decision 0357).",
        next "Look at where the page holds its token now, and correct the endpoint's `anti_forgery`.";
    ANTI_FORGERY_UNREADABLE = UnknownVariant, "anti_forgery_unreadable",
        meaning "The page that gives a write's anti-forgery token holds two different tokens where the endpoint says, or one that cannot be sent. The write was not sent (decision 0357).",
        next "Look at the page; a token is read from one place that holds one text.";
    RATE_LIMITED_BY_EDGE = EdgeBlocked, "rate_limited_by_edge",
        meaning "The surface answered with status 429, which the endpoint does not declare: a limit set by the edge in front of the system (decision 0357).",
        next "Look at the surface in a browser, and ask its owner before trying again. The answer is never retried.";
    FORBIDDEN_BY_EDGE = EdgeBlocked, "forbidden_by_edge",
        meaning "The surface answered with status 403 and a page that is not data, which the endpoint does not declare: a refusal by the edge in front of the system (decision 0357).",
        next "Look at the surface in a browser, and ask its owner before trying again. The answer is never retried.";
    SURFACE_NOT_DECLARED = Policy, "surface_not_declared",
        meaning "A request, or a redirect, that leaves the declared surfaces.",
        next "Declare the surface in `target.yaml`, or the destination in the operation's `permissions`.";
    LINK_NOT_ENCRYPTED = Policy, "link_not_encrypted",
        meaning "A request that would carry the session's credential over plain `http` to an address that is not a loopback address, from a surface that does not accept it. Nothing is sent (decision 0188).",
        next "Reach the surface over `https`. For a system on a private network that offers nothing else, the surface may declare `unencrypted_credentials: accepted` in `target.yaml`.";
    PRODUCT_HEADER_REFUSED = Policy, "product_header_refused",
        meaning "The session's provider placed a header whose name begins `a2m-`, one of the product's own carriers between its parts, which never goes on the wire. Nothing is sent (decision 0270).",
        next "Sign in to this surface with a provider made for an HTTP service, such as `api_key`, `bearer_static`, or `http_basic`.";
    READS_STILL_RUNNING = Busy, "reads_still_running",
        meaning "So many reads of answers are still running past their step's deadline, on this surface or in the pack, that no new one is begun. Nothing is sent (decision 0270).",
        next "Run the tool again once those reads have ended, or ask the endpoint's owner why its answers take so long to read.",
        passes by_itself;
    PATH_VALUE_REFUSED = Input, "path_value_refused",
        meaning "A value that cannot stand in one segment of a path.",
        next "Give a value that is one plain name or number, as the system shows it.";
    STEP_NOT_USABLE = Input, "step_not_usable",
        meaning "A step this driver cannot carry out as the plan wrote it.",
        next "Compile the target again with the version of the HTTP pack that is enabled.";
    TLS_NOT_ENABLED = Input, "tls_not_enabled",
        meaning "The surface is reached over HTTPS, and TLS is not in this build.",
        next "Use a surface reached over plain HTTP on this machine, or a build with TLS switched on.";
    AUTH_SCHEME_NOT_BUILT = Input, "auth_scheme_not_built",
        meaning "The session's provider needs a scheme this pack does not carry out yet.",
        next "Use a provider that places a header or a cookie, such as `api_key`, `bearer_static`, or `http_basic`.";
    DESCRIPTION_UNREADABLE = Input, "description_unreadable",
        meaning "A published description that cannot be read.",
        next "Give an OpenAPI 3.0 or 3.1 document or a Swagger 2.0 document, as JSON or YAML, that holds its own schemas.";
    DESCRIPTION_TOO_LARGE = Input, "description_too_large",
        meaning "A published description that is larger, deeper, or wider once its references are followed than the import reads.",
        next "Give a description of the part of the service the tools need, with fewer or smaller schemas.";
}

/// The session keeper's TLS failures, which the driver passes on as they
/// are (decision 0157): every code of `a2m_auth::tls` but
/// `trust_roots_unreadable`, which only the engine's start gives. The
/// manifest declares them after the pack's own, in this order.
pub const PASSED_ON: &[FailureCode] = &[
    a2m_auth::tls::TLS_SETUP_FAILED,
    a2m_auth::tls::CERTIFICATE_UNTRUSTED,
    a2m_auth::tls::CERTIFICATE_NAME_MISMATCH,
    a2m_auth::tls::CERTIFICATE_EXPIRED,
    a2m_auth::tls::CERTIFICATE_NOT_VERIFIED,
    a2m_auth::tls::TLS_HANDSHAKE_FAILED,
];

/// A failure of the code of `row`, in the class its table gives it.
pub(crate) fn failure(
    row: &a2m_core::CodeEntry,
    message: impl Into<String>,
    next: &str,
) -> Failure {
    row.failure(message, next)
}

/// A step the driver cannot carry out as written. Nothing was sent.
pub(crate) fn not_usable(why: impl Into<String>) -> StepFailure {
    StepFailure::not_sent(failure(
        &row::STEP_NOT_USABLE,
        format!(
            "The HTTP driver cannot carry out this step as the plan wrote it: {}.",
            why.into()
        ),
        "Compile the target again with the version of the HTTP pack that is enabled.",
    ))
}

#[cfg(not(feature = "tls"))]
pub(crate) fn tls_not_enabled() -> Failure {
    failure(
        &row::TLS_NOT_ENABLED,
        "The surface is reached over HTTPS, and TLS is not built into this build of the HTTP pack.",
        "Use a surface reached over plain HTTP on this machine, or a build with TLS switched on.",
    )
}

/// A header of the product's own, placed by a session's provider. Its name
/// is the product's and is said; no value is.
pub(crate) fn product_header_refused(name: &str) -> Failure {
    failure(
        &row::PRODUCT_HEADER_REFUSED,
        format!(
            "The session's sign-in placed the header `{}`, which belongs to the product and never goes to a system; nothing was sent.",
            shown(name)
        ),
        "Sign in to this surface with a provider made for an HTTP service, such as `api_key`, `bearer_static`, or `http_basic`.",
    )
}

pub(crate) fn surface_not_declared(what: &str) -> Failure {
    failure(
        &row::SURFACE_NOT_DECLARED,
        format!("{what} is not one of the addresses this run may reach."),
        "Declare the surface in `target.yaml`, or the destination in the operation's `permissions`.",
    )
}

/// A write whose endpoint sends the record's version, in a pass that read
/// none for the record. Nothing was sent.
pub(crate) fn version_not_read(endpoint: &str) -> Failure {
    failure(
        &row::VERSION_NOT_READ,
        format!(
            "The endpoint `{endpoint}` sends the record's version with a write, and no version of the record was read in this pass; nothing was sent."
        ),
        "Preview the change again, so that the record is read before it is written.",
    )
}

/// A write that sends an anti-forgery token, in a pass that fetched none.
/// Nothing was sent.
pub(crate) fn token_not_read(endpoint: &str) -> Failure {
    failure(
        &row::TOKEN_NOT_READ,
        format!(
            "The endpoint `{endpoint}` sends an anti-forgery token with a write, and no token of its page is held in this pass; nothing was sent."
        ),
        "Run the tool again, so that the page is fetched and its token read before the write.",
    )
}

/// Text from outside, cut and cleaned so it can stand in a message.
pub(crate) fn shown(text: &str) -> String {
    const SHOWN: usize = 80;
    let clean: String = text
        .chars()
        .filter(|c| !c.is_control())
        .take(SHOWN)
        .collect();
    if text.chars().count() > SHOWN {
        format!("{clean}…")
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No source of this crate gives a failure a class of its own: each is
    /// made from a row of a table, which gives it its class.
    #[test]
    fn every_failure_takes_its_class_from_a_table() {
        let found = a2m_core::testing::classes_of_their_own(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &[],
        )
        .unwrap();
        assert!(
            found.is_empty(),
            "made with a class of their own: {found:#?}"
        );
        for row in TABLE {
            let made = failure(row, "A case.", row.next);
            assert_eq!((made.class(), made.code().as_str()), (row.class, row.code));
        }
    }
}
