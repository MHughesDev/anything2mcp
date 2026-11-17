//! The failure codes this pack produces. Each belongs to exactly one class,
//! and each is declared in the pack's manifest. `timeout`,
//! `connection_lost`, and `no_confirmation` are the provider contract's own;
//! the TLS failures are the session keeper's, in `a2m_auth::tls`.

use a2m_core::{Failure, FailureClass, FailureCode};
use a2m_provider::StepFailure;

/// `rejected`: the system refused the request in a shape the endpoint
/// declares as a refusal.
pub const APP_VALIDATION_ERROR: FailureCode = FailureCode::fixed("app_validation_error");
/// `stale`: the version sent with a write is no longer the record's.
pub const VERSION_CONFLICT: FailureCode = FailureCode::fixed("version_conflict");
/// `stale`: a write that sends a record's version as a precondition, in a
/// pass that read no version for the record. Nothing is sent.
pub const VERSION_NOT_READ: FailureCode = FailureCode::fixed("version_not_read");
/// `drift`: the body of an accepted answer does not fit the declared shape.
pub const RESPONSE_SCHEMA_MISMATCH: FailureCode = FailureCode::fixed("response_schema_mismatch");
/// `unknown_variant`: a status the endpoint does not declare.
pub const UNEXPECTED_STATUS: FailureCode = FailureCode::fixed("unexpected_status");
/// `unknown_variant`: an answer larger than the limit.
pub const RESPONSE_TOO_LARGE: FailureCode = FailureCode::fixed("response_too_large");
/// `transient`: an answer the endpoint declares as "not now".
pub const UPSTREAM_UNAVAILABLE: FailureCode = FailureCode::fixed("upstream_unavailable");
/// `policy`: a request, or a redirect, that leaves the declared surfaces.
pub const SURFACE_NOT_DECLARED: FailureCode = FailureCode::fixed("surface_not_declared");
/// `input`: a value that cannot stand in one segment of a path.
pub const PATH_VALUE_REFUSED: FailureCode = FailureCode::fixed("path_value_refused");
/// `input`: a step this driver cannot carry out as the plan wrote it.
pub const STEP_NOT_USABLE: FailureCode = FailureCode::fixed("step_not_usable");
/// `input`: the surface is reached over HTTPS, and TLS is not in this build.
pub const TLS_NOT_ENABLED: FailureCode = FailureCode::fixed("tls_not_enabled");
/// `input`: the session's provider needs a scheme this pack does not carry
/// out yet.
pub const AUTH_SCHEME_NOT_BUILT: FailureCode = FailureCode::fixed("auth_scheme_not_built");
/// `input`: a published description that cannot be read.
pub const DESCRIPTION_UNREADABLE: FailureCode = FailureCode::fixed("description_unreadable");
/// `input`: a published description that is larger, deeper, or wider once
/// its references are followed than the import reads.
pub const DESCRIPTION_TOO_LARGE: FailureCode = FailureCode::fixed("description_too_large");

/// Every code of this pack with its one class, as the manifest declares
/// them. A test holds the two lists together.
pub const ALL: &[(FailureClass, &str)] = &[
    (FailureClass::Rejected, "app_validation_error"),
    (FailureClass::Stale, "version_conflict"),
    (FailureClass::Stale, "version_not_read"),
    (FailureClass::Drift, "response_schema_mismatch"),
    (FailureClass::UnknownVariant, "unexpected_status"),
    (FailureClass::UnknownVariant, "response_too_large"),
    (FailureClass::Transient, "upstream_unavailable"),
    (FailureClass::Policy, "surface_not_declared"),
    (FailureClass::Input, "path_value_refused"),
    (FailureClass::Input, "step_not_usable"),
    (FailureClass::Input, "tls_not_enabled"),
    (FailureClass::Input, "auth_scheme_not_built"),
    (FailureClass::Input, "description_unreadable"),
    (FailureClass::Input, "description_too_large"),
    // The TLS failures of `a2m_auth::tls`, passed on as they are.
    (FailureClass::Input, "tls_setup_failed"),
    (FailureClass::UnknownVariant, "certificate_untrusted"),
    (FailureClass::UnknownVariant, "certificate_name_mismatch"),
    (FailureClass::UnknownVariant, "certificate_expired"),
    (FailureClass::UnknownVariant, "certificate_not_verified"),
    (FailureClass::UnknownVariant, "tls_handshake_failed"),
];

pub(crate) fn failure(
    class: FailureClass,
    code: FailureCode,
    message: impl Into<String>,
    next: &str,
) -> Failure {
    Failure::new(class, code, message, next)
}

/// A step the driver cannot carry out as written. Nothing was sent.
pub(crate) fn not_usable(why: impl Into<String>) -> StepFailure {
    StepFailure::not_sent(failure(
        FailureClass::Input,
        STEP_NOT_USABLE,
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
        FailureClass::Input,
        TLS_NOT_ENABLED,
        "The surface is reached over HTTPS, and TLS is not built into this build of the HTTP pack.",
        "Use a surface reached over plain HTTP on this machine, or a build with TLS switched on.",
    )
}

pub(crate) fn surface_not_declared(what: &str) -> Failure {
    failure(
        FailureClass::Policy,
        SURFACE_NOT_DECLARED,
        format!("{what} is not one of the addresses this run may reach."),
        "Declare the surface in `target.yaml`, or the destination in the operation's `permissions`.",
    )
}

/// A write whose endpoint sends the record's version, in a pass that read
/// none for the record. Nothing was sent.
pub(crate) fn version_not_read(endpoint: &str) -> Failure {
    failure(
        FailureClass::Stale,
        VERSION_NOT_READ,
        format!(
            "The endpoint `{endpoint}` sends the record's version with a write, and no version of the record was read in this pass; nothing was sent."
        ),
        "Preview the change again, so that the record is read before it is written.",
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
