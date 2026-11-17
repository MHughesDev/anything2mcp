//! The kernel's lint codes: every code this crate can report, with the rule
//! in one line. A pack's codes carry the pack's name in place of `kernel`.

use a2m_core::LintCode;

macro_rules! kernel_codes {
    ($( $(#[$meta:meta])* $name:ident = $rule:literal : $summary:literal ),+ $(,)?) => {
        $(
            $(#[$meta])*
            #[doc = $summary]
            pub const $name: &str = concat!("kernel.", $rule);
        )+

        /// Every kernel code, with what its rule requires.
        pub const KERNEL_CODES: &[(&str, &str)] = &[ $( ($name, $summary) ),+ ];

        /// Every code above as a lint code. A badly formed one fails the
        /// build here, so `kernel` cannot fail when the crate runs.
        const _: &[LintCode] = &[ $( LintCode::fixed($name) ),+ ];
    };
}

kernel_codes! {
    // Stage 1: parse.
    YAML_MALFORMED = "yaml_malformed": "A file is well-formed YAML in UTF-8.",
    DUPLICATE_KEY = "duplicate_key": "A map names each key once.",
    YAML_FEATURE_REFUSED = "yaml_feature_refused": "Target files use no anchors, aliases, or tags.",
    // Loading.
    PATH_OUTSIDE_TARGET = "path_outside_target": "Nothing is read outside the target folder.",
    UNKNOWN_FILE = "unknown_file": "Every file belongs to a file kind somebody registered.",
    FILE_MISSING = "file_missing": "A target has a `target.yaml`.",
    // Stage 2: validate.
    UNKNOWN_KEY = "unknown_key": "A map holds only the keys its schema lists.",
    MISSING_FIELD = "missing_field": "A required field is given.",
    WRONG_TYPE = "wrong_type": "A value has the type its schema states.",
    UNKNOWN_VALUE = "unknown_value": "A value is one of the words its schema lists.",
    NAME_MALFORMED = "name_malformed": "A name is lowercase letters, digits, and underscores.",
    SCHEMA_VERSION_UNSUPPORTED = "schema_version_unsupported": "The files state a schema version this compiler reads.",
    OPEN_QUESTION = "open_question": "No open question from the drafter is left in a file.",
    UNKNOWN_SURFACE_KIND = "unknown_surface_kind": "A surface's kind is one an enabled pack registered.",
    UNKNOWN_CONTROL = "unknown_control": "A control is one an enabled pack registered.",
    PROGRAM_INVALID = "program_invalid": "A program a surface declares is named by an absolute path, and its digest is a digest.",
    // Stage 3: resolve.
    UNKNOWN_NAME = "unknown_name": "A name that is used is defined somewhere.",
    NAME_DEFINED_TWICE = "name_defined_twice": "A name is defined once.",
    UNKNOWN_FIELD_TYPE = "unknown_field_type": "A field type is built in or defined in `types.yaml`.",
    FIELD_TYPE_INCOMPLETE = "field_type_incomplete": "A field type states everything its base needs to normalize a value.",
    VALUE_NOT_OF_TYPE = "value_not_of_type": "A default or a test input is a value of its input's type.",
    // Stage 4: type-check.
    EXPRESSION_INVALID = "expression_invalid": "An expression parses and reads only names its place declares.",
    EXPRESSION_TYPE_ERROR = "expression_type_error": "An expression's types fit, and its result's type is known.",
    // Stage 5: lower.
    UNKNOWN_MODE = "unknown_mode": "A binding's mode is one an enabled pack registered.",
    UNKNOWN_VERB = "unknown_verb": "A binding's verb has a lowering registered for its mode.",
    UNKNOWN_STEP_KIND = "unknown_step_kind": "A lowered step's kind is one its lowering declared.",
    READ_KIND_NOT_REGISTERED = "read_kind_not_registered": "A file kind a lowering reads is owned by an enabled pack.",
    LOWERING_INVALID = "lowering_invalid": "What a lowering returns names a surface of the target and steps that hold together.",
    // Stage 6: analyze.
    WRITE_WITHOUT_IDENTITY = "write_without_identity": "A write operation declares how its record is found again.",
    IDENTITY_NOT_POSSIBLE = "identity_not_possible": "An operation's identity strategy is possible in each of its bindings.",
    IDENTITY_INVALID = "identity_invalid": "An identity names inputs of the operation, and fits the operation's effect.",
    WRITE_WITHOUT_READ_BACK = "write_without_read_back": "A write binding reads the record back.",
    COMMIT_POINTS_DIFFER = "commit_points_differ": "The steps that commit in each write binding number what the operation declares.",
    EFFECT_LOWER_THAN_DERIVED = "effect_lower_than_derived": "A declared effect is never lower than the derived one.",
    STEP_CYCLE = "step_cycle": "A pipeline step uses only the results of steps before it.",
    SURFACE_NOT_DECLARED = "surface_not_declared": "Nothing names a surface the target does not declare.",
    // Stage 7: lint.
    FOR_EACH_WITHOUT_CHECKPOINT = "for_each_without_checkpoint": "A `for_each` over a write has a checkpoint.",
    WRITE_PIPELINE_WITHOUT_LIMIT = "write_pipeline_without_limit": "A pipeline that writes declares a record limit.",
    CREDENTIAL_IN_FILE = "credential_in_file": "No file holds a credential; credentials are vault references.",
    PROBE_INVALID = "probe_invalid": "A sign-in's probe is a read that can be made as the file gives it, and no surface is signed in to twice.",
    OPERATION_WITHOUT_TEST = "operation_without_test": "Every operation and pipeline has an offline test.",
    FIXTURE_NOT_SCRUBBED = "fixture_not_scrubbed": "A fixture a test names exists and carries the scrubbed marker.",
    TOOL_NAME_SCHEME = "tool_name_scheme": "A tool name starts with the target's name, then its module where the target has modules.",
    TOOL_NAMES_TOO_ALIKE = "tool_names_too_alike": "No two tools differ only by a suffix.",
    DESCRIPTION_WEAK = "description_weak": "A description says what the tool does, within the length limits.",
    OUTPUT_NOT_MARKED = "output_not_marked": "Free text that came from the system is of a field type marked untrusted.",
    // Stage 8: describe.
    TOOL_INDISTINGUISHABLE = "tool_indistinguishable": "No two tools have the same description and the same inputs.",
    // Stage 9: emit.
    PLAN_NOT_ENCODABLE = "plan_not_encodable": "The plan holds together and encodes.",
}

/// The lint code for one of the constants above, each of which is checked
/// when the crate is built.
#[must_use]
pub(crate) const fn kernel(code: &'static str) -> LintCode {
    LintCode::fixed(code)
}

#[cfg(test)]
mod tests {
    use super::{KERNEL_CODES, kernel};

    #[test]
    fn every_kernel_code_is_a_well_formed_lint_code_and_is_listed_once() {
        let mut seen = std::collections::BTreeSet::new();
        for (code, summary) in KERNEL_CODES {
            let made = kernel(code);
            assert_eq!(made.owner(), "kernel");
            assert!(seen.insert(*code), "{code} is listed twice");
            assert!(summary.ends_with('.'), "{code}");
        }
    }
}
