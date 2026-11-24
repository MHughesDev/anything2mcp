//! The kernel's own file kinds: their names, where they live, and their
//! schemas (spec section 205 and the file format reference).

use std::collections::BTreeMap;
use std::sync::LazyLock;

use a2m_core::{
    ApproveWrites, BindingStatus, CaptureLevel, Checkpoint, DecimalMark, DescriptionBy, DriftRisk,
    EffectClass, EnvironmentKind, FieldBase, Hazard, IdentityStrategy, Rounding, Sensitivity,
    SignInMode, TestKind, WhitespaceRule,
};

use crate::schema::JsonSchema;
use crate::source::PartKind;

/// The owner of the kernel's file kinds and lint codes.
pub const KERNEL_OWNER: &str = "kernel";
/// The schema version this compiler reads.
pub const SCHEMA_VERSION: i64 = 1;

/// The file kind of `target.yaml`.
pub const TARGET: &str = "target";
/// The file kind of `auth.yaml`.
pub const AUTH: &str = "auth";
/// The file kind of `types.yaml`.
pub const TYPES: &str = "types";
/// The file kind of the files under `operations/`.
pub const OPERATION: &str = "operation";
/// The file kind of the files under `pipelines/`.
pub const PIPELINE: &str = "pipeline";
/// The file kind of the files under `sources/`.
pub const SOURCE: &str = "source";
/// The file kind of the files under `tests/`.
pub const TEST: &str = "test";
/// The file kind of everything under `fixtures/`.
pub const FIXTURE: &str = "fixture";

/// The name of the marker file in a fixture folder.
pub const FIXTURE_MARKER: &str = "fixture.yaml";

/// Every file kind the kernel owns.
pub const KERNEL_FILE_KINDS: &[&str] = &[
    TARGET, AUTH, TYPES, OPERATION, PIPELINE, SOURCE, TEST, FIXTURE,
];

/// The files the kernel owns at the top of a target folder, with the kind
/// of each.
pub const KERNEL_FILES: &[(&str, &str)] = &[
    ("target.yaml", TARGET),
    ("auth.yaml", AUTH),
    ("types.yaml", TYPES),
];

/// The folders the kernel owns, with the kind of each.
pub const KERNEL_FOLDER_KINDS: &[(&str, &str)] = &[
    ("operations", OPERATION),
    ("pipelines", PIPELINE),
    ("sources", SOURCE),
    ("tests", TEST),
    ("fixtures", FIXTURE),
];

/// The folders the kernel owns or reserves: `held` holds the drafts that
/// wait on an open question, and the loader does not read it (decision
/// 0298), so no pack may register a file kind in it.
pub const KERNEL_FOLDERS: &[&str] = &[
    "operations",
    "pipelines",
    "sources",
    "tests",
    "fixtures",
    "held",
];

/// The keys of a surface the kernel owns; the rest are its pack's.
pub const SURFACE_KEYS: &[&str] = &["kind", "same_session_as", "rate", "stands"];

/// The keys of a binding the kernel owns; the rest are its mode's.
pub const BINDING_KEYS: &[&str] = &["mode", "verb", "status", "precondition"];

/// The orders an operation may state for its bindings (decision 0337):
/// `listed` keeps the order of the file; stating none is by each mode's
/// rank in its provider's manifest.
pub const BINDING_ORDERS: &[&str] = &["listed"];

/// The name a success check reads: every record of the run was verified.
pub const ALL_RECORDS_VERIFIED: &str = "all_records_verified";

fn inputs() -> JsonSchema {
    JsonSchema::map_of(
        JsonSchema::object()
            .required("type", JsonSchema::field_type())
            .optional("description", JsonSchema::text())
            .optional("required", JsonSchema::bool())
            .optional("default", JsonSchema::scalar()),
    )
}

fn target() -> JsonSchema {
    let environment = JsonSchema::object()
        .required("kind", JsonSchema::words(EnvironmentKind::NAMES))
        .optional("surfaces", JsonSchema::map_of(JsonSchema::text()))
        .optional(
            "test_records",
            JsonSchema::list(JsonSchema::either(vec![
                JsonSchema::scalar(),
                JsonSchema::non_empty_list(JsonSchema::scalar()),
            ])),
        )
        .optional("approve_writes", JsonSchema::words(ApproveWrites::NAMES));
    JsonSchema::object()
        .required("schema_version", JsonSchema::int())
        .required("target", JsonSchema::name())
        .optional("title", JsonSchema::text())
        .required("surfaces", JsonSchema::map_of(JsonSchema::any()))
        .optional("environments", JsonSchema::map_of(environment))
        .optional("default_environment", JsonSchema::name())
        .optional(
            "environment",
            JsonSchema::object()
                .optional("locale", JsonSchema::text())
                .optional("timezone", JsonSchema::text()),
        )
        .optional("modules", JsonSchema::list(JsonSchema::name()))
        .optional(
            "defaults",
            JsonSchema::object()
                .optional(
                    "timeouts",
                    JsonSchema::map_of(JsonSchema::int_between(1, i64::from(u32::MAX))),
                )
                .optional(
                    "limits",
                    JsonSchema::object()
                        .optional(
                            "max_records",
                            JsonSchema::int_between(1, i64::from(u32::MAX)),
                        )
                        .optional("max_duration_ms", JsonSchema::int_between(1, i64::MAX)),
                )
                .optional("capture", JsonSchema::words(CaptureLevel::NAMES)),
        )
        .optional("exclusive_session", JsonSchema::bool())
        .optional("drift_risk", JsonSchema::words(DriftRisk::NAMES))
        .optional("authorization", JsonSchema::text())
}

/// The kernel's part of a surface.
pub(crate) fn surface() -> JsonSchema {
    JsonSchema::object()
        .required("kind", JsonSchema::name())
        .optional("same_session_as", JsonSchema::surface())
        .optional(
            "rate",
            JsonSchema::object()
                .required("requests", JsonSchema::int_between(1, i64::from(u32::MAX)))
                .required("per_ms", JsonSchema::int_between(1, i64::from(u32::MAX))),
        )
        .optional(
            "stands",
            JsonSchema::either(vec![
                JsonSchema::words(&["alone"]),
                JsonSchema::object().required("beneath", JsonSchema::surface()),
            ]),
        )
}

/// The kernel's part of a binding.
pub(crate) fn binding() -> JsonSchema {
    JsonSchema::object()
        .required("mode", JsonSchema::name())
        .optional("verb", JsonSchema::name())
        .optional("status", JsonSchema::words(BindingStatus::NAMES))
        .optional("precondition", JsonSchema::expression())
}

fn auth() -> JsonSchema {
    let entry = JsonSchema::object()
        .required(
            "surfaces",
            JsonSchema::non_empty_list(JsonSchema::surface()),
        )
        .required("mode", JsonSchema::words(SignInMode::NAMES))
        .required("provider", JsonSchema::name())
        .required("probe", JsonSchema::name())
        .optional("probe_inputs", JsonSchema::map_of(JsonSchema::scalar()))
        .optional(
            "expiry_signals",
            JsonSchema::list(JsonSchema::map_of(JsonSchema::any())),
        )
        .optional(
            "timeouts",
            JsonSchema::object()
                .optional("idle_ms", JsonSchema::int_between(1, i64::MAX))
                .optional("absolute_ms", JsonSchema::int_between(1, i64::MAX)),
        )
        .optional("sign_out", JsonSchema::name())
        .optional("settings", JsonSchema::map_of(JsonSchema::scalar()))
        .optional("credentials", JsonSchema::map_of(JsonSchema::text()))
        .optional("attempt_limit", JsonSchema::int_between(0, 10))
        .optional(
            "movable",
            JsonSchema::either(vec![
                JsonSchema::words(&["auto", "false"]),
                JsonSchema::bool(),
            ]),
        );
    JsonSchema::object().required("auth", JsonSchema::non_empty_list(entry))
}

fn types() -> JsonSchema {
    let field_type = JsonSchema::object()
        .required("base", JsonSchema::words(FieldBase::NAMES))
        .optional("unit", JsonSchema::text())
        .optional("display_unit", JsonSchema::text())
        .optional("step", JsonSchema::decimal())
        .optional("places", JsonSchema::int_between(0, 28))
        .optional("rounding", JsonSchema::words(Rounding::NAMES))
        .optional("currency", JsonSchema::text())
        .optional("format", JsonSchema::text())
        .optional("timezone", JsonSchema::text())
        .optional("case", JsonSchema::words(&["sensitive", "insensitive"]))
        .optional("whitespace", JsonSchema::words(WhitespaceRule::NAMES))
        .optional("decimal_mark", JsonSchema::words(DecimalMark::NAMES))
        .optional("true_text", JsonSchema::text())
        .optional("false_text", JsonSchema::text())
        .optional("codes", JsonSchema::map_of(JsonSchema::text()))
        .optional("sensitivity", JsonSchema::words(Sensitivity::NAMES))
        .optional("untrusted", JsonSchema::bool());
    JsonSchema::object().required("types", JsonSchema::map_of(field_type))
}

fn operation() -> JsonSchema {
    JsonSchema::object()
        .required("operation", JsonSchema::name())
        .optional("title", JsonSchema::text())
        .required("effect", JsonSchema::words(EffectClass::NAMES))
        .optional("hazard", JsonSchema::words(Hazard::NAMES))
        .optional(
            "identity",
            JsonSchema::object()
                .required("strategy", JsonSchema::words(IdentityStrategy::NAMES))
                .optional("inputs", JsonSchema::list(JsonSchema::name()))
                .optional("place", JsonSchema::words(&["fixed"])),
        )
        .optional(
            "key",
            JsonSchema::either(vec![
                JsonSchema::name(),
                JsonSchema::non_empty_list(JsonSchema::name()),
            ]),
        )
        .optional("commit_points", JsonSchema::int_between(0, 1000))
        .required("inputs", inputs())
        .required(
            "outputs",
            JsonSchema::map_of(
                JsonSchema::object()
                    .required("type", JsonSchema::field_type())
                    .optional("list", JsonSchema::bool())
                    .optional("may_be_absent", JsonSchema::bool()),
            ),
        )
        .required(
            "bindings",
            JsonSchema::non_empty_list(JsonSchema::map_of(JsonSchema::any())),
        )
        .required("description", JsonSchema::text())
        .optional("description_by", JsonSchema::words(DescriptionBy::NAMES))
        .optional("binding_order", JsonSchema::words(BINDING_ORDERS))
        .optional(
            "examples",
            JsonSchema::list(JsonSchema::map_of(JsonSchema::scalar())),
        )
        .optional(
            "permissions",
            JsonSchema::object()
                .optional("uploads", JsonSchema::list(JsonSchema::text()))
                .optional("downloads", JsonSchema::list(JsonSchema::text()))
                .optional("destinations", JsonSchema::list(JsonSchema::text())),
        )
        .optional(
            "decision_points",
            JsonSchema::list(
                JsonSchema::object()
                    .required("name", JsonSchema::name())
                    .required("question", JsonSchema::text()),
            ),
        )
}

fn pipeline() -> JsonSchema {
    let step = JsonSchema::object()
        .required("id", JsonSchema::name())
        .required("operation", JsonSchema::name())
        .optional("with", JsonSchema::map_of(JsonSchema::expression()))
        .optional("for_each", JsonSchema::expression())
        .optional("checkpoint", JsonSchema::words(Checkpoint::NAMES))
        .optional("when", JsonSchema::expression())
        .optional("expect", JsonSchema::expression());
    JsonSchema::object()
        .required("pipeline", JsonSchema::name())
        .optional("title", JsonSchema::text())
        .required("description", JsonSchema::text())
        .optional("description_by", JsonSchema::words(DescriptionBy::NAMES))
        .optional("effect", JsonSchema::words(EffectClass::NAMES))
        .optional("inputs", inputs())
        .optional(
            "source",
            JsonSchema::object().required("template", JsonSchema::name()),
        )
        .optional(
            "limits",
            JsonSchema::object()
                .optional(
                    "max_records",
                    JsonSchema::int_between(1, i64::from(u32::MAX)),
                )
                .optional("max_duration_ms", JsonSchema::int_between(1, i64::MAX)),
        )
        .required("steps", JsonSchema::non_empty_list(step))
        .required(
            "success",
            JsonSchema::non_empty_list(JsonSchema::expression()),
        )
        .optional(
            "headroom_ms",
            JsonSchema::int_between(0, i64::from(u32::MAX)),
        )
}

fn source() -> JsonSchema {
    let part = JsonSchema::object()
        .required("kind", JsonSchema::words(PartKind::NAMES))
        .required("place", JsonSchema::text())
        .required("value", JsonSchema::text());
    let version = JsonSchema::object()
        .required("version", JsonSchema::text())
        .required(
            "fingerprint",
            JsonSchema::object()
                .optional("digest", JsonSchema::text())
                .required("parts", JsonSchema::list(part)),
        );
    let field = JsonSchema::object()
        .required("type", JsonSchema::field_type())
        .optional("column", JsonSchema::text())
        .optional("expression", JsonSchema::expression())
        .optional("required", JsonSchema::bool());
    JsonSchema::object()
        .required("template", JsonSchema::name())
        .required("reader", JsonSchema::name())
        .optional("versions", JsonSchema::list(version))
        .optional(
            "locate",
            JsonSchema::either(vec![
                JsonSchema::words(&["document"]),
                JsonSchema::object().required("named", JsonSchema::text()),
                JsonSchema::object().required("sheet", JsonSchema::text()),
            ]),
        )
        .required("columns", JsonSchema::list(JsonSchema::text()))
        .required("fields", JsonSchema::map_of(field))
        .optional("key", JsonSchema::list(JsonSchema::name()))
        .optional("rows", JsonSchema::expression())
        .optional("missing", JsonSchema::list(JsonSchema::text()))
        .optional("hidden", JsonSchema::words(&["reject", "include"]))
        .optional(
            "options",
            JsonSchema::object().optional("delimiter", JsonSchema::text()),
        )
}

fn test() -> JsonSchema {
    JsonSchema::object()
        .required("test", JsonSchema::name())
        .optional("name", JsonSchema::name())
        .required("kind", JsonSchema::words(TestKind::NAMES))
        .optional("fixture", JsonSchema::name())
        .required("inputs", JsonSchema::map_of(JsonSchema::scalar()))
        .required("expect", JsonSchema::any())
        .optional("restore", JsonSchema::bool())
        .optional(
            "binding",
            JsonSchema::object()
                .required("mode", JsonSchema::text())
                .optional("position", JsonSchema::int_between(1, 64)),
        )
}

fn fixture() -> JsonSchema {
    JsonSchema::object()
        .required("scrubbed", JsonSchema::bool())
        .optional("note", JsonSchema::text())
}

static SCHEMAS: LazyLock<BTreeMap<&'static str, JsonSchema>> = LazyLock::new(|| {
    BTreeMap::from([
        (TARGET, target()),
        (AUTH, auth()),
        (TYPES, types()),
        (OPERATION, operation()),
        (PIPELINE, pipeline()),
        (SOURCE, source()),
        (TEST, test()),
        (FIXTURE, fixture()),
    ])
});

/// The schema of one of the kernel's file kinds.
#[must_use]
pub fn schema_of(kind: &str) -> Option<&'static JsonSchema> {
    SCHEMAS.get(kind)
}

/// The kernel's schemas as JSON Schema, by the name each is published under.
#[must_use]
pub fn schemas() -> BTreeMap<String, serde_json::Value> {
    let mut all: BTreeMap<String, serde_json::Value> = SCHEMAS
        .iter()
        .map(|(kind, schema)| (format!("{KERNEL_OWNER}.{kind}"), schema.to_json()))
        .collect();
    all.insert(format!("{KERNEL_OWNER}.surface"), surface().to_json());
    all.insert(format!("{KERNEL_OWNER}.binding"), binding().to_json());
    all
}
