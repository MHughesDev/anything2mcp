//! The failure codes the MCP pack produces. Each belongs to exactly one
//! class, and each is declared in the pack's manifest.
//!
//! No sentence here quotes anything an upstream server said: its text is
//! untrusted and never becomes part of a failure (spec section 502).

use a2m_core::{Failure, FailureClass, FailureCode};
use a2m_provider::StepFailure;

/// A mapped tool's definition is not the one its mapping pins, or the tool
/// is gone.
pub const UPSTREAM_TOOL_CHANGED: FailureCode = FailureCode::fixed("upstream_tool_changed");
/// A result does not fit the pinned output schema.
pub const RESPONSE_SCHEMA_MISMATCH: FailureCode = FailureCode::fixed("response_schema_mismatch");
/// The tool answered a well-formed call with its own error result.
pub const APP_VALIDATION_ERROR: FailureCode = FailureCode::fixed("app_validation_error");
/// The server answered with something the protocol or the mapping did not
/// foresee: an unforeseen protocol error, a status, a body that is not a
/// response, a revision this build does not speak.
pub const UNEXPECTED_STATUS: FailureCode = FailureCode::fixed("unexpected_status");
/// A lookup found more than one record.
pub const AMBIGUOUS_MATCH: FailureCode = FailureCode::fixed("ambiguous_match");
/// The answer did not come in time.
pub const TIMEOUT: FailureCode = FailureCode::fixed("timeout");
/// The connection, or the server program, was lost.
pub const CONNECTION_LOST: FailureCode = FailureCode::fixed("connection_lost");
/// The server could not be reached, or said it cannot serve now.
pub const UPSTREAM_UNAVAILABLE: FailureCode = FailureCode::fixed("upstream_unavailable");
/// A call to a tool the mapping does not allow.
pub const NOT_GRANTED: FailureCode = FailureCode::fixed("not_granted");
/// The arguments of a call do not fit the tool's pinned input schema.
pub const ARGUMENTS_INVALID: FailureCode = FailureCode::fixed("arguments_invalid");
/// The server is reached over HTTPS, and this build cannot make HTTPS
/// requests.
pub const TLS_NOT_ENABLED: FailureCode = FailureCode::fixed("tls_not_enabled");
/// A part of the pack's design that this build does not carry out.
pub const NOT_BUILT: FailureCode = FailureCode::fixed("not_built");

/// Every code of this crate with its one class, as the manifest declares
/// them. A test holds the two lists together.
pub const ALL: &[(FailureClass, &str)] = &[
    (FailureClass::Drift, "upstream_tool_changed"),
    (FailureClass::Drift, "response_schema_mismatch"),
    (FailureClass::Rejected, "app_validation_error"),
    (FailureClass::UnknownVariant, "unexpected_status"),
    (FailureClass::PossibleCommit, "ambiguous_match"),
    (FailureClass::Transient, "timeout"),
    (FailureClass::Transient, "connection_lost"),
    (FailureClass::Transient, "upstream_unavailable"),
    (FailureClass::Policy, "not_granted"),
    (FailureClass::Input, "arguments_invalid"),
    (FailureClass::Input, "tls_not_enabled"),
    (FailureClass::Input, "not_built"),
    (FailureClass::Input, "payload_unreadable"),
    // The TLS failures of `a2m_auth::tls`, passed on as they are.
    (FailureClass::Input, "tls_setup_failed"),
    (FailureClass::UnknownVariant, "certificate_untrusted"),
    (FailureClass::UnknownVariant, "certificate_name_mismatch"),
    (FailureClass::UnknownVariant, "certificate_expired"),
    (FailureClass::UnknownVariant, "certificate_not_verified"),
    (FailureClass::UnknownVariant, "tls_handshake_failed"),
];

const RE_PIN: &str = "Run discovery on the surface, review what changed, and pin the tool's new digest in the mapping.";

pub(crate) fn tool_changed(tool: &str, what: &str) -> Failure {
    Failure::new(
        FailureClass::Drift,
        UPSTREAM_TOOL_CHANGED,
        format!("The upstream tool `{}` {what}.", shown(tool)),
        RE_PIN,
    )
}

pub(crate) fn schema_mismatch(tool: &str) -> Failure {
    Failure::new(
        FailureClass::Drift,
        RESPONSE_SCHEMA_MISMATCH,
        format!(
            "The result of the upstream tool `{}` does not fit its pinned output schema.",
            shown(tool)
        ),
        "Read the record back before anything is written again; then run discovery and compare the tool's definition with its mapping.",
    )
}

pub(crate) fn rejected(tool: &str) -> Failure {
    Failure::new(
        FailureClass::Rejected,
        APP_VALIDATION_ERROR,
        format!(
            "The upstream tool `{}` refused the call with its own error result.",
            shown(tool)
        ),
        "Look at the step's capture for what the tool said, as untrusted text, and correct the input.",
    )
}

pub(crate) fn unexpected(what: impl Into<String>) -> Failure {
    Failure::new(
        FailureClass::UnknownVariant,
        UNEXPECTED_STATUS,
        what,
        "Nothing was guessed. Check the server and its mapping, and run discovery on the surface.",
    )
}

pub(crate) fn ambiguous(tool: &str, count: usize) -> Failure {
    Failure::new(
        FailureClass::PossibleCommit,
        AMBIGUOUS_MATCH,
        format!(
            "The lookup through the upstream tool `{}` found {count} records where one was expected.",
            shown(tool)
        ),
        "Look at the records in the system and decide which one the run made, before anything is written again.",
    )
}

pub(crate) fn timeout(what: &str) -> Failure {
    Failure::new(
        FailureClass::Transient,
        TIMEOUT,
        format!("The upstream server did not answer {what} in time."),
        "Try again later; if the call was a write, read the record back first.",
    )
}

pub(crate) fn connection_lost(what: &str) -> Failure {
    Failure::new(
        FailureClass::Transient,
        CONNECTION_LOST,
        format!("The connection to the upstream server was lost {what}."),
        "Try again; if a write was sent, read the record back before anything is written again.",
    )
}

pub(crate) fn unavailable(what: impl Into<String>) -> Failure {
    Failure::new(
        FailureClass::Transient,
        UPSTREAM_UNAVAILABLE,
        what,
        "Check that the upstream server runs and can be reached, then try again.",
    )
}

pub(crate) fn not_granted(what: impl Into<String>) -> Failure {
    Failure::new(
        FailureClass::Policy,
        NOT_GRANTED,
        what,
        "Map the tool in the surface's mapping file, review it, and publish the target again.",
    )
}

pub(crate) fn arguments_invalid(tool: &str, why: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        ARGUMENTS_INVALID,
        format!(
            "The arguments for the upstream tool `{}` do not fit its pinned input schema: {why}.",
            shown(tool)
        ),
        "Correct the binding's `args`, or the input, so that they fit the tool's input schema.",
    )
}

#[cfg(not(feature = "tls"))]
pub(crate) fn tls_not_enabled() -> Failure {
    Failure::new(
        FailureClass::Input,
        TLS_NOT_ENABLED,
        "The upstream server is reached over HTTPS, and TLS is not built into this build.",
        "Reach the server over plain HTTP on a loopback address, or use a build with TLS.",
    )
}

pub(crate) fn not_built(what: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        NOT_BUILT,
        format!("This build of the MCP pack cannot {what}."),
        "See the build order in the design of the crate `a2m-pack-mcp` for what is built.",
    )
}

pub(crate) fn plan_unreadable(what: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        FailureCode::PAYLOAD_UNREADABLE,
        format!("The published plan does not hold what the MCP pack compiled into it: {what}."),
        "Compile the target again with the versions of the packs that are enabled.",
    )
}

/// A failure before anything was sent.
pub(crate) fn unsent(failure: Failure) -> StepFailure {
    StepFailure::not_sent(failure)
}

/// A name a server or a file gave, made safe to show: only the characters a
/// tool's name is made of are kept, and it is cut short. Text that is not
/// such a name is never shown.
pub(crate) fn shown(name: &str) -> String {
    let plain: String = name
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '/')
        })
        .take(96)
        .collect();
    if plain.len() == name.len() {
        plain
    } else {
        format!("{plain}…")
    }
}
