//! The failure codes this crate produces, each with its one class. The
//! same constants are on [`FailureCode`](crate::FailureCode) as associated
//! constants, such as `FailureCode::SCHEMA_INVALID`.

crate::failure_codes! {
    SCHEMA_INVALID = Input, "schema_invalid",
        meaning "A file, an input, or a name does not have the shape its schema requires.",
        next "Correct the field or the name the message points to, in the shape its schema gives, and try again.";
    FINDING_INVALID = Input, "finding_invalid",
        meaning "A finding claims more than its source, its parents, or its content allow.",
        next "Make the finding at the level its source supports, with the parents it names, and record it again.";
    VALUE_OUT_OF_RANGE = Input, "value_out_of_range",
        meaning "A decimal, a date, a key part, or a moment cannot be represented.",
        next "Give a value inside the range the message names.";
    PLAN_HASH_MISMATCH = Input, "plan_hash_mismatch",
        meaning "A plan's bytes do not match the hash it was published with.",
        next "Compile and publish the target's version again, so that a new plan is stored; the tool stays unavailable until then.";
    PLAN_FORMAT_UNSUPPORTED = Input, "plan_format_unsupported",
        meaning "A plan's format is not one this engine loads.",
        next "Compile the target again with this engine, or run the engine the plan was compiled for.";
    PLAN_UNREADABLE = Input, "plan_unreadable",
        meaning "A plan's bytes match its hash and are not a well-formed plan.",
        next "Compile the target again with this engine and publish the new version.";
    PAYLOAD_UNREADABLE = Input, "payload_unreadable",
        meaning "A payload in a plan does not decode as the type its reader expects.",
        next "Compile the target again with the pack versions this engine has, and publish the new version.";
    PAYLOAD_NOT_ENCODABLE = Input, "payload_not_encodable",
        meaning "Data holds something the plan's encoding cannot carry, such as a float.",
        next "Give the value as a decimal, a text, or a whole number of at most 64 bits, and keys as text.";
}
