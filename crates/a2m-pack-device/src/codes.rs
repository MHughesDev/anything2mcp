//! The failure codes the device pack produces. Each belongs to exactly one
//! class, and each is declared in the pack's manifest.

use a2m_core::{Failure, FailureClass, FailureCode};

macro_rules! codes {
    ($( $(#[$doc:meta])* $name:ident = $class:ident, $text:literal; )+) => {
        $( $(#[$doc])* pub const $name: FailureCode = FailureCode::fixed($text); )+

        /// Every code of this pack with its one class.
        pub const ALL: &[(FailureClass, &str)] = &[ $( (FailureClass::$class, $text) ),+ ];
    };
}

codes! {
    /// A value is outside its register's safe range, or cannot be held by
    /// the register. Nothing was sent.
    VALUE_OUT_OF_RANGE = Input, "value_out_of_range";
    /// A value is not of the kind its register takes. Nothing was sent.
    SCHEMA_INVALID = Input, "schema_invalid";
    /// The plan's section for this pack does not decode.
    PAYLOAD_UNREADABLE = Input, "payload_unreadable";
    /// The link's address is not one the run may reach.
    SURFACE_NOT_DECLARED = Policy, "surface_not_declared";
    /// The device's model or firmware version is not one the map names.
    DEVICE_IDENTITY_MISMATCH = UnknownVariant, "device_identity_mismatch";
    /// The device answered a read with a reply of no declared form.
    UNEXPECTED_REPLY = UnknownVariant, "unexpected_reply";
    /// The device answered with an exception: an address or a value it will
    /// not take.
    DEVICE_REFUSED = Rejected, "device_refused";
    /// A protection on the device refused.
    DEVICE_PROTECTED = EdgeBlocked, "device_protected";
    /// No reply came in time, on a read or before a write frame was sent.
    TIMEOUT = Transient, "timeout";
    /// The link closed, on a read or before a write frame was sent.
    CONNECTION_LOST = Transient, "connection_lost";
    /// A reply was damaged, on a read.
    FRAME_CORRUPT = Transient, "frame_corrupt";
    /// The device answered that it is busy. Nothing was carried out.
    DEVICE_BUSY = Transient, "device_busy";
    /// A write frame was sent and no clear acknowledgement came.
    NO_CONFIRMATION = PossibleCommit, "no_confirmation";
    /// A device did not come back after a firmware update.
    DEVICE_NOT_RETURNED = PossibleCommit, "device_not_returned";
}

pub(crate) fn failure(
    class: FailureClass,
    code: FailureCode,
    message: impl Into<String>,
    next_step: impl Into<String>,
) -> Failure {
    Failure::new(class, code, message, next_step)
}
