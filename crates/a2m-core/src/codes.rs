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
    COMPILED_TARGET_HASH_MISMATCH = Input, "compiled_target_hash_mismatch",
        meaning "A compiled target's bytes do not match the hash it was published with.",
        next "Compile and publish the target's version again, so that a new compiled target is stored; the tool stays unavailable until then.";
    COMPILED_TARGET_FORMAT_UNSUPPORTED = Input, "compiled_target_format_unsupported",
        meaning "A compiled target's format is not one this engine loads.",
        next "Compile the target again with this engine, or run the engine the compiled target was compiled for.";
    COMPILED_TARGET_UNREADABLE = Input, "compiled_target_unreadable",
        meaning "A compiled target's bytes match its hash and are not a well-formed compiled target.",
        next "Compile the target again with this engine and publish the new version.";
    PAYLOAD_UNREADABLE = Input, "payload_unreadable",
        meaning "A payload in a compiled target does not decode as the type its reader expects.",
        next "Compile the target again with the pack versions this engine has, and publish the new version.";
    PAYLOAD_NOT_ENCODABLE = Input, "payload_not_encodable",
        meaning "Data holds something the compiled target's encoding cannot carry, such as a float.",
        next "Give the value as a decimal, a text, or a whole number of at most 64 bits, and keys as text.";
}
