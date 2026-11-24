//! The `api` mode (decision 0389): a read bound to an operation a
//! published description states, by its `operationId` or by its method and
//! path, rather than to an endpoint entry a person wrote.
//!
//! **The pinned description.** The `descriptions/` file kind holds the
//! pack's reading of a description's reads, one document for each, with
//! the digest of the description's text as the import read it and the
//! path it was served from. Every document of one description states the
//! same digest; one that states another is `http.api_description_changed`.
//! The documents compile into the same request templates an endpoint
//! entry compiles into, so an `api` call is sent, checked, and read as an
//! `http` request is.
//!
//! **Drift is named.** The digest is in the compiled target, so a published
//! version holds it. Before the first call of a pass that a description
//! serves, the driver reads the description again where it was served and
//! compares digests; a description that changed, or is no longer served
//! there, stops the call as `drift`, `description_changed`, with nothing
//! sent. A description handed in as a file names no place, and is not read
//! again.
//!
//! Not built: writes through `api` (a write is an `http` binding over an
//! endpoint entry, as before), GraphQL and OData.

use std::collections::BTreeMap;

use a2m_core::{
    Digest, EffectClass, FailsAs, FileKindName, IdentityStrategy, Mode, PackPayload, SectionIndex,
    StepKind, SurfaceKind, SurfaceName, Verb,
};
use a2m_files::{
    BindingStatement, CompiledSection, DefinedName, Diagnostics, Doc, DraftContext, DraftDoc,
    DraftValue, DraftingRule, Entry, ExprSite, FileKind, JsonSchema, KindContext, Located,
    LowerContext, Lowered, Lowering, Place, Possible, RelPath, ResolvedBinding, Sends, Stage,
    Statement, StepChanges, StepCommits,
};
use serde::{Deserialize, Serialize};

use a2m_core::{Failure, Finding, FindingKind};

use crate::codes;
use crate::endpoints::{HTTP_SURFACE, problem, template_of};
use crate::lower::{CALL, DEFAULT_TIMEOUT_MS, EXTRACT, Lower, READ_VERBS, VALIDATE};
use crate::openapi::{self, DescribedOperation};
use crate::template::{EndpointSection, EndpointTemplate, ExtractStep, Segment, parse_path};

/// The name of the mode.
pub const API_MODE: &str = "api";
/// The file kind of a pinned description, in `descriptions/`.
pub const DESCRIPTION_KIND: FileKindName = FileKindName::fixed("description");
/// The format of the section of the compiled target the file kind compiles
/// into.
pub(crate) const DESCRIBED_FORMAT: u16 = 1;

/// The places a parameter of a described operation may be in.
const PLACES: &[&str] = &["path", "query", "header", "cookie"];

/// The digest an `api` binding is pinned to: of the description's text as
/// it was read, once redacted, with no byte-order mark and every line
/// ending a line feed. The driver digests what the service serves the same
/// way.
pub(crate) fn digest_of(text: &str) -> String {
    Digest::of(a2m_files::normalize_text(text).as_bytes()).to_string()
}

/// The name a described operation is known by within the file kind and in the
/// compiled target: the description's name, a `/`, and the name an endpoint
/// entry drafted from the operation would take, made from its `operationId`,
/// or from its method and path where that makes none. The part after the `/`
/// is what a fixture's file for the call is named. Two operations whose names
/// are one are refused by the compiler as a name defined twice.
fn entry_name(description: &str, operation_id: Option<&str>, method: &str, path: &str) -> String {
    let from_id = operation_id.map(openapi::snake).unwrap_or_default();
    let key: String = if from_id.starts_with(|c: char| c.is_ascii_lowercase()) {
        from_id.chars().take(60).collect()
    } else {
        openapi::snake(&format!("{method} {path}"))
            .chars()
            .take(60)
            .collect()
    };
    format!("{description}/{key}")
}

/// One parameter of a described operation, as a binding's `args` fill it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DescribedParameter {
    /// Its name, as the description writes it.
    pub name: String,
    /// `path`, `query`, `header`, or `cookie`.
    pub place: String,
    pub required: bool,
}

/// What a pinned description is pinned to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pinned {
    /// The surface it describes.
    pub surface: String,
    /// The digest of its text, as [`digest_of`] makes it.
    pub digest: String,
    /// The literal path, on the surface, it is served at; none for one
    /// handed in as a file.
    pub served_at: Option<String>,
}

/// One operation of a pinned description, compiled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Described {
    /// The description it belongs to.
    pub description: String,
    pub operation_id: Option<String>,
    /// The method and the path as the description writes them.
    pub method: String,
    pub path: String,
    pub parameters: Vec<DescribedParameter>,
    /// The request it is sent as, the same template an endpoint entry
    /// compiles into.
    pub template: EndpointTemplate,
}

/// The section of the compiled target the `descriptions/` file kind compiles
/// into.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DescribedSection {
    /// Each pinned description, by its name.
    pub pinned: BTreeMap<String, Pinned>,
    /// Each operation, by the name [`entry_name`] gives it.
    pub operations: BTreeMap<String, Described>,
}

impl DescribedSection {
    /// The operations' templates, by the same names: what a step names.
    pub(crate) fn templates(&self) -> EndpointSection {
        self.operations
            .iter()
            .map(|(name, described)| (name.clone(), described.template.clone()))
            .collect()
    }
}

fn schema() -> JsonSchema {
    let status = || JsonSchema::int_between(100, 599);
    JsonSchema::object()
        .required("description", JsonSchema::name())
        .required("surface", JsonSchema::surface())
        .required("digest", JsonSchema::text())
        .optional("served_at", JsonSchema::text())
        .optional("operation_id", JsonSchema::text())
        .required("method", JsonSchema::words(&["GET", "HEAD"]))
        .required("path", JsonSchema::text())
        .optional("auth", JsonSchema::words(&["session", "token", "none"]))
        .optional(
            "parameters",
            JsonSchema::list(
                JsonSchema::object()
                    .required("name", JsonSchema::text())
                    .required("in", JsonSchema::words(PLACES))
                    .optional("required", JsonSchema::bool()),
            ),
        )
        .required("response_schema", JsonSchema::any())
        .optional(
            "responses",
            JsonSchema::object()
                .optional("ok", JsonSchema::non_empty_list(status()))
                .optional("absent", JsonSchema::list(status())),
        )
}

/// The `descriptions/` file kind: a pinned description's reads.
pub(crate) struct DescriptionKind {
    schema: JsonSchema,
    kinds: Vec<SurfaceKind>,
}

impl DescriptionKind {
    pub(crate) fn new() -> Self {
        Self {
            schema: schema(),
            kinds: vec![HTTP_SURFACE],
        }
    }
}

fn name_of(doc: &Located<Doc>) -> String {
    entry_name(
        doc.text("description").unwrap_or_default(),
        doc.text("operation_id"),
        doc.text("method").unwrap_or_default(),
        doc.text("path").unwrap_or_default(),
    )
}

/// One entry of a map, at the place of the value it holds.
fn entry(key: &str, value: Located<Doc>) -> Entry {
    Entry {
        key: Located::new(key.to_owned(), value.at.clone()),
        value,
    }
}

/// The endpoint entry a described operation is compiled as: the pinned
/// document's own method, auth, shapes, and statuses, its path as a
/// template of this pack, and `effect: read`.
fn endpoint_doc(doc: &Located<Doc>, name: &str, template_path: &str) -> Located<Doc> {
    let path_at = doc.get("path").map_or(&doc.at, |path| &path.at);
    let mut entries = vec![
        entry(
            "endpoint",
            Located::new(Doc::Text(name.to_owned()), doc.at.clone()),
        ),
        entry(
            "path",
            Located::new(Doc::Text(template_path.to_owned()), path_at.clone()),
        ),
        entry(
            "effect",
            Located::new(Doc::Text("read".to_owned()), doc.at.clone()),
        ),
    ];
    for key in ["surface", "method", "auth", "response_schema", "responses"] {
        if let Some(value) = doc.get(key) {
            entries.push(entry(key, value.clone()));
        }
    }
    Located::new(Doc::Map(entries), doc.at.clone())
}

/// Whether a path is one this pack can read again: literal segments only.
fn is_literal_path(path: &str) -> bool {
    parse_path(path).is_ok_and(|(segments, _)| {
        segments
            .iter()
            .all(|segment| matches!(segment, Segment::Literal(_)))
    })
}

impl FileKind for DescriptionKind {
    fn name(&self) -> FileKindName {
        DESCRIPTION_KIND
    }

    fn surface_kinds(&self) -> &[SurfaceKind] {
        &self.kinds
    }

    fn folder(&self) -> &str {
        "descriptions"
    }

    fn schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn names(&self, doc: &Located<Doc>) -> Vec<Located<DefinedName>> {
        let at = doc.get("operation_id").map_or(&doc.at, |id| &id.at);
        vec![Located::new(DefinedName { name: name_of(doc) }, at.clone())]
    }

    fn expressions(&self, _doc: &Located<Doc>) -> Vec<ExprSite> {
        // A description holds shapes and no expression.
        Vec::new()
    }

    /// A pinned description holds reads only: the schema takes the methods
    /// HTTP marks safe, GET and HEAD, and every document compiles as an
    /// endpoint marked `read`.
    fn writes(&self, _doc: &Located<Doc>) -> bool {
        false
    }

    fn compile(
        &self,
        docs: &[Located<Doc>],
        cx: &KindContext<'_>,
    ) -> Result<CompiledSection, Diagnostics> {
        let mut section = DescribedSection::default();
        let mut problems = Diagnostics::new();
        for doc in docs {
            let Some(surface) = cx.surface_of(doc) else {
                continue;
            };
            let description = doc.text("description").unwrap_or_default();
            let digest_at = doc.get("digest").map_or(&doc.at, |digest| &digest.at);
            let digest = doc.text("digest").unwrap_or_default();
            if digest.parse::<Digest>().is_err() {
                problems.push(problem(
                    "api_description_invalid",
                    Stage::Lower,
                    digest_at,
                    "A pinned description's `digest` is 64 lowercase hexadecimal characters.",
                    "Import the description again, so that the digest is the one its text was read with.",
                ));
                continue;
            }
            let served_at = doc.text("served_at").map(str::to_owned);
            if let Some(path) = &served_at
                && !is_literal_path(path)
            {
                let at = doc.get("served_at").map_or(&doc.at, |path| &path.at);
                problems.push(problem(
                    "api_description_invalid",
                    Stage::Lower,
                    at,
                    format!(
                        "The path a pinned description is served at, `{}`, is not a literal path of the surface.",
                        codes::shown(path)
                    ),
                    "Write the path the description was fetched from, beginning with `/`, with no placeholder.",
                ));
                continue;
            }
            let pinned = Pinned {
                surface: surface.name.as_str().to_owned(),
                digest: digest.to_owned(),
                served_at,
            };
            match section.pinned.get(description) {
                Some(known) if *known != pinned => {
                    problems.push(problem(
                        "api_description_changed",
                        Stage::Lower,
                        digest_at,
                        format!(
                            "The documents of the pinned description `{description}` do not all state one digest, place, and surface: some were pinned from another text of the description."
                        ),
                        "Import the description again, so that every operation is pinned from one text.",
                    ));
                    continue;
                }
                Some(_) => {}
                None => {
                    section.pinned.insert(description.to_owned(), pinned);
                }
            }
            let method = doc.text("method").unwrap_or_default();
            let path = doc.text("path").unwrap_or_default();
            let name = name_of(doc);
            let Some(template_path) = openapi::template_path_of(path) else {
                let at = doc.get("path").map_or(&doc.at, |path| &path.at);
                problems.push(problem(
                    "path_invalid",
                    Stage::Lower,
                    at,
                    format!(
                        "The path `{}` is not one this pack can send: every placeholder must be a whole segment of a path that begins with `/`.",
                        codes::shown(path)
                    ),
                    "Leave the operation out of the pinned description, and bind it over an endpoint entry.",
                ));
                continue;
            };
            let endpoint = endpoint_doc(doc, &name, &template_path);
            let Some(template) = template_of(&endpoint, surface.name.as_str(), &mut problems)
            else {
                continue;
            };
            let parameters = doc
                .get("parameters")
                .map(Located::items)
                .unwrap_or_default()
                .iter()
                .map(|parameter| DescribedParameter {
                    name: parameter.text("name").unwrap_or_default().to_owned(),
                    place: parameter.text("in").unwrap_or_default().to_owned(),
                    required: parameter.get("required").map(|flag| &flag.value)
                        == Some(&Doc::Bool(true)),
                })
                .collect();
            section.operations.insert(
                name,
                Described {
                    description: description.to_owned(),
                    operation_id: doc.text("operation_id").map(str::to_owned),
                    method: method.to_owned(),
                    path: path.to_owned(),
                    parameters,
                    template,
                },
            );
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        PackPayload::encode(&section)
            .map(|payload| CompiledSection {
                format: DESCRIBED_FORMAT,
                payload,
            })
            .map_err(|failure| {
                Diagnostics::from(problem(
                    "not_encodable",
                    Stage::Lower,
                    &docs.first().map_or_else(
                        || Place::start_of(&RelPath::target_file()),
                        |doc| doc.at.clone(),
                    ),
                    failure.message().to_owned(),
                    "Keep every number in a pinned description whole.",
                ))
            })
    }
}

/// The pinned description an operation of a finding is drafted into, and
/// its name: only for a GET the read rule drafts a read for, read with a
/// digest, on the one surface that certainly takes it. That read binds the
/// operation in `api`, and the pinned document is the one place its
/// request is stated: no endpoint entry is drafted for it (rule eight).
/// Every other operation keeps its endpoint entry. The description is
/// named after the surface.
pub(crate) fn pinned_draft(
    finding: &Finding,
    operation: &DescribedOperation,
    cx: &DraftContext<'_>,
) -> Option<(String, DraftDoc)> {
    let digest = operation.digest.as_deref()?;
    let response = operation.response.as_ref()?;
    if cx.target().is_none() || !crate::draft::would_draft(operation) {
        return None;
    }
    let surface = openapi::certain_surface(finding, operation, cx)?;
    let mut doc = vec![
        ("description", DraftValue::text(surface)),
        ("surface", DraftValue::text(surface)),
        ("digest", DraftValue::text(digest)),
    ];
    if let Some(served_at) = &operation.served_at {
        doc.push(("served_at", DraftValue::text(served_at)));
    }
    if let Some(id) = &operation.operation_id {
        doc.push(("operation_id", DraftValue::text(id)));
    }
    doc.push(("method", DraftValue::text(&operation.method)));
    doc.push(("path", DraftValue::text(&operation.path)));
    if operation.open {
        doc.push(("auth", DraftValue::text("none")));
    }
    if !operation.parameters.is_empty() {
        doc.push((
            "parameters",
            DraftValue::List(
                operation
                    .parameters
                    .iter()
                    .filter(|parameter| PLACES.contains(&parameter.place.as_str()))
                    .map(|parameter| {
                        DraftValue::map(vec![
                            ("name", DraftValue::text(&parameter.name)),
                            ("in", DraftValue::text(&parameter.place)),
                            ("required", DraftValue::Bool(parameter.required)),
                        ])
                    })
                    .collect(),
            ),
        ));
    }
    doc.push(("response_schema", openapi::draft_value(response)));
    let statuses = |list: &[u16]| {
        DraftValue::List(
            list.iter()
                .map(|status| DraftValue::Int(i64::from(*status)))
                .collect(),
        )
    };
    let mut responses = Vec::new();
    if !operation.ok.is_empty() {
        responses.push(("ok", statuses(&operation.ok)));
    }
    if !operation.absent.is_empty() {
        responses.push(("absent", statuses(&operation.absent)));
    }
    if !responses.is_empty() {
        doc.push(("responses", DraftValue::map(responses)));
    }
    let path = RelPath::new(&format!("descriptions/{surface}.yaml")).ok()?;
    Some((
        surface.to_owned(),
        DraftDoc {
            path,
            body: DraftValue::map(doc),
        },
    ))
}

/// From a `described_operation` finding to its document of the pinned
/// description.
pub(crate) struct PinnedDescriptionRule {
    pub(crate) kind: FindingKind,
}

impl DraftingRule for PinnedDescriptionRule {
    fn finding_kind(&self) -> FindingKind {
        self.kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        DESCRIPTION_KIND
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let Some(operation) = DescribedOperation::from_found(finding.found()) else {
            return Err(a2m_core::codes::row::FINDING_INVALID.failure(
                "A `described_operation` finding is not in the form the HTTP pack writes.",
                "Import the description again with the version of the HTTP pack that is enabled.",
            ));
        };
        let pinned: Vec<DraftDoc> = pinned_draft(finding, &operation, cx)
            .map(|(_, doc)| doc)
            .into_iter()
            .collect();
        if pinned.is_empty() {
            cx.not_drafted(NOT_PINNED);
        }
        Ok(pinned)
    }
}

/// Why an operation is not pinned: only a drafted read binds `api`.
const NOT_PINNED: &str = "No read is drafted in the `api` mode for the operation, so its request is not pinned; where an entry is drafted for it, it is in `endpoints/`.";

/// Why an operation has no endpoint entry: its request is pinned.
pub(crate) const PINNED_ELSEWHERE: &str = "The operation's request is stated in its pinned description, which its drafted `api` read binds, so no endpoint entry states it again.";

/// The lowering of the `api` mode.
pub(crate) struct ApiLowering {
    mode: Mode,
    schema: JsonSchema,
    verbs: Vec<Verb>,
    step_kinds: Vec<StepKind>,
}

impl ApiLowering {
    pub(crate) fn new(mode: Mode) -> Self {
        Self {
            mode,
            schema: JsonSchema::object()
                .required("description", JsonSchema::name())
                .optional("operation_id", JsonSchema::text())
                .optional("method", JsonSchema::words(&["GET", "HEAD"]))
                .optional("path", JsonSchema::text())
                .optional("args", JsonSchema::map_of(JsonSchema::expression()))
                .optional("extract", JsonSchema::map_of(JsonSchema::text())),
            verbs: READ_VERBS.iter().map(|verb| Verb::fixed(verb)).collect(),
            step_kinds: vec![CALL, VALIDATE, EXTRACT],
        }
    }
}

/// What a binding's `args` and `extract` make of a described operation:
/// the part [`Lower`] reads, with the operation's entry as its `endpoint`,
/// each path parameter under `path_params` by its placeholder's name, and
/// each query parameter under `query` by its own.
fn part_for(
    binding: &Located<Doc>,
    name: &str,
    described: &Described,
    problems: &mut Diagnostics,
) -> Located<Doc> {
    let at = binding.get("operation_id").map_or(&binding.at, |id| &id.at);
    let mut path_params = Vec::new();
    let mut query = Vec::new();
    let args = binding.get("args");
    for arg in args.map(Located::entries).unwrap_or_default() {
        let parameter = described
            .parameters
            .iter()
            .find(|parameter| parameter.name == arg.key.value);
        match parameter.map(|parameter| parameter.place.as_str()) {
            Some("path") => path_params.push(Entry {
                key: Located::new(openapi::snake(&arg.key.value), arg.key.at.clone()),
                value: arg.value.clone(),
            }),
            Some("query") => query.push(arg.clone()),
            Some(place) => problems.push(problem(
                "api_arg_unknown",
                Stage::Resolve,
                &arg.key.at,
                format!(
                    "The parameter `{}` goes in the request's {place}, which the `api` mode does not fill yet.",
                    codes::shown(&arg.key.value)
                ),
                "Remove the entry; bind the operation over an endpoint entry if it needs the value.",
            )),
            None => problems.push(problem(
                "api_arg_unknown",
                Stage::Resolve,
                &arg.key.at,
                format!(
                    "The described operation `{name}` states no parameter named `{}`.",
                    codes::shown(&arg.key.value)
                ),
                "Name a parameter the pinned description states for the operation, or remove the entry.",
            )),
        }
    }
    for parameter in &described.parameters {
        let given = args.and_then(|args| args.get(&parameter.name)).is_some();
        if parameter.required && parameter.place != "path" && !given {
            problems.push(problem(
                "api_arg_missing",
                Stage::Resolve,
                at,
                format!(
                    "The described operation `{name}` requires the parameter `{}`, in the request's {}, and the binding gives it no value under `args`.",
                    codes::shown(&parameter.name),
                    parameter.place
                ),
                "Give the parameter a value under `args`.",
            ));
        }
    }
    let mut entries = vec![entry(
        "endpoint",
        Located::new(Doc::Text(name.to_owned()), at.clone()),
    )];
    if !path_params.is_empty() {
        entries.push(entry(
            "path_params",
            Located::new(Doc::Map(path_params), at.clone()),
        ));
    }
    if !query.is_empty() {
        entries.push(entry("query", Located::new(Doc::Map(query), at.clone())));
    }
    if let Some(extract) = binding.get("extract") {
        entries.push(entry("extract", extract.clone()));
    }
    Located::new(Doc::Map(entries), binding.at.clone())
}

/// The described operation a binding names, by its `operationId` or by
/// its method and path, in the description it names.
fn named<'s>(
    binding: &Located<Doc>,
    section: Option<&'s DescribedSection>,
    problems: &mut Diagnostics,
) -> Option<(&'s String, &'s Described)> {
    let description = binding.text("description").unwrap_or_default();
    let id = binding.text("operation_id");
    let method = binding.text("method");
    let path = binding.text("path");
    let at = binding
        .get("operation_id")
        .or_else(|| binding.get("path"))
        .map_or(&binding.at, |named| &named.at);
    let by_id = id.is_some() && method.is_none() && path.is_none();
    let by_place = id.is_none() && method.is_some() && path.is_some();
    if !by_id && !by_place {
        problems.push(problem(
            "api_operation_missing",
            Stage::Resolve,
            at,
            "An `api` binding names its operation by `operation_id`, or by `method` and `path`, and not by both.",
            "Name the operation by the `operationId` the description gives it, or, where it gives none, by its method and its path as the description writes it.",
        ));
        return None;
    }
    let pinned = section.is_some_and(|section| section.pinned.contains_key(description));
    let found = section.and_then(|section| {
        section.operations.iter().find(|(_, described)| {
            described.description == description
                && match id {
                    Some(id) => described.operation_id.as_deref() == Some(id),
                    None => {
                        Some(described.method.as_str()) == method
                            && Some(described.path.as_str()) == path
                    }
                }
        })
    });
    if found.is_none() {
        let message = if pinned {
            format!(
                "The pinned description `{description}` states no read {}.",
                id.map_or_else(
                    || format!(
                        "`{} {}`",
                        method.unwrap_or_default(),
                        codes::shown(path.unwrap_or_default())
                    ),
                    |id| format!("whose `operationId` is `{}`", codes::shown(id))
                )
            )
        } else {
            format!(
                "No file under `descriptions/` pins a description named `{}`.",
                codes::shown(description)
            )
        };
        problems.push(problem(
            "api_operation_missing",
            Stage::Resolve,
            at,
            message,
            "Import the description again, or name an operation the pinned description states.",
        ));
    }
    found
}

impl Lowering for ApiLowering {
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

    /// No step of the mode saves: a `call` sends only an operation of a
    /// pinned description, and a pinned description holds reads only.
    fn commits(&self, _kind: &StepKind) -> StepCommits {
        StepCommits::Never
    }

    fn changes(&self, _kind: &StepKind) -> StepChanges {
        StepChanges::Never
    }

    fn least_effect(&self, _binding: &ResolvedBinding<'_>) -> EffectClass {
        EffectClass::Read
    }

    fn absent_outputs(&self, _binding: &ResolvedBinding<'_>, lowered: &Lowered) -> Vec<String> {
        lowered
            .act
            .iter()
            .filter(|step| step.kind == EXTRACT)
            .filter_map(|step| step.payload.decode::<ExtractStep>().ok())
            .flat_map(|payload| payload.values)
            .filter(|value| value.may_be_absent)
            .map(|value| value.name)
            .collect()
    }

    fn binding_schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn identity(&self, _binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible {
        match strategy {
            IdentityStrategy::None | IdentityStrategy::Natural | IdentityStrategy::Composite => {
                Possible::Yes
            }
            IdentityStrategy::Stamped | IdentityStrategy::Find => Possible::No(
                "finding a record by a stamp or a search is not built in this version of the HTTP pack."
                    .to_owned(),
            ),
        }
    }

    /// The same as an `http` binding's: it sends to its surface's address
    /// and to no other, the description's place included.
    fn statement(
        &self,
        _binding: &ResolvedBinding<'_>,
        _lowered: &Lowered,
        _cx: &LowerContext<'_>,
    ) -> Statement {
        Statement::Stated(BindingStatement {
            sends: Sends::Only {
                surface: true,
                addresses: Vec::new(),
            },
            ..BindingStatement::default()
        })
    }

    fn lower(
        &self,
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
    ) -> Result<Lowered, Diagnostics> {
        let mut problems = Diagnostics::new();
        let mut section: Option<(SectionIndex, DescribedSection)> = None;
        if let Some((index, compiled)) = cx.section(&DESCRIPTION_KIND, &HTTP_SURFACE) {
            match compiled.payload.decode::<DescribedSection>() {
                Ok(decoded) => section = Some((index, decoded)),
                Err(failure) => problems.push(problem(
                    "not_encodable",
                    Stage::Lower,
                    &binding.doc.at,
                    failure.message().to_owned(),
                    "Report this to the HTTP pack's author.",
                )),
            }
        }
        let Some((name, described)) = named(
            binding.doc,
            section.as_ref().map(|(_, section)| section),
            &mut problems,
        ) else {
            return Err(problems);
        };
        let part = part_for(binding.doc, name, described, &mut problems);
        let Some((index, section)) = &section else {
            return Err(problems);
        };
        let mut lower = Lower {
            cx,
            section: Some((*index, section.templates())),
            timeout_ms: cx
                .target()
                .default_timeout_ms("request")
                .unwrap_or(DEFAULT_TIMEOUT_MS),
            problems,
        };
        let Some(found) = lower.endpoint(&part) else {
            return Err(lower.problems);
        };
        let mut call = lower.request(&found, &part, false, None);
        call.kind = CALL;
        call.fails_as = FailsAs::from(&codes::row::UNEXPECTED_STATUS);
        let lookup = binding.verb.as_str() == "read" && !found.template.answers.absent.is_empty();
        call.lookup = lookup;
        let mut validate = lower.validate(&found);
        validate.lookup = lookup;
        let mut extract = lower.extract(&found, &part);
        extract.lookup = lookup;
        let surface = SurfaceName::new(&found.template.surface);
        if !lower.problems.is_empty() {
            return Err(lower.problems);
        }
        Ok(Lowered {
            surface: surface.map_err(|_| {
                Diagnostics::from(problem(
                    "unknown_endpoint",
                    Stage::Resolve,
                    &binding.doc.at,
                    "The pinned description's surface has no well-formed name.",
                    "Name a surface the target declares.",
                ))
            })?,
            act: vec![call, validate, extract],
            confirm: Vec::new(),
            read_back: Vec::new(),
            lookup: Vec::new(),
            settle_ms: 0,
            prefer_response: false,
        })
    }
}

/// `http.endpoint_stated_twice`: an endpoint entry and the pinned
/// description an `api` binding names state one operation, on one surface,
/// by one method and path. Rule eight keeps an operation's request in one
/// place of the endpoint catalog: an endpoint entry, or a pinned
/// description's document (decision 0389).
pub(crate) struct StatedTwice;

/// What states one request: the surface, the method, and the path as a
/// template of this pack.
fn request_of(doc: &Located<Doc>, path: Option<String>) -> Option<(String, String, String)> {
    Some((
        doc.text("surface")?.to_owned(),
        doc.text("method")?.to_ascii_uppercase(),
        path?,
    ))
}

impl a2m_files::LintRule for StatedTwice {
    fn code(&self) -> a2m_core::LintCode {
        crate::endpoints::code("endpoint_stated_twice")
    }

    fn level(&self, _mode: a2m_files::CompileMode) -> a2m_files::Level {
        a2m_files::Level::Error
    }

    fn summary(&self) -> &str {
        "No endpoint entry states the operation an `api` binding binds through its pinned description."
    }

    fn check(&self, view: &a2m_files::ResolvedView<'_>) -> Vec<a2m_files::Diagnostic> {
        let pinned = view.entries(&DESCRIPTION_KIND, &HTTP_SURFACE);
        let mut bound = Vec::new();
        for operation in view.operations() {
            let bindings = operation.doc().get("bindings");
            for binding in bindings.map(Located::items).unwrap_or_default() {
                if binding.text("mode") != Some(API_MODE) {
                    continue;
                }
                let description = binding.text("description");
                let id = binding.text("operation_id");
                let found = pinned.iter().find(|doc| {
                    doc.text("description") == description
                        && match id {
                            Some(id) => doc.text("operation_id") == Some(id),
                            None => {
                                doc.text("method") == binding.text("method")
                                    && doc.text("path") == binding.text("path")
                            }
                        }
                });
                if let Some(request) = found.and_then(|doc| {
                    request_of(doc, doc.text("path").and_then(openapi::template_path_of))
                }) {
                    bound.push(request);
                }
            }
        }
        view.entries(&crate::endpoints::ENDPOINT_KIND, &HTTP_SURFACE)
            .iter()
            .filter(|entry| {
                request_of(entry, entry.text("path").map(str::to_owned))
                    .is_some_and(|request| bound.contains(&request))
            })
            .map(|entry| {
                let at = entry.get("path").map_or(&entry.at, |path| &path.at);
                problem(
                    "endpoint_stated_twice",
                    Stage::Lint,
                    at,
                    "This endpoint entry states the request of an operation that an `api` binding binds through its pinned description: the operation would be stated in two places.",
                    "Remove the endpoint entry and keep the `api` binding, or bind the operation in `http` over this entry and remove the `api` binding.",
                )
            })
            .collect()
    }
}
