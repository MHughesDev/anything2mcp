//! The failure codes this crate produces, each with its one class. A failure
//! from this crate is built from names and fixed phrases; it never carries a
//! credential, a token, or text an identity service returned. The codes of
//! TLS are the last seven rows, which `crate::tls::CODES` lists.

a2m_core::failure_codes! {
    NEEDS_LOGIN = SessionExpired, "needs_login",
        meaning "The session is absent, expired, or revoked, or a wait for it ran out.",
        next "Sign in with `anything2mcp session login`, then run the tool again or resume the run.";
    NEEDS_MFA = SessionExpired, "needs_mfa",
        meaning "A second factor is demanded and no person answered in time.",
        next "Answer the sign-in request, or sign in with `anything2mcp session login`, then run the tool again.";
    DEVICE_REQUIRED = SessionExpired, "device_required",
        meaning "The session's credentials are held on a runner that is not this one.",
        next "Run on the session's home runner, or sign in again on this one.";
    HEADROOM_SHORT = SessionExpired, "headroom_short",
        meaning "The session will end before the time a pipeline asked to have left.",
        next "Sign in again to start a fresh session, then start the pipeline.";
    LEASE_STALE = SessionExpired, "lease_stale",
        meaning "The lease was granted for credentials that have since been replaced.",
        next "Ask for a new lease.";
    CREDENTIAL_REFUSED = SessionExpired, "credential_refused",
        meaning "A stored credential was refused by the side that checks it, or was tried as often as the auth file allows.",
        next "Check the stored credential and store it again if it changed, then sign in with `anything2mcp session login`.";
    IDENTITY_NOT_ALLOWED = Policy, "identity_not_allowed",
        meaning "The run does not act for the person the session belongs to.",
        next "Call as the person the session belongs to, or use a session of your own.";
    AUTH_PROVIDER_UNKNOWN = Input, "auth_provider_unknown",
        meaning "An auth entry names a provider that nobody registered.",
        next "Name a registered auth provider in `auth.yaml`, or enable the provider that supplies it.";
    AUTH_SETTINGS_INVALID = Input, "auth_settings_invalid",
        meaning "An auth entry lacks a setting its provider needs, or holds a wrong one.",
        next "Correct the entry in `auth.yaml` and publish the target again.";
    CREDENTIAL_MISSING = Input, "credential_missing",
        meaning "A credential reference names nothing that is stored.",
        next "Store the credential under that reference, then try again.";
    UPSTREAM_UNAVAILABLE = Transient, "upstream_unavailable",
        meaning "The service did not answer, or answered that it cannot serve.",
        next "Try again in a moment; if it keeps failing, check the address in the target's files and the network.";
    UNEXPECTED_RESPONSE = UnknownVariant, "unexpected_response",
        meaning "The identity service answered with something that is not a token response.",
        next "Check the provider's settings in `auth.yaml` against the identity service's documentation.";
    DESTINATION_NOT_DECLARED = Policy, "destination_not_declared",
        meaning "A sign-in would send a credential, a token, or the person to an address the target does not declare for its surfaces in the session's environment. Nothing was sent.",
        next "Declare the address as a surface of the target, or correct the setting in `auth.yaml`, and publish the target again.";
    CREDENTIAL_OF_ANOTHER_TARGET = Input, "credential_of_another_target",
        meaning "An auth entry names a credential reference that belongs to another target: its first part is not the target's name.",
        next "Name a reference under `vault://<target>/` in `auth.yaml`, and store the credential there.";
    CREDENTIAL_UNREADABLE = Input, "credential_unreadable",
        meaning "What is stored under a reference does not open for this target: it was stored for another target, or before a credential was bound to its target.",
        next "Store the credential again: type it on the sign-in page when an agent asks, or sign in with `anything2mcp session login`.";
    TLS_NOT_ENABLED = Input, "tls_not_enabled",
        meaning "This build cannot make HTTPS requests, and an address needs one.",
        next "Use a build of the engine with the `tls` feature switched on.";
    AUTH_PROVIDER_DUPLICATE = Input, "auth_provider_duplicate",
        meaning "Two providers were given the same name.",
        next "Enable only one provider of that name.";
    SESSION_MOVE_ILLEGAL = UnknownVariant, "session_move_illegal",
        meaning "The keeper was asked for a move that is not in the table of states. It is a fault of the keeper itself, and the move is not made.",
        next "Report it as a fault in the engine, with the session's timeline.";
    // The codes of TLS: the last seven rows, which `crate::tls::CODES` lists.
    TLS_SETUP_FAILED = Input, "tls_setup_failed",
        meaning "The TLS configuration of this machine could not be made.",
        next "Run `anything2mcp doctor`, and report it if it says nothing more.";
    TRUST_ROOTS_UNREADABLE = Input, "trust_roots_unreadable",
        meaning "A file of further trust roots holds no certificate, or one that cannot be read.",
        next "Name a PEM file that holds the certificate of the authority, and start the engine again.";
    CERTIFICATE_UNTRUSTED = UnknownVariant, "certificate_untrusted",
        meaning "The certificate the system showed does not lead to an authority this machine trusts.",
        next "If the system uses an authority of your organization, name that authority's certificate in the setting `network.trust_roots`; otherwise do not connect to it.";
    CERTIFICATE_NAME_MISMATCH = UnknownVariant, "certificate_name_mismatch",
        meaning "The certificate is for another name than the address.",
        next "Check the address in the target's files: the certificate names another host.";
    CERTIFICATE_EXPIRED = UnknownVariant, "certificate_expired",
        meaning "The certificate's time has run out.",
        next "Ask the system's operator to renew its certificate, and check this machine's clock.";
    CERTIFICATE_NOT_VERIFIED = UnknownVariant, "certificate_not_verified",
        meaning "The certificate was refused for another reason, or the operating system's verifier refused it without saying which.",
        next "Check the system's certificate with its operator.";
    TLS_HANDSHAKE_FAILED = UnknownVariant, "tls_handshake_failed",
        meaning "The TLS handshake failed for a reason that is not the certificate: no version or cipher in common, or the far end ended it.",
        next "Check that the system offers TLS 1.2 or 1.3 at this address.";
}
