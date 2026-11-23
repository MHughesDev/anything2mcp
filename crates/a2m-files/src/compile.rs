//! The compiler: located documents to a checked, hashed plan (spec section
//! 206). Stage 1, parsing, is the loader's; this module runs stages 2 to 9
//! and reports every problem it can find in one pass.

use std::collections::{BTreeMap, BTreeSet};

use a2m_core::{
    AddressOverride, ApproveWrites, AuthPlan, AuthProviderName, AuthSetting, BindingPlan,
    BindingStatus, CanonicalJson, CaptureLevel, Checkpoint, CredentialRef, CredentialUse, Date,
    Decimal, DecisionPoint, DeclaredProgram, Digest, DriftRisk, EffectClass, EncodedPlan,
    Environment, EnvironmentIndex, EnvironmentKind, EnvironmentPlan, FieldOf, FieldTypeIndex,
    FileKindName, Hazard, IdentityPlan, IdentityStrategy, InputIndex, InputPlan, LintCode, Millis,
    Mode, OperationIndex, OperationName, OperationPlan, OutputPlan, PLAN_FORMAT, PackPayload,
    PackRef, PackSection, PermissionManifest, PipelineIndex, PipelineLimits, PipelinePlan,
    PipelineStep, Plan, PlanHeader, ProbePlan, RatePlan, RecordKey, SectionIndex, SignInMode,
    SourcePlan, Stands, Step, StepArg, SurfaceIndex, SurfaceKind, SurfaceName, SurfacePlan,
    TargetName, TargetPlan, TestKind, TestPlan, ToolAnnotations, ToolIndex, ToolPlan, ToolRuns,
    Value, ValueType, Values, Verb, Version, WriteEvidence,
};
use a2m_expr::{TypeEnv, names};

use crate::codes::{self, kernel};
use crate::diag::{Diagnostic, Diagnostics, Level, Stage};
use crate::doc::{Doc, Entry, Located, Place, RelPath};
use crate::kernel::{
    self as k, ALL_RECORDS_VERIFIED, BINDING_KEYS, FIXTURE_MARKER, SCHEMA_VERSION, SURFACE_KEYS,
};
use crate::load::{SourceFile, Sources};
use crate::register::{
    CompileMode, CompilerSet, Lowered, Lowering, Possible, StepCommits, SurfaceKindSpec,
};
use crate::review::{OpReview, PlaceReview, ReviewSummary, review_of};
use crate::schema::{JsonSchema, Reference, fix_for_unknown, shown};
use crate::source::{
    Fingerprint, FingerprintPart, HiddenRule, Locate, OPERATION_READER, ReaderOptions, RowField,
    SourceSpec, TemplateVersion,
};
use crate::types;
use crate::view::{
    InputInfo, LowerContext, Model, OpInfo, OperationView, ResolvedBinding, ResolvedView,
    SurfaceInfo, check_expr, check_site, current_type, env_type, expr_text, input_type,
    record_type, type_env, unknown_env,
};

/// The version of this compiler, which a plan's header names.
pub const COMPILER_VERSION: Version = Version::new(0, 0, 1);
/// The oldest engine that may run a plan this compiler makes.
pub const MINIMUM_ENGINE: Version = Version::new(0, 0, 1);
/// The version a draft is compiled as.
pub const DRAFT_VERSION: Version = Version::new(0, 0, 0);

/// A compiled target.
#[derive(Debug, Clone)]
pub struct Compiled {
    /// The plan.
    pub plan: Plan,
    /// The plan's bytes and hash, from `a2m-core`'s one encoding.
    pub encoded: EncodedPlan,
    /// What the developer should look at; nothing here stopped the compile.
    pub warnings: Diagnostics,
    /// What each tool may do, ordered by risk.
    pub review: ReviewSummary,
    /// For each test of the plan, in the plan's order: the place, from 0,
    /// of the binding of its operation the test names with `binding:`, or
    /// `None` where it names none and tests the binding that would act
    /// live. The plan's test does not carry it; the test file does, and
    /// its digest is among the plan's sources.
    pub test_bindings: Vec<Option<usize>>,
}

/// Compiles a target's files into a plan.
///
/// `version` is the version the plan is published as; a draft passes
/// [`DRAFT_VERSION`]. The same sources, set, mode, and version give the same
/// bytes on every machine.
///
/// # Errors
///
/// Every problem stages 2 to 9 found, errors and warnings together, each
/// with a code, a place, and a fix.
pub fn compile(
    sources: &Sources,
    set: &CompilerSet,
    mode: CompileMode,
    version: Version,
) -> Result<Compiled, Diagnostics> {
    let mut compiler = Compiler {
        sources,
        set,
        mode,
        problems: Vec::new(),
        model: Model::default(),
        environments: Vec::new(),
        about: None,
        default_limits: PipelineLimits::default(),
        auth: Vec::new(),
        operations: Vec::new(),
        op_plans: Vec::new(),
        pipelines: Vec::new(),
        source_plans: BTreeMap::new(),
        tools: Vec::new(),
        tests: Vec::new(),
        test_bindings: Vec::new(),
        tool_names: BTreeMap::new(),
        reviews: BTreeMap::new(),
    };
    compiler.run(version)
}

/// Checks a target after one file was edited, for an agent's `draft.lint`:
/// the edited file takes the place of the one at the same path, or is added,
/// and the whole target is compiled as a draft.
///
/// Every stage runs again; nothing is reused yet.
#[must_use]
pub fn lint_one(previous: &Sources, edited: SourceFile, set: &CompilerSet) -> Diagnostics {
    let mut files: Vec<SourceFile> = previous
        .files
        .iter()
        .filter(|file| file.path != edited.path)
        .cloned()
        .collect();
    files.push(edited);
    let sources = Sources::from_files(previous.root.clone(), files);
    match compile(&sources, set, CompileMode::Draft, DRAFT_VERSION) {
        Ok(compiled) => compiled.warnings,
        Err(problems) => problems,
    }
}

struct PipelineInfo {
    plan: PipelinePlan,
    title: String,
    description: String,
    at: Place,
}

struct Compiler<'a> {
    sources: &'a Sources,
    set: &'a CompilerSet,
    mode: CompileMode,
    problems: Vec<Diagnostic>,
    model: Model,
    environments: Vec<EnvironmentPlan>,
    /// What `target.yaml` states for the target as a whole.
    about: Option<TargetPlan>,
    /// `defaults.limits`: what a pipeline that states no limit takes.
    default_limits: PipelineLimits,
    auth: Vec<AuthPlan>,
    operations: Vec<OpInfo>,
    op_plans: Vec<(OperationPlan, String, String)>,
    pipelines: Vec<PipelineInfo>,
    source_plans: BTreeMap<String, (SourcePlan, ValueType)>,
    tools: Vec<ToolPlan>,
    tests: Vec<TestPlan>,
    test_bindings: Vec<Option<usize>>,
    tool_names: BTreeMap<String, Place>,
    /// For the review summary: what each operation's packs stated, and its
    /// fixed place.
    reviews: BTreeMap<OperationName, OpReview>,
}

fn index(position: usize) -> u32 {
    u32::try_from(position).unwrap_or(u32::MAX)
}

/// The digest a file states for a program: 64 lowercase hexadecimal
/// characters, with or without `sha256:` before them.
fn digest_of(text: &str) -> Option<Digest> {
    text.strip_prefix("sha256:").unwrap_or(text).parse().ok()
}

fn to_u32(number: i64) -> u32 {
    u32::try_from(number).unwrap_or(u32::MAX)
}

/// A map made of some of another map's entries, at the same place, so that
/// each owner's schema checks only its own keys.
fn part_of(doc: &Located<Doc>, keep: impl Fn(&str) -> bool) -> Located<Doc> {
    let entries: Vec<Entry> = doc
        .entries()
        .iter()
        .filter(|entry| keep(&entry.key.value))
        .cloned()
        .collect();
    Located::new(Doc::Map(entries), doc.at.clone())
}

fn is_simple_name(text: &str) -> bool {
    crate::schema::is_dotted_name(text) && !text.contains('.')
}

/// A value of `ty` written in a file, when the document is one.
fn value_of(doc: &Located<Doc>, ty: &ValueType) -> Option<Value> {
    match (ty, &doc.value) {
        (ValueType::Text | ValueType::Choice, Doc::Text(text)) => Some(Value::Text(text.clone())),
        (ValueType::Int, Doc::Int(number)) => Some(Value::Int(*number)),
        (ValueType::Bool, Doc::Bool(flag)) => Some(Value::Bool(*flag)),
        (ValueType::Decimal, Doc::Int(number)) => Some(Value::Decimal(Decimal::from_i64(*number))),
        (ValueType::Decimal, Doc::Text(text)) => Decimal::parse(text).ok().map(Value::Decimal),
        (ValueType::Date, Doc::Text(text)) => text.parse::<Date>().ok().map(Value::Date),
        (ValueType::Duration, Doc::Int(number)) => Some(Value::Duration(Millis::new(*number))),
        _ => None,
    }
}

/// A plain value as a file wrote it, with no field type to read it by: text
/// is text, and a whole number is a whole number.
fn written_value(doc: &Located<Doc>) -> Option<Value> {
    match &doc.value {
        Doc::Text(text) => Some(Value::Text(text.clone())),
        Doc::Int(number) => Some(Value::Int(*number)),
        Doc::Bool(flag) => Some(Value::Bool(*flag)),
        _ => None,
    }
}

/// A plain value as the text of a setting.
fn written_text(doc: &Located<Doc>) -> Option<String> {
    match &doc.value {
        Doc::Text(text) => Some(text.clone()),
        Doc::Int(number) => Some(number.to_string()),
        Doc::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// The JSON Schema of a value type, as a tool's caller sees it. A decimal
/// travels as text, so no caller ever sends a float.
fn json_schema_of(ty: &ValueType, codes: &[a2m_core::CodeLabel]) -> serde_json::Value {
    use serde_json::json;
    match ty {
        ValueType::Null => json!({"type": "null"}),
        ValueType::Bool => json!({"type": "boolean"}),
        ValueType::Int => json!({"type": "integer"}),
        ValueType::Text => json!({"type": "string"}),
        ValueType::Decimal => json!({"type": "string", "pattern": "^-?[0-9]+(\\.[0-9]+)?$"}),
        ValueType::Instant => json!({"type": "string", "format": "date-time"}),
        ValueType::Date => json!({"type": "string", "format": "date"}),
        ValueType::Duration => json!({"type": "integer", "description": "milliseconds"}),
        ValueType::Choice => {
            if codes.is_empty() {
                json!({"type": "string"})
            } else {
                json!({"enum": codes.iter().map(|pair| pair.code.as_str()).collect::<Vec<_>>()})
            }
        }
        ValueType::List(item) => json!({"type": "array", "items": json_schema_of(item, &[])}),
        ValueType::Record(fields) => {
            let properties: serde_json::Map<String, serde_json::Value> = fields
                .iter()
                .map(|field| (field.name.clone(), json_schema_of(&field.ty, &[])))
                .collect();
            json!({"type": "object", "properties": properties})
        }
    }
}

/// The JSON Schema of a value a tool returns. A number, a date, a moment,
/// or a length of time that holds a registered secret reaches the caller as
/// the text `[redacted]` (decision 0153), so where one is declared that text
/// is allowed too, and a caller that checks a result against its schema
/// never fails a run for it (decision 0161).
fn output_schema_of(ty: &ValueType, codes: &[a2m_core::CodeLabel]) -> serde_json::Value {
    use serde_json::json;
    match ty {
        ValueType::Int
        | ValueType::Decimal
        | ValueType::Instant
        | ValueType::Date
        | ValueType::Duration => json!({
            "anyOf": [json_schema_of(ty, codes), {"const": a2m_core::REDACTED}]
        }),
        ValueType::List(item) => json!({"type": "array", "items": output_schema_of(item, &[])}),
        ValueType::Record(fields) => {
            let properties: serde_json::Map<String, serde_json::Value> = fields
                .iter()
                .map(|field| (field.name.clone(), output_schema_of(&field.ty, &[])))
                .collect();
            json!({"type": "object", "properties": properties})
        }
        _ => json_schema_of(ty, codes),
    }
}

impl<'a> Compiler<'a> {
    fn error(
        &mut self,
        code: &'static str,
        stage: Stage,
        at: &Place,
        message: String,
        fix: String,
    ) {
        self.problems
            .push(Diagnostic::error(kernel(code), stage, at, message, fix));
    }

    /// Checks a document against a schema; true when it fits. References to
    /// controls, field types, and surfaces are resolved here.
    fn fits(&mut self, schema: &JsonSchema, doc: &Located<Doc>) -> bool {
        let checked = schema.check(doc);
        let mut fine = checked.diagnostics.is_empty();
        self.problems.extend(checked.diagnostics);
        for (reference, name) in checked.references {
            fine &= self.resolve(reference, &name);
        }
        fine
    }

    fn resolve(&mut self, reference: Reference, name: &Located<String>) -> bool {
        match reference {
            Reference::Control => {
                let known = self
                    .set
                    .controls
                    .keys()
                    .any(|control| control.as_str() == name.value);
                if !known {
                    let controls: Vec<&str> = self
                        .set
                        .controls
                        .keys()
                        .map(a2m_core::ControlName::as_str)
                        .collect();
                    self.error(
                        codes::UNKNOWN_CONTROL,
                        Stage::Validate,
                        &name.at,
                        format!(
                            "No enabled pack registered a control named `{}`.",
                            shown(&name.value)
                        ),
                        fix_for_unknown(&name.value, &controls),
                    );
                }
                known
            }
            Reference::FieldType => self.field_type(name).is_some(),
            Reference::Surface => {
                let known = self.model.surface(&name.value).is_some();
                if !known {
                    let surfaces: Vec<&str> = self
                        .model
                        .surfaces
                        .iter()
                        .map(|surface| surface.plan.name.as_str())
                        .collect();
                    self.error(
                        codes::SURFACE_NOT_DECLARED,
                        Stage::Resolve,
                        &name.at,
                        format!(
                            "The target declares no surface named `{}`.",
                            shown(&name.value)
                        ),
                        fix_for_unknown(&name.value, &surfaces),
                    );
                }
                known
            }
        }
    }

    /// The field type with this name, or a diagnostic at the name.
    fn field_type(&mut self, name: &Located<String>) -> Option<(FieldTypeIndex, ValueType)> {
        if let Some(found) = self.model.field_type(&name.value) {
            return Some((found.index, found.plan.ty.clone()));
        }
        if let Some(needs) = self.model.incomplete_types.get(&name.value).cloned() {
            self.error(
                codes::FIELD_TYPE_INCOMPLETE,
                Stage::Resolve,
                &name.at,
                format!("The field type `{}` cannot be used as it stands: it needs {needs}.", name.value),
                format!("Define a field type in `types.yaml` with the base `{}` that states {needs}, and name it here.", name.value),
            );
            return None;
        }
        let known: Vec<&str> = self
            .model
            .field_types
            .iter()
            .map(|field_type| field_type.name.as_str())
            .collect();
        let fix = fix_for_unknown(&name.value, &known);
        self.error(
            codes::UNKNOWN_FIELD_TYPE,
            Stage::Resolve,
            &name.at,
            format!(
                "`{}` is not a field type: it is not built in and `types.yaml` does not define it.",
                shown(&name.value)
            ),
            fix,
        );
        None
    }

    fn expr(
        &mut self,
        doc: &Located<Doc>,
        env: &TypeEnv,
        expected: Option<&ValueType>,
    ) -> Option<(a2m_core::ExprIndex, ValueType)> {
        let text = expr_text(doc)?;
        match check_expr(&text, &doc.at, env, expected) {
            Ok(plan_expr) => {
                let ty = plan_expr.result.clone();
                Some((self.model.exprs.borrow_mut().intern(plan_expr), ty))
            }
            Err(problem) => {
                self.problems.push(problem);
                None
            }
        }
    }

    fn run(&mut self, version: Version) -> Result<Compiled, Diagnostics> {
        if self.mode == CompileMode::Draft && version != DRAFT_VERSION {
            // A draft under a release version would have the bytes and the
            // hash of the plan published as that version (decision 0161).
            return Err(Diagnostics::from(Diagnostic::error(
                kernel(codes::DRAFT_UNDER_RELEASE_VERSION),
                Stage::Emit,
                &Place::start_of(&RelPath::target_file()),
                format!(
                    "A draft was asked for under the version {version}, which a published plan carries."
                ),
                "Compile a draft under the draft version, or publish to compile a release.",
            )));
        }
        self.target();
        self.field_types();
        self.pack_kinds();
        self.operations();
        self.auth();
        self.source_files();
        self.pipelines();
        self.describe();
        self.tests();
        self.lint();
        self.hide_credentials();

        let (errors, warnings) = self.take_problems().split();
        if !errors.is_empty() {
            let mut all = errors;
            all.extend(warnings);
            return Err(all);
        }
        let (Some(target), Some(about)) = (self.model.target.clone(), self.about.take()) else {
            return Err(warnings);
        };
        let plan = Plan {
            header: PlanHeader {
                format: PLAN_FORMAT,
                minimum_engine: MINIMUM_ENGINE,
                compiler: COMPILER_VERSION,
                target,
                version,
                sources: self.sources.digest,
            },
            target: about,
            surfaces: self
                .model
                .surfaces
                .iter()
                .map(|info| info.plan.clone())
                .collect(),
            environments: std::mem::take(&mut self.environments),
            auth: std::mem::take(&mut self.auth),
            field_types: std::mem::take(&mut self.model.field_types),
            expressions: std::mem::take(&mut self.model.exprs.borrow_mut().exprs),
            operations: self
                .op_plans
                .iter()
                .map(|(plan, _, _)| plan.clone())
                .collect(),
            pipelines: self
                .pipelines
                .iter()
                .map(|info| info.plan.clone())
                .collect(),
            tools: std::mem::take(&mut self.tools),
            packs: std::mem::take(&mut self.model.sections),
            tests: std::mem::take(&mut self.tests),
        };
        let review = review_of(&plan, &self.reviews);
        match plan.encode() {
            Ok(encoded) => Ok(Compiled {
                plan,
                encoded,
                warnings,
                review,
                test_bindings: std::mem::take(&mut self.test_bindings),
            }),
            Err(failure) => Err(Diagnostics::from(Diagnostic::error(
                kernel(codes::PLAN_NOT_ENCODABLE),
                Stage::Emit,
                &Place::start_of(&RelPath::target_file()),
                format!("The plan does not hold together: {}", failure.message()),
                "A registered lowering or file kind returned a reference to something the plan does not hold, or steps that break a rule of a write; correct the pack.",
            ))),
        }
    }

    fn take_problems(&mut self) -> Diagnostics {
        std::mem::take(&mut self.problems).into_iter().collect()
    }

    // ---- target.yaml: the target, its surfaces, its environments ----

    fn target(&mut self) {
        let Some(file) = self.sources.file("target.yaml") else {
            self.error(
                codes::FILE_MISSING,
                Stage::Validate,
                &Place::start_of(&RelPath::target_file()),
                "The target has no `target.yaml`.".to_owned(),
                "Add `target.yaml` with `schema_version`, `target`, and `surfaces`.".to_owned(),
            );
            return;
        };
        let [doc] = file.docs.as_slice() else {
            self.error(
                codes::WRONG_TYPE,
                Stage::Validate,
                &Place::start_of(&file.path),
                format!(
                    "`target.yaml` holds {} documents and must hold one.",
                    file.docs.len()
                ),
                "Write one document: a map with `schema_version`, `target`, and `surfaces`."
                    .to_owned(),
            );
            return;
        };
        let Some(schema) = k::schema_of(k::TARGET) else {
            return;
        };
        let checked = schema.check(doc);
        self.problems.extend(checked.diagnostics);

        if let Some(stated) = doc.get("schema_version")
            && let Some(number) = stated.as_int()
            && number != SCHEMA_VERSION
        {
            self.error(
                codes::SCHEMA_VERSION_UNSUPPORTED,
                Stage::Validate,
                &stated.at,
                format!("The files state schema version {number}, and this compiler reads version {SCHEMA_VERSION}."),
                format!("Write `schema_version: {SCHEMA_VERSION}`, or use the compiler the files were written for."),
            );
        }
        if let Some(name) = doc.get("target")
            && let Some(text) = name.as_text()
        {
            match TargetName::new(text) {
                Ok(target) => self.model.target = Some(target),
                Err(_) => self.error(
                    codes::NAME_MALFORMED,
                    Stage::Validate,
                    &name.at,
                    format!("`{}` is not a target name.", shown(text)),
                    "Write one lowercase name of at most 32 characters, such as `example`."
                        .to_owned(),
                ),
            }
        }
        self.model.modules = doc
            .get("modules")
            .map(|modules| {
                modules
                    .items()
                    .iter()
                    .filter_map(|module| module.as_text().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();

        if let Some(surfaces) = doc.get("surfaces") {
            self.surfaces(surfaces);
        }
        let default_environment = self.environments(doc);

        // Defaults are inputs to compiling, not data for the engine: a
        // timeout reaches a step through its lowering, and a limit reaches a
        // pipeline that states none. Only the capture level travels.
        let defaults = doc.get("defaults");
        self.model.default_timeouts = defaults
            .and_then(|defaults| defaults.get("timeouts"))
            .map(Located::entries)
            .unwrap_or_default()
            .iter()
            .filter_map(|entry| {
                entry
                    .value
                    .as_int()
                    .map(|number| (entry.key.value.clone(), to_u32(number)))
            })
            .collect();
        let limit = |key: &str| {
            defaults
                .and_then(|defaults| defaults.get("limits"))
                .and_then(|limits| limits.get(key))
                .and_then(Located::as_int)
        };
        self.default_limits = PipelineLimits {
            max_records: limit("max_records").map(to_u32),
            max_duration_ms: limit("max_duration_ms").and_then(|number| u64::try_from(number).ok()),
        };
        let environment = doc.get("environment");
        let timezone = environment.and_then(|environment| environment.get("timezone"));
        if let Some(zone) = timezone
            && zone
                .as_text()
                .is_none_or(|text| types::utc_offset_minutes(text).is_none())
        {
            self.error(
                codes::UNKNOWN_VALUE,
                Stage::Validate,
                &zone.at,
                "The target's time zone is not one the compiler can fix.".to_owned(),
                "Write `UTC`, or an offset such as `\"+02:00\"`; a named zone is not built yet."
                    .to_owned(),
            );
        }
        self.about = Some(TargetPlan {
            title: doc
                .text("title")
                .or_else(|| doc.text("target"))
                .unwrap_or_default()
                .to_owned(),
            modules: self.model.modules.clone(),
            default_environment,
            locale: environment
                .and_then(|environment| environment.text("locale"))
                .map(str::to_owned),
            timezone: timezone.and_then(Located::as_text).map(str::to_owned),
            capture: defaults
                .and_then(|defaults| defaults.text("capture"))
                .and_then(|level| level.parse::<CaptureLevel>().ok()),
            exclusive_session: doc
                .get("exclusive_session")
                .and_then(Located::as_bool)
                .unwrap_or(false),
            drift_risk: doc
                .text("drift_risk")
                .and_then(|risk| risk.parse::<DriftRisk>().ok()),
            authorization: doc.text("authorization").map(str::to_owned),
        });
    }

    fn surfaces(&mut self, surfaces: &Located<Doc>) {
        let kernel_part = k::surface();
        let mut shared: Vec<(usize, Located<String>)> = Vec::new();
        for entry in surfaces.entries() {
            let Ok(name) = SurfaceName::new(&entry.key.value) else {
                self.error(
                    codes::NAME_MALFORMED,
                    Stage::Validate,
                    &entry.key.at,
                    format!("`{}` is not a surface name.", shown(&entry.key.value)),
                    "Write one lowercase name of at most 32 characters, such as `web`.".to_owned(),
                );
                continue;
            };
            let doc = &entry.value;
            if !matches!(doc.value, Doc::Map(_)) {
                self.error(
                    codes::WRONG_TYPE,
                    Stage::Validate,
                    &doc.at,
                    format!(
                        "The surface `{name}` is a {}, and a map is needed.",
                        doc.value.kind()
                    ),
                    "Write a map with `kind` and the fields of that kind.".to_owned(),
                );
                continue;
            }
            // References to other surfaces wait until every surface is known.
            let checked = kernel_part.check(&part_of(doc, |key| SURFACE_KEYS.contains(&key)));
            let fine = checked.diagnostics.is_empty();
            self.problems.extend(checked.diagnostics);
            let Some(kind_doc) = doc.get("kind") else {
                continue;
            };
            let Some(kind) = kind_doc
                .as_text()
                .and_then(|kind| SurfaceKind::new(kind).ok())
            else {
                continue;
            };
            let Some(spec) = self.set.surface_kinds.get(&kind) else {
                let known: Vec<&str> = self
                    .set
                    .surface_kinds
                    .keys()
                    .map(SurfaceKind::as_str)
                    .collect();
                self.error(
                    codes::UNKNOWN_SURFACE_KIND,
                    Stage::Validate,
                    &kind_doc.at,
                    format!("No enabled pack registered a surface kind named `{kind}`."),
                    format!(
                        "{} Or enable the pack that serves `{kind}` surfaces.",
                        fix_for_unknown(kind.as_str(), &known)
                    ),
                );
                continue;
            };
            let spec = spec.item.clone();
            let pack_part = part_of(doc, |key| !SURFACE_KEYS.contains(&key));
            if !self.fits(&spec.fields, &pack_part) || !fine {
                continue;
            }
            // An address of nothing but blanks states no address: a
            // surface that leads nowhere is not guessed at (decision 0199).
            let Some(address) = pack_part
                .text(&spec.address_key)
                .filter(|address| !address.trim().is_empty())
            else {
                self.error(
                    codes::MISSING_FIELD,
                    Stage::Validate,
                    &doc.at,
                    format!(
                        "The surface `{name}` does not state its address, `{}`.",
                        spec.address_key
                    ),
                    format!(
                        "Add `{}:` with the address of the surface.",
                        spec.address_key
                    ),
                );
                continue;
            };
            let rest = part_of(&pack_part, |key| key != spec.address_key);
            let payload = if rest.entries().is_empty() {
                Some(PackPayload::none())
            } else {
                PackPayload::encode(&rest.to_json()).ok()
            };
            let Some(payload) = payload else { continue };
            let programs = self.declared_programs(&spec, &pack_part);
            let rate = doc.get("rate").map(|rate| RatePlan {
                requests: to_u32(rate.get("requests").and_then(Located::as_int).unwrap_or(1)),
                per_ms: to_u32(rate.get("per_ms").and_then(Located::as_int).unwrap_or(1)),
            });
            if let Some(other) = doc.get("same_session_as")
                && let Some(text) = other.as_text()
            {
                shared.push((
                    self.model.surfaces.len(),
                    Located::new(text.to_owned(), other.at.clone()),
                ));
            }
            self.model.surfaces.push(SurfaceInfo {
                plan: SurfacePlan {
                    name,
                    kind,
                    address: address.to_owned(),
                    same_session_as: None,
                    stands: None,
                    rate,
                    programs,
                    payload,
                },
                doc: doc.clone(),
            });
        }
        for (position, other) in shared {
            if self.resolve(Reference::Surface, &other) {
                let found = self
                    .model
                    .surface(&other.value)
                    .map(|surface| surface.index);
                self.model.surfaces[position].plan.same_session_as = found;
            }
        }
        // `stands: {beneath: surface}` names a surface too.
        for entry in surfaces.entries() {
            let Some(stands) = entry.value.get("stands") else {
                continue;
            };
            let found = match stands.get("beneath") {
                None => Some(Stands::Alone),
                Some(beneath) => beneath.as_text().and_then(|text| {
                    let named = Located::new(text.to_owned(), beneath.at.clone());
                    if !self.resolve(Reference::Surface, &named) {
                        return None;
                    }
                    self.model
                        .surface(text)
                        .map(|owner| Stands::Beneath(owner.index))
                }),
            };
            if let Some(position) = self
                .model
                .surfaces
                .iter()
                .position(|surface| surface.plan.name.as_str() == entry.key.value)
            {
                self.model.surfaces[position].plan.stands = found;
            }
        }
    }

    /// The programs a surface declares under the fields its kind names. A
    /// path that is not absolute and a digest that is not one are reported
    /// and left out.
    fn declared_programs(
        &mut self,
        spec: &SurfaceKindSpec,
        fields: &Located<Doc>,
    ) -> Vec<DeclaredProgram> {
        let mut programs = Vec::new();
        for field in &spec.programs {
            let stated = field.digest_key.as_deref().and_then(|key| fields.get(key));
            let digest = stated.map(|stated| {
                let digest = stated.as_text().and_then(digest_of);
                if digest.is_none() {
                    self.error(
                        codes::PROGRAM_INVALID,
                        Stage::Validate,
                        &stated.at,
                        "The digest of the program is not a SHA-256 digest.".to_owned(),
                        "Write the 64 lowercase hexadecimal characters of the file's SHA-256 digest, with or without `sha256:` before them.".to_owned(),
                    );
                }
                digest
            });
            let Some(path) = fields.get(&field.path_key) else {
                if let Some(stated) = stated {
                    self.error(
                        codes::PROGRAM_INVALID,
                        Stage::Validate,
                        &stated.at,
                        format!(
                            "The surface states a digest and no program under `{}`.",
                            field.path_key
                        ),
                        format!(
                            "Add `{}:` with the absolute path of the program, or remove the digest.",
                            field.path_key
                        ),
                    );
                }
                continue;
            };
            // A program whose digest is stated badly is left out: it would
            // otherwise be declared with no digest at all.
            let stated_badly = digest == Some(None);
            if let Some(program) = self.declared_program(path, digest.flatten())
                && !stated_badly
                && !programs.contains(&program)
            {
                programs.push(program);
            }
        }
        programs
    }

    /// Where a surface's address is a program, its address in an
    /// environment is one too, with the digest the surface states.
    fn program_at_address(&mut self, surface: SurfaceIndex, address: &Located<Doc>) {
        let Some(info) = self.model.surfaces.get(surface.get()) else {
            return;
        };
        let Some(spec) = self.set.surface_kinds.get(&info.plan.kind) else {
            return;
        };
        let Some(field) = spec
            .item
            .programs
            .iter()
            .find(|field| field.path_key == spec.item.address_key)
        else {
            return;
        };
        let digest = field
            .digest_key
            .as_deref()
            .and_then(|key| info.doc.text(key))
            .and_then(digest_of);
        if let Some(program) = self.declared_program(address, digest)
            && let Some(info) = self.model.surfaces.get_mut(surface.get())
            && !info.plan.programs.contains(&program)
        {
            info.plan.programs.push(program);
        }
    }

    /// One declared program, when its path is text and absolute.
    fn declared_program(
        &mut self,
        path: &Located<Doc>,
        digest: Option<Digest>,
    ) -> Option<DeclaredProgram> {
        let text = path.as_text().unwrap_or_default();
        if DeclaredProgram::is_absolute_path(text) {
            if !DeclaredProgram::is_direct_path(text) {
                self.error(
                    codes::PROGRAM_INVALID,
                    Stage::Validate,
                    &path.at,
                    "The path of a program steps through `.` or `..`, so the file it names is not the one it reads as.".to_owned(),
                    "Write the path of the program itself, with no `.` or `..` in it.".to_owned(),
                );
                return None;
            }
            if DeclaredProgram::is_share_path(text) && digest.is_none() {
                self.error(
                    codes::PROGRAM_INVALID,
                    Stage::Validate,
                    &path.at,
                    "A program on a network share is declared with no digest, so nothing holds the file another machine serves to the bytes that were reviewed.".to_owned(),
                    "State the SHA-256 digest of the program beside its path, or install the program on the machine that runs it.".to_owned(),
                );
                return None;
            }
            return Some(DeclaredProgram {
                path: text.to_owned(),
                digest,
            });
        }
        self.error(
            codes::PROGRAM_INVALID,
            Stage::Validate,
            &path.at,
            format!(
                "A program is named by its absolute path, and `{}` is not one.",
                shown(text)
            ),
            "Write the whole path from the root or the drive, such as `/opt/example/bin/example-tool`; a program is never found by searching.".to_owned(),
        );
        None
    }

    /// The target's environments, and which of them a call runs in when it
    /// names none: the one the target states, or `production`.
    fn environments(&mut self, doc: &Located<Doc>) -> EnvironmentIndex {
        let mut names = BTreeSet::new();
        if let Some(environments) = doc.get("environments") {
            for entry in environments.entries() {
                let Ok(name) = Environment::new(&entry.key.value) else {
                    self.error(
                        codes::NAME_MALFORMED,
                        Stage::Validate,
                        &entry.key.at,
                        format!("`{}` is not an environment name.", shown(&entry.key.value)),
                        "Write one lowercase name, such as `test`.".to_owned(),
                    );
                    continue;
                };
                let Some(kind) = entry
                    .value
                    .text("kind")
                    .and_then(|kind| kind.parse::<EnvironmentKind>().ok())
                else {
                    continue;
                };
                let mut addresses = Vec::new();
                for address in entry
                    .value
                    .get("surfaces")
                    .map(Located::entries)
                    .unwrap_or_default()
                {
                    let surface = Located::new(address.key.value.clone(), address.key.at.clone());
                    if self.resolve(Reference::Surface, &surface)
                        && let Some(found) = self.model.surface(&surface.value)
                        && let Some(text) = address.value.as_text()
                    {
                        let index = found.index;
                        addresses.push(AddressOverride {
                            surface: index,
                            address: text.to_owned(),
                        });
                        self.program_at_address(index, &address.value);
                    }
                }
                let mut test_records = Vec::new();
                for record in entry
                    .value
                    .get("test_records")
                    .map(Located::items)
                    .unwrap_or_default()
                {
                    let parts: Option<Vec<Value>> = match &record.value {
                        Doc::List(parts) => parts.iter().map(written_value).collect(),
                        _ => written_value(record).map(|part| vec![part]),
                    };
                    match parts.and_then(|parts| RecordKey::new(parts).ok()) {
                        Some(key) => test_records.push(key),
                        None => self.error(
                            codes::WRONG_TYPE,
                            Stage::Validate,
                            &record.at,
                            "A test record is a record key: one plain value, or a list of them, parent first.".to_owned(),
                            "Write the key as the system shows it, such as `\"SO-1001\"`, or `[\"SO-1001\", \"L-101\"]` for a key of two parts.".to_owned(),
                        ),
                    }
                }
                names.insert(name.as_str().to_owned());
                self.environments.push(EnvironmentPlan {
                    name,
                    kind,
                    addresses,
                    test_records,
                    approve_writes: entry
                        .value
                        .text("approve_writes")
                        .and_then(|rule| rule.parse::<ApproveWrites>().ok()),
                });
            }
        }
        // `production` is implied when the target does not declare it.
        if !names.contains("production")
            && let Ok(name) = Environment::new("production")
        {
            names.insert("production".to_owned());
            self.environments.push(EnvironmentPlan {
                name,
                kind: EnvironmentKind::Production,
                addresses: Vec::new(),
                test_records: Vec::new(),
                approve_writes: None,
            });
        }
        if let Some(default) = doc.get("default_environment")
            && let Some(text) = default.as_text()
            && !names.contains(text)
        {
            let known: Vec<&str> = names.iter().map(String::as_str).collect();
            self.error(
                codes::UNKNOWN_NAME,
                Stage::Resolve,
                &default.at,
                format!(
                    "The target declares no environment named `{}`.",
                    shown(text)
                ),
                fix_for_unknown(text, &known),
            );
        }
        let default = doc.text("default_environment").unwrap_or("production");
        let position = self
            .environments
            .iter()
            .position(|environment| environment.name.as_str() == default)
            .unwrap_or(0);
        EnvironmentIndex::new(index(position))
    }

    // ---- types.yaml: field types ----

    fn field_types(&mut self) {
        let start = Place::start_of(&RelPath::target_file());
        for (name, rule) in types::built_ins() {
            let made = types::plan_of(
                name,
                &rule,
                types::built_in_labels(name),
                &mut self.model.exprs.borrow_mut(),
            );
            match made {
                Ok(plan) => self.model.field_types.push(plan),
                Err(why) => self.error(
                    codes::FIELD_TYPE_INCOMPLETE,
                    Stage::Resolve,
                    &start,
                    format!("The built-in field type `{name}` does not compile to a normalizer: {why}."),
                    "Use the versions of the compiler and the expression library that were released together.".to_owned(),
                ),
            }
        }
        for (name, needs) in types::INCOMPLETE_BUILT_INS {
            self.model
                .incomplete_types
                .insert((*name).to_owned(), (*needs).to_owned());
        }
        let fixed_zone = self
            .sources
            .docs_of(k::TARGET)
            .next()
            .and_then(|doc| doc.get("environment"))
            .and_then(|environment| environment.text("timezone"))
            .unwrap_or("UTC")
            .to_owned();
        let Some(schema) = k::schema_of(k::TYPES) else {
            return;
        };
        let docs: Vec<&Located<Doc>> = self.sources.docs_of(k::TYPES).collect();
        let mut defined: BTreeMap<String, Place> = BTreeMap::new();
        let complete_built_ins: Vec<&str> = types::built_ins()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        for doc in docs {
            if !self.fits(schema, doc) {
                continue;
            }
            for entry in doc.get("types").map(Located::entries).unwrap_or_default() {
                let name = entry.key.value.as_str();
                if !is_simple_name(name) {
                    self.error(
                        codes::NAME_MALFORMED,
                        Stage::Validate,
                        &entry.key.at,
                        format!("`{}` is not a field type name.", shown(name)),
                        "Write one lowercase name, such as `length_ft_in`.".to_owned(),
                    );
                    continue;
                }
                if let Some(first) = defined.get(name) {
                    let message = format!(
                        "The field type `{name}` is defined twice; it is first defined at {first}."
                    );
                    self.error(
                        codes::NAME_DEFINED_TWICE,
                        Stage::Resolve,
                        &entry.key.at,
                        message,
                        "Remove one of the two definitions.".to_owned(),
                    );
                    continue;
                }
                defined.insert(name.to_owned(), entry.key.at.clone());
                // A complete built-in keeps its rule and its labels: a
                // target's type of its name would replace them, and could
                // drop the untrusted mark of `untrusted_text` unseen. Only
                // a built-in that needs more (`money`, `choice`) is
                // completed by a target's type of its name.
                if complete_built_ins.contains(&name) {
                    self.error(
                        codes::BUILT_IN_TYPE_REDEFINED,
                        Stage::Resolve,
                        &entry.key.at,
                        format!("The field type `{name}` is built in, and a target cannot define it again."),
                        format!("Give the target's type a name of its own, such as `{name}_local`, and use that name where the target means it."),
                    );
                    continue;
                }
                let (rule, labels) = match types::rule_of(name, &entry.value, &fixed_zone) {
                    Ok(pair) => pair,
                    Err(problems) => {
                        self.problems.extend(problems);
                        continue;
                    }
                };
                let made = types::plan_of(name, &rule, labels, &mut self.model.exprs.borrow_mut());
                match made {
                    Ok(plan) => {
                        // A target's type of an incomplete built-in's name
                        // completes it; every other name is the target's own.
                        self.model.incomplete_types.remove(name);
                        self.model.field_types.push(plan);
                    }
                    Err(why) => self.error(
                        codes::FIELD_TYPE_INCOMPLETE,
                        Stage::Resolve,
                        &entry.value.at,
                        format!("The field type `{name}` does not compile to a normalizer: {why}."),
                        "State a unit, a format, a zone, and code lists the expression library knows; `anything2mcp docs` lists them.".to_owned(),
                    ),
                }
            }
        }
    }

    // ---- auth.yaml ----

    /// One entry per group of surfaces that share a sign-in. It runs after
    /// the operations and the pack sections, because a probe names one of
    /// them.
    fn auth(&mut self) {
        let Some(schema) = k::schema_of(k::AUTH) else {
            return;
        };
        let docs: Vec<&Located<Doc>> = self.sources.docs_of(k::AUTH).collect();
        let mut covered: BTreeMap<SurfaceIndex, Place> = BTreeMap::new();
        for doc in docs {
            if !self.fits(schema, doc) {
                continue;
            }
            for entry in doc.get("auth").map(Located::items).unwrap_or_default() {
                if let Some(plan) = self.auth_entry(entry, &mut covered) {
                    self.auth.push(plan);
                }
            }
        }
    }

    fn auth_entry(
        &mut self,
        entry: &Located<Doc>,
        covered: &mut BTreeMap<SurfaceIndex, Place>,
    ) -> Option<AuthPlan> {
        let mut fine = true;
        let mut surfaces = Vec::new();
        for named in entry
            .get("surfaces")
            .map(Located::items)
            .unwrap_or_default()
        {
            let Some(found) = named
                .as_text()
                .and_then(|name| self.model.surface(name))
                .map(|surface| surface.index)
            else {
                fine = false;
                continue;
            };
            if let Some(first) = covered.get(&found) {
                let message = format!(
                    "The surface `{}` is signed in to twice; its first entry is at {first}.",
                    named.as_text().unwrap_or_default()
                );
                self.error(
                    codes::PROBE_INVALID,
                    Stage::Resolve,
                    &named.at,
                    message,
                    "Name each surface in one entry of `auth.yaml`; surfaces that share a sign-in go in the same entry.".to_owned(),
                );
                fine = false;
                continue;
            }
            covered.insert(found, named.at.clone());
            surfaces.push(found);
        }
        let mode = entry
            .text("mode")
            .and_then(|mode| mode.parse::<SignInMode>().ok())?;
        let provider_doc = entry.get("provider")?;
        let Some(provider) = provider_doc
            .as_text()
            .and_then(|name| AuthProviderName::new(name).ok())
        else {
            self.error(
                codes::NAME_MALFORMED,
                Stage::Validate,
                &provider_doc.at,
                "This is not the name of an auth provider.".to_owned(),
                "Write one lowercase name of at most 32 characters, such as `api_key`.".to_owned(),
            );
            return None;
        };
        let probe = self.probe(entry, &surfaces);

        let mut expiry_signals = Vec::new();
        for signal in entry
            .get("expiry_signals")
            .map(Located::items)
            .unwrap_or_default()
        {
            match PackPayload::encode(&signal.to_json()) {
                Ok(payload) => expiry_signals.push(payload),
                Err(_) => fine = false,
            }
        }
        let mut settings = Vec::new();
        for setting in entry
            .get("settings")
            .map(Located::entries)
            .unwrap_or_default()
        {
            if let Some(value) = written_text(&setting.value) {
                settings.push(AuthSetting {
                    name: setting.key.value.clone(),
                    value,
                });
            }
        }
        settings.sort_by(|a, b| a.name.cmp(&b.name));
        let mut credentials = Vec::new();
        for credential in entry
            .get("credentials")
            .map(Located::entries)
            .unwrap_or_default()
        {
            let reference = credential
                .value
                .as_text()
                .and_then(|text| CredentialRef::new(text).ok());
            match reference {
                Some(reference) => credentials.push(CredentialUse {
                    role: credential.key.value.clone(),
                    reference,
                }),
                None => {
                    // The value may be the credential itself: it is never shown.
                    self.error(
                        codes::CREDENTIAL_IN_FILE,
                        Stage::Lint,
                        &credential.value.at,
                        format!("The credential `{}` is not a vault reference.", shown(&credential.key.value)),
                        "Store the value in the vault and write its reference here, such as `vault://example/service/password`.".to_owned(),
                    );
                    fine = false;
                }
            }
        }
        credentials.sort_by(|a, b| a.role.cmp(&b.role));
        let timeout = |key: &str| {
            entry
                .get("timeouts")
                .and_then(|timeouts| timeouts.get(key))
                .and_then(Located::as_int)
                .and_then(|number| u64::try_from(number).ok())
        };
        let movable = match entry.get("movable").map(|movable| &movable.value) {
            Some(Doc::Bool(flag)) => *flag,
            Some(Doc::Text(text)) => text != "false",
            _ => true,
        };
        let probe = probe?;
        fine.then(|| AuthPlan {
            surfaces,
            mode,
            provider,
            probe,
            expiry_signals,
            idle_ms: timeout("idle_ms"),
            absolute_ms: timeout("absolute_ms"),
            sign_out: entry.text("sign_out").map(str::to_owned),
            settings,
            credentials,
            attempt_limit: entry
                .get("attempt_limit")
                .and_then(Located::as_int)
                .map_or(1, to_u32),
            movable,
        })
    }

    /// The probe of one entry of `auth.yaml`: a read operation of the
    /// target, with the inputs the entry gives it, or an entry of a pack's
    /// file kind on one of the entry's surfaces.
    fn probe(&mut self, entry: &Located<Doc>, surfaces: &[SurfaceIndex]) -> Option<ProbePlan> {
        let named = entry.get("probe")?;
        let name = named.as_text()?;
        let given = entry.get("probe_inputs");
        let invalid = |this: &mut Self, at: &Place, message: String, fix: &str| {
            this.error(
                codes::PROBE_INVALID,
                Stage::Resolve,
                at,
                message,
                fix.to_owned(),
            );
        };
        if let Some(position) = self
            .operations
            .iter()
            .position(|info| info.name.as_str() == name)
        {
            let info = self.operations[position].clone();
            let mut fine = true;
            if info.effect != EffectClass::Read {
                invalid(
                    self,
                    &named.at,
                    format!(
                        "The probe `{name}` has the effect `{}`; a session is checked with a read.",
                        info.effect
                    ),
                    "Name an operation whose effect is `read`.",
                );
                fine = false;
            }
            let mut inputs = Values::new();
            for input in given.map(Located::entries).unwrap_or_default() {
                let Some(declared) = info
                    .inputs
                    .iter()
                    .find(|declared| declared.plan.name == input.key.value)
                else {
                    let known: Vec<&str> = info
                        .inputs
                        .iter()
                        .map(|declared| declared.plan.name.as_str())
                        .collect();
                    let fix = fix_for_unknown(&input.key.value, &known);
                    self.error(
                        codes::UNKNOWN_NAME,
                        Stage::Resolve,
                        &input.key.at,
                        format!("`{}` is not an input of `{name}`.", shown(&input.key.value)),
                        fix,
                    );
                    fine = false;
                    continue;
                };
                match value_of(&input.value, &declared.plan.ty) {
                    Some(value) => {
                        inputs.insert(declared.plan.name.clone(), value);
                    }
                    None => {
                        self.error(
                            codes::VALUE_NOT_OF_TYPE,
                            Stage::Resolve,
                            &input.value.at,
                            format!(
                                "The probe gives `{}` a {}, and the input is a {}.",
                                declared.plan.name,
                                input.value.value.kind(),
                                declared.plan.ty.name()
                            ),
                            format!(
                                "Write a {} here; text goes in quotes.",
                                declared.plan.ty.name()
                            ),
                        );
                        fine = false;
                    }
                }
            }
            for declared in &info.inputs {
                if declared.plan.required
                    && declared.plan.default.is_none()
                    && !inputs.contains(&declared.plan.name)
                {
                    invalid(
                        self,
                        &named.at,
                        format!(
                            "The probe `{name}` needs the input `{}`, and the entry does not give it.",
                            declared.plan.name
                        ),
                        &format!(
                            "Add `probe_inputs: {{{}: ...}}` with a record that always exists, or name a read that needs no input.",
                            declared.plan.name
                        ),
                    );
                    fine = false;
                }
            }
            return fine.then(|| ProbePlan::Operation {
                operation: OperationIndex::new(index(position)),
                inputs,
            });
        }
        // Not an operation: an entry of a pack's file kind on one of the
        // entry's surfaces, such as an endpoint.
        let kinds: BTreeSet<&SurfaceKind> = surfaces
            .iter()
            .filter_map(|surface| self.model.surfaces.get(surface.get()))
            .map(|surface| &surface.plan.kind)
            .collect();
        let found: Vec<SectionIndex> = self
            .model
            .sections
            .iter()
            .enumerate()
            .filter(|(_, section)| {
                kinds.contains(&section.surface_kind)
                    && self
                        .model
                        .defined
                        .get(&(section.kind.clone(), section.surface_kind.clone()))
                        .is_some_and(|names| names.iter().any(|known| known == name))
            })
            .map(|(position, _)| SectionIndex::new(index(position)))
            .collect();
        match found.as_slice() {
            [section] => {
                if let Some(extra) = given {
                    invalid(
                        self,
                        &extra.at,
                        format!(
                            "`probe_inputs` are an operation's inputs, and the probe `{name}` is not an operation."
                        ),
                        "Remove `probe_inputs`, or name a read operation as the probe.",
                    );
                    return None;
                }
                Some(ProbePlan::Entry(PackRef {
                    section: *section,
                    entry: name.to_owned(),
                }))
            }
            [] if self.mode == CompileMode::Draft
                && !self
                    .sources
                    .docs_of(k::OPERATION)
                    .any(|doc| doc.text("operation") == Some(name)) =>
            {
                // A system that answers no read before a sign-in leaves
                // nothing to draft a probe from until the first session: a
                // draft may name its probe first (decision 0252). A publish
                // never may, and the plan's own check refuses one.
                self.problems.push(
                    Diagnostic::error(
                        kernel(codes::PROBE_PENDING),
                        Stage::Resolve,
                        &named.at,
                        format!(
                            "The probe `{}` is not drafted yet: this draft's sign-in is not confirmed until the read exists and passes, and the target cannot be published until then.",
                            shown(name)
                        ),
                        "Sign in, let the scout draft a read, and name it here, or write the read by hand; `probe_inputs` are checked once it exists.",
                    )
                    .as_warning(),
                );
                Some(ProbePlan::Pending {
                    name: name.to_owned(),
                })
            }
            [] => {
                let known: Vec<&str> = self
                    .operations
                    .iter()
                    .map(|info| info.name.as_str())
                    .collect();
                let fix = fix_for_unknown(name, &known);
                self.error(
                    codes::UNKNOWN_NAME,
                    Stage::Resolve,
                    &named.at,
                    format!("The probe `{}` is not an operation of the target, or the one that is has errors, and no file on the entry's surfaces defines it.", shown(name)),
                    fix,
                );
                None
            }
            _ => {
                invalid(
                    self,
                    &named.at,
                    format!(
                        "The probe `{name}` is defined by more than one kind of file on the entry's surfaces."
                    ),
                    "Name a read operation as the probe, or rename one of the two.",
                );
                None
            }
        }
    }

    // ---- file kinds that packs own ----

    fn pack_kinds(&mut self) {
        let set = self.set;
        let mut defined: BTreeMap<(FileKindName, SurfaceKind, String), Place> = BTreeMap::new();
        let files: Vec<&SourceFile> = self
            .sources
            .files
            .iter()
            .filter(|file| !k::KERNEL_FILE_KINDS.contains(&file.kind.as_str()))
            .collect();
        for file in files {
            for doc in &file.docs {
                let Some(surface) = self.model.surface_of(doc, &file.kind, set) else {
                    let kinds: Vec<&str> = set
                        .surface_kinds_of(&file.kind)
                        .into_iter()
                        .map(SurfaceKind::as_str)
                        .collect();
                    let at = doc
                        .get("surface")
                        .map_or(&doc.at, |named| &named.at)
                        .clone();
                    self.error(
                        codes::SURFACE_NOT_DECLARED,
                        Stage::Resolve,
                        &at,
                        format!(
                            "This `{}` lives on no surface of the target: it names none that exists, and the target declares no surface of kind {}.",
                            file.kind,
                            kinds.join(" or ")
                        ),
                        "Name one of the target's surfaces under `surface:`, or declare a surface of that kind in `target.yaml`.".to_owned(),
                    );
                    continue;
                };
                let surface_kind = surface.kind.clone();
                let Some(owner) = set.file_kind(&file.kind, &surface_kind) else {
                    let kinds: Vec<&str> = set
                        .surface_kinds_of(&file.kind)
                        .into_iter()
                        .map(SurfaceKind::as_str)
                        .collect();
                    self.error(
                        codes::UNKNOWN_FILE,
                        Stage::Validate,
                        &doc.at,
                        format!(
                            "No enabled pack registered the file kind `{}` for `{surface_kind}` surfaces.",
                            file.kind
                        ),
                        format!("Put it on a surface of kind {}, or enable the pack that owns it.", kinds.join(" or ")),
                    );
                    continue;
                };
                if !self.fits(owner.item.schema(), doc) {
                    continue;
                }
                let mut fine = true;
                for name in owner.item.names(doc) {
                    let key = (
                        file.kind.clone(),
                        surface_kind.clone(),
                        name.value.name.clone(),
                    );
                    if let Some(first) = defined.get(&key) {
                        let message = format!(
                            "The {} `{}` is defined twice; it is first defined at {first}.",
                            file.kind, name.value.name
                        );
                        self.error(
                            codes::NAME_DEFINED_TWICE,
                            Stage::Resolve,
                            &name.at,
                            message,
                            "Remove one of the two definitions, or rename one.".to_owned(),
                        );
                        fine = false;
                    } else {
                        defined.insert(key, name.at);
                    }
                }
                for site in owner.item.expressions(doc) {
                    if let Err(problem) = check_site(&site) {
                        self.problems.push(problem);
                        fine = false;
                    }
                }
                if fine {
                    let key = (file.kind.clone(), surface_kind);
                    self.model.defined.entry(key.clone()).or_default().extend(
                        owner
                            .item
                            .names(doc)
                            .into_iter()
                            .map(|name| name.value.name),
                    );
                    self.model.owners.insert(key.clone(), owner.pack.clone());
                    self.model.entries.entry(key).or_default().push(doc.clone());
                }
            }
        }
        // One section per file kind and surface kind, in order of name.
        let keys: Vec<(FileKindName, SurfaceKind)> = self.model.entries.keys().cloned().collect();
        for (kind, surface_kind) in keys {
            let Some(owner) = set.file_kind(&kind, &surface_kind) else {
                continue;
            };
            let compiled = {
                let cx = crate::view::KindContext {
                    model: &self.model,
                    set,
                    kind: &kind,
                    surface_kind: &surface_kind,
                };
                let docs = self
                    .model
                    .entries
                    .get(&(kind.clone(), surface_kind.clone()))
                    .map_or(&[][..], Vec::as_slice);
                owner.item.compile(docs, &cx)
            };
            match compiled {
                Ok(section) => self.model.sections.push(PackSection {
                    pack: owner.pack.clone(),
                    kind,
                    surface_kind,
                    format: section.format,
                    payload: section.payload,
                }),
                Err(problems) => self.problems.extend(problems),
            }
        }
    }

    // ---- operations ----

    fn tool_name(&mut self, name: &Located<Doc>) -> Option<OperationName> {
        let text = name.as_text()?;
        let Ok(operation) = OperationName::new(text) else {
            self.error(
                codes::NAME_MALFORMED,
                Stage::Validate,
                &name.at,
                format!("`{}` is not a tool name.", shown(text)),
                "Write one lowercase name of at most 64 characters, such as `example_order_get`."
                    .to_owned(),
            );
            return None;
        };
        if let Some(first) = self.tool_names.get(text) {
            let message =
                format!("The tool name `{text}` is defined twice; it is first defined at {first}.");
            self.error(
                codes::NAME_DEFINED_TWICE,
                Stage::Resolve,
                &name.at,
                message,
                "Rename one of the two; an operation and a pipeline share one space of names."
                    .to_owned(),
            );
            return None;
        }
        self.tool_names.insert(text.to_owned(), name.at.clone());
        if let Some(target) = self.model.target.clone() {
            let rest = text
                .strip_prefix(target.as_str())
                .and_then(|rest| rest.strip_prefix('_'));
            let in_module = self.model.modules.is_empty()
                || rest.is_some_and(|rest| {
                    self.model.modules.iter().any(|module| {
                        rest.strip_prefix(module.as_str())
                            .is_some_and(|rest| rest.starts_with('_'))
                    })
                });
            if !operation.belongs_to(&target) || !in_module {
                let scheme = if self.model.modules.is_empty() {
                    format!("{target}_{{object}}_{{verb}}")
                } else {
                    format!(
                        "{target}_{{module}}_{{object}}_{{verb}}, with a module of `target.yaml`"
                    )
                };
                self.error(
                    codes::TOOL_NAME_SCHEME,
                    Stage::Lint,
                    &name.at,
                    format!("The tool name `{text}` does not follow the naming scheme of the target `{target}`."),
                    format!("Name it as {scheme}."),
                );
            }
        }
        Some(operation)
    }

    fn inputs(&mut self, doc: &Located<Doc>) -> Vec<InputInfo> {
        let mut inputs = Vec::new();
        for entry in doc.get("inputs").map(Located::entries).unwrap_or_default() {
            let name = entry.key.value.as_str();
            if !is_simple_name(name) {
                self.error(
                    codes::NAME_MALFORMED,
                    Stage::Validate,
                    &entry.key.at,
                    format!("`{}` is not an input name.", shown(name)),
                    "Write one lowercase name, such as `order_id`.".to_owned(),
                );
                continue;
            }
            let Some(type_doc) = entry.value.get("type") else {
                continue;
            };
            let Some(type_name) = type_doc.as_text() else {
                continue;
            };
            let Some((field_type, ty)) =
                self.field_type(&Located::new(type_name.to_owned(), type_doc.at.clone()))
            else {
                continue;
            };
            let default = entry.value.get("default").and_then(|default| {
                let value = value_of(default, &ty);
                if value.is_none() {
                    self.error(
                        codes::VALUE_NOT_OF_TYPE,
                        Stage::Resolve,
                        &default.at,
                        format!(
                            "The default of `{name}` is a {}, and the input is a {}.",
                            default.value.kind(),
                            ty.name()
                        ),
                        format!("Write a {} here; text goes in quotes.", ty.name()),
                    );
                }
                value
            });
            inputs.push(InputInfo {
                plan: InputPlan {
                    name: name.to_owned(),
                    field_type,
                    ty,
                    required: entry
                        .value
                        .get("required")
                        .and_then(Located::as_bool)
                        .unwrap_or(true),
                    default,
                    description: entry
                        .value
                        .text("description")
                        .unwrap_or_default()
                        .to_owned(),
                },
                at: entry.key.at.clone(),
            });
        }
        inputs
    }

    fn identity(
        &mut self,
        doc: &Located<Doc>,
        effect: EffectClass,
        inputs: &[InputInfo],
    ) -> (IdentityStrategy, Vec<String>, Option<Place>) {
        let names = |list: &Located<Doc>| -> Vec<Located<String>> {
            match &list.value {
                Doc::Text(text) => vec![Located::new(text.clone(), list.at.clone())],
                _ => list
                    .items()
                    .iter()
                    .filter_map(|item| {
                        item.as_text()
                            .map(|text| Located::new(text.to_owned(), item.at.clone()))
                    })
                    .collect(),
            }
        };
        let invalid = |this: &mut Self, at: &Place, message: String, fix: &str| {
            this.error(
                codes::IDENTITY_INVALID,
                Stage::Analyze,
                at,
                message,
                fix.to_owned(),
            );
        };
        let mut fixed = None;
        let (strategy, key, at) = match (doc.get("key"), doc.get("identity")) {
            (Some(key), Some(_)) => {
                invalid(
                    self,
                    &key.at,
                    "The operation states both `key` and `identity`.".to_owned(),
                    "Keep one: `key` is the short form of `identity` for `natural` and `composite`.",
                );
                return (IdentityStrategy::None, Vec::new(), None);
            }
            (Some(key), None) => {
                let strategy = if matches!(key.value, Doc::Text(_)) {
                    IdentityStrategy::Natural
                } else {
                    IdentityStrategy::Composite
                };
                (strategy, names(key), key.at.clone())
            }
            (None, Some(identity)) => {
                let strategy = identity
                    .text("strategy")
                    .and_then(|strategy| strategy.parse::<IdentityStrategy>().ok())
                    .unwrap_or(IdentityStrategy::None);
                let key = identity.get("inputs").map(names).unwrap_or_default();
                fixed = identity.get("place").map(|place| place.at.clone());
                (strategy, key, identity.at.clone())
            }
            (None, None) => {
                if effect == EffectClass::Write {
                    self.error(
                        codes::WRITE_WITHOUT_IDENTITY,
                        Stage::Analyze,
                        &doc.at,
                        "The operation writes and does not declare how its record is found again.".to_owned(),
                        "Add `key:` with the input, or the inputs, that identify the record; or `identity:` with a strategy; or declare the effect `irreversible`.".to_owned(),
                    );
                }
                return (IdentityStrategy::None, Vec::new(), None);
            }
        };
        let mut fine = true;
        for name in &key {
            let input = inputs.iter().find(|input| input.plan.name == name.value);
            match input {
                None => {
                    let known: Vec<&str> = inputs
                        .iter()
                        .map(|input| input.plan.name.as_str())
                        .collect();
                    invalid(
                        self,
                        &name.at,
                        format!(
                            "The identity names `{}`, which is not an input of the operation.",
                            shown(&name.value)
                        ),
                        &fix_for_unknown(&name.value, &known),
                    );
                    fine = false;
                }
                Some(input) if !input.plan.required => {
                    invalid(
                        self,
                        &name.at,
                        format!(
                            "The identity input `{}` is not required, so a call may have no record key.",
                            name.value
                        ),
                        "Make the input required.",
                    );
                    fine = false;
                }
                Some(_) => {}
            }
        }
        if let Some(place) = &fixed {
            // The record is the one place every binding names: no input
            // chooses it, and nothing but `natural` finds a record by its
            // own key in the system (decision 0252).
            let why = if strategy != IdentityStrategy::Natural {
                Some(format!(
                    "`place: fixed` says the record is found by its own key, the place, and the strategy is `{strategy}`."
                ))
            } else if !key.is_empty() {
                Some("`place: fixed` says no input chooses the record, and the identity names inputs.".to_owned())
            } else {
                None
            };
            if let Some(why) = why {
                self.error(
                    codes::PLACE_NOT_FIXED,
                    Stage::Analyze,
                    place,
                    why,
                    "Write `identity: {strategy: natural, place: fixed}` with no `inputs`, or name the inputs that are the record's key and leave out `place`.".to_owned(),
                );
                return (IdentityStrategy::None, Vec::new(), None);
            }
            return (strategy, Vec::new(), fixed);
        }
        let count_fits = match strategy {
            IdentityStrategy::Natural => key.len() == 1,
            IdentityStrategy::Composite => !key.is_empty(),
            IdentityStrategy::None => key.is_empty(),
            IdentityStrategy::Stamped | IdentityStrategy::Find => true,
        };
        if !count_fits {
            invalid(
                self,
                &at,
                format!(
                    "The strategy `{strategy}` does not take {} identity inputs.",
                    key.len()
                ),
                "`natural` takes one input, `composite` takes one or more, and `none` takes none.",
            );
            fine = false;
        }
        if strategy == IdentityStrategy::None && effect == EffectClass::Write {
            invalid(
                self,
                &at,
                "The strategy `none` says the record cannot be found again, and the operation's effect is `write`.".to_owned(),
                "Declare the effect `irreversible`, or choose a strategy that finds the record.",
            );
            fine = false;
        }
        if fine {
            (
                strategy,
                key.into_iter().map(|name| name.value).collect(),
                None,
            )
        } else {
            (IdentityStrategy::None, Vec::new(), None)
        }
    }

    fn operations(&mut self) {
        let Some(schema) = k::schema_of(k::OPERATION) else {
            return;
        };
        let docs: Vec<&Located<Doc>> = self.sources.docs_of(k::OPERATION).collect();
        for doc in docs {
            if !self.fits(schema, doc) {
                continue;
            }
            let Some(name) = doc.get("operation").and_then(|name| self.tool_name(name)) else {
                continue;
            };
            let effect = doc
                .text("effect")
                .and_then(|effect| effect.parse::<EffectClass>().ok())
                .unwrap_or(EffectClass::Read);
            let hazard = doc
                .text("hazard")
                .and_then(|hazard| hazard.parse::<Hazard>().ok())
                .unwrap_or(Hazard::None);
            if hazard != Hazard::None && effect != EffectClass::Irreversible {
                let at = doc
                    .get("effect")
                    .map_or(&doc.at, |effect| &effect.at)
                    .clone();
                self.error(
                    codes::EFFECT_LOWER_THAN_DERIVED,
                    Stage::Analyze,
                    &at,
                    format!("The operation declares the hazard `{hazard}` and the effect `{effect}`; an operation with a hazard is `irreversible`."),
                    "Declare `effect: irreversible`.".to_owned(),
                );
            }
            let inputs = self.inputs(doc);
            let mut outputs = Vec::new();
            for entry in doc.get("outputs").map(Located::entries).unwrap_or_default() {
                let Some(type_doc) = entry.value.get("type") else {
                    continue;
                };
                let Some(type_name) = type_doc.as_text() else {
                    continue;
                };
                let Some((field_type, ty)) =
                    self.field_type(&Located::new(type_name.to_owned(), type_doc.at.clone()))
                else {
                    continue;
                };
                let list = entry
                    .value
                    .get("list")
                    .and_then(Located::as_bool)
                    .unwrap_or(false);
                if !is_simple_name(&entry.key.value) {
                    self.error(
                        codes::NAME_MALFORMED,
                        Stage::Validate,
                        &entry.key.at,
                        format!("`{}` is not an output name.", shown(&entry.key.value)),
                        "Write one lowercase name, such as `status`.".to_owned(),
                    );
                    continue;
                }
                let free_text = self
                    .model
                    .field_types
                    .get(field_type.get())
                    .is_some_and(|known| known.name == "text" && !known.untrusted);
                if free_text {
                    self.problems.push(
                        Diagnostic::error(
                            kernel(codes::OUTPUT_NOT_MARKED),
                            Stage::Lint,
                            &type_doc.at,
                            format!("The output `{}` is free text from the system and its field type is not marked untrusted.", entry.key.value),
                            "Define a field type with `base: text` and `untrusted: true` in `types.yaml`, and use it here; or use a narrower type such as `identifier`.",
                        )
                        .as_warning(),
                    );
                }
                outputs.push(OutputPlan {
                    name: entry.key.value.clone(),
                    ty: if list { ValueType::list_of(ty) } else { ty },
                    field_type: Some(field_type),
                    may_be_absent: entry
                        .value
                        .get("may_be_absent")
                        .and_then(Located::as_bool)
                        .unwrap_or(false),
                });
            }
            let (strategy, key, fixed) = self.identity(doc, effect, &inputs);
            let info = OpInfo {
                name,
                doc: doc.clone(),
                effect,
                hazard,
                inputs,
                outputs,
                strategy,
                key,
                fixed,
                commit_points: doc
                    .get("commit_points")
                    .and_then(Located::as_int)
                    .map(to_u32),
            };
            let examples = self.examples(doc, &info);
            let decision_points = self.decision_points(doc);
            if let (Some(mut plan), Some(examples), Some(decision_points)) =
                (self.bindings(&info), examples, decision_points)
            {
                plan.examples = examples;
                plan.decision_points = decision_points;
                let title = doc.text("title").unwrap_or(info.name.as_str()).to_owned();
                let description = doc.text("description").unwrap_or_default().to_owned();
                self.op_plans.push((plan, title, description));
                self.operations.push(info);
            }
        }
    }

    /// An operation's example inputs, each typed by the inputs it names.
    fn examples(&mut self, doc: &Located<Doc>, info: &OpInfo) -> Option<Vec<Values>> {
        let mut fine = true;
        let mut examples = Vec::new();
        let listed = doc.get("examples").map(Located::items).unwrap_or_default();
        if listed.len() > 3 {
            self.error(
                codes::WRONG_TYPE,
                Stage::Validate,
                &listed[3].at,
                format!(
                    "The operation has {} examples, and a tool shows at most three.",
                    listed.len()
                ),
                "Keep the one to three examples that show the tool best.".to_owned(),
            );
            fine = false;
        }
        for example in listed {
            let mut values = Values::new();
            for given in example.entries() {
                let Some(input) = info
                    .inputs
                    .iter()
                    .find(|input| input.plan.name == given.key.value)
                else {
                    let known: Vec<&str> = info
                        .inputs
                        .iter()
                        .map(|input| input.plan.name.as_str())
                        .collect();
                    let fix = fix_for_unknown(&given.key.value, &known);
                    self.error(
                        codes::UNKNOWN_NAME,
                        Stage::Resolve,
                        &given.key.at,
                        format!(
                            "`{}` is not an input of `{}`.",
                            shown(&given.key.value),
                            info.name
                        ),
                        fix,
                    );
                    fine = false;
                    continue;
                };
                match value_of(&given.value, &input.plan.ty) {
                    Some(value) => {
                        values.insert(input.plan.name.clone(), value);
                    }
                    None => {
                        self.error(
                            codes::VALUE_NOT_OF_TYPE,
                            Stage::Resolve,
                            &given.value.at,
                            format!(
                                "The example gives `{}` a {}, and the input is a {}.",
                                input.plan.name,
                                given.value.value.kind(),
                                input.plan.ty.name()
                            ),
                            format!(
                                "Write a {} here; text goes in quotes.",
                                input.plan.ty.name()
                            ),
                        );
                        fine = false;
                    }
                }
            }
            examples.push(values);
        }
        fine.then_some(examples)
    }

    /// The questions an operation declares that a person may have to answer.
    fn decision_points(&mut self, doc: &Located<Doc>) -> Option<Vec<DecisionPoint>> {
        let mut fine = true;
        let mut points: Vec<DecisionPoint> = Vec::new();
        for point in doc
            .get("decision_points")
            .map(Located::items)
            .unwrap_or_default()
        {
            let (Some(name), Some(question)) = (point.get("name"), point.text("question")) else {
                continue;
            };
            let text = name.as_text().unwrap_or_default();
            if !is_simple_name(text) || points.iter().any(|known| known.name == text) {
                self.error(
                    codes::NAME_DEFINED_TWICE,
                    Stage::Resolve,
                    &name.at,
                    format!("The decision point `{}` is named twice in the operation, or is not one name.", shown(text)),
                    "Give each decision point its own lowercase name.".to_owned(),
                );
                fine = false;
                continue;
            }
            points.push(DecisionPoint {
                name: text.to_owned(),
                question: question.to_owned(),
            });
        }
        fine.then_some(points)
    }

    fn bindings(&mut self, info: &OpInfo) -> Option<OperationPlan> {
        let set = self.set;
        let doc = &info.doc;
        let kernel_part = k::binding();
        let writes = info.effect >= EffectClass::Write;
        let declared_commits = info.commit_points.unwrap_or(u32::from(writes));
        let env = type_env(&[("input", input_type(&info.inputs)), ("env", env_type())]);
        // A binding's steps may also read what the engine's last read of the
        // record gave, and the record's key in the system.
        let steps_env = type_env(&[
            (names::INPUT, input_type(&info.inputs)),
            (names::ENV, env_type()),
            (names::CURRENT, current_type(&info.inputs)),
            (names::RECORD, record_type()),
        ]);
        let mut bindings = Vec::new();
        // The outputs a lowering finds may be absent (decision 0305).
        let mut absent: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut review = OpReview::default();
        let mut fine = true;
        // Whatever a pack marks, a write saves at least once and a read
        // saves nothing: the count is what puts an intent in the ledger.
        if writes != (declared_commits > 0) {
            let at = doc
                .get("commit_points")
                .map_or(&doc.at, |declared| &declared.at)
                .clone();
            let (message, fix) = if writes {
                (
                    format!(
                        "The operation declares the effect `{}` and no commit point; a write saves at least once, and each save is recorded before it is made.",
                        info.effect
                    ),
                    "Write `commit_points:` with the number of times the system saves, at least 1; or declare `effect: read` if nothing is saved.",
                )
            } else {
                (
                    format!(
                        "The operation declares the effect `read` and {declared_commits} commit points; a read saves nothing."
                    ),
                    "Remove `commit_points`, or declare `effect: write` with a key and a read-back.",
                )
            };
            self.error(
                codes::COMMIT_POINTS_DIFFER,
                Stage::Analyze,
                &at,
                message,
                fix.to_owned(),
            );
            fine = false;
        }
        fine &= self.scope(doc);
        for (position, binding) in doc
            .get("bindings")
            .map(Located::items)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            if !self.fits(
                &kernel_part,
                &part_of(binding, |key| BINDING_KEYS.contains(&key)),
            ) {
                fine = false;
                continue;
            }
            let Some(mode_doc) = binding.get("mode") else {
                continue;
            };
            let Some(mode) = mode_doc.as_text().and_then(|mode| Mode::new(mode).ok()) else {
                self.error(
                    codes::UNKNOWN_MODE,
                    Stage::Lower,
                    &mode_doc.at,
                    "This is not the name of a mode.".to_owned(),
                    "Write the name of a mode an enabled pack registered.".to_owned(),
                );
                fine = false;
                continue;
            };
            let modes = set.modes();
            if !modes.contains(&&mode) {
                let known: Vec<&str> = modes.iter().map(|mode| mode.as_str()).collect();
                self.error(
                    codes::UNKNOWN_MODE,
                    Stage::Lower,
                    &mode_doc.at,
                    format!("No enabled pack registered a mode named `{mode}`."),
                    format!(
                        "{} Or enable the pack that serves the mode `{mode}`.",
                        fix_for_unknown(mode.as_str(), &known)
                    ),
                );
                fine = false;
                continue;
            }
            let verbs = set.verbs(&mode);
            let known_verbs: Vec<&str> = verbs.iter().map(|verb| verb.as_str()).collect();
            let stated = binding.get("verb");
            let verb = match stated {
                Some(verb) => verb.as_text().and_then(|verb| Verb::new(verb).ok()),
                None => set.default_verbs.get(&mode).cloned(),
            };
            let registered = verb
                .as_ref()
                .and_then(|verb| set.lowerings.get(&(mode.clone(), verb.clone())));
            let (Some(verb), Some(registered)) = (verb, registered) else {
                let (at, message) = match stated {
                    Some(verb) => (
                        &verb.at,
                        format!(
                            "No lowering is registered for the verb `{}` of the mode `{mode}`.",
                            shown(verb.as_text().unwrap_or_default())
                        ),
                    ),
                    None => (
                        &binding.at,
                        format!(
                            "The binding states no verb, and the mode `{mode}` has no default verb."
                        ),
                    ),
                };
                self.error(
                    codes::UNKNOWN_VERB,
                    Stage::Lower,
                    at,
                    message,
                    format!("Write `verb:` with one of: {}.", known_verbs.join(", ")),
                );
                fine = false;
                continue;
            };
            let lowering = &registered.item;
            let pack_part = part_of(binding, |key| !BINDING_KEYS.contains(&key));
            if !self.fits(lowering.binding_schema(), &pack_part) {
                fine = false;
                continue;
            }
            let mut readable = true;
            for (kind, surface_kind) in lowering.reads() {
                if set.file_kind(kind, surface_kind).is_none() {
                    self.error(
                        codes::READ_KIND_NOT_REGISTERED,
                        Stage::Lower,
                        &mode_doc.at,
                        format!("The mode `{mode}` reads the file kind `{kind}` for `{surface_kind}` surfaces, and no enabled pack owns it."),
                        format!("Enable the pack that owns `{kind}` files, or use another mode."),
                    );
                    readable = false;
                }
            }
            if !readable {
                fine = false;
                continue;
            }
            let status = binding
                .text("status")
                .and_then(|status| status.parse::<BindingStatus>().ok())
                .unwrap_or(BindingStatus::Draft);
            let precondition = match binding.get("precondition") {
                Some(precondition) => match self.expr(precondition, &env, Some(&ValueType::Bool)) {
                    Some((index, _)) => Some(index),
                    None => {
                        fine = false;
                        continue;
                    }
                },
                None => None,
            };
            let resolved = ResolvedBinding {
                operation: OperationView { info },
                index: position,
                mode: &mode,
                verb: &verb,
                status,
                doc: binding,
            };
            if info.strategy != IdentityStrategy::None
                && let Possible::No(why) = lowering.identity(&resolved, info.strategy)
            {
                self.error(
                    codes::IDENTITY_NOT_POSSIBLE,
                    Stage::Analyze,
                    &binding.at,
                    format!("The identity strategy `{}` is not possible in this `{mode}` binding: {why}", info.strategy),
                    "Choose a strategy every binding of the operation can carry out, or remove the binding.".to_owned(),
                );
                fine = false;
            }
            let least = lowering.least_effect(&resolved);
            let lowered = {
                let cx = LowerContext {
                    model: &self.model,
                    pack: &registered.pack,
                    reads: lowering.reads(),
                    operation: info,
                    env: steps_env.clone(),
                };
                lowering.lower(&resolved, &cx).map(|lowered| {
                    let statement = lowering.statement(&resolved, &lowered, &cx);
                    (lowered, statement)
                })
            };
            let (lowered, statement) = match lowered {
                Ok(lowered) => lowered,
                Err(problems) => {
                    self.problems.extend(problems);
                    fine = false;
                    continue;
                }
            };
            if let Some(written) = &info.fixed {
                match self.fixed_place_of(lowering.as_ref(), &registered.pack, &resolved, &lowered)
                {
                    Ok(place) => match &review.place {
                        Some(first) if *first != place => {
                            self.error(
                                codes::PLACE_NOT_FIXED,
                                Stage::Analyze,
                                &binding.at,
                                format!(
                                    "This binding names the place `{}` of `{}` files on `{}`, and an earlier binding names `{}` of `{}` files on `{}`; the record is one place.",
                                    shown(&place.name), place.kind, place.surface,
                                    shown(&first.name), first.kind, first.surface
                                ),
                                "Name the same place in every binding of the operation, or write each place's operation on its own.".to_owned(),
                            );
                            fine = false;
                        }
                        _ => review.place = Some(place),
                    },
                    Err((message, fix)) => {
                        let at = if binding.at.file == written.file {
                            binding.at.clone()
                        } else {
                            written.clone()
                        };
                        self.error(codes::PLACE_NOT_FIXED, Stage::Analyze, &at, message, fix);
                        fine = false;
                    }
                }
            }
            absent.extend(lowering.absent_outputs(&resolved, &lowered));
            let evidence = lowering.evidence(&resolved, &lowered);
            let by = LoweredBy {
                lowering: lowering.as_ref(),
                pack: &registered.pack,
                verb: &verb,
                least,
                evidence,
            };
            match self.analyze(info, binding, &mode, &by, &lowered, declared_commits) {
                Some(surface) => bindings.push({
                    review.bindings.push((mode.clone(), surface, statement));
                    BindingPlan {
                        mode,
                        verb,
                        status,
                        surface,
                        act: lowered.act,
                        confirm: lowered.confirm,
                        read_back: lowered.read_back,
                        lookup: lowered.lookup,
                        settle_ms: lowered.settle_ms,
                        evidence,
                        precondition,
                    }
                }),
                None => fine = false,
            }
        }
        if !fine {
            return None;
        }
        self.reviews.insert(info.name.clone(), review);
        let key: Vec<InputIndex> = info
            .key
            .iter()
            .filter_map(|name| {
                info.inputs
                    .iter()
                    .position(|input| &input.plan.name == name)
            })
            .map(|position| InputIndex::new(index(position)))
            .collect();
        let mut surfaces: Vec<SurfaceIndex> =
            bindings.iter().map(|binding| binding.surface).collect();
        surfaces.sort();
        surfaces.dedup();
        let list = |key: &str| -> Vec<String> {
            doc.get("permissions")
                .and_then(|permissions| permissions.get(key))
                .map(|items| {
                    items
                        .items()
                        .iter()
                        .filter_map(|item| item.as_text().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };
        Some(OperationPlan {
            name: info.name.clone(),
            effect: info.effect,
            hazard: info.hazard,
            inputs: info.inputs.iter().map(|input| input.plan.clone()).collect(),
            outputs: info
                .outputs
                .iter()
                .map(|output| OutputPlan {
                    may_be_absent: output.may_be_absent || absent.contains(&output.name),
                    ..output.clone()
                })
                .collect(),
            identity: IdentityPlan {
                strategy: info.strategy,
                key,
            },
            commit_points: declared_commits,
            bindings,
            manifest: PermissionManifest {
                surfaces,
                uploads: list("uploads"),
                downloads: list("downloads"),
                destinations: list("destinations"),
            },
            examples: Vec::new(),
            decision_points: Vec::new(),
        })
    }

    /// The fixed place one binding names, held to what the file kind that
    /// declares it says of the documents on the binding's surface
    /// (decision 0252). A lowering names the place; only its file kind,
    /// reading the target's own document, can say that the place is fixed.
    fn fixed_place_of(
        &self,
        lowering: &dyn Lowering,
        pack: &str,
        resolved: &ResolvedBinding<'_>,
        lowered: &Lowered,
    ) -> Result<PlaceReview, (String, String)> {
        let mode = resolved.mode;
        let Some(named) = lowering.fixed_place(resolved) else {
            return Err((
                format!("The operation's record is one fixed place, and this `{mode}` binding names no fixed place."),
                "Name one place the target's files fix, in the way the mode's pack reads it; or give the operation the inputs that are the record's key and leave out `place: fixed`.".to_owned(),
            ));
        };
        let Some(surface) = self.model.surface(lowered.surface.as_str()) else {
            return Err((
                format!("The `{mode}` binding acts on a surface the target does not declare."),
                "Correct the pack's lowering.".to_owned(),
            ));
        };
        let kind = &named.kind;
        let owned = self.set.file_kind(kind, surface.kind).filter(|registered| {
            registered.pack == pack
                || lowering
                    .reads()
                    .iter()
                    .any(|(read, of)| read == kind && of == surface.kind)
        });
        let Some(registered) = owned else {
            return Err((
                format!(
                    "The `{mode}` binding names a place of `{kind}` files, which its pack neither owns nor reads for `{}` surfaces.",
                    surface.kind
                ),
                "Correct the pack's lowering: a fixed place is declared by a file kind of its own pack.".to_owned(),
            ));
        };
        let declared = self
            .model
            .entries
            .get(&(kind.clone(), surface.kind.clone()))
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter(|doc| {
                self.model
                    .surface_of(doc, kind, self.set)
                    .is_some_and(|on| on.name == surface.name)
            })
            .any(|doc| {
                registered
                    .item
                    .fixed_places(doc)
                    .iter()
                    .any(|place| place.value.name == named.name)
            });
        if !declared {
            return Err((
                format!(
                    "The `{mode}` binding names `{}`, which no `{kind}` file on the surface `{}` declares a fixed place.",
                    shown(&named.name),
                    surface.name
                ),
                "Name a place that always exists and that no input chooses, or give the operation the inputs that are the record's key and leave out `place: fixed`.".to_owned(),
            ));
        }
        Ok(PlaceReview {
            surface: surface.name.clone(),
            kind: kind.clone(),
            name: named.name,
        })
    }

    /// Stage 6 for one lowered binding: its surface, its step kinds, its
    /// commit points, its read-back. Gives the surface when all hold.
    fn analyze(
        &mut self,
        info: &OpInfo,
        binding: &Located<Doc>,
        mode: &Mode,
        by: &LoweredBy<'_>,
        lowered: &Lowered,
        declared_commits: u32,
    ) -> Option<SurfaceIndex> {
        let mut fine = true;
        let step_kinds = by.lowering.step_kinds();
        let found = self
            .model
            .surface(lowered.surface.as_str())
            .map(|surface| (surface.index, surface.kind.clone()));
        let surface = found.as_ref().map(|(index, _)| *index);
        // A lowering acts on a surface of a kind its own pack serves: its
        // steps are run by its pack's driver, which knows no other kind.
        if let Some((_, kind)) = &found
            && let Some(owner) = self.set.surface_kinds.get(kind)
            && owner.pack != by.pack
        {
            self.error(
                codes::SURFACE_OF_ANOTHER_PACK,
                Stage::Lower,
                &binding.at,
                format!("The lowering of this `{mode}` binding, from the pack `{}`, names the surface `{}` of the kind `{kind}`, which the pack `{}` registered.", by.pack, lowered.surface, owner.pack),
                "Bind the operation to a surface of a kind the mode's own pack serves, or correct the pack.".to_owned(),
            );
            fine = false;
        }
        // A binding whose only evidence is an acknowledgement cannot be
        // read back or compared, so it is irreversible by what it does.
        let least = if writes_by_steps(lowered) && by.evidence == WriteEvidence::Acknowledged {
            EffectClass::Irreversible
        } else {
            by.least
        };
        if least > info.effect {
            let at = info
                .doc
                .get("effect")
                .map_or(&info.doc.at, |effect| &effect.at)
                .clone();
            self.error(
                codes::EFFECT_LOWER_THAN_DERIVED,
                Stage::Analyze,
                &at,
                format!("The operation declares the effect `{}`, and its `{mode}` binding with the verb `{}` is `{least}` by what it does{}.", info.effect, by.verb, if least > by.least { ", because nothing it writes can be read back" } else { "" }),
                format!("Declare `effect: {least}`, or bind the operation to something that does less."),
            );
            fine = false;
        }
        if surface.is_none() {
            self.error(
                codes::LOWERING_INVALID,
                Stage::Lower,
                &binding.at,
                format!("The lowering of this `{mode}` binding names the surface `{}`, which the target does not declare.", lowered.surface),
                "Declare the surface in `target.yaml`, or correct what the binding refers to.".to_owned(),
            );
            fine = false;
        }
        let lists: [(&str, &Vec<Step>); 4] = [
            ("act", &lowered.act),
            ("confirm", &lowered.confirm),
            ("read_back", &lowered.read_back),
            ("lookup", &lowered.lookup),
        ];
        for (list, steps) in lists {
            for step in steps {
                if !step_kinds.contains(&step.kind) {
                    self.error(
                        codes::UNKNOWN_STEP_KIND,
                        Stage::Lower,
                        &binding.at,
                        format!("The lowering of this `{mode}` binding gave a step of kind `{}`, which it did not declare.", step.kind),
                        "Correct the pack: a lowering declares every step kind its steps may carry.".to_owned(),
                    );
                    fine = false;
                }
                let stated = by.lowering.commits(&step.kind);
                if (stated == StepCommits::Never && step.commits)
                    || (stated == StepCommits::Always && !step.commits)
                {
                    let (marked, declared) = if step.commits {
                        ("is marked as a save", "never saves")
                    } else {
                        ("is not marked as a save", "always saves")
                    };
                    self.error(
                        codes::LOWERING_INVALID,
                        Stage::Lower,
                        &binding.at,
                        format!("A `{list}` step of kind `{}` of this `{mode}` binding {marked}, and its pack declares that a step of that kind {declared}.", step.kind),
                        "Correct the pack: whether a kind of step saves is stated once for the kind, and every lowered step agrees with it.".to_owned(),
                    );
                    fine = false;
                }
                if step.commits && list != "act" {
                    self.error(
                        codes::LOWERING_INVALID,
                        Stage::Lower,
                        &binding.at,
                        format!("A `{list}` step of this `{mode}` binding commits; only a step that does the work may save."),
                        "Correct the pack: a step that confirms, reads back, or looks up never commits.".to_owned(),
                    );
                    fine = false;
                }
            }
        }
        let commits = index(lowered.act.iter().filter(|step| step.commits).count());
        let writes = info.effect >= EffectClass::Write;
        if !writes && commits > 0 {
            let at = info
                .doc
                .get("effect")
                .map_or(&info.doc.at, |effect| &effect.at)
                .clone();
            self.error(
                codes::EFFECT_LOWER_THAN_DERIVED,
                Stage::Analyze,
                &at,
                format!("The operation declares the effect `read`, and its `{mode}` binding has {commits} steps that save."),
                "Declare `effect: write`, with a key and a read-back, or bind the operation to something that only reads.".to_owned(),
            );
            fine = false;
        }
        if writes && commits != declared_commits {
            let at = info
                .doc
                .get("commit_points")
                .map_or(&binding.at, |declared| &declared.at)
                .clone();
            self.error(
                codes::COMMIT_POINTS_DIFFER,
                Stage::Analyze,
                &at,
                format!("The operation declares {declared_commits} commit points, and its `{mode}` binding has {commits} steps that save."),
                format!("Write `commit_points: {commits}` if every binding saves {commits} times, or correct the binding."),
            );
            fine = false;
        }
        let needs_read_back = by.evidence != WriteEvidence::Acknowledged
            && (info.effect == EffectClass::Write
                || (info.effect == EffectClass::Irreversible
                    && info.strategy != IdentityStrategy::None));
        // The plan's check asks the same question (decision 0193).
        if by.evidence == WriteEvidence::Acknowledged
            && !a2m_core::acknowledgement_may_stand(info.strategy, &lowered.read_back)
        {
            self.error(
                codes::ACKNOWLEDGED_WHERE_READ_BACK,
                Stage::Analyze,
                &binding.at,
                format!("This `{mode}` binding reads its record back by its key, and its pack states the system's acknowledgement as its only evidence: a record that can be read back is verified by that read."),
                "Leave the read-back as the binding's evidence, or, where the record truly cannot be read again, declare the operation with no `key` or `identity`.".to_owned(),
            );
            fine = false;
        }
        // A lookup beside an acknowledgement is asked only whether the
        // record landed, never for its values (decision 0196): one that
        // returns the record's values is a read-back by another name, and
        // the author is told so. A warning, in every mode.
        if by.evidence == WriteEvidence::Acknowledged
            && info.strategy != IdentityStrategy::None
            && lowered.read_back.is_empty()
            && !lowered.lookup.is_empty()
        {
            self.problems.push(
                Diagnostic::error(
                    kernel(codes::ACKNOWLEDGED_WITH_LOOKUP),
                    Stage::Lint,
                    &binding.at,
                    format!("This `{mode}` binding ends its keyed record on an acknowledgement and declares a lookup; the engine asks the lookup only whether the record landed."),
                    "A lookup that returns the record's values is a read-back: name it `read_back:` and make the read-back its evidence.",
                )
                .as_warning(),
            );
        }
        if needs_read_back && (binding.get("read_back").is_none() || lowered.read_back.is_empty()) {
            self.error(
                codes::WRITE_WITHOUT_READ_BACK,
                Stage::Analyze,
                &binding.at,
                format!("This `{mode}` binding writes and does not read the record back."),
                "Add `read_back:` naming how the record is read, reached from the identity inputs alone.".to_owned(),
            );
            fine = false;
        }
        if fine { surface } else { None }
    }

    /// The Scope rule for one operation's `permissions`: each destination is
    /// one exact address, each upload and download one name, and each is
    /// listed once. True when all hold.
    fn scope(&mut self, doc: &Located<Doc>) -> bool {
        let mut fine = true;
        let Some(permissions) = doc.get("permissions") else {
            return true;
        };
        for key in ["uploads", "downloads", "destinations"] {
            let mut seen: Vec<&str> = Vec::new();
            for item in permissions.get(key).map(Located::items).unwrap_or_default() {
                let Some(text) = item.as_text() else {
                    continue;
                };
                let problem = if seen.contains(&text) {
                    Some("it is listed twice")
                } else if key == "destinations" {
                    destination_problem(text)
                } else if is_permission_name(text) {
                    None
                } else {
                    Some("it is not one name of letters, digits, underscores, and hyphens")
                };
                seen.push(text);
                if let Some(problem) = problem {
                    let (what, fix) = if key == "destinations" {
                        (
                            "destination",
                            "Write one exact address: a host name such as `files.example.com`, with a port or a scheme and `://` before it where needed, and nothing after it.",
                        )
                    } else {
                        (
                            "upload or download",
                            "Write the name of the input or of the allowed root, such as `order_sheet`.",
                        )
                    };
                    self.error(
                        codes::SCOPE_INVALID,
                        Stage::Lint,
                        &item.at,
                        format!("This {what} is not one the operation may declare: {problem}."),
                        fix.to_owned(),
                    );
                    fine = false;
                }
            }
        }
        fine
    }

    /// Takes out of every diagnostic the values the credential lint knows:
    /// a diagnostic at such a value's place never quotes it, and a value
    /// that does not read as a name is quoted nowhere.
    fn hide_credentials(&mut self) {
        let mut found = Vec::new();
        for file in &self.sources.files {
            for doc in &file.docs {
                credentials(None, doc, &mut found);
            }
        }
        if found.is_empty() {
            return;
        }
        for problem in &mut self.problems {
            for suspect in &found {
                let here = problem.at == suspect.at;
                if !here && !suspect.everywhere {
                    continue;
                }
                problem.message = without(&problem.message, &suspect.value, here);
                problem.fix = without(&problem.fix, &suspect.value, here);
            }
        }
    }

    // ---- sources/ ----

    fn source_files(&mut self) {
        let Some(schema) = k::schema_of(k::SOURCE) else {
            return;
        };
        let docs: Vec<&Located<Doc>> = self.sources.docs_of(k::SOURCE).collect();
        for doc in docs {
            if !self.fits(schema, doc) {
                continue;
            }
            let Some(template) = doc.get("template") else {
                continue;
            };
            let Some(name) = template.as_text() else {
                continue;
            };
            if self.source_plans.contains_key(name) {
                self.error(
                    codes::NAME_DEFINED_TWICE,
                    Stage::Resolve,
                    &template.at,
                    format!("The source template `{name}` is defined twice."),
                    "Rename one of the two.".to_owned(),
                );
                continue;
            }
            if let Some((plan, row)) = self.source(doc, name) {
                self.source_plans.insert(name.to_owned(), (plan, row));
            }
        }
    }

    /// One source definition: the plan's source, whose payload is the shape
    /// the source readers load (decision 0087), and the type of a row.
    fn source(&mut self, doc: &Located<Doc>, name: &str) -> Option<(SourcePlan, ValueType)> {
        let mut fine = true;
        let reader = doc.text("reader").unwrap_or_default();
        let texts = |key: &str| -> Vec<Located<String>> {
            doc.get(key)
                .map(Located::items)
                .unwrap_or_default()
                .iter()
                .filter_map(|item| {
                    item.as_text()
                        .map(|text| Located::new(text.to_owned(), item.at.clone()))
                })
                .collect()
        };
        let twice = |this: &mut Self, what: &str, names: &[Located<String>]| {
            let mut seen = BTreeSet::new();
            let mut fine = true;
            for name in names {
                if !seen.insert(name.value.clone()) {
                    this.error(
                        codes::NAME_DEFINED_TWICE,
                        Stage::Resolve,
                        &name.at,
                        format!("The {what} `{}` is listed twice.", shown(&name.value)),
                        "Remove one of the two.".to_owned(),
                    );
                    fine = false;
                }
            }
            fine
        };
        let columns = texts("columns");
        fine &= twice(self, "column", &columns);
        let versions: Vec<Located<String>> = doc
            .get("versions")
            .map(Located::items)
            .unwrap_or_default()
            .iter()
            .filter_map(|version| {
                version
                    .text("version")
                    .map(|text| Located::new(text.to_owned(), version.at.clone()))
            })
            .collect();
        fine &= twice(self, "template version", &versions);
        if versions.is_empty() && reader != OPERATION_READER {
            self.error(
                codes::MISSING_FIELD,
                Stage::Validate,
                &doc.at,
                format!("The source `{name}` is read from a file and knows no template version."),
                "Add `versions:` with the version and the fingerprint of each form of the file; `anything2mcp source inspect` gives a file's fingerprint.".to_owned(),
            );
            fine = false;
        }

        // Row fields: a bare column is data; an expression goes to the plan.
        let mut fields = Vec::new();
        let mut row = Vec::new();
        let mut written: Vec<(String, &Located<Doc>)> = Vec::new();
        for field in doc.get("fields").map(Located::entries).unwrap_or_default() {
            let field_name = field.key.value.as_str();
            if !is_simple_name(field_name) {
                self.error(
                    codes::NAME_MALFORMED,
                    Stage::Validate,
                    &field.key.at,
                    format!("`{}` is not a row field name.", shown(field_name)),
                    "Write one lowercase name, such as `code`.".to_owned(),
                );
                fine = false;
                continue;
            }
            let Some(type_doc) = field.value.get("type") else {
                continue;
            };
            let Some(type_name) = type_doc.as_text() else {
                continue;
            };
            let Some((field_type, ty)) =
                self.field_type(&Located::new(type_name.to_owned(), type_doc.at.clone()))
            else {
                fine = false;
                continue;
            };
            let required = field
                .value
                .get("required")
                .and_then(Located::as_bool)
                .unwrap_or(true);
            let column = field.value.get("column");
            let expression = field.value.get("expression");
            match (column, expression) {
                (Some(column), None) => {
                    let header = column.as_text().unwrap_or_default();
                    if !columns.iter().any(|known| known.value == header) {
                        let known: Vec<&str> =
                            columns.iter().map(|known| known.value.as_str()).collect();
                        let fix = fix_for_unknown(header, &known);
                        self.error(
                            codes::UNKNOWN_NAME,
                            Stage::Resolve,
                            &column.at,
                            format!("The row field `{field_name}` reads the column `{}`, which `columns` does not list.", shown(header)),
                            fix,
                        );
                        fine = false;
                    }
                }
                (None, Some(expression)) => written.push((field_name.to_owned(), expression)),
                _ => {
                    self.error(
                        codes::MISSING_FIELD,
                        Stage::Validate,
                        &field.value.at,
                        format!("The row field `{field_name}` needs one of `column` and `expression`, and has both or neither."),
                        "Give the header of the column it is read from under `column`, or an expression over `row` under `expression`.".to_owned(),
                    );
                    fine = false;
                }
            }
            row.push(if required {
                FieldOf::required(field_name, ty)
            } else {
                FieldOf::optional(field_name, ty)
            });
            fields.push(RowField {
                name: field_name.to_owned(),
                field_type,
                required,
                column: column.and_then(Located::as_text).map(str::to_owned),
            });
        }
        if fields.is_empty() {
            self.error(
                codes::MISSING_FIELD,
                Stage::Validate,
                &doc.at,
                format!("The source `{name}` has no row field."),
                "Add a row field under `fields:` with its `type` and its `column`.".to_owned(),
            );
            fine = false;
        }
        // An expression reads `row`, the record of the fields that are bare
        // columns, and `source`. Neither has a type until a file is read, so
        // an expression must end in a type of its own: a function's result,
        // or a comparison.
        let env = unknown_env(&["row", "source"]);
        let mut mapping = Vec::new();
        for (field_name, expression) in written {
            match self.expr(expression, &env, None) {
                Some((value, _)) => mapping.push(StepArg {
                    name: field_name,
                    value,
                }),
                None => fine = false,
            }
        }
        let rows = match doc.get("rows") {
            Some(rows) => {
                let checked = self.expr(rows, &env, Some(&ValueType::Bool));
                fine &= checked.is_some();
                checked.map(|(index, _)| index)
            }
            None => None,
        };
        let key = texts("key");
        for part in &key {
            if !row.iter().any(|field| field.name == part.value) {
                let known: Vec<&str> = row.iter().map(|field| field.name.as_str()).collect();
                let fix = fix_for_unknown(&part.value, &known);
                self.error(
                    codes::UNKNOWN_NAME,
                    Stage::Resolve,
                    &part.at,
                    format!(
                        "The key names `{}`, which is not a row field.",
                        shown(&part.value)
                    ),
                    fix,
                );
                fine = false;
            }
        }
        let delimiter = doc
            .get("options")
            .and_then(|options| options.get("delimiter"));
        if let Some(delimiter) = delimiter
            && delimiter.as_text().is_none_or(|text| {
                text.len() != 1 || text.contains(['"', '\n', '\r']) || !text.is_ascii()
            })
        {
            self.error(
                codes::UNKNOWN_VALUE,
                Stage::Validate,
                &delimiter.at,
                "A delimiter is one ASCII character that is not a quote or a line break."
                    .to_owned(),
                "Write the one character that stands between fields, such as `\";\"`.".to_owned(),
            );
            fine = false;
        }
        let mut template_versions = Vec::new();
        for version in doc.get("versions").map(Located::items).unwrap_or_default() {
            let Some(fingerprint) = version.get("fingerprint") else {
                continue;
            };
            let parts: Vec<FingerprintPart> = fingerprint
                .get("parts")
                .map(Located::items)
                .unwrap_or_default()
                .iter()
                .filter_map(|part| serde_json::from_value(part.to_json()).ok())
                .collect();
            let computed = Fingerprint::of_parts(parts);
            if let Some(stated) = fingerprint.get("digest")
                && stated.as_text() != Some(computed.digest.to_string().as_str())
            {
                self.error(
                    codes::UNKNOWN_VALUE,
                    Stage::Validate,
                    &stated.at,
                    "The fingerprint's digest is not the digest of its parts.".to_owned(),
                    format!("Write `digest: \"{}\"`, or leave the digest out; the compiler works it out from the parts.", computed.digest),
                );
                fine = false;
            }
            template_versions.push(TemplateVersion {
                version: version.text("version").unwrap_or_default().to_owned(),
                fingerprint: computed,
            });
        }
        let locate = match doc.get("locate") {
            None => Locate::Document,
            Some(locate) => match (locate.text("named"), locate.text("sheet")) {
                (Some(name), _) => Locate::Named(name.to_owned()),
                (_, Some(sheet)) => Locate::Sheet(sheet.to_owned()),
                _ => Locate::Document,
            },
        };
        let payload = SourceSpec {
            versions: template_versions,
            locate,
            columns: columns.into_iter().map(|column| column.value).collect(),
            fields,
            key: key.into_iter().map(|part| part.value).collect(),
            missing: texts("missing")
                .into_iter()
                .map(|text| text.value)
                .collect(),
            hidden: if doc.text("hidden") == Some("include") {
                HiddenRule::Include
            } else {
                HiddenRule::Reject
            },
            options: ReaderOptions {
                delimiter: delimiter
                    .and_then(Located::as_text)
                    .and_then(|text| text.chars().next()),
            },
        };
        let payload = PackPayload::encode(&payload).ok()?;
        fine.then(|| {
            (
                SourcePlan {
                    template: name.to_owned(),
                    reader: reader.to_owned(),
                    mapping,
                    rows,
                    payload,
                },
                ValueType::Record(row),
            )
        })
    }

    // ---- pipelines/ ----

    fn pipelines(&mut self) {
        let Some(schema) = k::schema_of(k::PIPELINE) else {
            return;
        };
        let docs: Vec<&Located<Doc>> = self.sources.docs_of(k::PIPELINE).collect();
        for doc in docs {
            if !self.fits(schema, doc) {
                continue;
            }
            let Some(name) = doc.get("pipeline").and_then(|name| self.tool_name(name)) else {
                continue;
            };
            if let Some(plan) = self.pipeline(doc, name) {
                let title = doc.text("title").unwrap_or(plan.name.as_str()).to_owned();
                let description = doc.text("description").unwrap_or_default().to_owned();
                self.pipelines.push(PipelineInfo {
                    plan,
                    title,
                    description,
                    at: doc.at.clone(),
                });
            }
        }
    }

    fn pipeline(&mut self, doc: &Located<Doc>, name: OperationName) -> Option<PipelinePlan> {
        let mut fine = true;
        let mut inputs = self.inputs(doc);
        let declared_inputs = input_type(&inputs);
        let mut source = None;
        let mut row_type = None;
        if let Some(template) = doc.get("source").and_then(|source| source.get("template"))
            && let Some(text) = template.as_text()
        {
            match self.source_plans.get(text).cloned() {
                Some((plan, row)) => {
                    if plan.reader != OPERATION_READER {
                        if let Some(taken) = inputs.iter().find(|input| input.plan.name == "source")
                        {
                            let at = taken.at.clone();
                            self.error(
                                codes::NAME_DEFINED_TWICE,
                                Stage::Resolve,
                                &at,
                                "A pipeline with a source has a reserved input named `source`, which carries the file to read.".to_owned(),
                                "Rename this input.".to_owned(),
                            );
                            fine = false;
                        } else if let Some(file) = self.model.field_type("file") {
                            inputs.push(InputInfo {
                                plan: InputPlan {
                                    name: "source".to_owned(),
                                    field_type: file.index,
                                    ty: file.plan.ty.clone(),
                                    required: true,
                                    default: None,
                                    description: format!("The file to read, in the form of the source template `{text}`."),
                                },
                                at: template.at.clone(),
                            });
                        }
                    }
                    source = Some(plan);
                    row_type = Some(row);
                }
                None => {
                    let known: Vec<&str> = self.source_plans.keys().map(String::as_str).collect();
                    let fix = fix_for_unknown(text, &known);
                    self.error(
                        codes::UNKNOWN_NAME,
                        Stage::Resolve,
                        &template.at,
                        format!("No file under `sources/` defines the template `{}`, or the one that does has errors.", shown(text)),
                        fix,
                    );
                    fine = false;
                }
            }
        }

        let source_type = row_type
            .map(|row| ValueType::Record(vec![FieldOf::required("rows", ValueType::list_of(row))]));
        let step_docs = doc.get("steps").map(Located::items).unwrap_or_default();
        // Every step's result type, by id, for `steps.<id>.result`.
        let mut results: Vec<(String, ValueType, usize)> = Vec::new();
        for step in step_docs {
            let Some(id) = step.text("id") else { continue };
            let Some(operation) = step.text("operation") else {
                continue;
            };
            if let Some(position) = self
                .operations
                .iter()
                .position(|info| info.name.as_str() == operation)
            {
                let result = ValueType::Record(
                    self.operations[position]
                        .outputs
                        .iter()
                        .map(|output| FieldOf::required(output.name.clone(), output.ty.clone()))
                        .collect(),
                );
                results.push((id.to_owned(), result, position));
            }
        }
        let steps_type = |upto: usize| {
            ValueType::Record(
                results
                    .iter()
                    .take(upto)
                    .map(|(id, result, _)| {
                        FieldOf::required(
                            id.clone(),
                            ValueType::Record(vec![FieldOf::required("result", result.clone())]),
                        )
                    })
                    .collect(),
            )
        };
        let base_env = |steps: ValueType| {
            let mut names = vec![
                ("input", declared_inputs.clone()),
                ("env", env_type()),
                ("steps", steps),
            ];
            if let Some(source) = &source_type {
                names.push(("source", source.clone()));
            }
            names
        };

        let mut steps = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut effect = EffectClass::Read;
        let mut hazard = Hazard::None;
        for step in step_docs {
            let Some(id_doc) = step.get("id") else {
                continue;
            };
            let Some(id) = id_doc.as_text() else { continue };
            if !is_simple_name(id) || !seen.insert(id.to_owned()) {
                self.error(
                    codes::NAME_DEFINED_TWICE,
                    Stage::Resolve,
                    &id_doc.at,
                    format!(
                        "The step id `{}` is used twice in the pipeline, or is not one name.",
                        shown(id)
                    ),
                    "Give each step its own lowercase id.".to_owned(),
                );
                fine = false;
                continue;
            }
            let Some(operation_doc) = step.get("operation") else {
                continue;
            };
            let operation = operation_doc.as_text().unwrap_or_default();
            let Some(position) = self
                .operations
                .iter()
                .position(|info| info.name.as_str() == operation)
            else {
                let known: Vec<&str> = self
                    .operations
                    .iter()
                    .map(|info| info.name.as_str())
                    .collect();
                let fix = fix_for_unknown(operation, &known);
                self.error(
                    codes::UNKNOWN_NAME,
                    Stage::Resolve,
                    &operation_doc.at,
                    format!("No operation is named `{}`, or the one that is has errors; a pipeline names operations only.", shown(operation)),
                    fix,
                );
                fine = false;
                continue;
            };
            let info = self.operations[position].clone();
            effect = effect.max(info.effect);
            hazard = hazard.max(info.hazard);
            let before = results
                .iter()
                .position(|(known, _, _)| known == id)
                .unwrap_or(0);
            let all_steps = steps_type(results.len());
            let mut names = base_env(steps_type(before));
            let mut relaxed = base_env(all_steps);

            // `for_each` first: it declares `row` for the rest of the step.
            let mut for_each = None;
            if let Some(rows) = step.get("for_each") {
                match self.step_expr(rows, &type_env(&names), &type_env(&relaxed), None) {
                    Some((index, ValueType::List(item))) => {
                        for_each = Some(index);
                        names.push(("row", (*item).clone()));
                        relaxed.push(("row", *item));
                    }
                    Some((_, other)) => {
                        self.error(
                            codes::EXPRESSION_TYPE_ERROR,
                            Stage::TypeCheck,
                            &rows.at,
                            format!(
                                "`for_each` needs a list of rows, and this expression gives a {}.",
                                other.name()
                            ),
                            "Give the rows to repeat over, such as `source.rows`.".to_owned(),
                        );
                        fine = false;
                    }
                    None => fine = false,
                }
            }
            let env = type_env(&names);
            let relaxed = type_env(&relaxed);
            let mut with = Vec::new();
            let mut given = BTreeSet::new();
            for arg in step.get("with").map(Located::entries).unwrap_or_default() {
                let Some(input) = info
                    .inputs
                    .iter()
                    .find(|input| input.plan.name == arg.key.value)
                else {
                    let known: Vec<&str> = info
                        .inputs
                        .iter()
                        .map(|input| input.plan.name.as_str())
                        .collect();
                    let fix = fix_for_unknown(&arg.key.value, &known);
                    self.error(
                        codes::UNKNOWN_NAME,
                        Stage::Resolve,
                        &arg.key.at,
                        format!(
                            "`{}` is not an input of the operation `{}`.",
                            shown(&arg.key.value),
                            info.name
                        ),
                        fix,
                    );
                    fine = false;
                    continue;
                };
                given.insert(arg.key.value.clone());
                match self.step_expr(&arg.value, &env, &relaxed, Some(&input.plan.ty)) {
                    Some((value, _)) => with.push(StepArg {
                        name: arg.key.value.clone(),
                        value,
                    }),
                    None => fine = false,
                }
            }
            for input in &info.inputs {
                if input.plan.required
                    && input.plan.default.is_none()
                    && !given.contains(&input.plan.name)
                {
                    self.error(
                        codes::MISSING_FIELD,
                        Stage::Resolve,
                        &step.at,
                        format!("The step `{id}` does not pass the required input `{}` of `{}`.", input.plan.name, info.name),
                        format!("Add `{}:` under `with`; inputs are passed only there, never by matching names.", input.plan.name),
                    );
                    fine = false;
                }
            }
            let when = match step.get("when") {
                Some(when) => {
                    let checked = self.step_expr(when, &env, &relaxed, Some(&ValueType::Bool));
                    fine &= checked.is_some();
                    checked.map(|(index, _)| index)
                }
                None => None,
            };
            let expect = match step.get("expect") {
                Some(expect) => {
                    let result = results
                        .iter()
                        .find(|(known, _, _)| known == id)
                        .map(|(_, result, _)| result.clone())
                        .unwrap_or(ValueType::Record(Vec::new()));
                    let mut with_result = names.clone();
                    with_result.push(("result", result));
                    let env = type_env(&with_result);
                    let checked = self.step_expr(expect, &env, &env, Some(&ValueType::Bool));
                    fine &= checked.is_some();
                    checked.map(|(index, _)| index)
                }
                None => None,
            };
            let checkpoint = step
                .text("checkpoint")
                .and_then(|checkpoint| checkpoint.parse::<Checkpoint>().ok());
            if for_each.is_some() && info.effect >= EffectClass::Write && checkpoint.is_none() {
                self.error(
                    codes::FOR_EACH_WITHOUT_CHECKPOINT,
                    Stage::Lint,
                    &step.at,
                    format!("The step `{id}` repeats a write over rows and has no checkpoint."),
                    "Add `checkpoint: per_record`.".to_owned(),
                );
                fine = false;
            }
            steps.push(PipelineStep {
                id: id.to_owned(),
                operation: OperationIndex::new(index(position)),
                with,
                for_each,
                checkpoint,
                when,
                expect,
            });
        }

        if let Some(declared) = doc.get("effect")
            && let Some(class) = declared
                .as_text()
                .and_then(|text| text.parse::<EffectClass>().ok())
        {
            if class < effect {
                self.error(
                    codes::EFFECT_LOWER_THAN_DERIVED,
                    Stage::Analyze,
                    &declared.at,
                    format!("The pipeline declares the effect `{class}`, and its operations make it `{effect}`."),
                    format!("Write `effect: {effect}`, or leave the effect out; it can be declared higher, never lower."),
                );
                fine = false;
            }
            effect = effect.max(class);
        }
        // A limit the pipeline does not state is the target's default, so
        // the plan holds the limit and the engine applies no default.
        let limits = PipelineLimits {
            max_records: doc
                .get("limits")
                .and_then(|limits| limits.get("max_records"))
                .and_then(Located::as_int)
                .map(to_u32)
                .or(self.default_limits.max_records),
            max_duration_ms: doc
                .get("limits")
                .and_then(|limits| limits.get("max_duration_ms"))
                .and_then(Located::as_int)
                .and_then(|number| u64::try_from(number).ok())
                .or(self.default_limits.max_duration_ms),
        };
        if effect >= EffectClass::Write && limits.max_records.is_none() {
            self.error(
                codes::WRITE_PIPELINE_WITHOUT_LIMIT,
                Stage::Lint,
                &doc.at,
                "The pipeline writes and declares no record limit.".to_owned(),
                "Add `limits: {max_records: N}` with the most records one run may touch, or state one for every pipeline under `defaults.limits` in `target.yaml`."
                    .to_owned(),
            );
            fine = false;
        }
        let mut success_names = base_env(steps_type(results.len()));
        success_names.push((ALL_RECORDS_VERIFIED, ValueType::Bool));
        let success_env = type_env(&success_names);
        let mut success = Vec::new();
        for check in doc.get("success").map(Located::items).unwrap_or_default() {
            match self.expr(check, &success_env, Some(&ValueType::Bool)) {
                Some((index, _)) => success.push(index),
                None => fine = false,
            }
        }
        fine.then(|| PipelinePlan {
            name,
            effect,
            hazard,
            inputs: inputs.into_iter().map(|input| input.plan).collect(),
            source,
            limits,
            steps,
            success,
            headroom_ms: doc
                .get("headroom_ms")
                .and_then(Located::as_int)
                .map_or(0, to_u32),
        })
    }

    /// An expression of a pipeline step. When it fails only because it reads
    /// the result of a step that does not come before it, the problem is
    /// reported as a cycle.
    fn step_expr(
        &mut self,
        doc: &Located<Doc>,
        env: &TypeEnv,
        relaxed: &TypeEnv,
        expected: Option<&ValueType>,
    ) -> Option<(a2m_core::ExprIndex, ValueType)> {
        let text = expr_text(doc)?;
        match check_expr(&text, &doc.at, env, expected) {
            Ok(plan_expr) => {
                let ty = plan_expr.result.clone();
                Some((self.model.exprs.borrow_mut().intern(plan_expr), ty))
            }
            Err(problem) => {
                if check_expr(&text, &doc.at, relaxed, expected).is_ok() {
                    self.error(
                        codes::STEP_CYCLE,
                        Stage::Analyze,
                        &doc.at,
                        format!("The expression `{}` uses the result of a step that does not come before this one.", shown(&text)),
                        "Move the step it reads from above this one; a step uses only the results of earlier steps.".to_owned(),
                    );
                } else {
                    self.problems.push(problem);
                }
                None
            }
        }
    }

    // ---- stage 8: describe ----

    fn describe(&mut self) {
        let mut tools = Vec::new();
        let mut places = Vec::new();
        let operations: Vec<(usize, OperationPlan, String, String)> = self
            .op_plans
            .iter()
            .enumerate()
            .map(|(position, (plan, title, description))| {
                (position, plan.clone(), title.clone(), description.clone())
            })
            .collect();
        for (position, plan, title, description) in operations {
            let idempotent = plan.effect == EffectClass::Read
                || (plan.effect == EffectClass::Write
                    && matches!(
                        plan.identity.strategy,
                        IdentityStrategy::Natural | IdentityStrategy::Composite
                    ));
            let output: serde_json::Map<String, serde_json::Value> = plan
                .outputs
                .iter()
                .map(|output| {
                    let schema = self.output_schema_of_type(&output.ty, output.field_type);
                    // An output that may be absent is returned as `null`
                    // under its own name, so its schema admits `null`
                    // beside its type (decision 0305).
                    let schema = if output.may_be_absent {
                        serde_json::json!({"anyOf": [schema, {"type": "null"}]})
                    } else {
                        schema
                    };
                    (output.name.clone(), schema)
                })
                .collect();
            // Every output is in every result, so every output is required.
            let required: Vec<&str> = plan
                .outputs
                .iter()
                .map(|output| output.name.as_str())
                .collect();
            let at = self.operations[position].doc.at.clone();
            let tool = self.tool(
                plan.name.clone(),
                ToolRuns::Operation(OperationIndex::new(index(position))),
                title,
                description,
                &plan.inputs,
                serde_json::json!({"type": "object", "properties": output, "required": required}),
                ToolAnnotations {
                    read_only: plan.effect == EffectClass::Read,
                    destructive: plan.effect == EffectClass::Irreversible
                        || plan.hazard != Hazard::None,
                    idempotent,
                    open_world: plan.hazard == Hazard::SendsOutside
                        || !plan.manifest.destinations.is_empty(),
                },
                &at,
            );
            tools.extend(tool);
            places.push(at);
        }
        let pipelines: Vec<(usize, PipelinePlan, String, String, Place)> = self
            .pipelines
            .iter()
            .enumerate()
            .map(|(position, info)| {
                (
                    position,
                    info.plan.clone(),
                    info.title.clone(),
                    info.description.clone(),
                    info.at.clone(),
                )
            })
            .collect();
        for (position, plan, title, description, at) in pipelines {
            let tool = self.tool(
                plan.name.clone(),
                ToolRuns::Pipeline(PipelineIndex::new(index(position))),
                title,
                description,
                &plan.inputs,
                serde_json::json!({"type": "object"}),
                ToolAnnotations {
                    read_only: plan.effect == EffectClass::Read,
                    destructive: plan.effect == EffectClass::Irreversible
                        || plan.hazard != Hazard::None,
                    idempotent: plan.effect == EffectClass::Read,
                    open_world: plan.hazard == Hazard::SendsOutside,
                },
                &at,
            );
            tools.extend(tool);
            places.push(at);
        }
        // Two tools a caller cannot tell apart, and names that differ only
        // by a suffix.
        for (later, tool) in tools.iter().enumerate() {
            for earlier in &tools[..later] {
                if tool.description == earlier.description
                    && tool.input_schema == earlier.input_schema
                {
                    self.error(
                        codes::TOOL_INDISTINGUISHABLE,
                        Stage::Describe,
                        &places[later],
                        format!("The tools `{}` and `{}` have the same description and the same inputs; a caller cannot tell them apart.", earlier.name, tool.name),
                        "Say in each description what it does and when to prefer the other.".to_owned(),
                    );
                }
                let (short, long) = if tool.name.as_str().len() < earlier.name.as_str().len() {
                    (tool.name.as_str(), earlier.name.as_str())
                } else {
                    (earlier.name.as_str(), tool.name.as_str())
                };
                let suffix_only = long.strip_prefix(short).is_some_and(|suffix| {
                    let suffix = suffix.strip_prefix('_').unwrap_or(suffix);
                    let version = suffix.strip_prefix('v').unwrap_or(suffix);
                    ["new", "old", "copy", "tmp"].contains(&suffix)
                        || (!version.is_empty()
                            && version.bytes().all(|byte| byte.is_ascii_digit()))
                });
                if suffix_only {
                    self.error(
                        codes::TOOL_NAMES_TOO_ALIKE,
                        Stage::Lint,
                        &places[later],
                        format!("The tool names `{short}` and `{long}` differ only by a suffix."),
                        "Name each tool for what it does; a breaking change is a new major number, published as a new version.".to_owned(),
                    );
                }
            }
        }
        self.tools = tools;
    }

    /// The module a tool belongs to: the longest of the target's modules
    /// that follows the target's name in the tool's name.
    fn module_of(&self, name: &OperationName) -> Option<String> {
        let target = self.model.target.as_ref()?;
        let rest = name
            .as_str()
            .strip_prefix(target.as_str())?
            .strip_prefix('_')?;
        self.model
            .modules
            .iter()
            .filter(|module| {
                rest.strip_prefix(module.as_str())
                    .is_some_and(|rest| rest.starts_with('_'))
            })
            .max_by_key(|module| module.len())
            .cloned()
    }

    fn schema_of_type(
        &self,
        ty: &ValueType,
        field_type: Option<FieldTypeIndex>,
    ) -> serde_json::Value {
        let codes = field_type
            .and_then(|index| self.model.field_types.get(index.get()))
            .map_or(&[][..], |known| known.codes.as_slice());
        json_schema_of(ty, codes)
    }

    fn output_schema_of_type(
        &self,
        ty: &ValueType,
        field_type: Option<FieldTypeIndex>,
    ) -> serde_json::Value {
        let codes = field_type
            .and_then(|index| self.model.field_types.get(index.get()))
            .map_or(&[][..], |known| known.codes.as_slice());
        output_schema_of(ty, codes)
    }

    #[allow(clippy::too_many_arguments)]
    fn tool(
        &mut self,
        name: OperationName,
        runs: ToolRuns,
        title: String,
        description: String,
        inputs: &[InputPlan],
        output_schema: serde_json::Value,
        annotations: ToolAnnotations,
        at: &Place,
    ) -> Option<ToolPlan> {
        let length = description.trim().chars().count();
        if !(20..=1024).contains(&length) {
            self.problems.push(
                Diagnostic::error(
                    kernel(codes::DESCRIPTION_WEAK),
                    Stage::Lint,
                    at,
                    format!("The description of `{name}` has {length} characters; a description has from 20 to 1024."),
                    "Say what the tool does, when to prefer another tool, and its side effects and limits.",
                )
                .as_warning(),
            );
        }
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();
        for input in inputs {
            let mut schema = self.schema_of_type(&input.ty, Some(input.field_type));
            if let (false, Some(map)) = (input.description.is_empty(), schema.as_object_mut()) {
                map.insert(
                    "description".to_owned(),
                    serde_json::Value::from(input.description.clone()),
                );
            }
            properties.insert(input.name.clone(), schema);
            if input.required && input.default.is_none() {
                required.push(input.name.clone());
            }
        }
        let input_schema = serde_json::json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        });
        let (Ok(input_schema), Ok(output_schema)) = (
            CanonicalJson::encode(&input_schema),
            CanonicalJson::encode(&output_schema),
        ) else {
            return None;
        };
        let module = self.module_of(&name);
        Some(ToolPlan {
            name,
            runs,
            module,
            title,
            description: description.trim().to_owned(),
            input_schema,
            output_schema,
            annotations,
        })
    }

    // ---- tests/ and fixtures/ ----

    /// The place, from 0, of the binding a test names with `binding:`
    /// (audit finding AC-6): of its operation's bindings, the one of that
    /// mode, or, where `position` is given, the one at that place, counted
    /// from 1, which must be of that mode. A retired binding never runs, so
    /// no test of one is taken. `None`, with an error, otherwise.
    fn tested_binding(&mut self, tool: usize, named: &Located<Doc>) -> Option<usize> {
        let refuse = |this: &mut Self, message: String| {
            this.error(
                codes::TEST_BINDING_UNKNOWN,
                Stage::Resolve,
                &named.at,
                message,
                "Name the binding by its `mode`, and by its `position` among the operation's bindings, counted from 1, where two share the mode.".to_owned(),
            );
            None
        };
        let ToolRuns::Operation(operation) = self.tools[tool].runs else {
            return refuse(
                self,
                "A test of a pipeline names no binding: each of its operations is chosen for as it would be live.".to_owned(),
            );
        };
        let mode = named.text("mode").unwrap_or_default().to_owned();
        let position = named
            .get("position")
            .and_then(|position| position.as_int())
            .and_then(|position| usize::try_from(position).ok())
            .and_then(|position| position.checked_sub(1));
        let bindings = &self.op_plans[operation.get()].0.bindings;
        let of_mode: Vec<usize> = bindings
            .iter()
            .enumerate()
            .filter(|(at, binding)| {
                binding.mode.as_str() == mode && position.is_none_or(|position| position == *at)
            })
            .map(|(at, _)| at)
            .collect();
        let found = match of_mode.as_slice() {
            [one] => *one,
            [] => {
                return refuse(
                    self,
                    format!(
                        "The operation has no binding of the mode `{}`{}.",
                        shown(&mode),
                        position.map_or_else(String::new, |at| format!(" at position {}", at + 1))
                    ),
                );
            }
            _ => {
                return refuse(
                    self,
                    format!(
                        "The operation has more than one binding of the mode `{}`, and the test does not say which by `position`.",
                        shown(&mode)
                    ),
                );
            }
        };
        if bindings[found].status == BindingStatus::Retired {
            return refuse(
                self,
                format!(
                    "The binding {} of the mode `{}` is retired, and a retired binding never runs.",
                    found + 1,
                    shown(&mode)
                ),
            );
        }
        Some(found)
    }

    fn tests(&mut self) {
        let Some(schema) = k::schema_of(k::TEST) else {
            return;
        };
        let Some(fixture_schema) = k::schema_of(k::FIXTURE) else {
            return;
        };
        let test_level = self.mode;
        let as_test_rule = |problem: Diagnostic| match test_level {
            CompileMode::Publish => problem,
            CompileMode::Draft => problem.as_warning(),
        };
        // Which fixture folders carry the marker that says they were scrubbed.
        let mut scrubbed: BTreeSet<String> = BTreeSet::new();
        let markers: Vec<&SourceFile> = self
            .sources
            .of_kind(k::FIXTURE)
            .filter(|file| file.path.file_name() == FIXTURE_MARKER)
            .collect();
        for marker in markers {
            for doc in &marker.docs {
                if self.fits(fixture_schema, doc)
                    && doc.get("scrubbed").and_then(Located::as_bool) == Some(true)
                    && let Some(folder) = marker.path.parts().nth(1)
                {
                    scrubbed.insert(folder.to_owned());
                }
            }
        }
        let mut tested: BTreeSet<String> = BTreeSet::new();
        let mut names: BTreeSet<String> = BTreeSet::new();
        let files: Vec<&SourceFile> = self.sources.of_kind(k::TEST).collect();
        for file in files {
            for (position, doc) in file.docs.iter().enumerate() {
                if !self.fits(schema, doc) {
                    continue;
                }
                let Some(tool_doc) = doc.get("test") else {
                    continue;
                };
                let tool_name = tool_doc.as_text().unwrap_or_default();
                let Some(tool) = self
                    .tools
                    .iter()
                    .position(|tool| tool.name.as_str() == tool_name)
                else {
                    let known: Vec<&str> =
                        self.tools.iter().map(|tool| tool.name.as_str()).collect();
                    let fix = fix_for_unknown(tool_name, &known);
                    self.error(
                        codes::UNKNOWN_NAME,
                        Stage::Resolve,
                        &tool_doc.at,
                        format!("No operation or pipeline is named `{}`, or the one that is has errors.", shown(tool_name)),
                        fix,
                    );
                    continue;
                };
                let name = doc.text("name").map_or_else(
                    || {
                        if file.docs.len() == 1 {
                            file.path.stem().to_owned()
                        } else {
                            format!("{}_{}", file.path.stem(), position + 1)
                        }
                    },
                    str::to_owned,
                );
                if !names.insert(name.clone()) {
                    self.error(
                        codes::NAME_DEFINED_TWICE,
                        Stage::Resolve,
                        &doc.at,
                        format!("Two tests are named `{name}`."),
                        "Give one of them a `name:` of its own.".to_owned(),
                    );
                    continue;
                }
                let kind = doc
                    .text("kind")
                    .and_then(|kind| kind.parse::<TestKind>().ok())
                    .unwrap_or(TestKind::Offline);
                let inputs: Vec<InputPlan> = match self.tools[tool].runs {
                    ToolRuns::Operation(operation) => {
                        self.op_plans[operation.get()].0.inputs.clone()
                    }
                    ToolRuns::Pipeline(pipeline) => {
                        self.pipelines[pipeline.get()].plan.inputs.clone()
                    }
                };
                let mut values = Values::new();
                let mut fine = true;
                for given in doc.get("inputs").map(Located::entries).unwrap_or_default() {
                    let Some(input) = inputs.iter().find(|input| input.name == given.key.value)
                    else {
                        let known: Vec<&str> =
                            inputs.iter().map(|input| input.name.as_str()).collect();
                        let fix = fix_for_unknown(&given.key.value, &known);
                        self.error(
                            codes::UNKNOWN_NAME,
                            Stage::Resolve,
                            &given.key.at,
                            format!(
                                "`{}` is not an input of `{tool_name}`.",
                                shown(&given.key.value)
                            ),
                            fix,
                        );
                        fine = false;
                        continue;
                    };
                    match value_of(&given.value, &input.ty) {
                        Some(value) => {
                            values.insert(input.name.clone(), value);
                        }
                        None => {
                            self.error(
                                codes::VALUE_NOT_OF_TYPE,
                                Stage::Resolve,
                                &given.value.at,
                                format!(
                                    "The test gives `{}` a {}, and the input is a {}.",
                                    input.name,
                                    given.value.value.kind(),
                                    input.ty.name()
                                ),
                                format!("Write a {} here; text goes in quotes.", input.ty.name()),
                            );
                            fine = false;
                        }
                    }
                }
                let fixture = doc.get("fixture");
                match (kind, fixture) {
                    (TestKind::Offline, None) => {
                        self.error(
                            codes::MISSING_FIELD,
                            Stage::Validate,
                            &doc.at,
                            "An offline test names the fixture it runs against.".to_owned(),
                            "Add `fixture:` with the name of a folder under `fixtures/`."
                                .to_owned(),
                        );
                        fine = false;
                    }
                    (_, Some(folder)) => {
                        let folder_name = folder.as_text().unwrap_or_default();
                        if !scrubbed.contains(folder_name) {
                            self.problems.push(as_test_rule(Diagnostic::error(
                                kernel(codes::FIXTURE_NOT_SCRUBBED),
                                Stage::Lint,
                                &folder.at,
                                format!("The fixture `{folder_name}` does not exist under `fixtures/`, or does not carry the marker that says it was scrubbed."),
                                format!("Record the fixture with `anything2mcp fixture record`, which scrubs it and writes `fixtures/{folder_name}/{FIXTURE_MARKER}` with `scrubbed: true`."),
                            )));
                        }
                    }
                    (TestKind::Live, None) => {}
                }
                let Ok(expect) = doc.get("expect").map_or_else(
                    || Ok(CanonicalJson::none()),
                    |expect| CanonicalJson::encode(&expect.to_json()),
                ) else {
                    continue;
                };
                let binding = match doc.get("binding") {
                    Some(named) => match self.tested_binding(tool, named) {
                        Some(position) => Some(position),
                        None => continue,
                    },
                    None => None,
                };
                if !fine {
                    continue;
                }
                if kind == TestKind::Offline {
                    tested.insert(tool_name.to_owned());
                }
                self.test_bindings.push(binding);
                self.tests.push(TestPlan {
                    name,
                    tool: ToolIndex::new(index(tool)),
                    kind,
                    fixture: fixture
                        .and_then(Located::as_text)
                        .map(|folder| format!("fixtures/{folder}")),
                    inputs: values,
                    expect,
                    restore: doc
                        .get("restore")
                        .and_then(Located::as_bool)
                        .unwrap_or(false),
                });
            }
        }
        let untested: Vec<(String, Place)> = self
            .operations
            .iter()
            .map(|info| (info.name.as_str().to_owned(), info.doc.at.clone()))
            .chain(
                self.pipelines
                    .iter()
                    .map(|info| (info.plan.name.as_str().to_owned(), info.at.clone())),
            )
            .filter(|(name, _)| !tested.contains(name))
            .collect();
        for (name, at) in untested {
            self.problems.push(as_test_rule(Diagnostic::error(
                kernel(codes::OPERATION_WITHOUT_TEST),
                Stage::Lint,
                &at,
                format!("`{name}` has no offline test against a fixture."),
                format!("Add a file under `tests/` with `test: {name}`, `kind: offline`, a fixture, inputs, and the expected result."),
            )));
        }
    }

    // ---- stage 7: the rules that look across files ----

    fn lint(&mut self) {
        // No value that looks like a credential, in any file of any kind.
        let mut found = Vec::new();
        for file in &self.sources.files {
            for doc in &file.docs {
                credentials(None, doc, &mut found);
            }
        }
        for suspect in found {
            let Some(what) = suspect.what else {
                continue;
            };
            if suspect.certain {
                self.error(
                    codes::CREDENTIAL_IN_FILE,
                    Stage::Lint,
                    &suspect.at,
                    format!("This value looks like a credential: {what}."),
                    "Store the value in the vault and refer to it as `vault://...`; no file holds a secret.".to_owned(),
                );
            } else {
                self.problems.push(
                    Diagnostic::error(
                        kernel(codes::CREDENTIAL_SUSPECTED),
                        Stage::Lint,
                        &suspect.at,
                        format!("This value may be a credential: {what}."),
                        "If it is a secret, store it in the vault and refer to it as `vault://...`. If it is the name of a token, a cookie, or a header, write it as a lowercase name.",
                    )
                    .as_warning(),
                );
            }
        }
        // The rules packs registered.
        let set = self.set;
        let mut reported = Vec::new();
        {
            let view = ResolvedView {
                model: &self.model,
                sources: self.sources,
                mode: self.mode,
                operations: &self.operations,
            };
            for (code, rule) in &set.lint_rules {
                let level = rule.item.level(self.mode);
                for mut problem in rule.item.check(&view) {
                    problem.code = LintCode::clone(code);
                    problem.level = level;
                    problem.stage = Stage::Lint;
                    reported.push(problem);
                }
            }
        }
        self.problems.extend(reported);
    }
}

/// What a lowering said of itself, for the analysis of one binding.
/// Whether a lowered binding has a step that saves.
fn writes_by_steps(lowered: &Lowered) -> bool {
    lowered.act.iter().any(|step| step.commits)
}

struct LoweredBy<'a> {
    lowering: &'a dyn Lowering,
    /// The pack that registered it.
    pack: &'a str,
    verb: &'a Verb,
    /// The least effect the binding can have, by the lowering's word.
    least: EffectClass,
    /// What the binding offers as evidence that a write landed.
    evidence: WriteEvidence,
}

/// What stands in a diagnostic where a value the credential lint knows was
/// quoted.
const NOT_SHOWN: &str = "[not shown]";

/// One value the credential lint knows.
struct Suspect {
    at: Place,
    /// The value, as the file wrote it.
    value: String,
    /// What makes it look like a credential, when the lint reports it. A
    /// value that is only kept out of diagnostics has none.
    what: Option<&'static str>,
    /// An error; otherwise a warning under its own code.
    certain: bool,
    /// Whether the value is taken out of every diagnostic, not only those
    /// at its own place: true unless it reads as a name or is very short.
    everywhere: bool,
}

/// `text` without `value`: whole, and as a diagnostic cuts it. `quoted_too`
/// also takes out a short value where it stands between backticks.
fn without(text: &str, value: &str, quoted_too: bool) -> String {
    let cut: String = value.chars().take(64).collect();
    let mut out = text.to_owned();
    for form in [value, cut.as_str()] {
        if form.chars().count() >= 4 {
            out = out.replace(form, NOT_SHOWN);
        } else if quoted_too && !form.is_empty() {
            out = out.replace(&format!("`{form}`"), &format!("`{NOT_SHOWN}`"));
        }
    }
    out
}

/// How a key names what it holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyNames {
    /// A secret, plainly: a password, a private key, an access token.
    Secret,
    /// What is often a secret and sometimes the name of one: a token, a
    /// cookie, an authorization, a PIN.
    PerhapsSecret,
}

/// The words that make a key a certain secret's.
const SECRET_WORDS: [&str; 15] = [
    "password",
    "passwd",
    "passphrase",
    "secret",
    "api_key",
    "apikey",
    "private_key",
    "access_token",
    "refresh_token",
    "auth_token",
    "id_token",
    "session_token",
    "bearer_token",
    "client_secret",
    "secret_key",
];

/// Whether a key, already in lower case with `-` as `_`, holds one of them.
fn secret_key_word(key: &str) -> bool {
    SECRET_WORDS.iter().any(|word| key.contains(word))
}

/// Whether the compiler takes `name` for a certain secret's: a key that
/// holds one of the words of a password, a private key, a secret, or an
/// access token. It is the test the compiler's refusal of a secret in a
/// file (`kernel.credential_in_file`) uses, so a drafter that asks it
/// cannot write a key the compiler would refuse (decision 0302).
#[must_use]
pub fn names_a_secret(name: &str) -> bool {
    secret_key_word(&name.to_ascii_lowercase().replace('-', "_"))
}

/// The sentence that ends a drafted description when inputs, outputs,
/// columns, or values were left out because their names are a secret's:
/// each name quoted in backticks and cut at 60 characters, no more than
/// eight named, and the rest counted. It is empty when nothing was left
/// out. Every pack's drafter ends a description with it, so the sentence
/// has one form (decision 0302).
#[must_use]
pub fn left_out_sentence(secrets: &[String]) -> String {
    const MOST_NAMED: usize = 8;
    if secrets.is_empty() {
        return String::new();
    }
    let named: Vec<String> = secrets
        .iter()
        .take(MOST_NAMED)
        .map(|name| format!("`{}`", name.chars().take(60).collect::<String>()))
        .collect();
    let more = secrets.len().saturating_sub(MOST_NAMED);
    let tail = if more > 0 {
        format!(" and {more} more")
    } else {
        String::new()
    };
    format!(
        " Left out because their names are a secret's, so that a secret is never an output: {}{tail}.",
        named.join(", ")
    )
}

/// What the name of a key says of its value, when it says anything. `exact`
/// is true when the whole key is the word, which is all a number is judged
/// by: `pin: 4821` is a secret, and `token_ttl_ms: 60000` is not.
fn key_names(key: &str) -> Option<(KeyNames, bool)> {
    const PERHAPS: [&str; 7] = [
        "token",
        "tokens",
        "cookie",
        "cookies",
        "authorization",
        "pin",
        "bearer",
    ];
    // Words of a key split at `_`, `-`, `.`, and a change from lower case
    // to upper case, so `pinCode` and `X-Auth-Token` have the words `pin`
    // and `token`; `spinner_count` and `tokenizer_mode` have neither.
    let mut spaced = String::with_capacity(key.len() + 4);
    let mut before: Option<char> = None;
    for c in key.chars() {
        if c.is_ascii_uppercase() && before.is_some_and(|b| b.is_ascii_lowercase()) {
            spaced.push('_');
        }
        spaced.push(c);
        before = Some(c);
    }
    let words = spaced.to_ascii_lowercase().replace('-', "_");
    let key = key.to_ascii_lowercase().replace('-', "_");
    if secret_key_word(&key) {
        return Some((KeyNames::Secret, SECRET_WORDS.contains(&key.as_str())));
    }
    words
        .split(['_', '.'])
        .any(|part| PERHAPS.contains(&part))
        .then(|| (KeyNames::PerhapsSecret, PERHAPS.contains(&key.as_str())))
}

/// What makes `text` a credential by its shape alone, under any key.
fn credential_shape(text: &str) -> Option<&'static str> {
    if text.starts_with("-----BEGIN ") {
        return Some("it is key material");
    }
    let header = "it is an authorization header's value";
    if (text.starts_with("Bearer ") || text.starts_with("Basic ")) && text.len() >= 20 {
        return Some(header);
    }
    // The same in any letter case, and with the scheme `Token`: one scheme
    // and one word that is not plain lowercase prose.
    if let Some((scheme, word)) = text.split_once(' ')
        && ["bearer", "basic", "token"].contains(&scheme.to_ascii_lowercase().as_str())
        && word.chars().count() >= 16
        && !word.contains(char::is_whitespace)
        && !word.chars().all(|c| c.is_ascii_lowercase())
    {
        return Some(header);
    }
    (text.starts_with("eyJ") && text.matches('.').count() == 2 && text.len() >= 40)
        .then_some("it has the shape of a signed token")
}

/// What in a fixture's recorded file is a credential by its shape, and the
/// line it is on: key material, an authorization header's value, a signed
/// token. A recording is text a system sent, so it is read by line and by
/// word, not as a document. Only what is plainly one of the three is found.
pub(crate) fn recorded_credential(text: &str) -> Option<(u32, &'static str)> {
    let apart = |c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\'' | ',' | ';' | '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}'
            )
    };
    for (number, line) in text.lines().enumerate() {
        let found = if line.contains("-----BEGIN ") {
            Some("it is key material")
        } else {
            let words: Vec<&str> = line.split(apart).filter(|word| !word.is_empty()).collect();
            let header = words.windows(2).any(|pair| {
                ["bearer", "basic"].contains(&pair[0].to_ascii_lowercase().as_str())
                    && pair[1].chars().count() >= 16
                    && !pair[1].chars().all(|c| c.is_ascii_lowercase())
            });
            let signed = words.iter().any(|word| {
                word.starts_with("eyJ") && word.matches('.').count() == 2 && word.len() >= 40
            });
            if header {
                Some("it is an authorization header's value")
            } else if signed {
                Some("it has the shape of a signed token")
            } else {
                None
            }
        };
        if let Some(what) = found {
            return Some((
                u32::try_from(number).map_or(u32::MAX, |n| n.saturating_add(1)),
                what,
            ));
        }
    }
    None
}

/// Whether `text` is a path into an answer, as an output map writes one:
/// names joined by dots, where a part may also be `*` for every item, such
/// as `*.pin_order` or `body.items.*.id`. It names a place a value is taken
/// from, and holds no value, so it is a name for the credential lint. At
/// least one part is a name, and a bare `*` is not a path.
fn is_extract_path(text: &str) -> bool {
    text.contains('*')
        && text.split('.').any(|part| part != "*")
        && text
            .split('.')
            .all(|part| part == "*" || crate::schema::is_dotted_name(part))
}

/// Finds the values the credential lint knows: a value under a key whose
/// name says it is a secret, or may be one, that is not a vault reference;
/// and text in the shape of a token, under any key. Each is reported by the
/// lint or only kept out of diagnostics, as its `what` says.
fn credentials(key: Option<&str>, doc: &Located<Doc>, found: &mut Vec<Suspect>) {
    let named = key.and_then(key_names);
    match &doc.value {
        Doc::Text(text) => {
            let vaulted = text.starts_with("vault://");
            let reads_as_name = crate::schema::is_dotted_name(text) || is_extract_path(text);
            let long_enough = text.chars().count() >= 4;
            let (what, certain) = match (named, credential_shape(text)) {
                _ if vaulted => (None, false),
                (Some((KeyNames::Secret, _)), _) if long_enough => (
                    Some("its key names a secret and its value is not a vault reference"),
                    true,
                ),
                (_, Some(shape)) => (Some(shape), true),
                (Some((KeyNames::PerhapsSecret, _)), None) if long_enough && !reads_as_name => (
                    Some(
                        "its key names a token, a cookie, an authorization, or a PIN, and its value is neither a name nor a vault reference",
                    ),
                    false,
                ),
                _ => (None, false),
            };
            if !vaulted && (named.is_some() || what.is_some()) {
                found.push(Suspect {
                    at: doc.at.clone(),
                    value: text.clone(),
                    what,
                    certain,
                    everywhere: long_enough && (!reads_as_name || what.is_some()),
                });
            }
        }
        // A secret written as a number, under a key that is the word itself.
        Doc::Int(number) => {
            if let Some((_, true)) = named {
                found.push(Suspect {
                    at: doc.at.clone(),
                    value: number.to_string(),
                    what: Some(
                        "its key names a secret and its value is a number, not a vault reference",
                    ),
                    certain: false,
                    everywhere: false,
                });
            }
        }
        Doc::List(items) => {
            for item in items {
                credentials(key, item, found);
            }
        }
        Doc::Map(entries) => {
            for entry in entries {
                credentials(Some(&entry.key.value), &entry.value, found);
            }
        }
        _ => {}
    }
}

/// Whether `text` is one name an upload or a download may be declared by.
fn is_permission_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// What is wrong with `text` as a destination an operation declares, when
/// something is. A destination is one exact address: a host name or an
/// address literal, with an optional port, and an optional scheme and
/// `://` before it. Nothing in it may stand for more than one address, and
/// nothing may follow it.
fn destination_problem(text: &str) -> Option<&'static str> {
    if text.is_empty() || text.len() > 253 {
        return Some("it is empty or longer than an address");
    }
    if !text.is_ascii() || text.bytes().any(|byte| byte <= b' ' || byte == 0x7f) {
        return Some("it holds a space or a character outside plain ASCII");
    }
    if text.contains('*') {
        return Some("it holds a wildcard, and a destination is one exact address");
    }
    let rest = match text.split_once("://") {
        Some((scheme, rest)) => {
            let well_formed = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
            if !well_formed {
                return Some("what stands before `://` is not a scheme");
            }
            rest
        }
        None => text,
    };
    if rest.contains('@') {
        return Some("it holds user information");
    }
    if rest.contains(['/', '?', '#', '\\', '%']) {
        return Some("it holds a path, a query, or a range of addresses after the address");
    }
    let (host, port) = if let Some(bracketed) = rest.strip_prefix('[') {
        let Some((host, after)) = bracketed.split_once(']') else {
            return Some("its bracket is not closed");
        };
        let well_formed = !host.is_empty()
            && host
                .chars()
                .all(|c| c.is_ascii_hexdigit() || matches!(c, ':' | '.'));
        if !well_formed {
            return Some("what stands between its brackets is not an address");
        }
        match after.strip_prefix(':') {
            Some(port) => (host, Some(port)),
            None if after.is_empty() => (host, None),
            None => return Some("something follows its closing bracket"),
        }
    } else {
        match rest.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (rest, None),
        }
    };
    if let Some(port) = port
        && !(port.bytes().all(|byte| byte.is_ascii_digit())
            && port.parse::<u16>().is_ok_and(|port| port > 0))
    {
        return Some("its port is not a number from 1 to 65535");
    }
    let labels_hold = host.split('.').all(|label| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':')
    });
    if host.is_empty() || !labels_hold {
        return Some("its host is not a host name or an address");
    }
    // The address that means every address.
    let digits_only: String = host.chars().filter(|c| !matches!(c, '.' | ':')).collect();
    if digits_only.chars().all(|c| c == '0') {
        return Some("it is the address that stands for every address");
    }
    None
}

/// The level a registered rule's finding has, for a caller that shows it.
#[must_use]
pub fn is_error(diagnostic: &Diagnostic) -> bool {
    diagnostic.level == Level::Error
}

#[cfg(test)]
mod secret_names {
    use super::{KeyNames, key_names, left_out_sentence, names_a_secret};

    /// The answers `key_names` gave before the certain-secret test was
    /// shared: the certain words, the perhaps words, and ordinary names.
    #[test]
    fn the_shared_test_gives_the_answers_the_compiler_always_gave() {
        let table: [(&str, Option<(bool, bool)>); 20] = [
            ("password", Some((true, true))),
            ("client_secret", Some((true, true))),
            ("clientSecret", Some((true, false))),
            ("X-Api-Key", Some((true, false))),
            ("apiKey", Some((true, true))),
            ("access_token", Some((true, true))),
            ("secrets_count", Some((true, false))),
            ("secretary", Some((true, false))),
            ("accessToken", Some((false, false))),
            ("token", Some((false, true))),
            ("tokens", Some((false, true))),
            ("cookie", Some((false, true))),
            ("pin", Some((false, true))),
            ("pinCode", Some((false, false))),
            ("X-Auth-Token", Some((true, false))),
            ("spinner_count", None),
            ("tokenizer_mode", None),
            ("order_id", None),
            ("name", None),
            ("", None),
        ];
        for (key, want) in table {
            let got = key_names(key).map(|(kind, exact)| (kind == KeyNames::Secret, exact));
            assert_eq!(got, want, "{key:?}");
            assert_eq!(
                names_a_secret(key),
                matches!(want, Some((true, _))),
                "{key:?}"
            );
        }
    }

    /// The one sentence every drafter ends a description with: nothing when
    /// nothing was left out, eight names at most, each cut at 60 characters.
    #[test]
    fn the_left_out_sentence_names_eight_and_counts_the_rest() {
        assert_eq!(left_out_sentence(&[]), "");
        assert_eq!(
            left_out_sentence(&["client_secret".to_owned()]),
            " Left out because their names are a secret's, so that a secret is never an output: `client_secret`."
        );
        let many: Vec<String> = (0..10).map(|n| format!("token_{n}")).collect();
        let sentence = left_out_sentence(&many);
        assert!(sentence.ends_with("`token_7` and 2 more."), "{sentence}");
        assert!(!sentence.contains("token_8"), "{sentence}");
        let long = "p".repeat(80);
        assert!(left_out_sentence(&[long]).contains(&format!("`{}`", "p".repeat(60))));
    }
}
