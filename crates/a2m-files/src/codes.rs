//! The kernel's lint codes: every code this crate can report, with the rule
//! in one line. A pack's codes carry the pack's name in place of `kernel`.
//!
//! Below them, the failure codes this crate produces, each with its one
//! class, in the table form every crate uses.

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
    FILE_TOO_LARGE = "file_too_large": "A target file is at most 1 MiB, and a fixture's recorded file at most 16 MiB.",
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
    BUILT_IN_TYPE_REDEFINED = "built_in_type_redefined": "A target defines no field type of a complete built-in type's name.",
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
    PRECONDITION_INVALID = "precondition_invalid": "An input marked `precondition` belongs to an operation that writes and is not part of its key.",
    PLACE_NOT_FIXED = "place_not_fixed": "An operation whose record is one fixed place takes no identity input, and every binding names the same place, which its file kind declares fixed on the binding's surface.",
    WRITE_WITHOUT_READ_BACK = "write_without_read_back": "A write binding reads the record back.",
    ACKNOWLEDGED_WHERE_READ_BACK = "acknowledged_where_read_back": "A binding that reads its record back by its key is verified by that read, never ended on an acknowledgement.",
    COMMIT_POINTS_DIFFER = "commit_points_differ": "The steps that commit in each write binding number what the operation declares.",
    EFFECT_LOWER_THAN_DERIVED = "effect_lower_than_derived": "A declared effect is never lower than the derived one.",
    READ_ACTS = "read_acts": "An operation whose effect is `read` holds no step of a kind its pack states can change the system.",
    STEP_CYCLE = "step_cycle": "A pipeline step uses only the results of steps before it.",
    SURFACE_NOT_DECLARED = "surface_not_declared": "Nothing names a surface the target does not declare.",
    SCOPE_INVALID = "scope_invalid": "A permission names one exact address or one name, once: no wildcard, no range of addresses, no path, and no user information.",
    URL_WITHOUT_DESTINATION = "url_without_destination": "An operation with an input of the type `url` declares at least one destination with a scheme in `permissions.destinations`, and a pipeline with one calls an operation that does: such an input accepts only those destinations.",
    SURFACE_OF_ANOTHER_PACK = "surface_of_another_pack": "A lowering names a surface of a kind its own pack registered.",
    // Stage 7: lint.
    ACKNOWLEDGED_WITH_LOOKUP = "acknowledged_with_lookup": "A keyed binding that ends on an acknowledgement, with no read-back, declares no lookup that stands for a read-back; it is always a warning.",
    FOR_EACH_WITHOUT_CHECKPOINT = "for_each_without_checkpoint": "A `for_each` over a write has a checkpoint.",
    WRITE_PIPELINE_WITHOUT_LIMIT = "write_pipeline_without_limit": "A pipeline that writes declares a record limit.",
    CREDENTIAL_IN_FILE = "credential_in_file": "No file holds a credential; credentials are vault references.",
    CREDENTIAL_SUSPECTED = "credential_suspected": "A value under a key that names a token, a cookie, an authorization, or a PIN is a name, an expression, or a vault reference.",
    PROBE_INVALID = "probe_invalid": "A sign-in's probe is a read that can be made as the file gives it, and no surface is signed in to twice.",
    PROBE_PENDING = "probe_pending": "A sign-in's probe names a read that is drafted; in a draft, a probe nothing defines yet is a warning, and the sign-in is marked as not confirmed.",
    OPERATION_WITHOUT_TEST = "operation_without_test": "Every operation and pipeline has an offline test.",
    NO_BINDING_MAY_ACT = "no_binding_may_act": "An operation has a binding that is not `draft` or `retired`, so that a call of it can run once published; it is always a warning.",
    FIXTURE_NOT_SCRUBBED = "fixture_not_scrubbed": "A fixture a test names exists and carries the scrubbed marker.",
    TEST_BINDING_UNKNOWN = "test_binding_unknown": "A test that names the binding it tests names one binding of its operation, by mode and, where two share the mode, by position, and not a retired one.",
    TOOL_NAME_SCHEME = "tool_name_scheme": "A tool name starts with the target's name, then its module where the target has modules.",
    TOOL_NAMES_TOO_ALIKE = "tool_names_too_alike": "No two tools differ only by a suffix.",
    DESCRIPTION_WEAK = "description_weak": "A description says what the tool does, in at least twenty characters.",
    DESCRIPTION_UNBOUNDED = "description_unbounded": "A description is plain text of at most 1,024 characters, with no control character.",
    FIELD_TEXT_UNBOUNDED = "field_text_unbounded": "A field description, an enum label, or a text default of a tool's definition is plain text of at most 1,024 characters, with no control, bidirectional, or invisible character but a newline and a tab.",
    DEFINITION_UNBOUNDED = "definition_unbounded": "A tool's definition holds at most 8,192 characters of free text in all.",
    DESCRIPTION_INSTRUCTS_THE_READER = "description_instructs_the_reader": "A tool's description and its inputs' descriptions hold none of the phrases or markers of an instruction to the model that reads them, which the module `addressed` lists; it is always a warning.",
    DESCRIPTION_NAMES_AN_UNPUBLISHED_TOOL = "description_names_an_unpublished_tool": "A tool's description and its inputs' descriptions name no tool this version does not publish, by the forms the module `addressed` lists; it is always a warning.",
    DESCRIPTION_NAMES_A_PLACE_OUTSIDE = "description_names_a_place_outside": "A tool's description and its inputs' descriptions name no address the target does not reach and no file of the machine, by the forms the module `addressed` lists; it is always a warning.",
    DESCRIPTION_ASKS_TO_HIDE = "description_asks_to_hide": "A tool's description and its inputs' descriptions hold none of the phrases of a request to keep something from the person, which the module `addressed` lists; it is always a warning.",
    OUTPUT_NOT_MARKED = "output_not_marked": "Free text that came from the system is of a field type marked untrusted.",
    // Stage 8: describe.
    TOOL_INDISTINGUISHABLE = "tool_indistinguishable": "No two tools have the same description and the same inputs.",
    // Stage 9: emit.
    COMPILED_TARGET_NOT_ENCODABLE = "compiled_target_not_encodable": "The compiled target holds together and encodes.",
    DRAFT_UNDER_RELEASE_VERSION = "draft_under_release_version": "A draft is compiled under the draft version, so that it never has a published compiled target's bytes.",
}

/// The lint code for one of the constants above, each of which is checked
/// when the crate is built.
#[must_use]
pub(crate) const fn kernel(code: &'static str) -> LintCode {
    LintCode::fixed(code)
}

a2m_core::failure_codes! {
    LINT_FAILED = Input, "lint_failed",
        meaning "A target's files have an error diagnostic, so they do not compile.",
        next "Correct the file at the place the first diagnostic names, as its fix says, and compile again.";
    PATH_NOT_ALLOWED = Input, "path_not_allowed",
        meaning "A path or a link leaves the folder it must stay inside.",
        next "Name the file by its path inside the target folder: names joined by `/`, with no `..`, no drive, and no leading `/`.";
    DRAFT_CONFLICT = Input, "draft_conflict",
        meaning "The drafter would overwrite a file the developer changed.",
        next "Keep the file as it is, or remove it and draft again.";
    REGISTRATION_CONFLICT = Input, "registration_conflict",
        meaning "Two registrations for the same key, or one that is not the registering pack's to make.",
        next "Disable one of the two providers, or correct the provider that registers a name that is not its own.";
    TARGET_UNREADABLE = Input, "target_unreadable",
        meaning "The target folder could not be read, or a draft could not be written into it.",
        next "Make the target folder exist and be readable and writable by this user, then try again.";
    REPLY_CHANGED = Drift, "reply_changed",
        meaning "A call's reply no longer has the shape the target's files describe.",
        next "Explore the call again, update its file to the reply the system now gives, and run the tests again.";
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
