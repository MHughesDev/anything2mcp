//! The failure codes this crate declares. Every one is of class `input`: the
//! input was refused, and nothing of it was accepted. The crate also
//! produces two codes `a2m-core` declares, `schema_invalid` and
//! `value_out_of_range`, named in `crate::error`.

a2m_core::failure_codes! {
    SOURCE_TEMPLATE_UNKNOWN = Input, "source_template_unknown",
        meaning "The input's structure matches no template version the definition knows.",
        next "Give a file made from a known version of the template, or add this layout to the source definition as a new version.";
    FILE_TYPE_MISMATCH = Input, "file_type_mismatch",
        meaning "The content is not the format its reader reads.",
        next "Give a file of the format the source definition names.";
    FORMULA_WITHOUT_VALUE = Input, "formula_without_value",
        meaning "A formula cell in the data has no cached value.",
        next "Open the workbook in the program that made it, let it calculate, save it, and give it again.";
    SOURCE_TOO_LARGE = Input, "source_too_large",
        meaning "The bytes, the rows, the cells, or one cell exceed the caller's limits.",
        next "Give a smaller input, or have the limits raised by whoever sets them.";
    SOURCE_MALFORMED = Input, "source_malformed",
        meaning "The content claims its format and breaks the format's rules.",
        next "Save the file again from the program that made it, or correct it, and try again.";
    SOURCE_ENCODING_UNSUPPORTED = Input, "source_encoding_unsupported",
        meaning "The text is in an encoding the readers do not read.",
        next "Save the file as UTF-8 and try again.";
    SOURCE_PROTECTED = Input, "source_protected",
        meaning "The workbook is encrypted, so it cannot be read without its password.",
        next "Save a copy without the password and give that copy.";
    SOURCE_READER_UNKNOWN = Input, "source_reader_unknown",
        meaning "The definition names a reader that is not registered.",
        next "Use a reader the engine has, or enable the plug-in that brings it.";
    SOURCE_DEFINITION_INVALID = Input, "source_definition_invalid",
        meaning "The compiled source definition does not hold together.",
        next "Compile the target again with this engine.";
    KEY_NOT_UNIQUE = Input, "source_key_not_unique",
        meaning "Two rows have the same key.",
        next "Correct the rows the problems name so that each key appears once, or key the source on fields that tell its rows apart, and give the input again.";
}

/// The next step this crate's table gives for `code`, for a failure whose
/// case adds nothing to it.
pub(crate) fn next_for(code: &a2m_core::FailureCode) -> &'static str {
    TABLE
        .iter()
        .find(|entry| entry.code == code.as_str())
        .map_or("Correct the input and give it again.", |entry| entry.next)
}
