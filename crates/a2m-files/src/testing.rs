//! A small imaginary pack, for tests of this crate and of the crates above
//! it. It is compiled only for tests and under the `testing` feature, and
//! is never part of the product.
//!
//! The pack is called `stand`. It serves one surface kind, `stand`, owns one
//! file kind, `call` in the folder `calls/`, and lowers the verbs `read` and
//! `upsert` of the mode `stand` into steps of the kinds `fetch` and `send`.
//! It registers one control, one lint rule, and one drafting rule. The
//! `example` target under `targets/example/` is written for it.
//!
//! Two ways to use it. [`stand_set`] gives a compiler set with the pack in
//! it, for a test that only compiles. A test that needs a whole pack, with
//! a driver beside the compiler side, registers the parts itself through
//! `a2m-provider`: [`surface_kind_spec`], [`control`], [`file_kind`],
//! [`lowering`], [`lint_rule`], and [`drafting_rule`] are the objects, and
//! [`PACK`], [`FOLDER`], [`surface_kind`], [`call_kind`], [`mode`],
//! [`verbs`], [`step_kinds`], [`lint_code`], and [`finding_kind`] are the
//! names its manifest declares.
//!
//! What a driver for the pack meets in a plan: a `fetch` step reads and may
//! report a record absent; a `send` step writes, and always commits. The
//! pack states both once, for the kind of step, and the compiler holds
//! every lowered step to it: a `send` can never be lowered under an
//! operation that declares `read`, whatever its call's file says, and a
//! `fetch` of a call whose effect is `write` does not compile. The verb
//! `upsert` is a write by what it does. Each step refers to its call by name in the pack
//! section of the kind `call`, whose payload is a JSON map from the call's
//! name to its `path`, its `effect`, its `surface`, and, where the call has
//! one, `accept`, the index of an expression. A step's arguments are the
//! placeholders of the call's path, then the binding's other `args`.

use std::path::Path;
use std::sync::Arc;

use a2m_core::{
    ControlName, EffectClass, FailsAs, Failure, FailureClass, FailureCode, FileKindName, Finding,
    FindingKind, IdentityStrategy, LintCode, Mode, PackPayload, PackRef, Step, StepArg, StepKind,
    SurfaceKind, ValueType, Verb, WriteEvidence,
};

use crate::diag::{Diagnostic, Diagnostics, Level, Stage};
use crate::doc::{Doc, Located, Place, RelPath};
use crate::draft::{DraftContext, DraftDoc, DraftValue};
use crate::load::TargetRoot;
use crate::register::{
    CompileMode, CompiledSection, CompilerSet, CompilerSetBuilder, DeclaredName, DefinedName,
    DraftingRule, ExprSite, FileKind, LintRule, Lowered, Lowering, Possible, ProgramField,
    StepCommits, SurfaceKindSpec,
};
use crate::schema::JsonSchema;
use crate::view::{KindContext, LowerContext, ResolvedBinding, ResolvedView};

/// The pack's name.
pub const PACK: &str = "stand";

/// A name the pack uses. Every name here is well formed, so the fallback is
/// never taken; it only keeps the helpers free of panics.
macro_rules! name {
    ($ty:ty, $text:expr) => {
        <$ty>::new($text).unwrap_or_else(|_| unreachable_name())
    };
}

fn unreachable_name<T>() -> T {
    // The names in this module are constants checked by its tests.
    unreachable!("a name of the test pack is well formed")
}

/// The surface kind the pack serves.
#[must_use]
pub fn surface_kind() -> SurfaceKind {
    name!(SurfaceKind, "stand")
}

/// The file kind the pack owns.
#[must_use]
pub fn call_kind() -> FileKindName {
    name!(FileKindName, "call")
}

/// The mode the pack lowers.
#[must_use]
pub fn mode() -> Mode {
    name!(Mode, "stand")
}

/// The folder the pack's file kind lives in.
pub const FOLDER: &str = "calls";

/// The one control the pack registers.
#[must_use]
pub fn control() -> ControlName {
    name!(ControlName, "box")
}

/// The verbs the pack lowers. The first is the mode's default verb.
#[must_use]
pub fn verbs() -> Vec<Verb> {
    vec![name!(Verb, "read"), name!(Verb, "upsert")]
}

/// The kinds of step the pack's lowering emits, which its driver must know.
#[must_use]
pub fn step_kinds() -> Vec<StepKind> {
    vec![name!(StepKind, "fetch"), name!(StepKind, "send")]
}

/// The code of the pack's lint rule.
#[must_use]
pub fn lint_code() -> LintCode {
    code("path_not_rooted")
}

/// The kind of finding the pack's drafting rule reads.
#[must_use]
pub fn finding_kind() -> FindingKind {
    name!(FindingKind, "stand_call")
}

fn code(rule: &str) -> LintCode {
    name!(LintCode, &format!("{PACK}.{rule}"))
}

fn problem(rule: &str, stage: Stage, at: &Place, message: String, fix: &str) -> Diagnostic {
    Diagnostic::error(code(rule), stage, at, message, fix)
}

/// The placeholders of a path template: the names between braces.
fn placeholders(path: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = path;
    while let Some((_, after)) = rest.split_once('{') {
        let Some((name, after)) = after.split_once('}') else {
            break;
        };
        names.push(name.to_owned());
        rest = after;
    }
    names
}

struct CallKind {
    schema: JsonSchema,
    kinds: Vec<SurfaceKind>,
}

impl FileKind for CallKind {
    fn name(&self) -> FileKindName {
        call_kind()
    }

    fn surface_kinds(&self) -> &[SurfaceKind] {
        &self.kinds
    }

    fn folder(&self) -> &str {
        FOLDER
    }

    fn schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn names(&self, doc: &Located<Doc>) -> Vec<Located<DefinedName>> {
        doc.get("call")
            .and_then(|name| {
                name.as_text().map(|text| {
                    Located::new(
                        DefinedName {
                            name: text.to_owned(),
                        },
                        name.at.clone(),
                    )
                })
            })
            .into_iter()
            .collect()
    }

    fn expressions(&self, doc: &Located<Doc>) -> Vec<ExprSite> {
        doc.get("accept")
            .and_then(|accept| {
                accept.as_text().map(|text| ExprSite {
                    text: text.to_owned(),
                    at: accept.at.clone(),
                    names: vec![DeclaredName {
                        name: "reply".to_owned(),
                        ty: ValueType::Text,
                    }],
                    expected: Some(ValueType::Bool),
                })
            })
            .into_iter()
            .collect()
    }

    /// A call whose effect is not `read` describes a write. One whose
    /// effect is an open question is not known to read, and is not drafted
    /// as if it did.
    fn writes(&self, doc: &Located<Doc>) -> bool {
        doc.text("effect") == Some("write")
    }

    fn compile(
        &self,
        docs: &[Located<Doc>],
        cx: &KindContext<'_>,
    ) -> Result<CompiledSection, Diagnostics> {
        let mut calls = serde_json::Map::new();
        let mut problems = Diagnostics::new();
        for doc in docs {
            let name = doc.text("call").unwrap_or_default();
            let mut entry = serde_json::json!({
                "path": doc.text("path").unwrap_or_default(),
                "effect": doc.text("effect").unwrap_or_default(),
                "surface": cx.surface_of(doc).map(|surface| surface.name.as_str().to_owned()),
            });
            for site in self.expressions(doc) {
                match cx.expr(&site) {
                    Ok(index) => entry["accept"] = serde_json::json!(index.get()),
                    Err(problem) => problems.push(problem),
                }
            }
            calls.insert(name.to_owned(), entry);
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        PackPayload::encode(&calls)
            .map(|payload| CompiledSection { format: 1, payload })
            .map_err(|failure| {
                Diagnostics::from(problem(
                    "call_not_encodable",
                    Stage::Lower,
                    &Place::start_of(&RelPath::target_file()),
                    failure.message().to_owned(),
                    "Keep numbers whole in a call.",
                ))
            })
    }
}

struct StandLowering {
    schema: JsonSchema,
    verbs: Vec<Verb>,
    reads: Vec<(FileKindName, SurfaceKind)>,
    step_kinds: Vec<StepKind>,
}

impl StandLowering {
    /// One step that carries out `call`, with a value for each placeholder
    /// of its path and each entry of `args`.
    fn step(
        &self,
        kind: &str,
        call: &Located<Doc>,
        args: Option<&Located<Doc>>,
        from_inputs_only: bool,
        cx: &LowerContext<'_>,
        problems: &mut Diagnostics,
    ) -> Option<Step> {
        let name = call.as_text()?;
        let entries = cx.entries(&call_kind(), &surface_kind())?;
        let Some(entry) = entries
            .iter()
            .find(|entry| entry.text("call") == Some(name))
        else {
            problems.push(problem(
                "unknown_call",
                Stage::Resolve,
                &call.at,
                format!("No file under `calls/` defines a call named `{name}`."),
                "Name a call that `calls/` defines.",
            ));
            return None;
        };
        let mut step_args = Vec::new();
        for placeholder in placeholders(entry.text("path").unwrap_or_default()) {
            let given = args
                .filter(|_| !from_inputs_only)
                .and_then(|args| args.get(&placeholder));
            let value = match given {
                Some(given) => cx.expr(given, Some(&ValueType::Text)),
                None if cx.operation().input(&placeholder).is_some() => cx.expr_at(
                    &format!("input.{placeholder}"),
                    &call.at,
                    Some(&ValueType::Text),
                ),
                None => Err(problem(
                    "placeholder_unbound",
                    Stage::Resolve,
                    &call.at,
                    format!(
                        "The placeholder `{placeholder}` of the call `{name}` matches no input and has no entry under `args`."
                    ),
                    "Add an input of that name, or an entry under `args`.",
                )),
            };
            match value {
                Ok(value) => step_args.push(StepArg {
                    name: placeholder,
                    value,
                }),
                Err(problem) => problems.push(problem),
            }
        }
        if !from_inputs_only {
            for arg in args.map(Located::entries).unwrap_or_default() {
                if step_args.iter().any(|known| known.name == arg.key.value) {
                    continue;
                }
                match cx.expr(&arg.value, None) {
                    Ok(value) => step_args.push(StepArg {
                        name: arg.key.value.clone(),
                        value,
                    }),
                    Err(problem) => problems.push(problem),
                }
            }
        }
        if kind == "fetch" && entry.text("effect") == Some("write") {
            problems.push(problem(
                "fetch_of_a_write",
                Stage::Lower,
                &call.at,
                format!("The call `{name}` writes, and it is named where a call is only read."),
                "Name a call whose effect is `read` here; a call that writes is the `call` of an `upsert` binding.",
            ));
            return None;
        }
        let section = cx.section_index(&call_kind(), &surface_kind())?;
        Some(Step {
            kind: name!(StepKind, kind),
            refers_to: Some(PackRef {
                section,
                entry: name.to_owned(),
            }),
            args: step_args,
            timeout_ms: 10_000,
            fails_as: FailsAs {
                class: FailureClass::Drift,
                code: FailureCode::fixed("reply_changed"),
            },
            // By the kind of step alone: what the call's file says of its
            // effect is data a target's author wrote, and never decides
            // whether an intent is recorded.
            commits: kind == "send",
            lookup: kind == "fetch",
            idempotent: false,
            when: None,
            payload: PackPayload::none(),
        })
    }
}

impl Lowering for StandLowering {
    fn mode(&self) -> Mode {
        mode()
    }

    fn verbs(&self) -> &[Verb] {
        &self.verbs
    }

    fn default_verb(&self) -> Option<Verb> {
        self.verbs.first().cloned()
    }

    fn reads(&self) -> &[(FileKindName, SurfaceKind)] {
        &self.reads
    }

    fn step_kinds(&self) -> &[StepKind] {
        &self.step_kinds
    }

    fn commits(&self, kind: &StepKind) -> StepCommits {
        if kind.as_str() == "send" {
            StepCommits::Always
        } else {
            StepCommits::Never
        }
    }

    fn least_effect(&self, binding: &ResolvedBinding<'_>) -> EffectClass {
        if binding.verb.as_str() == "read" {
            EffectClass::Read
        } else {
            EffectClass::Write
        }
    }

    fn binding_schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn evidence(&self, binding: &ResolvedBinding<'_>, lowered: &Lowered) -> WriteEvidence {
        match binding.field("evidence").and_then(Located::as_text) {
            Some("acknowledged") => WriteEvidence::Acknowledged,
            _ if lowered.prefer_response => WriteEvidence::Response,
            _ => WriteEvidence::ReadBack,
        }
    }

    fn identity(&self, _binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible {
        match strategy {
            IdentityStrategy::Natural | IdentityStrategy::Composite | IdentityStrategy::None => {
                Possible::Yes
            }
            IdentityStrategy::Stamped | IdentityStrategy::Find => {
                Possible::No("a call has no field that carries a stamp and no search.".to_owned())
            }
        }
    }

    fn lower(
        &self,
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
    ) -> Result<Lowered, Diagnostics> {
        let mut problems = Diagnostics::new();
        let reading = binding.verb.as_str() == "read";
        let args = binding.field("args");
        let act = binding.field("call").and_then(|call| {
            let kind = if reading { "fetch" } else { "send" };
            self.step(kind, call, args, false, cx, &mut problems)
        });
        let read_back = binding
            .field("read_back")
            .and_then(|read_back| read_back.get("call"))
            .and_then(|call| self.step("fetch", call, None, true, cx, &mut problems));
        // A lookup asks only whether the record landed: reached from the
        // identity inputs alone, like a read-back.
        let lookup = binding
            .field("lookup")
            .and_then(|lookup| lookup.get("call"))
            .and_then(|call| self.step("fetch", call, None, true, cx, &mut problems));
        let surface = binding
            .field("call")
            .and_then(Located::as_text)
            .and_then(|name| {
                cx.entries(&call_kind(), &surface_kind())?
                    .iter()
                    .find(|entry| entry.text("call") == Some(name))
            })
            .and_then(|entry| match entry.text("surface") {
                Some(named) => cx.target().surface(named),
                None => cx
                    .target()
                    .surfaces()
                    .into_iter()
                    .find(|surface| *surface.kind == surface_kind()),
            });
        let (Some(act), Some(surface), true) = (act, surface, problems.is_empty()) else {
            if problems.is_empty() {
                problems.push(problem(
                    "unknown_call",
                    Stage::Resolve,
                    &binding.doc.at,
                    "The binding's call cannot be lowered.".to_owned(),
                    "Name a call that `calls/` defines, on a surface of kind `stand`.",
                ));
            }
            return Err(problems);
        };
        Ok(Lowered {
            surface: surface.name.clone(),
            act: vec![act],
            confirm: Vec::new(),
            read_back: read_back.into_iter().collect(),
            lookup: lookup.into_iter().collect(),
            settle_ms: 0,
            prefer_response: binding
                .field("read_back")
                .and_then(|read_back| read_back.text("prefer"))
                == Some("response"),
        })
    }
}

struct CallHasLeadingSlash;

impl LintRule for CallHasLeadingSlash {
    fn code(&self) -> LintCode {
        lint_code()
    }

    fn level(&self, mode: CompileMode) -> Level {
        match mode {
            CompileMode::Publish => Level::Error,
            CompileMode::Draft => Level::Warning,
        }
    }

    fn summary(&self) -> &str {
        "A call's path starts with a slash."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        view.entries(&call_kind(), &surface_kind())
            .iter()
            .filter_map(|entry| entry.get("path"))
            .filter(|path| path.as_text().is_some_and(|text| !text.starts_with('/')))
            .map(|path| {
                problem(
                    "path_not_rooted",
                    Stage::Lint,
                    &path.at,
                    "The call's path does not start with a slash.".to_owned(),
                    "Start the path with `/`.",
                )
            })
            .collect()
    }
}

struct CallFromFinding;

impl DraftingRule for CallFromFinding {
    fn finding_kind(&self) -> FindingKind {
        finding_kind()
    }

    fn file_kind(&self) -> FileKindName {
        call_kind()
    }

    /// The finding's `found` is `name path`, or `name path effect`. A
    /// finding that does not state the effect leaves an open question.
    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let mut parts = finding.found().split_whitespace();
        let (Some(name), Some(path)) = (parts.next(), parts.next()) else {
            return Err(Failure::new(
                FailureClass::Input,
                FailureCode::FINDING_INVALID,
                "A `stand_call` finding states a name and a path.",
                "Correct the provider that made the finding.",
            ));
        };
        let effect = parts.next().map_or_else(
            || cx.open_question(&format!("Does the call `{name}` read or write?")),
            DraftValue::text,
        );
        Ok(vec![DraftDoc {
            path: RelPath::new(&format!("calls/{name}.yaml"))?,
            body: DraftValue::map(vec![
                ("call", DraftValue::text(name)),
                ("path", DraftValue::text(path)),
                ("effect", effect),
            ]),
        }])
    }
}

/// The pack's surface kind, as data: an address, an optional profile, and
/// an optional helper program with its digest, which is the one program a
/// surface of this kind declares.
#[must_use]
pub fn surface_kind_spec() -> SurfaceKindSpec {
    SurfaceKindSpec {
        kind: surface_kind(),
        address_key: "address".to_owned(),
        fields: JsonSchema::object()
            .required("address", JsonSchema::text())
            .optional("profile", JsonSchema::name())
            .optional("helper", JsonSchema::text())
            .optional("helper_digest", JsonSchema::text()),
        programs: vec![ProgramField {
            path_key: "helper".to_owned(),
            digest_key: Some("helper_digest".to_owned()),
        }],
        notices: Vec::new(),
    }
}

/// The pack's file kind, `call`, for surfaces of the kind `stand`.
#[must_use]
pub fn file_kind() -> Arc<dyn FileKind> {
    Arc::new(CallKind {
        schema: JsonSchema::object()
            .required("call", JsonSchema::name())
            .optional("surface", JsonSchema::surface())
            .required("path", JsonSchema::text())
            .required("effect", JsonSchema::words(&["read", "write"]))
            .optional("control", JsonSchema::control())
            .optional("accept", JsonSchema::expression()),
        kinds: vec![surface_kind()],
    })
}

/// The same file kind, `call`, registered for surfaces of another kind, as
/// a second pack would register a kind of one name for its own surfaces
/// (decision 0203).
#[must_use]
pub fn file_kind_for(kind: SurfaceKind) -> Arc<dyn FileKind> {
    Arc::new(CallKind {
        schema: JsonSchema::object()
            .required("call", JsonSchema::name())
            .optional("surface", JsonSchema::surface())
            .required("path", JsonSchema::text())
            .required("effect", JsonSchema::words(&["read", "write"])),
        kinds: vec![kind],
    })
}

/// A surface kind of another name, as data, for a second pack.
#[must_use]
pub fn surface_kind_spec_for(kind: SurfaceKind) -> SurfaceKindSpec {
    SurfaceKindSpec {
        kind,
        ..surface_kind_spec()
    }
}

/// The pack's lowering of the mode `stand`, for the verbs of [`verbs`].
#[must_use]
pub fn lowering() -> Arc<dyn Lowering> {
    Arc::new(StandLowering {
        schema: JsonSchema::object()
            .required("call", JsonSchema::name())
            .optional("args", JsonSchema::map_of(JsonSchema::expression()))
            .optional(
                "read_back",
                JsonSchema::object()
                    .required("call", JsonSchema::name())
                    .optional("prefer", JsonSchema::words(&["response"])),
            )
            .optional("evidence", JsonSchema::words(&["acknowledged"]))
            .optional(
                "lookup",
                JsonSchema::object().required("call", JsonSchema::name()),
            ),
        verbs: verbs(),
        reads: Vec::new(),
        step_kinds: step_kinds(),
    })
}

/// The pack's lint rule: a call's path starts with a slash.
#[must_use]
pub fn lint_rule() -> Arc<dyn LintRule> {
    Arc::new(CallHasLeadingSlash)
}

/// The pack's drafting rule: from a `stand_call` finding to a `call` file.
#[must_use]
pub fn drafting_rule() -> Arc<dyn DraftingRule> {
    Arc::new(CallFromFinding)
}

/// The kinds of thing the pack registers, as [`register_part`] names them.
pub const STAND_PARTS: &[&str] = &[
    "surface_kind",
    "control",
    "file_kind",
    "lowering",
    "lint_rule",
    "drafting_rule",
];

/// Registers one of the pack's parts under the name `pack`.
///
/// # Errors
///
/// `registration_conflict`, as the builder gives it.
pub fn register_part(
    builder: &mut CompilerSetBuilder,
    pack: &str,
    part: &str,
) -> Result<(), Failure> {
    match part {
        "surface_kind" => builder.surface_kind(pack, surface_kind_spec())?,
        "control" => builder.control(pack, control())?,
        "file_kind" => builder.file_kind(pack, file_kind())?,
        "lowering" => builder.lowering(pack, lowering())?,
        "lint_rule" => builder.lint_rule(pack, lint_rule())?,
        _ => builder.drafting_rule(pack, drafting_rule())?,
    };
    Ok(())
}

/// Registers the whole `stand` pack.
///
/// # Errors
///
/// `registration_conflict` when any part of it is already registered.
pub fn register_stand(builder: &mut CompilerSetBuilder) -> Result<(), Failure> {
    STAND_PARTS
        .iter()
        .try_for_each(|part| register_part(builder, PACK, part))
}

/// A compiler set with the `stand` pack registered.
///
/// # Errors
///
/// `registration_conflict`, which a fresh builder never gives.
pub fn stand_set() -> Result<CompilerSet, Failure> {
    let mut builder = CompilerSet::builder();
    register_stand(&mut builder)?;
    Ok(builder.build())
}

/// The folder of the `example` target in this repository, which is written
/// for the `stand` pack.
#[must_use]
pub fn example_root() -> TargetRoot {
    TargetRoot::new(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("targets")
            .join("example"),
    )
}
