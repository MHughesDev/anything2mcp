//! The failure codes this crate produces. Every one is of class `input`: an
//! expression, or a value it was given, was refused, and nothing was
//! written.

a2m_core::failure_codes! {
    EXPRESSION_INVALID = Input, "expression_invalid",
        meaning "The text does not parse, or names something the environment does not declare.",
        next "Correct the expression at the place the message names, using only names the place declares, and compile again.";
    EXPRESSION_TYPE_ERROR = Input, "expression_type_error",
        meaning "A type does not fit, including any use of a float.",
        next "Give each operator and function values of the types it takes; write a decimal as decimal(\"1.5\"). Then compile again.";
    VALUE_OUT_OF_RANGE = Input, "value_out_of_range",
        meaning "A host function was given a value it cannot represent.",
        next "Give a value the field type can hold, and run again.";
    VALUE_UNPARSABLE = Input, "value_unparsable",
        meaning "A parse function met text it cannot read.",
        next "Correct the value, or the field type's format, and run again.";
    VALUE_ABSENT = Input, "value_absent",
        meaning "A value was absent where the expression needs one.",
        next "Give the value, or a default with coalesce, and run again.";
    EXPRESSION_COST_EXCEEDED = Input, "expression_cost_exceeded",
        meaning "Evaluation passed its cost limit.",
        next "Give the expression less to work over, such as fewer rows, or simplify it.";
    COMPILED_EXPRESSION_UNLOADABLE = Input, "compiled_expression_unloadable",
        meaning "A compiled target's expression names a library this engine does not have, no longer checks as the type it states, or was run without a name it reads.",
        next "Compile the target again with this engine.";
}

/// The next step this crate's table gives for `code`, for a failure whose
/// case adds nothing to it.
pub(crate) fn next_for(code: &a2m_core::FailureCode) -> &'static str {
    TABLE
        .iter()
        .find(|entry| entry.code == code.as_str())
        .map_or("Correct the expression and compile again.", |entry| {
            entry.next
        })
}
