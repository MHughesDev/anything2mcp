//! How a `file` binding becomes steps, and the pack's lint rules.
//!
//! A read is `open`, `locate`, `read`. A write is `open`, `locate`, `set`,
//! `save`, with `save` its one commit point, a `read` that confirms the
//! file at the path is the one saved, and a read-back that opens the file
//! again. No value from an input reaches a file name or a place: an input
//! fills the value of a key or of a field, and nothing else.

use a2m_core::{
    EffectClass, FailsAs, FailureClass, FailureCode, FileKindName, IdentityStrategy, LintCode,
    Mode, PackPayload, PackRef, SectionIndex, Step, StepArg, StepKind, SurfaceKind, SurfaceName,
    ValueType, Verb,
};
use a2m_files::{
    CompileMode, Diagnostic, Diagnostics, Doc, JsonSchema, Level, LintRule, Located, LowerContext,
    Lowered, Lowering, OperationView, Possible, ResolvedBinding, ResolvedView, Stage, SurfaceView,
};

use crate::codes;
use crate::kind::document_name;
use crate::model::{
    Change, DOCUMENT_KIND, DocumentSpec, Expect, Format, LOCATE, MODE, OPEN, READ, RecordSpec,
    SAVE, SET, SURFACE_KIND, Section, StepSpec, VERB_CREATE, VERB_DELETE, VERB_LIST, VERB_READ,
    VERB_UPDATE, field_arg, key_arg,
};

/// A binding that names a document, a record kind, or a field the target
/// does not declare, or leaves out part of a key.
pub const BINDING_INVALID: LintCode = LintCode::fixed("file.binding_invalid");
/// See the manifest for what each rule requires.
pub const STANDS_UNDECLARED: LintCode = LintCode::fixed("file.stands_undeclared");
/// A write to a file beneath an application.
pub const BENEATH_APPLICATION: LintCode = LintCode::fixed("file.beneath_application");
/// A write to a file that may be open, with no lock signal.
pub const OPEN_WITHOUT_SIGNAL: LintCode = LintCode::fixed("file.open_without_signal");
/// A write to a format with no patch editor.
pub const FORMAT_READ_ONLY: LintCode = LintCode::fixed("file.format_read_only");

/// How long a step may take when the target states no `step` timeout.
const STEP_TIMEOUT_MS: u32 = 30_000;
/// How many rows a list returns when its binding states no limit.
const LIST_LIMIT: u32 = 1_000;

/// The lowering of the `file` mode.
pub struct FileLowering {
    mode: Mode,
    schema: JsonSchema,
    verbs: Vec<Verb>,
    step_kinds: Vec<StepKind>,
}

impl FileLowering {
    /// The lowering, for the mode given.
    #[must_use]
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            schema: JsonSchema::object()
                .required("document", JsonSchema::name())
                .required("record", JsonSchema::name())
                .optional("key", JsonSchema::map_of(JsonSchema::expression()))
                .optional("fields", JsonSchema::map_of(JsonSchema::expression()))
                .optional("limit", JsonSchema::int_between(1, 1_000_000))
                .optional(
                    "read_back",
                    JsonSchema::object().required("record", JsonSchema::name()),
                ),
            verbs: vec![VERB_READ, VERB_LIST, VERB_UPDATE, VERB_CREATE, VERB_DELETE],
            step_kinds: vec![OPEN, LOCATE, SET, READ, SAVE],
        }
    }
}

fn problem(at: &Located<Doc>, message: impl Into<String>, fix: &str) -> Diagnostic {
    Diagnostic::error(BINDING_INVALID, Stage::Lower, &at.at, message, fix)
}

/// What every step of one binding shares.
struct Steps<'a> {
    section: SectionIndex,
    document: &'a str,
    timeout_ms: u32,
    at: &'a Located<Doc>,
}

impl Steps<'_> {
    #[allow(clippy::too_many_arguments)] // one step's facts, each named where it is called
    fn step(
        &self,
        kind: StepKind,
        spec: &StepSpec,
        args: Vec<StepArg>,
        fails: FailureCode,
        commits: bool,
        lookup: bool,
    ) -> Result<Step, Diagnostic> {
        let payload = PackPayload::encode(spec)
            .map_err(|failure| problem(self.at, failure.message().to_owned(), "Report this."))?;
        let class = codes::ALL
            .iter()
            .find(|(_, code)| *code == fails)
            .map_or(FailureClass::PossibleCommit, |(class, _)| *class);
        Ok(Step {
            kind,
            refers_to: Some(PackRef {
                section: self.section,
                entry: self.document.to_owned(),
            }),
            args,
            timeout_ms: self.timeout_ms,
            fails_as: FailsAs { class, code: fails },
            commits,
            lookup,
            idempotent: !commits,
            when: None,
            payload,
        })
    }

    fn open(&self, for_write: bool) -> Result<Step, Diagnostic> {
        self.step(
            OPEN,
            &StepSpec::Open { for_write },
            Vec::new(),
            codes::FILE_MISSING,
            false,
            false,
        )
    }

    fn locate(
        &self,
        record: &str,
        expect: Expect,
        key: Vec<StepArg>,
        lookup: bool,
    ) -> Result<Step, Diagnostic> {
        self.step(
            LOCATE,
            &StepSpec::Locate {
                record: record.to_owned(),
                expect,
            },
            key,
            codes::PLACE_NOT_FOUND,
            false,
            lookup,
        )
    }

    fn one(&self, record: &str) -> Result<Step, Diagnostic> {
        self.step(
            READ,
            &StepSpec::One {
                record: record.to_owned(),
            },
            Vec::new(),
            codes::VALUE_UNREADABLE,
            false,
            false,
        )
    }
}

/// The type of a field's values, from the field type its document names.
fn value_type(
    cx: &LowerContext<'_>,
    document: &str,
    record: &str,
    field: &str,
) -> Option<ValueType> {
    let entries = cx.entries(&DOCUMENT_KIND, &SURFACE_KIND)?;
    let doc = entries.iter().find(|doc| document_name(doc) == document)?;
    let name = doc
        .get("records")?
        .get(record)?
        .get("fields")?
        .get(field)?
        .text("type")?;
    cx.target()
        .field_type(name)
        .map(|found| found.plan.ty.clone())
}

struct Bound<'a> {
    binding: &'a ResolvedBinding<'a>,
    cx: &'a LowerContext<'a>,
    document: &'a str,
    record_name: &'a str,
    record: &'a RecordSpec,
}

impl Bound<'_> {
    /// The key of the binding, as arguments: every key field, and no other.
    fn key(&self, problems: &mut Diagnostics) -> Vec<StepArg> {
        let stated = self.binding.field("key");
        let entries = stated.map(Located::entries).unwrap_or_default();
        let mut args = Vec::new();
        for name in &self.record.key {
            let Some(entry) = entries.iter().find(|entry| &entry.key.value == name) else {
                problems.push(problem(
                    stated.unwrap_or(self.binding.doc),
                    format!("The binding gives no value for the key field `{name}`."),
                    "Add the field under `key:` with the input that holds it.",
                ));
                continue;
            };
            let ty = value_type(self.cx, self.document, self.record_name, name);
            match self.cx.expr(&entry.value, ty.as_ref()) {
                Ok(value) => args.push(StepArg {
                    name: key_arg(name),
                    value,
                }),
                Err(found) => problems.push(found),
            }
        }
        for entry in entries {
            if !self.record.key.contains(&entry.key.value) {
                problems.push(problem(
                    &entry.value,
                    format!(
                        "`{}` is not a key field of the record kind `{}`.",
                        entry.key.value, self.record_name
                    ),
                    "Name only the fields the document lists under `key`.",
                ));
            }
        }
        args
    }

    /// The fields the binding sets, as arguments. Each is declared and
    /// marked `writable`.
    fn fields(&self, problems: &mut Diagnostics) -> Vec<StepArg> {
        let stated = self.binding.field("fields");
        let entries = stated.map(Located::entries).unwrap_or_default();
        let mut args = Vec::new();
        for entry in entries {
            let name = &entry.key.value;
            if !self.record.field(name).is_some_and(|field| field.writable) {
                problems.push(problem(
                    &entry.value,
                    format!(
                        "The record kind `{}` has no field `{name}` marked `writable`.",
                        self.record_name
                    ),
                    "Set only fields the document marks `writable: true`.",
                ));
                continue;
            }
            let ty = value_type(self.cx, self.document, self.record_name, name);
            match self.cx.expr(&entry.value, ty.as_ref()) {
                Ok(value) => args.push(StepArg {
                    name: field_arg(name),
                    value,
                }),
                Err(found) => problems.push(found),
            }
        }
        args
    }
}

impl Lowering for FileLowering {
    fn mode(&self) -> Mode {
        self.mode.clone()
    }

    fn verbs(&self) -> &[Verb] {
        &self.verbs
    }

    fn reads(&self) -> &[(FileKindName, SurfaceKind)] {
        &[]
    }

    fn step_kinds(&self) -> &[StepKind] {
        &self.step_kinds
    }

    fn binding_schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn identity(&self, _binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible {
        match strategy {
            IdentityStrategy::Natural | IdentityStrategy::Composite | IdentityStrategy::None => {
                Possible::Yes
            }
            IdentityStrategy::Stamped | IdentityStrategy::Find => Possible::No(
                "a file never assigns a key, so it has no field that carries a stamp and no search."
                    .to_owned(),
            ),
        }
    }

    #[allow(clippy::too_many_lines)] // one verb after another; splitting hides the order of the steps
    fn lower(
        &self,
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
    ) -> Result<Lowered, Diagnostics> {
        let fail = |found: Diagnostic| Err(Diagnostics::from(found));
        let document_doc = binding.field("document").unwrap_or(binding.doc);
        let document = document_doc.as_text().unwrap_or_default();
        let section = cx.section(&DOCUMENT_KIND, &SURFACE_KIND);
        let decoded = section.and_then(|(index, section)| {
            section
                .payload
                .decode::<Section>()
                .ok()
                .map(|decoded| (index, decoded))
        });
        let Some((section, spec)) = decoded
            .and_then(|(index, mut decoded)| decoded.remove(document).map(|spec| (index, spec)))
        else {
            return fail(problem(
                document_doc,
                format!("No file under `documents/` is named `{document}`."),
                "Name a document by the name of its file, without the extension.",
            ));
        };
        let spec: DocumentSpec = spec;
        let record_doc = binding.field("record").unwrap_or(binding.doc);
        let record_name = record_doc.as_text().unwrap_or_default();
        let Some(record) = spec.records.get(record_name) else {
            return fail(problem(
                record_doc,
                format!("The document `{document}` declares no record kind `{record_name}`."),
                "Name a record kind the document lists under `records`.",
            ));
        };
        let Ok(surface) = SurfaceName::new(&spec.surface) else {
            return fail(problem(
                document_doc,
                format!("The document `{document}` is on no surface."),
                "Name the document's surface.",
            ));
        };
        let steps = Steps {
            section,
            document,
            timeout_ms: cx
                .target()
                .default_timeout_ms("step")
                .unwrap_or(STEP_TIMEOUT_MS),
            at: binding.doc,
        };
        let bound = Bound {
            binding,
            cx,
            document,
            record_name,
            record,
        };
        let mut problems = Diagnostics::new();
        let verb = binding.verb;
        let writes = *verb == VERB_UPDATE || *verb == VERB_CREATE || *verb == VERB_DELETE;
        let unexpected = |key: &str, problems: &mut Diagnostics| {
            if let Some(found) = binding.field(key) {
                problems.push(problem(
                    found,
                    format!("A `{verb}` binding takes no `{key}`."),
                    "Remove it.",
                ));
            }
        };
        let built: Result<Lowered, Diagnostic> = (|| {
            let mut lowered = Lowered {
                surface,
                act: Vec::new(),
                confirm: Vec::new(),
                read_back: Vec::new(),
                lookup: Vec::new(),
                settle_ms: 0,
                prefer_response: false,
            };
            if *verb == VERB_LIST {
                unexpected("key", &mut problems);
                unexpected("fields", &mut problems);
                let limit = binding
                    .field("limit")
                    .and_then(Located::as_int)
                    .and_then(|limit| u32::try_from(limit).ok())
                    .unwrap_or(LIST_LIMIT);
                lowered.act = vec![
                    steps.open(false)?,
                    steps.locate(record_name, Expect::All, Vec::new(), false)?,
                    steps.step(
                        READ,
                        &StepSpec::List {
                            record: record_name.to_owned(),
                            limit,
                        },
                        Vec::new(),
                        codes::VALUE_UNREADABLE,
                        false,
                        false,
                    )?,
                ];
                return Ok(lowered);
            }
            unexpected("limit", &mut problems);
            let key = bound.key(&mut problems);
            if *verb == VERB_READ {
                unexpected("fields", &mut problems);
                lowered.act = vec![
                    steps.open(false)?,
                    steps.locate(record_name, Expect::Present, key, true)?,
                    steps.one(record_name)?,
                ];
                return Ok(lowered);
            }
            let (expect, change) = if *verb == VERB_UPDATE {
                (Expect::Present, Change::Update)
            } else if *verb == VERB_CREATE {
                (Expect::Absent, Change::Create)
            } else {
                (Expect::Present, Change::Delete)
            };
            let fields = if change == Change::Delete {
                unexpected("fields", &mut problems);
                Vec::new()
            } else {
                bound.fields(&mut problems)
            };
            let none_stated = binding
                .field("fields")
                .is_none_or(|stated| stated.entries().is_empty());
            if change == Change::Update && none_stated {
                problems.push(problem(
                    binding.doc,
                    "An `update` binding sets at least one field.",
                    "Add `fields:` with each field to set and the input that holds its value.",
                ));
            }
            if change != Change::Update && record.key.is_empty() {
                problems.push(problem(
                    binding.doc,
                    format!("The record kind `{record_name}` is one place with no key, so nothing can be added to it or removed from it."),
                    "Use `update`, or bind a record kind that has a key.",
                ));
            }
            // A new row carries its key and its fields; the other writes
            // carry the key at `locate` and the fields at `set`.
            let set_args = if change == Change::Create {
                key.iter().cloned().chain(fields).collect()
            } else {
                fields
            };
            lowered.act = vec![
                steps.open(true)?,
                steps.locate(record_name, expect, key.clone(), false)?,
                steps.step(
                    SET,
                    &StepSpec::Set {
                        record: record_name.to_owned(),
                        change,
                    },
                    set_args,
                    codes::ROUND_TRIP_FAILED,
                    false,
                    false,
                )?,
                steps.step(
                    SAVE,
                    &StepSpec::Save,
                    Vec::new(),
                    a2m_provider::codes::NO_CONFIRMATION,
                    true,
                    false,
                )?,
            ];
            lowered.confirm = vec![steps.step(
                READ,
                &StepSpec::Confirm,
                Vec::new(),
                codes::CHANGED_SINCE_PREVIEW,
                false,
                false,
            )?];
            lowered.read_back = vec![
                steps.open(false)?,
                steps.locate(record_name, Expect::Present, key, true)?,
                steps.one(record_name)?,
            ];
            Ok(lowered)
        })();
        if writes
            && let Some(read_back) = binding.field("read_back")
            && read_back.text("record") != Some(record_name)
        {
            problems.push(problem(
                read_back,
                "A file write is read back from the record kind it wrote.",
                "Name the same record kind under `read_back`.",
            ));
        }
        if !writes && let Some(read_back) = binding.field("read_back") {
            problems.push(problem(
                read_back,
                "A binding that only reads has no read-back.",
                "Remove `read_back`.",
            ));
        }
        match built {
            Ok(lowered) if problems.is_empty() => Ok(lowered),
            Ok(_) => Err(problems),
            Err(found) => {
                problems.push(found);
                Err(problems)
            }
        }
    }
}

/// One binding of the `file` mode that writes, with what it writes to.
struct WriteBinding<'a> {
    operation: OperationView<'a>,
    binding: &'a Located<Doc>,
    document: &'a Located<Doc>,
    surface: Option<SurfaceView<'a>>,
}

fn write_bindings<'a>(view: &ResolvedView<'a>) -> Vec<WriteBinding<'a>> {
    let documents = view.entries(&DOCUMENT_KIND, &SURFACE_KIND);
    let mut found = Vec::new();
    for operation in view.operations() {
        let bindings = operation
            .doc()
            .get("bindings")
            .map(Located::items)
            .unwrap_or_default();
        for binding in bindings {
            let writes = matches!(binding.text("verb"), Some("update" | "create" | "delete"));
            if binding.text("mode") != Some(MODE) || !writes {
                continue;
            }
            let Some(document) = binding
                .text("document")
                .and_then(|name| documents.iter().find(|doc| document_name(doc) == name))
            else {
                continue;
            };
            let surface = match document.text("surface") {
                Some(name) => view.target().surface(name),
                None => view
                    .target()
                    .surfaces()
                    .into_iter()
                    .find(|surface| *surface.kind == SURFACE_KIND),
            };
            found.push(WriteBinding {
                operation,
                binding,
                document,
                surface,
            });
        }
    }
    found
}

type Check = fn(&WriteBinding<'_>, &ResolvedView<'_>) -> Option<(String, &'static str)>;

/// A rule about bindings that write a file.
pub struct WriteRule {
    code: LintCode,
    summary: &'static str,
    check: Check,
}

impl LintRule for WriteRule {
    fn code(&self) -> LintCode {
        self.code.clone()
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Error
    }

    fn summary(&self) -> &str {
        self.summary
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        write_bindings(view)
            .iter()
            .filter_map(|write| {
                (self.check)(write, view).map(|(message, fix)| {
                    Diagnostic::error(
                        self.code.clone(),
                        Stage::Lint,
                        &write.binding.at,
                        message,
                        fix,
                    )
                })
            })
            .collect()
    }
}

fn stands<'a>(write: &WriteBinding<'a>) -> Option<&'a Located<Doc>> {
    write.surface.and_then(|surface| surface.doc.get("stands"))
}

/// The pack's lint rules, as the manifest declares them.
#[must_use]
pub fn lint_rules() -> Vec<WriteRule> {
    vec![
        WriteRule {
            code: STANDS_UNDECLARED,
            summary: "A binding that writes a file is on a surface that declares `stands`.",
            check: |write, _| {
                stands(write).is_none().then(|| {
                    (
                        "This binding writes a file, and its surface does not say whether the file stands alone or beneath an application.".to_owned(),
                        "Add `stands: alone` to the surface, or `stands: {beneath: <surface>}` when an application owns the file.",
                    )
                })
            },
        },
        WriteRule {
            code: BENEATH_APPLICATION,
            summary: "A write to a file beneath an application is irreversible and has the owner's word in the authorization record.",
            check: |write, view| {
                stands(write)?.get("beneath")?;
                let authorized = view
                    .sources()
                    .file("target.yaml")
                    .and_then(|file| file.docs.first())
                    .is_some_and(|target| target.get("authorization").is_some());
                (write.operation.effect() != EffectClass::Irreversible || !authorized).then(|| {
                    (
                        "This binding writes a file that stands beneath an application.".to_owned(),
                        "Declare the operation `effect: irreversible`, and name the authorization record under `authorization` in `target.yaml`.",
                    )
                })
            },
        },
        WriteRule {
            code: OPEN_WITHOUT_SIGNAL,
            summary: "A binding that writes a file an application may hold open declares the sign that it does.",
            check: |write, _| {
                let may_be_open =
                    write.document.get("may_be_open").and_then(Located::as_bool) == Some(true);
                (may_be_open && write.document.get("lock_signal").is_none()).then(|| {
                    (
                        "This binding writes a file an application may hold open, and the document declares no lock signal.".to_owned(),
                        "Add `lock_signal: {sibling: <name>}` to the document, or do not write the file.",
                    )
                })
            },
        },
        WriteRule {
            code: FORMAT_READ_ONLY,
            summary: "A binding writes only a format the pack has a patch editor for: delimited text or JSON.",
            check: |write, _| {
                let format = write.document.text("format").and_then(Format::from_name);
                (!format.is_some_and(Format::writable)).then(|| {
                    (
                        "This binding writes a file whose format the pack can read and cannot write back without loss.".to_owned(),
                        "Bind only reads to this document.",
                    )
                })
            },
        },
    ]
}
