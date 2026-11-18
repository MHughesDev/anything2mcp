//! The failures this crate produces: one class and one code each.
//!
//! A failure from this crate is built from names and fixed phrases. It never
//! carries a credential, a token, or text an identity service returned.

use a2m_core::{Failure, FailureClass, FailureCode};

/// The session is absent, expired, or revoked, or a wait for it ran out.
pub const NEEDS_LOGIN: FailureCode = FailureCode::fixed("needs_login");
/// A second factor is demanded and no person answered in time.
pub const NEEDS_MFA: FailureCode = FailureCode::fixed("needs_mfa");
/// The session's credentials are held on a runner that is not this one.
pub const DEVICE_REQUIRED: FailureCode = FailureCode::fixed("device_required");
/// The session will end before the time a pipeline asked to have left.
pub const HEADROOM_SHORT: FailureCode = FailureCode::fixed("headroom_short");
/// The lease was granted for credentials that have since been replaced.
pub const LEASE_STALE: FailureCode = FailureCode::fixed("lease_stale");
/// A stored credential was refused by the side that checks it.
pub const CREDENTIAL_REFUSED: FailureCode = FailureCode::fixed("credential_refused");
/// The run does not act for the person the session belongs to.
pub const IDENTITY_NOT_ALLOWED: FailureCode = FailureCode::fixed("identity_not_allowed");
/// An auth entry names a provider that nobody registered.
pub const AUTH_PROVIDER_UNKNOWN: FailureCode = FailureCode::fixed("auth_provider_unknown");
/// An auth entry lacks a setting its provider needs, or holds a wrong one.
pub const AUTH_SETTINGS_INVALID: FailureCode = FailureCode::fixed("auth_settings_invalid");
/// A credential reference names nothing that is stored.
pub const CREDENTIAL_MISSING: FailureCode = FailureCode::fixed("credential_missing");
/// The identity service did not answer, or answered that it cannot serve.
pub const UPSTREAM_UNAVAILABLE: FailureCode = FailureCode::fixed("upstream_unavailable");
/// The identity service answered with something that is not a token response.
pub const UNEXPECTED_RESPONSE: FailureCode = FailureCode::fixed("unexpected_response");

/// A sign-in would send a credential, a token, or the person to an address
/// the target does not declare for its surfaces in the session's
/// environment. Nothing was sent (decision 0248).
pub const DESTINATION_NOT_DECLARED: FailureCode = FailureCode::fixed("destination_not_declared");
/// An auth entry names a credential reference that belongs to another
/// target: its first part is not the target's name (decision 0248).
pub const CREDENTIAL_OF_ANOTHER_TARGET: FailureCode =
    FailureCode::fixed("credential_of_another_target");
/// What is stored under a reference does not open for this target: it was
/// stored for another target, or before a credential was bound to its
/// target (decision 0248).
pub const CREDENTIAL_UNREADABLE: FailureCode = FailureCode::fixed("credential_unreadable");

/// This build cannot make HTTPS requests, and an identity service needs one.
pub const TLS_NOT_ENABLED: FailureCode = FailureCode::fixed("tls_not_enabled");

/// The session cannot be used until someone signs in.
pub(crate) fn needs_login(why: &'static str) -> Failure {
    Failure::new(
        FailureClass::SessionExpired,
        NEEDS_LOGIN,
        format!("The session cannot be used: {why}."),
        "Sign in with `anything2mcp session login`, then run the tool again or resume the run.",
    )
}

/// A second factor is wanted and nobody gave it in time.
pub(crate) fn needs_mfa() -> Failure {
    Failure::new(
        FailureClass::SessionExpired,
        NEEDS_MFA,
        "The identity provider demands a second factor and nobody answered in time.",
        "Answer the sign-in request, or sign in with `anything2mcp session login`, then run the tool again.",
    )
}

/// The credentials are not on this runner.
pub(crate) fn device_required() -> Failure {
    Failure::new(
        FailureClass::SessionExpired,
        DEVICE_REQUIRED,
        "The session's credentials are not held on this runner.",
        "Run on the session's home runner, or sign in again on this one.",
    )
}

/// Less time is left than the pipeline asked for.
pub(crate) fn headroom_short() -> Failure {
    Failure::new(
        FailureClass::SessionExpired,
        HEADROOM_SHORT,
        "The session will end before the time this pipeline asked to have left.",
        "Sign in again to start a fresh session, then start the pipeline.",
    )
}

/// The lease is older than the session's credentials.
pub(crate) fn lease_stale() -> Failure {
    Failure::new(
        FailureClass::SessionExpired,
        LEASE_STALE,
        "The lease was granted before the session's credentials were replaced or ended.",
        "Ask for a new lease.",
    )
}

/// A stored credential was refused. `by` is a fixed phrase.
pub(crate) fn credential_refused(by: &'static str) -> Failure {
    Failure::new(
        FailureClass::SessionExpired,
        CREDENTIAL_REFUSED,
        format!("The stored credential was refused by {by}."),
        "Check the stored credential and replace it if it changed, then sign in with `anything2mcp session login`. It is not tried again automatically.",
    )
}

/// The run acts for someone other than the session's person.
pub(crate) fn identity_not_allowed() -> Failure {
    Failure::new(
        FailureClass::Policy,
        IDENTITY_NOT_ALLOWED,
        "A person's session may be used only by that person or an agent acting for them.",
        "Call as the person the session belongs to, or use a session of your own.",
    )
}

/// No provider of that name is registered.
pub(crate) fn provider_unknown(name: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        AUTH_PROVIDER_UNKNOWN,
        format!("The auth entry names the provider `{name}`, which is not registered."),
        "Name a registered auth provider in `auth.yaml`, or enable the provider that supplies it.",
    )
}

/// A provider setting is absent or wrong. `what` names the setting and never
/// holds its value.
pub(crate) fn settings_invalid(provider: &str, what: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        AUTH_SETTINGS_INVALID,
        format!("The auth entry for the provider `{provider}` is not usable: {what}."),
        "Correct the entry in `auth.yaml` and publish the target again.",
    )
}

/// Nothing is stored under a credential reference. The reference is a name.
pub(crate) fn credential_missing(reference: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        CREDENTIAL_MISSING,
        format!("Nothing is stored under the credential reference `{reference}`."),
        "Store the credential under that reference, then try again.",
    )
}

/// An address a sign-in would send to is not one the target declares.
/// `what` names the setting or the surface, never a value of a credential.
pub(crate) fn destination_not_declared(provider: &str, what: &str) -> Failure {
    Failure::new(
        FailureClass::Policy,
        DESTINATION_NOT_DECLARED,
        format!(
            "The sign-in through the provider `{provider}` would send to {what}, and that address is not one the target declares for its surfaces in this environment. Nothing was sent."
        ),
        "Declare the address as a surface of the target, or correct the setting in `auth.yaml`, and publish the target again.",
    )
}

/// A sign-in declared by drafts would act without its person: a person
/// must sign in, as for any `needs_login`, on a page that shows them every
/// address the sign-in sends to (decision 0248).
pub(crate) fn not_published(what: &'static str) -> Failure {
    Failure::new(
        FailureClass::SessionExpired,
        NEEDS_LOGIN,
        format!(
            "This sign-in is declared by the target's drafts, and {what}: only a sign-in a published version declares may, and nothing was sent."
        ),
        "Sign in with `anything2mcp session login`: the person is shown every address the sign-in sends to and carries it out themself. Or publish the target.",
    )
}

/// The entry names another target's credential. The reference is a name.
pub(crate) fn credential_of_another_target(reference: &str, target: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        CREDENTIAL_OF_ANOTHER_TARGET,
        format!(
            "The credential reference `{reference}` belongs to another target: a reference of the target `{target}` starts with `vault://{target}/`."
        ),
        format!(
            "Name a reference under `vault://{target}/` in `auth.yaml`, and store the credential there."
        ),
    )
}

/// A reference whose first part is not a target's name belongs to none.
pub(crate) fn reference_names_no_target(reference: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        CREDENTIAL_OF_ANOTHER_TARGET,
        format!(
            "The credential reference `{reference}` belongs to no target: its first part must be the name of the target it is for, as in `vault://example/service/token`."
        ),
        "Name a reference under `vault://<target>/` in `auth.yaml`, and store the credential there.",
    )
}

/// What is stored does not open for this target. The reference is a name.
pub(crate) fn credential_unreadable(reference: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        CREDENTIAL_UNREADABLE,
        format!(
            "What is stored under the credential reference `{reference}` does not open for this target: it was stored before a credential was bound to the target it is for, or for another target."
        ),
        "Store the credential again: type it on the sign-in page when an agent asks, or sign in with `anything2mcp session login`.",
    )
}

/// The identity service could not be reached or could not serve.
pub(crate) fn upstream_unavailable(what: &'static str) -> Failure {
    Failure::new(
        FailureClass::Transient,
        UPSTREAM_UNAVAILABLE,
        format!("The identity service could not be used: {what}."),
        "Try again in a moment. If it keeps failing, check the token endpoint in `auth.yaml` and the network.",
    )
}

/// The identity service answered with something no rule covers.
pub(crate) fn unexpected_response(what: &'static str) -> Failure {
    Failure::new(
        FailureClass::UnknownVariant,
        UNEXPECTED_RESPONSE,
        format!("The identity service answered in a way no rule covers: {what}."),
        "Check the provider's settings in `auth.yaml` against the identity service's documentation.",
    )
}

/// An `https` endpoint in a build that cannot make HTTPS requests.
#[cfg(not(feature = "tls"))]
pub(crate) fn tls_not_enabled() -> Failure {
    Failure::new(
        FailureClass::Input,
        TLS_NOT_ENABLED,
        "The identity service is reached over HTTPS, and this build of the engine cannot make HTTPS requests.",
        "Use a build with the `tls` feature of the session keeper switched on.",
    )
}
