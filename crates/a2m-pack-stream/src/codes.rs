//! The failure codes the stream driver returns, each with its one class
//! (spec section 309, "Failures"). The manifest declares the same list, and
//! a test holds the two together.

use a2m_core::{Failure, FailureClass, FailureCode};
use a2m_provider::{Sent, StepFailure};

macro_rules! codes {
    ($( $(#[$doc:meta])* $name:ident = $class:ident, $text:literal; )+) => {
        $( $(#[$doc])* pub const $name: FailureCode = FailureCode::fixed($text); )+

        /// Every code the driver returns with its one class.
        pub const ALL: &[(FailureClass, &str)] = &[ $( (FailureClass::$class, $text) ),+ ];

        /// The class of a code of this pack.
        pub(crate) fn class_of(code: &FailureCode) -> FailureClass {
            $( if code.as_str() == $text { return FailureClass::$class; } )+
            FailureClass::UnknownVariant
        }
    };
}

codes! {
    /// A message does not fit its channel's schema; nothing was sent.
    SCHEMA_INVALID = Input, "schema_invalid";
    /// A value has no form in the channel's encoding; nothing was sent.
    VALUE_OUT_OF_RANGE = Input, "value_out_of_range";
    /// A message is larger than its channel allows.
    MESSAGE_TOO_LARGE = Input, "message_too_large";
    /// The surface names a wire protocol this build does not speak.
    PROTOCOL_UNKNOWN = Input, "protocol_unknown";
    /// An acknowledgement was asked for on a queue the runtime does not own.
    ACK_NOT_OWNED = Policy, "ack_not_owned";
    /// The broker's address is not one the run may reach.
    SURFACE_NOT_DECLARED = Policy, "surface_not_declared";
    /// A log holds more than a scan may read, so the scan cannot say "absent".
    SCAN_LIMIT = Policy, "scan_limit";
    /// The broker refused the credentials.
    NEEDS_LOGIN = SessionExpired, "needs_login";
    /// The broker refused the message, with its reason.
    BROKER_REFUSED = Rejected, "broker_refused";
    /// The channel, or the runtime's own subscription on it, is gone.
    CHANNEL_MISSING = Drift, "channel_missing";
    /// The channel's shape or key is not the one its file declares.
    SHAPE_CHANGED = Drift, "shape_changed";
    /// A message read does not fit its channel's schema.
    RESPONSE_SCHEMA_MISMATCH = Drift, "response_schema_mismatch";
    /// A position the run was to read from is no longer held.
    POSITION_OUT_OF_RANGE = Drift, "position_out_of_range";
    /// The broker answered in a way the driver has no rule for.
    UNEXPECTED_REPLY = UnknownVariant, "unexpected_reply";
    /// A time limit passed on something that commits nothing.
    TIMEOUT = Transient, "timeout";
    /// The connection to the broker was lost.
    CONNECTION_LOST = Transient, "connection_lost";
    /// The broker could not be reached.
    UPSTREAM_UNAVAILABLE = Transient, "upstream_unavailable";
    /// No message matched within the limit, and the operation needs one.
    NOTHING_ARRIVED = Transient, "nothing_arrived";
    /// A publish was sent and no confirmation came.
    NO_CONFIRMATION = PossibleCommit, "no_confirmation";
    /// The runtime's own subscription is held by another run.
    SUBSCRIPTION_IN_USE = Busy, "subscription_in_use";
}

/// The codes the provider contract and the session keeper already define,
/// which a manifest does not declare again.
pub(crate) const CONTRACT_CODES: &[&str] =
    &["timeout", "connection_lost", "no_confirmation", "needs_login"];

/// Text that came from a broker, cut and cleaned before it is put in a
/// message. The handle redacts the whole failure afterwards.
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
