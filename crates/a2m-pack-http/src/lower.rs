//! The lowering of the `http` mode: one binding into `request`,
//! `validate`, and `extract` steps, each naming its endpoint in the pack's
//! section of the plan and carrying this pack's own payload.
//!
//! | Verb | Act | Confirm | Read back |
//! | --- | --- | --- | --- |
//! | `read`, `list`, `search` | `request`, `validate`, `extract` | | |
//! | the write verbs | `request`, marked as the commit point | `validate` | `request`, `validate`, `extract` on the endpoint `read_back` names |
//!
//! With `read_back: {prefer: response}` the confirm list also ends in an
//! `extract` of the save's own answer.

use a2m_core::{
    EffectClass, FailsAs, FailureClass, FileKindName, IdentityStrategy, Mode, PackPayload, PackRef,
    SectionIndex, Step, StepArg, StepKind, SurfaceKind, SurfaceName, ValueType, Verb,
};
use a2m_files::{
    Diagnostics, Doc, JsonSchema, Located, LowerContext, Lowered, Lowering, Place, Possible,
    ResolvedBinding, Stage, StepCommits,
};

use crate::codes;
use crate::endpoints::{ENDPOINT_KIND, HTTP_SURFACE, problem};
use crate::shape::Kind;
use crate::template::{
    BodyField, EndpointSection, EndpointTemplate, ExtractStep, Extracted, RequestStep, Take,
    VersionSink,
};

/// The step that sends one request built from an endpoint's template.
pub const REQUEST: StepKind = StepKind::fixed("request");
/// The step that checks the answer's status and shape.
pub const VALIDATE: StepKind = StepKind::fixed("validate");
/// The step that takes typed values out of the answer.
pub const EXTRACT: StepKind = StepKind::fixed("extract");
/// The step only a discoverer makes: one safe request with no body.
pub const PROBE: StepKind = StepKind::fixed("probe");

const READ_VERBS: &[&str] = &["read", "list", "search"];
const WRITE_VERBS: &[&str] = &["create", "update", "upsert", "delete", "submit", "call"];
/// The verbs whose writes cannot be taken back, whatever reads them back.
const IRREVERSIBLE_VERBS: &[&str] = &["delete", "submit"];

/// How long a step may take when the target states no `request` timeout.
const DEFAULT_TIMEOUT_MS: u32 = 30_000;

/// The time limit of a step that judges or reads an answer already held.
/// It sends nothing and waits for nothing, so it does not take a request's
/// limit: the engine sums a binding's step limits to decide whether a
/// job's lease leaves time for a write, and five request limits would
/// pass the lease of the in-process runner.
const HELD_ANSWER_TIMEOUT_MS: u32 = 1_000;

fn binding_part() -> JsonSchema {
    JsonSchema::object()
        .required("endpoint", JsonSchema::name())
        .optional("path_params", JsonSchema::map_of(JsonSchema::expression()))
        .optional("query", JsonSchema::map_of(JsonSchema::expression()))
        .optional("extract", JsonSchema::map_of(JsonSchema::text()))
}

/// The lowering of the `http` mode.
pub(crate) struct HttpLowering {
    mode: Mode,
    schema: JsonSchema,
    verbs: Vec<Verb>,
    step_kinds: Vec<StepKind>,
}

impl HttpLowering {
    pub(crate) fn new(mode: Mode) -> Self {
        Self {
            mode,
            schema: binding_part()
                .optional("body", JsonSchema::map_of(JsonSchema::expression()))
                .optional(
                    "read_back",
                    binding_part().optional("prefer", JsonSchema::words(&["response"])),
                ),
            verbs: READ_VERBS
                .iter()
                .chain(WRITE_VERBS)
                .map(|verb| Verb::fixed(verb))
                .collect(),
            step_kinds: vec![REQUEST, VALIDATE, EXTRACT],
        }
    }
}

/// One endpoint as a lowering uses it.
struct Found<'a> {
    name: &'a str,
    at: &'a Place,
    section: SectionIndex,
    template: EndpointTemplate,
}

struct Lower<'a, 'b> {
    cx: &'a LowerContext<'b>,
    section: Option<(SectionIndex, EndpointSection)>,
    timeout_ms: u32,
    problems: Diagnostics,
}

impl<'a> Lower<'a, '_> {
    fn refuse(&mut self, rule: &'static str, stage: Stage, at: &Place, message: String, fix: &str) {
        self.problems.push(problem(rule, stage, at, message, fix));
    }

    /// The endpoint a binding, or its `read_back`, names.
    fn endpoint(&mut self, part: &'a Located<Doc>) -> Option<Found<'a>> {
        let named = part.get("endpoint")?;
        let name = named.as_text()?;
        let found = self.section.as_ref().and_then(|(index, section)| {
            section.get(name).map(|template| Found {
                name,
                at: &named.at,
                section: *index,
                template: template.clone(),
            })
        });
        if found.is_none() {
            self.refuse(
                "unknown_endpoint",
                Stage::Resolve,
                &named.at,
                format!(
                    "No file under `endpoints/` defines an endpoint named `{name}` on a surface of the kind `http`."
                ),
                "Name an endpoint that `endpoints/` defines on an `http` surface.",
            );
        }
        found
    }

    fn step(&self, kind: StepKind, found: &Found<'_>, fails_as: FailsAs) -> Step {
        let timeout_ms = if kind == REQUEST {
            self.timeout_ms
        } else {
            HELD_ANSWER_TIMEOUT_MS
        };
        Step {
            kind,
            refers_to: Some(PackRef {
                section: found.section,
                entry: found.name.to_owned(),
            }),
            args: Vec::new(),
            timeout_ms,
            fails_as,
            commits: false,
            lookup: false,
            idempotent: false,
            when: None,
            payload: PackPayload::none(),
        }
    }

    fn encoded<T: serde::Serialize>(&mut self, value: &T, at: &Place) -> PackPayload {
        match PackPayload::encode(value) {
            Ok(payload) => payload,
            Err(failure) => {
                self.refuse(
                    "not_encodable",
                    Stage::Lower,
                    at,
                    failure.message().to_owned(),
                    "Report this to the HTTP pack's author.",
                );
                PackPayload::none()
            }
        }
    }

    /// The `request` step of one endpoint: a value for every placeholder of
    /// its path, and one for each `query` and `body` entry the binding
    /// writes. `from_inputs` is the binding's own doc, or its `read_back`.
    fn request(
        &mut self,
        found: &Found<'_>,
        part: &Located<Doc>,
        with_body: bool,
        version_from: Option<String>,
    ) -> Step {
        let scalar = |ty: &ValueType| {
            matches!(
                ty,
                ValueType::Text
                    | ValueType::Int
                    | ValueType::Decimal
                    | ValueType::Bool
                    | ValueType::Date
                    | ValueType::Choice
            )
        };
        let mut args = Vec::new();
        let params = part.get("path_params");
        for hole in found.template.holes() {
            let value = match params.and_then(|params| params.get(hole)) {
                Some(given) => self.cx.expr(given, None).map(|index| (index, &given.at)),
                None if self.cx.operation().input(hole).is_some() => self
                    .cx
                    .expr_at(&format!("input.{hole}"), found.at, None)
                    .map(|index| (index, found.at)),
                None => {
                    self.refuse(
                        "placeholder_unbound",
                        Stage::Resolve,
                        found.at,
                        format!(
                            "The placeholder `{hole}` of the endpoint `{}` matches no input and has no entry under `path_params`.",
                            found.name
                        ),
                        "Add an input of that name, or an entry under `path_params`.",
                    );
                    continue;
                }
            };
            match value {
                Ok((index, at)) => {
                    let fits = self.cx.expr_type(index).is_some_and(|ty| {
                        matches!(ty, ValueType::Text | ValueType::Int | ValueType::Choice)
                    });
                    if !fits {
                        self.refuse(
                            "position_type",
                            Stage::TypeCheck,
                            at,
                            format!("The value for the path placeholder `{hole}` is not text or a whole number."),
                            "Give a value that is one segment of the path.",
                        );
                    }
                    args.push(StepArg {
                        name: format!("path.{hole}"),
                        value: index,
                    });
                }
                Err(wrong) => self.problems.push(wrong),
            }
        }
        for extra in params.map(Located::entries).unwrap_or_default() {
            if !found.template.holes().any(|hole| hole == extra.key.value) {
                self.refuse(
                    "placeholder_unknown",
                    Stage::Resolve,
                    &extra.key.at,
                    format!(
                        "The endpoint `{}` has no placeholder named `{}`.",
                        found.name,
                        codes::shown(&extra.key.value)
                    ),
                    "Remove the entry, or correct its name.",
                );
            }
        }

        let mut payload = RequestStep {
            version_from,
            ..RequestStep::default()
        };
        for entry in part.get("query").map(Located::entries).unwrap_or_default() {
            match self.cx.expr(&entry.value, None) {
                Ok(index) => {
                    if !self.cx.expr_type(index).is_some_and(|ty| scalar(&ty)) {
                        self.refuse(
                            "position_type",
                            Stage::TypeCheck,
                            &entry.value.at,
                            format!(
                                "The query value `{}` is not one text, number, or truth value.",
                                entry.key.value
                            ),
                            "Give a value that is one query value.",
                        );
                    }
                    payload.query.push(entry.key.value.clone());
                    args.push(StepArg {
                        name: format!("query.{}", entry.key.value),
                        value: index,
                    });
                }
                Err(wrong) => self.problems.push(wrong),
            }
        }

        let body = part.get("body").filter(|_| with_body);
        if let Some(body) = body {
            let shape = found.template.request.as_ref();
            if shape.is_none() {
                self.refuse(
                    "body_undeclared",
                    Stage::Resolve,
                    &body.at,
                    format!(
                        "The binding writes a body, and the endpoint `{}` declares no `request_schema`.",
                        found.name
                    ),
                    "Add `request_schema:` to the endpoint, with each body field and its type.",
                );
            }
            for entry in body.entries() {
                let name = &entry.key.value;
                let Some(kind) = shape
                    .and_then(|shape| shape.properties.get(name))
                    .map(|p| p.kind)
                else {
                    if shape.is_some() {
                        self.refuse(
                            "body_undeclared",
                            Stage::Resolve,
                            &entry.key.at,
                            format!(
                                "The endpoint `{}` declares no body field named `{}`.",
                                found.name,
                                codes::shown(name)
                            ),
                            "Add the field to the endpoint's `request_schema`, or remove it from the binding.",
                        );
                    }
                    continue;
                };
                match self.cx.expr(&entry.value, None) {
                    Ok(index) => {
                        let fits = self.cx.expr_type(index).is_some_and(|ty| match kind {
                            Kind::String => matches!(
                                ty,
                                ValueType::Text
                                    | ValueType::Choice
                                    | ValueType::Int
                                    | ValueType::Decimal
                                    | ValueType::Date
                            ),
                            Kind::Integer => ty == ValueType::Int,
                            Kind::Number => matches!(ty, ValueType::Int | ValueType::Decimal),
                            Kind::Boolean => ty == ValueType::Bool,
                            Kind::Any => matches!(
                                ty,
                                ValueType::Text
                                    | ValueType::Choice
                                    | ValueType::Int
                                    | ValueType::Decimal
                                    | ValueType::Bool
                            ),
                            Kind::Null | Kind::Array | Kind::Object => false,
                        });
                        if !fits {
                            self.refuse(
                                "position_type",
                                Stage::TypeCheck,
                                &entry.value.at,
                                format!(
                                    "The value for the body field `{name}` does not fit the type `{}` the endpoint declares.",
                                    kind.word()
                                ),
                                "Give a value of the declared type, or correct the endpoint's `request_schema`.",
                            );
                        }
                        payload.body.push(BodyField {
                            name: name.clone(),
                            kind,
                        });
                        args.push(StepArg {
                            name: format!("body.{name}"),
                            value: index,
                        });
                    }
                    Err(wrong) => self.problems.push(wrong),
                }
            }
            // A field the endpoint requires must be written by the binding,
            // unless it is the version, which the driver sends itself.
            let sent_version = match &found.template.version.send {
                Some(VersionSink::Body(field)) => Some(field.as_str()),
                _ => None,
            };
            for required in shape
                .map(|shape| shape.required.as_slice())
                .unwrap_or_default()
            {
                let written = payload.body.iter().any(|field| field.name == *required);
                if !written && sent_version != Some(required.as_str()) {
                    self.refuse(
                        "body_field_missing",
                        Stage::Resolve,
                        &body.at,
                        format!(
                            "The endpoint `{}` requires the body field `{required}`, and the binding does not write it.",
                            found.name
                        ),
                        "Add the field under `body`.",
                    );
                }
            }
        }

        let mut step = self.step(
            REQUEST,
            found,
            FailsAs {
                class: FailureClass::UnknownVariant,
                code: codes::UNEXPECTED_STATUS,
            },
        );
        step.args = args;
        step.payload = self.encoded(&payload, found.at);
        step
    }

    /// The endpoint whose read of the record supplies the version a write
    /// sends as its precondition: the one the record is read back through,
    /// which must say where the version is read. Without it the write could
    /// only ever go out with no precondition, and the driver refuses that.
    fn version_reader(
        &mut self,
        found: &Found<'_>,
        reader: Option<&Found<'_>>,
        names_read_back: bool,
    ) -> Option<String> {
        found.template.version.send.as_ref()?;
        match reader {
            Some(reader) if reader.template.version.read.is_some() => Some(reader.name.to_owned()),
            Some(reader) => {
                self.refuse(
                    "version_invalid",
                    Stage::Analyze,
                    reader.at,
                    format!(
                        "The endpoint `{}` sends the record's version with a write, and the record is read back through `{}`, which reads no version.",
                        found.name, reader.name
                    ),
                    "Add `version: {read: ...}` to the endpoint the record is read back through, with the field or the header that holds the version.",
                );
                None
            }
            // An endpoint that is not in the catalog is refused by name already.
            None if names_read_back => None,
            None => {
                self.refuse(
                    "version_invalid",
                    Stage::Analyze,
                    found.at,
                    format!(
                        "The endpoint `{}` sends the record's version with a write, and the binding names no `read_back` endpoint that reads it.",
                        found.name
                    ),
                    "Name a `read_back` endpoint that declares `version: {read: ...}`.",
                );
                None
            }
        }
    }

    fn validate(&self, found: &Found<'_>) -> Step {
        self.step(
            VALIDATE,
            found,
            FailsAs {
                class: FailureClass::Drift,
                code: codes::RESPONSE_SCHEMA_MISMATCH,
            },
        )
    }

    /// The `extract` step: one value for each output of the operation, taken
    /// from the answer's field of the same name, or from the path the
    /// binding's `extract` map gives.
    fn extract(&mut self, found: &Found<'_>, part: &Located<Doc>) -> Step {
        let renamed = part.get("extract");
        let mut payload = ExtractStep::default();
        for output in self.cx.operation().outputs() {
            let written = renamed.and_then(|map| map.get(&output.name));
            let at = written.map_or(found.at, |written| &written.at);
            let path: Vec<String> = written
                .and_then(Located::as_text)
                .unwrap_or(&output.name)
                .split('.')
                .map(str::to_owned)
                .collect();
            let (scalar, listed) = match &output.ty {
                ValueType::List(item) => (item.as_ref(), true),
                other => (other, false),
            };
            let take = match scalar {
                ValueType::Text | ValueType::Choice => Take::Text,
                ValueType::Int => Take::Int,
                ValueType::Decimal => Take::Decimal,
                ValueType::Bool => Take::Bool,
                _ => {
                    self.refuse(
                        "output_type",
                        Stage::TypeCheck,
                        at,
                        format!(
                            "The output `{}` is of a type the HTTP pack cannot take out of an answer yet: only text, whole numbers, exact numbers, truth values, and lists of them.",
                            output.name
                        ),
                        "Give the output a field type built on one of those.",
                    );
                    continue;
                }
            };
            let stars = path.iter().filter(|part| *part == "*").count();
            let shape = found
                .template
                .response
                .at_path(&path)
                .map(|(shape, _)| shape);
            let fits = shape.is_some_and(|shape| match take {
                Take::Text => matches!(shape.kind, Kind::String | Kind::Any),
                Take::Int => matches!(shape.kind, Kind::Integer | Kind::Any),
                Take::Decimal => {
                    matches!(
                        shape.kind,
                        Kind::Number | Kind::Integer | Kind::String | Kind::Any
                    )
                }
                Take::Bool => matches!(shape.kind, Kind::Boolean | Kind::Any),
            });
            if !fits || stars != usize::from(listed) {
                self.refuse(
                    "extract_unknown",
                    Stage::Resolve,
                    at,
                    format!(
                        "The output `{}` cannot be taken from `{}` of the answer of `{}`: the endpoint's `response_schema` declares no value of its type there.",
                        output.name,
                        codes::shown(&path.join(".")),
                        found.name
                    ),
                    "Name the field under `extract`, as property names joined by dots, with one `*` for a list; or correct the endpoint's `response_schema`.",
                );
                continue;
            }
            payload.values.push(Extracted {
                name: output.name.clone(),
                path,
                take,
            });
        }
        for extra in renamed.map(Located::entries).unwrap_or_default() {
            if !self
                .cx
                .operation()
                .outputs()
                .iter()
                .any(|output| output.name == extra.key.value)
            {
                self.refuse(
                    "extract_unknown",
                    Stage::Resolve,
                    &extra.key.at,
                    format!(
                        "The operation has no output named `{}`.",
                        codes::shown(&extra.key.value)
                    ),
                    "Remove the entry, or declare the output.",
                );
            }
        }
        let mut step = self.step(
            EXTRACT,
            found,
            FailsAs {
                class: FailureClass::Drift,
                code: codes::RESPONSE_SCHEMA_MISMATCH,
            },
        );
        step.payload = self.encoded(&payload, found.at);
        step
    }
}

impl Lowering for HttpLowering {
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

    /// A `request` sends a read and a write alike, and which it is is the
    /// `effect` of the endpoint it names: the lowering marks it as the
    /// commit point exactly for a verb that writes, which it binds only to
    /// an endpoint marked `write`, and the driver refuses an endpoint that
    /// writes from a `request` not so marked. Every
    /// other kind reads an answer already held, or is a discoverer's safe
    /// request, and never saves.
    fn commits(&self, kind: &StepKind) -> StepCommits {
        if *kind == REQUEST {
            StepCommits::AsDeclared
        } else {
            StepCommits::Never
        }
    }

    /// A verb that reads is a read: the lowering binds it only to an
    /// endpoint marked `read`. `delete` and `submit` cannot be taken back,
    /// and are `irreversible`. Any other verb that writes is a write when
    /// the binding names how the record is read back, and `irreversible`
    /// when it names nothing: what was written could then never be found
    /// again.
    fn least_effect(&self, binding: &ResolvedBinding<'_>) -> EffectClass {
        let verb = binding.verb.as_str();
        if READ_VERBS.contains(&verb) {
            EffectClass::Read
        } else if IRREVERSIBLE_VERBS.contains(&verb) {
            EffectClass::Irreversible
        } else if binding.field("read_back").is_some() {
            EffectClass::Write
        } else {
            EffectClass::Irreversible
        }
    }

    fn binding_schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn identity(&self, binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible {
        match strategy {
            IdentityStrategy::None => Possible::Yes,
            IdentityStrategy::Natural | IdentityStrategy::Composite => {
                let reads = READ_VERBS.contains(&binding.verb.as_str());
                if reads || binding.field("read_back").is_some() {
                    Possible::Yes
                } else {
                    Possible::No(
                        "the binding names no `read_back` endpoint that reads the record by its key."
                            .to_owned(),
                    )
                }
            }
            IdentityStrategy::Stamped | IdentityStrategy::Find => Possible::No(
                "finding a record by a stamp or a search is not built in this version of the HTTP pack."
                    .to_owned(),
            ),
        }
    }

    fn lower(
        &self,
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
    ) -> Result<Lowered, Diagnostics> {
        let mut lower = Lower {
            cx,
            section: None,
            timeout_ms: cx
                .target()
                .default_timeout_ms("request")
                .unwrap_or(DEFAULT_TIMEOUT_MS),
            problems: Diagnostics::new(),
        };
        if let Some((index, section)) = cx.section(&ENDPOINT_KIND, &HTTP_SURFACE) {
            match section.payload.decode::<EndpointSection>() {
                Ok(decoded) => lower.section = Some((index, decoded)),
                Err(failure) => lower.refuse(
                    "not_encodable",
                    Stage::Lower,
                    &binding.doc.at,
                    failure.message().to_owned(),
                    "Report this to the HTTP pack's author.",
                ),
            }
        }
        let reads = READ_VERBS.contains(&binding.verb.as_str());
        let Some(found) = lower.endpoint(binding.doc) else {
            return Err(lower.problems);
        };
        if found.template.writes == reads {
            let (verb, effect) = (binding.verb, if reads { "write" } else { "read" });
            lower.refuse(
                "effect_mismatch",
                Stage::Analyze,
                found.at,
                format!(
                    "The verb `{verb}` is bound to the endpoint `{}`, which is marked `{effect}`.",
                    found.name
                ),
                "Bind a verb that reads to an endpoint marked `read`, and one that writes to an endpoint marked `write`.",
            );
        }

        // The endpoint a write's record is read back through. It also
        // supplies the version a write sends as its precondition.
        let read_back = binding.field("read_back").filter(|_| !reads);
        let reader = read_back.and_then(|part| lower.endpoint(part));
        let version_from = if reads {
            None
        } else {
            lower.version_reader(&found, reader.as_ref(), read_back.is_some())
        };
        let mut act = lower.request(&found, binding.doc, !reads, version_from);
        let mut lowered = Lowered {
            surface: SurfaceName::new(&found.template.surface).map_err(|_| {
                Diagnostics::from(problem(
                    "unknown_endpoint",
                    Stage::Resolve,
                    found.at,
                    "The endpoint's surface has no well-formed name.",
                    "Name a surface the target declares.",
                ))
            })?,
            act: Vec::new(),
            confirm: Vec::new(),
            read_back: Vec::new(),
            lookup: Vec::new(),
            settle_ms: 0,
            prefer_response: false,
        };
        if reads {
            // A read of one record may find none; a list or a search answers
            // with what there is.
            let lookup =
                binding.verb.as_str() == "read" && !found.template.answers.absent.is_empty();
            act.lookup = lookup;
            let mut validate = lower.validate(&found);
            validate.lookup = lookup;
            let mut extract = lower.extract(&found, binding.doc);
            extract.lookup = lookup;
            lowered.act = vec![act, validate, extract];
        } else {
            act.commits = true;
            act.idempotent = found.template.idempotency_header.is_some();
            lowered.act = vec![act];
            lowered.confirm = vec![lower.validate(&found)];
            if let Some(read_back) = read_back {
                lowered.prefer_response = read_back.text("prefer") == Some("response");
                if lowered.prefer_response {
                    let extract = lower.extract(&found, binding.doc);
                    lowered.confirm.push(extract);
                }
                if let Some(reader) = reader {
                    if reader.template.writes {
                        lower.refuse(
                            "effect_mismatch",
                            Stage::Analyze,
                            reader.at,
                            format!(
                                "The record is read back through `{}`, which is marked `write`.",
                                reader.name
                            ),
                            "Name an endpoint marked `read`.",
                        );
                    }
                    if reader.template.answers.absent.is_empty() {
                        lower.refuse(
                            "read_back_never_absent",
                            Stage::Analyze,
                            reader.at,
                            format!(
                                "The record is read back through `{}`, which declares no status for a record that does not exist.",
                                reader.name
                            ),
                            "Add `responses: {absent: [404]}` to the endpoint, with the status the system answers.",
                        );
                    }
                    let mut steps = vec![
                        lower.request(&reader, read_back, false, None),
                        lower.validate(&reader),
                        lower.extract(&reader, read_back),
                    ];
                    for step in &mut steps {
                        step.lookup = true;
                    }
                    lowered.read_back = steps;
                }
            }
        }
        if lower.problems.is_empty() {
            Ok(lowered)
        } else {
            Err(lower.problems)
        }
    }
}
