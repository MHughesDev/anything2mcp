//! The failure codes the stream driver returns, each with its one class
//! (spec section 309, "Failures"). The manifest declares the same list, and
//! a test holds the two together.
//!
//! Whether a `busy` code passes by itself, and why:
//!
//! - `subscription_in_use` does: the other run that holds the subscription
//!   ends.
//! - `message_may_be_held` does not: the record that took the message first
//!   is in doubt, and is settled before the message is taken again.

use a2m_core::{Failure, FailureClass, FailureCode};
use a2m_provider::{Sent, StepFailure};

a2m_core::failure_codes! {
    SCHEMA_INVALID = Input, "schema_invalid",
        meaning "A message does not fit its channel's schema; nothing was sent.",
        next "Correct the input to fit the channel's schema; nothing was sent.";
    VALUE_OUT_OF_RANGE = Input, "value_out_of_range",
        meaning "A value has no form in the channel's encoding; nothing was sent.",
        next "Correct the value, or the channel file; nothing was sent.";
    MESSAGE_TOO_LARGE = Input, "message_too_large",
        meaning "A message is larger than its channel allows.",
        next "Shorten the message; nothing was sent.";
    PROTOCOL_UNKNOWN = Input, "protocol_unknown",
        meaning "The surface names a wire protocol this build does not speak.",
        next "Use a protocol the stream pack lists, or a build that speaks this one.";
    CREDENTIAL_NOT_CARRIED = Input, "credential_not_carried",
        meaning "The session holds a credential the wire protocol cannot present.",
        next "Sign in to the target with a provider that gives one token, or use a protocol that carries this credential.";
    NOTHING_RECORDED = Input, "nothing_recorded",
        meaning "A step that settles a record by what its earlier steps recorded was given nothing recorded, or nothing of the form it needs; nothing was sent.",
        next "Settle the record by hand, with what the run's ledger holds for it.";
    TRANSPORT_REFUSED = Policy, "transport_refused",
        meaning "The connection would carry the session's credential in plain text to an address that is not this machine's own, the broker asks for TLS, or the address is not one a connection can be made to.",
        next "A credential goes over plain TCP only to this machine's loopback IP address (127.0.0.1 or [::1]): write the broker's address as one, reach a broker that asks for no sign-in, or use a build that speaks TLS to it.";
    ACK_NOT_OWNED = Policy, "ack_not_owned",
        meaning "An acknowledgement was asked for on a queue the runtime does not own.",
        next "Look at the queue with `list`; declare `ownership: runtime_owned` only when nothing else reads it.";
    SURFACE_NOT_DECLARED = Policy, "surface_not_declared",
        meaning "An address outside the target's declared surfaces was asked for.",
        next "Declare the address as a surface of the target in the run's environment, then try again.";
    SCAN_LIMIT = Policy, "scan_limit",
        meaning "A log or a queue holds more than a scan may read, so the scan cannot say whether the record's message is there.",
        next "Look at the log or the queue by hand before anything is sent again, or raise the channel's scan limit.";
    TAKE_TOO_LARGE = Policy, "take_too_large",
        meaning "A message taken is larger than may be recorded before it is acknowledged; it was given back.",
        next "Raise `limits.message_bytes` in the channel file, or have the message removed from the queue by hand.";
    NEEDS_LOGIN = SessionExpired, "needs_login",
        meaning "The broker refused the credentials.",
        next "Sign in to the target again with `anything2mcp session login`.";
    BROKER_REFUSED = Rejected, "broker_refused",
        meaning "The broker refused the message, with its reason.",
        next "Correct what the broker names, then send the message again.";
    CHANNEL_MISSING = Drift, "channel_missing",
        meaning "The channel, or the runtime's own subscription on it, is gone.",
        next "Check the channel's `address` and `subscription` against the broker, and correct the channel file.";
    SHAPE_CHANGED = Drift, "shape_changed",
        meaning "The channel's shape or key is not the one its file declares.",
        next "Run a discovery of the broker and correct the channel file.";
    RESPONSE_SCHEMA_MISMATCH = Drift, "response_schema_mismatch",
        meaning "A message read does not fit its channel's schema.",
        next "Compare the channel's schema with what the broker now sends, and correct the target's files.";
    POSITION_OUT_OF_RANGE = Drift, "position_out_of_range",
        meaning "A position the run was to read from is no longer held.",
        next "Start from a position the log still holds.";
    UNEXPECTED_REPLY = UnknownVariant, "unexpected_reply",
        meaning "The broker answered in a way the driver has no rule for.",
        next "Check that the surface names the protocol the system speaks, and the target's files against the system's documentation; then report this.";
    TIMEOUT = Transient, "timeout",
        meaning "A deadline passed on something that commits nothing.",
        next "Try again, or raise the step's time limit if the broker is known to be slow.";
    CONNECTION_LOST = Transient, "connection_lost",
        meaning "The connection to the broker was lost.",
        next "Try again; a read changes nothing.";
    UPSTREAM_UNAVAILABLE = Transient, "upstream_unavailable",
        meaning "The broker could not be reached.",
        next "Check that the broker is running and that its address is the surface's, then try again.";
    NOTHING_ARRIVED = Transient, "nothing_arrived",
        meaning "No message matched within the limit, and the operation needs one.",
        next "Try again when the message is expected, or raise the wait where the channel allows.";
    NO_CONFIRMATION = PossibleCommit, "no_confirmation",
        meaning "A publish was sent and no confirmation came.",
        next "Reconcile the record: a look at the channel says whether the message is there.";
    SUBSCRIPTION_IN_USE = Busy, "subscription_in_use",
        meaning "The runtime's own subscription is held by another run.",
        next "Wait for the other run to end, then try again.",
        passes by_itself;
    MESSAGE_MAY_BE_HELD = Busy, "message_may_be_held",
        meaning "A message delivered again may be held by the record that took it first; nothing was taken.",
        next "Settle the record that took the message first; the message is then taken by the next run.",
        passes not_by_itself;
    SCAN_INCONCLUSIVE = Policy, "scan_inconclusive",
        meaning "A look cannot say whether the record's message is in the log: the stream may lose a message from inside its range, or the range read was not whole.",
        next "Look at the log by hand before anything is sent again: the record stays in doubt.";
}

/// The class of a code of this pack, from its table; `unknown_variant` for
/// a code the table does not hold.
pub(crate) fn class_of(code: &FailureCode) -> FailureClass {
    ALL.iter()
        .find(|(_, text)| *text == code.as_str())
        .map_or(FailureClass::UnknownVariant, |(class, _)| *class)
}

/// The codes the provider contract and the session keeper already define.
/// The driver returns them, and the manifest does not declare them again.
pub const CONTRACT_CODES: &[&str] = &[
    "timeout",
    "connection_lost",
    "no_confirmation",
    "needs_login",
];

/// Text that came from a broker, cleaned and cut for a message. The
/// caller redacts the text first: a cut made before that could leave part
/// of a secret, which no redactor would then know.
pub(crate) fn shown(text: &str) -> String {
    const SHOWN: usize = 120;
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

/// A failure of this pack: the class comes from the code.
pub(crate) fn failure(
    code: FailureCode,
    message: impl Into<String>,
    next_step: impl Into<String>,
) -> Failure {
    Failure::new(class_of(&code), code, message, next_step)
}

/// A step failure of this pack.
pub(crate) fn stopped(
    sent: Sent,
    code: FailureCode,
    message: impl Into<String>,
    next_step: impl Into<String>,
) -> StepFailure {
    StepFailure {
        failure: failure(code, message, next_step),
        sent,
    }
}
