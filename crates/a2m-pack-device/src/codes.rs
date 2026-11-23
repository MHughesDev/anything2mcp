//! The failure codes the device pack produces. Each belongs to exactly one
//! class, and each is declared in the pack's manifest.

use a2m_core::{CodeEntry, Failure};

a2m_core::failure_codes! {
    VALUE_OUT_OF_RANGE = Input, "value_out_of_range",
        meaning "A value is outside its register's safe range, or cannot be held by the register. Nothing was sent.",
        next "Give a value inside the register's range that the register can hold exactly.";
    SCHEMA_INVALID = Input, "schema_invalid",
        meaning "A value is not of the kind its register takes. Nothing was sent.",
        next "Give a value of the register's field type.";
    PAYLOAD_UNREADABLE = Input, "payload_unreadable",
        meaning "The plan's section for this pack does not decode.",
        next "Compile the target again with the device pack that is enabled now.";
    IMAGE_NOT_ACCEPTED = Input, "image_not_accepted",
        meaning "The firmware image is not one an update will send: it is not hexadecimal text, it is not of the form an image has, its checksum does not hold, it is larger than the device takes, it carries another version than the operation names, or its digest is not on the operation's list. Nothing was sent.",
        next "Give the image the device's maker issued for this model, as hexadecimal text, with the version it carries; add its digest to the operation's `accepts` list if the list is meant to include it.";
    SURFACE_NOT_DECLARED = Policy, "surface_not_declared",
        meaning "An address outside the target's declared surfaces was asked for.",
        next "Declare the address as a surface of the target in the run's environment, then try again.";
    DEVICE_SERIAL_NOT_DECLARED = Policy, "device_serial_not_declared",
        meaning "The surface declares no serial for the device of the run's environment, so nothing is written to it. Nothing was sent.",
        next "Declare the unit's serial for this environment under `serials:` on the surface, after proving the map against that unit.";
    UPDATE_PRECONDITION_UNMET = Policy, "update_precondition_unmet",
        meaning "A condition the update declares, or the update mechanism's own state, does not hold on the device. Nothing was written.",
        next "Bring the device to the state its documentation sets for an update, such as a pump stopped and no update in progress, then run the operation again.";
    DISCOVERY_IDENTIFIES_ONLY = Policy, "discovery_identifies_only",
        meaning "A frame other than identification was asked for in the discovery posture. Nothing was sent.",
        next "Read a register through an operation, in a run that is not discovery.";
    FRAME_BUDGET_EXCEEDED = Policy, "frame_budget_exceeded",
        meaning "The run reached the most frames it may send on a link, or a write would leave too few for its read-back. Nothing was sent.",
        next "Do the work in smaller runs, or raise the device pack's setting `max_frames_per_run`.";
    DEVICE_IDENTITY_MISMATCH = UnknownVariant, "device_identity_mismatch",
        meaning "The device's model, firmware version, or serial is not the one the map and the surface name.",
        next "Check the surface's address and protocol in this environment; if the unit was replaced, prove the map against the new unit in a test environment and declare its serial.";
    PHYSICAL_NOT_BUILT = UnknownVariant, "physical_not_built",
        meaning "A step writes a register with the hazard `physical`, and such a write is not built. Nothing was sent.",
        next "Leave this operation out until `confirm_by` and the safe-state operation are built.";
    UNEXPECTED_REPLY = UnknownVariant, "unexpected_reply",
        meaning "The device answered a read with a reply of no declared form.",
        next "Check that the surface names the protocol the system speaks, and the target's files against the system's documentation; then report this.";
    IMAGE_REFUSED = Rejected, "image_refused",
        meaning "The device took the image into update mode and then refused it, or refused a block of it. It was told to leave update mode; the image it held in its other slot was given up when update mode began.",
        next "Check the image and the device's reason (the update error it reports) against the device's documentation; the device still runs its present firmware, and its other slot holds no image until an update is committed.";
    DEVICE_REFUSED = Rejected, "device_refused",
        meaning "The device answered with an exception: an address or a value it will not take.",
        next "Check the register's address, layout, range, and access in the map against the device's documentation.";
    TIMEOUT = Transient, "timeout",
        meaning "No reply came in time, on a read or before a write frame was sent.",
        next "Check that the device is on and reachable, then try again.";
    CONNECTION_LOST = Transient, "connection_lost",
        meaning "The link closed, on a read or before a write frame was sent.",
        next "Open the link again and read again.";
    FRAME_CORRUPT = Transient, "frame_corrupt",
        meaning "A reply was damaged, on a read.",
        next "Open the link again and read again.";
    DEVICE_BUSY = Transient, "device_busy",
        meaning "The device answered that it is busy. Nothing was carried out.",
        next "Run the operation again when the device is free.";
    IMAGE_CHECKSUM_DIFFERS = Mismatch, "image_checksum_differs",
        meaning "After the update the device reports a checksum of its image that is not the checksum the image carries.",
        next "Do not trust the device's firmware. Follow the recovery route the operation declares, then read the device's version and checksum before anything else is written.";
    DEVICE_NOT_RETURNED = PossibleCommit, "device_not_returned",
        meaning "The update was committed and the device did not come back, or did not identify itself, within the restart window the map declares. Nothing is retried.",
        next "Alert a person and show the recovery route the operation declares; do not send the update again.";
    NO_CONFIRMATION = PossibleCommit, "no_confirmation",
        meaning "A write frame was sent and no clear acknowledgement came.",
        next "Reconcile: open the link, identify the device, and read the register before anything is written again.";
}

/// A failure of the code of `row`, in the class its table gives it.
pub(crate) fn failure(
    row: &CodeEntry,
    message: impl Into<String>,
    next_step: impl Into<String>,
) -> Failure {
    row.failure(message, next_step)
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
