//! The `page_requests/` file kind for surfaces of the kind `web`: the
//! requests a page makes from inside itself, which the `page_http` mode
//! makes again with the page's own session (decisions 0190 and 0206).
//!
//! An entry is written in the form of an entry of the endpoint catalog,
//! and holds a part of it: the name under `endpoint`, the surface, the
//! method, the path, the effect, the answers under `responses`, and the
//! shape of the answer under `response_schema`, read by the provider
//! contract's shared shape (`a2m_provider::shape`, decision 0394). A page
//! request takes, in this version, a part of that shape: an object whose
//! `properties` each state only a `type`, other than `any`, with `required`
//! and `additionalProperties`; any other word the shared shape takes is
//! refused here, where it is written.
//! It adds one field the catalog has no word for: `page`, the path of the
//! page that makes the request, whose session it uses.
//!
//! A read is `method: GET`, `effect: read`, and may declare the statuses
//! of a record that does not exist under `responses.absent`. A write is
//! `method: PUT`, `effect: write`: its body's fields and their kinds under
//! `request_schema`, how the body is written under `request_encoding`
//! (`json`, or `form`), the statuses that mean it was written under
//! `responses.ok` and the application's refusals under `responses.refused`,
//! and where the page's anti-forgery token is read and sent under
//! `anti_forgery`, or `anti_forgery: none` for an endpoint that takes none.
//! `POST`, `PATCH`, and `DELETE` are written in the same words and are not
//! built: such an entry does not compile. A write entry is written by a
//! person; no drafting rule writes one.
//!
//! The entries live here, in a kind this pack owns, because the drafter
//! lets a pack draft only the kinds it owns, and the catalog is the HTTP
//! pack's.

use std::collections::BTreeMap;

use a2m_core::{FileKindName, PackPayload, SurfaceKind};
use a2m_files::{
    CompiledSection, DefinedName, Diagnostics, Doc, ExprSite, FileKind, JsonSchema, KindContext,
    Located, Place, Stage,
};
pub(crate) use a2m_provider::shape::Kind;
use a2m_provider::shape::Shape;
use serde::{Deserialize, Serialize};

use crate::codes::shown;
use crate::model::{PAYLOAD_FORMAT, Segment};
use crate::screens::{WEB_SURFACE, parse_reach, problem};

/// The name of the file kind.
pub const REQUEST_KIND: FileKindName = FileKindName::fixed("page_request");

/// One answer an entry declares: a status, and the top-level fields of its
/// JSON body that must hold exactly these texts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Answer {
    pub status: u16,
    #[serde(default)]
    pub when: BTreeMap<String, String>,
}

impl Answer {
    /// Whether an answer of `status`, whose JSON body is `body`, is this
    /// one.
    pub(crate) fn matches(&self, status: u16, body: Option<&serde_json::Value>) -> bool {
        status == self.status
            && self.when.iter().all(|(field, text)| {
                body.and_then(|body| body.get(field))
                    .and_then(serde_json::Value::as_str)
                    == Some(text.as_str())
            })
    }
}

/// Where the page holds its anti-forgery token, and where it is sent: each
/// a kind and a name, as the entry writes them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Token {
    /// `meta`, `input`, or `cookie`.
    pub from: String,
    pub from_name: String,
    /// `header`, or `field`.
    pub to: String,
    pub to_name: String,
    /// The answers by which the application refuses the token.
    pub refused: Vec<Answer>,
}

/// What a write entry holds beyond a read's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WriteForm {
    /// The method: `PUT` in this version.
    pub method: String,
    /// `json` or `form`.
    pub encoding: String,
    /// The body's fields and their kinds.
    pub fields: BTreeMap<String, Kind>,
    /// The fields the body must hold.
    pub required: Vec<String>,
    /// The page's anti-forgery token; `None` for `anti_forgery: none`.
    pub token: Option<Token>,
    /// The application's refusals of the values.
    pub refused: Vec<Answer>,
}

/// One compiled entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PageRequest {
    /// The surface it is made on.
    pub surface: String,
    /// The path of the request, segment by segment.
    pub path: Vec<Segment>,
    /// The path of the page that makes it.
    pub page: Vec<Segment>,
    /// The values the answer's object declares, by name.
    pub properties: BTreeMap<String, Kind>,
    /// The names it must hold.
    pub required: Vec<String>,
    /// Whether the object may hold only the names declared. The catalog's
    /// rule: an object holds other names unless `additionalProperties` is
    /// `false`.
    pub closed: bool,
    /// The statuses of an answer that is the record; 200 when empty.
    #[serde(default)]
    pub ok: Vec<u16>,
    /// The statuses that mean the record does not exist, on a lookup.
    #[serde(default)]
    pub absent: Vec<u16>,
    /// What a write sends; `None` for a read.
    #[serde(default)]
    pub write: Option<WriteForm>,
}

impl PageRequest {
    /// The shape of the answer, as the shared shape checks it: an object
    /// whose declared values are each of one kind.
    pub(crate) fn answer(&self) -> Shape {
        let mut shape = Shape::of(Kind::Object);
        shape.properties = self
            .properties
            .iter()
            .map(|(name, kind)| (name.clone(), Shape::of(*kind)))
            .collect();
        shape.required.clone_from(&self.required);
        shape.closed = self.closed;
        shape
    }

    /// Whether an answer's status is one that is the record.
    pub(crate) fn is_ok(&self, status: u16) -> bool {
        if self.ok.is_empty() {
            status == 200
        } else {
            self.ok.contains(&status)
        }
    }
}

/// Every entry of the target's web surfaces, by name.
pub(crate) type RequestSection = BTreeMap<String, PageRequest>;

/// The pattern of a path, written back with its placeholders.
pub(crate) fn pattern(path: &[Segment]) -> String {
    let mut out = String::new();
    for segment in path {
        out.push('/');
        match segment {
            Segment::Literal(text) => out.push_str(text),
            Segment::Hole(name) => {
                out.push('{');
                out.push_str(name);
                out.push('}');
            }
        }
    }
    if out.is_empty() {
        out.push('/');
    }
    out
}

/// The words of the methods an entry may be written with. Only `GET` and
/// `PUT` are built.
const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE"];

fn answer_schema() -> JsonSchema {
    JsonSchema::object()
        .required("status", JsonSchema::int_between(100, 599))
        .optional("when", JsonSchema::map_of(JsonSchema::text()))
}

fn schema() -> JsonSchema {
    let place = |words: &[&str]| {
        let mut either = Vec::new();
        for word in words {
            either.push(JsonSchema::object().required(word, JsonSchema::text()));
        }
        JsonSchema::either(either)
    };
    let token = JsonSchema::object()
        .required("from", place(&["meta", "input", "cookie"]))
        .optional("header", JsonSchema::text())
        .optional("field", JsonSchema::text())
        .optional("refused", JsonSchema::list(answer_schema()));
    let statuses = JsonSchema::list(JsonSchema::int_between(100, 599));
    JsonSchema::object()
        .required("endpoint", JsonSchema::name())
        .optional("surface", JsonSchema::surface())
        .required("method", JsonSchema::words(METHODS))
        .required("path", JsonSchema::text())
        .required("page", JsonSchema::text())
        .required("effect", JsonSchema::words(&["read", "write"]))
        .optional(
            "anti_forgery",
            JsonSchema::either(vec![JsonSchema::words(&["none"]), token]),
        )
        .optional("request_schema", JsonSchema::any())
        .optional("request_encoding", JsonSchema::words(&["json", "form"]))
        .optional(
            "responses",
            JsonSchema::object()
                .optional("ok", statuses.clone())
                .optional("absent", statuses)
                .optional("refused", JsonSchema::list(answer_schema())),
        )
        .required("response_schema", JsonSchema::any())
}

/// The file kind.
pub(crate) struct RequestKind {
    kinds: Vec<SurfaceKind>,
    schema: JsonSchema,
}

impl RequestKind {
    pub(crate) fn new() -> Self {
        Self {
            kinds: vec![WEB_SURFACE],
            schema: schema(),
        }
    }
}

/// The words a page request's shape takes at its top, and in each of its
/// properties, in this version.
const TOP_WORDS: &[&str] = &["type", "properties", "required", "additionalProperties"];

/// A shape, as `response_schema` and `request_schema` write it: read by the
/// shared shape, then held to the part of it a page request takes. Each
/// problem is pushed at its place.
fn shape_of(written: &Located<Doc>, problems: &mut Diagnostics) -> Option<Shape> {
    let mut refuse = |at: &Place, message: String, fix: &str| {
        problems.push(problem(
            "request_shape_invalid",
            Stage::Resolve,
            at,
            message,
            fix,
        ));
    };
    let shape = match Shape::from_doc(written) {
        Ok(shape) => shape,
        Err(why) => {
            refuse(
                &why.at,
                why.message,
                "Write the shape with `type: object`, `properties` that each state a `type`, `required`, and `additionalProperties` only.",
            );
            return None;
        }
    };
    let mut usable = true;
    if shape.kind != Kind::Object || written.get("properties").is_none() {
        refuse(
            &written.at,
            "A page request's shape is an object that lists its `properties`.".to_owned(),
            "Write `type: object` and `properties:` with each value the body or the answer holds.",
        );
        usable = false;
    }
    for entry in written.entries() {
        if !TOP_WORDS.contains(&entry.key.value.as_str()) {
            refuse(
                &entry.key.at,
                format!(
                    "A page request's shape does not take `{}` in this version.",
                    shown(&entry.key.value)
                ),
                "Remove it.",
            );
            usable = false;
        }
    }
    for property in written
        .get("properties")
        .map(Located::entries)
        .unwrap_or_default()
    {
        let flat = property
            .value
            .entries()
            .iter()
            .all(|word| word.key.value == "type");
        if !flat || property.value.text("type") == Some("any") {
            refuse(
                &property.value.at,
                format!(
                    "The value `{}` of a page request's shape states only its `type`, which is not `any`, in this version.",
                    shown(&property.key.value)
                ),
                "Write the value as `{type: string}`, or as another kind other than `any`.",
            );
            usable = false;
        }
    }
    usable.then_some(shape)
}

fn statuses(doc: Option<&Located<Doc>>) -> Vec<u16> {
    doc.map(Located::items)
        .unwrap_or_default()
        .iter()
        .filter_map(|item| item.as_int().and_then(|status| u16::try_from(status).ok()))
        .collect()
}

fn answers(doc: Option<&Located<Doc>>) -> Vec<Answer> {
    doc.map(Located::items)
        .unwrap_or_default()
        .iter()
        .filter_map(|item| {
            let status = u16::try_from(item.get("status")?.as_int()?).ok()?;
            let when = item
                .get("when")
                .map(Located::entries)
                .unwrap_or_default()
                .iter()
                .filter_map(|entry| {
                    entry
                        .value
                        .as_text()
                        .map(|text| (entry.key.value.clone(), text.to_owned()))
                })
                .collect();
            Some(Answer { status, when })
        })
        .collect()
}

/// Whether a header may carry a page's token: a plain name the browser
/// engine would send, and none the browser or the connection sets.
fn header_may_carry(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    a2m_pack_web_browser::plain_write_name(name)
        && !a2m_pack_web_browser::REFUSED_HEADERS.contains(&lower.as_str())
        && !lower.starts_with("sec-")
        && !lower.starts_with("proxy-")
}

/// What a write entry sends, checked; `None` with a problem for each part
/// that cannot be sent.
fn write_form(
    doc: &Located<Doc>,
    method: &str,
    ok: &[u16],
    problems: &mut Diagnostics,
) -> Option<WriteForm> {
    let at = &doc.at;
    let mut refuse = |rule: &'static str, at: &Place, message: String, fix: &str| {
        problems.push(problem(rule, Stage::Analyze, at, message, fix));
    };
    if method != "PUT" {
        refuse(
            "request_method_not_built",
            at,
            format!(
                "A write from inside the page with `{method}` is not built in this version: only `PUT`, which writes the whole record at its own path, so that the same write sent twice leaves the record as once (decision 0206)."
            ),
            "Write the entry with `method: PUT`, or bind the write in the `ui` mode.",
        );
        return None;
    }
    let Some(request) = doc.get("request_schema") else {
        refuse(
            "body_undeclared",
            at,
            "A write entry declares its body's fields under `request_schema`.".to_owned(),
            "Add `request_schema:` with each field the write sends and its type.",
        );
        return None;
    };
    let shape = shape_of(request, problems)?;
    let mut refuse = |rule: &'static str, at: &Place, message: String, fix: &str| {
        problems.push(problem(rule, Stage::Analyze, at, message, fix));
    };
    let mut usable = true;
    let fields: BTreeMap<String, Kind> = shape
        .properties
        .iter()
        .map(|(name, value)| (name.clone(), value.kind))
        .collect();
    for (name, kind) in &fields {
        if !a2m_pack_web_browser::plain_write_name(name) {
            refuse(
                "request_shape_invalid",
                &request.at,
                format!(
                    "The body's field `{}` has a name a write does not send: a letter, then letters, digits, `-`, and `_`.",
                    shown(name)
                ),
                "Rename the field as the application names it, if it is such a name.",
            );
            usable = false;
        }
        if !matches!(kind, Kind::String | Kind::Integer | Kind::Boolean) {
            refuse(
                "request_shape_invalid",
                &request.at,
                format!(
                    "The body's field `{}` is a `{}`; a write from inside the page sends a string, an integer, or a boolean, never a number with a fraction or a nested value.",
                    shown(name),
                    kind.word()
                ),
                "Declare the field as one of those kinds, or bind the write in another mode.",
            );
            usable = false;
        }
    }
    if ok.is_empty() {
        refuse(
            "responses_undeclared",
            at,
            "A write entry declares under `responses.ok` the statuses that mean it was written."
                .to_owned(),
            "Add `responses: {ok: [200]}`, with the status the application answers a write with.",
        );
        usable = false;
    }
    let refused = answers(doc.get("responses").and_then(|r| r.get("refused")));
    let token = match doc.get("anti_forgery") {
        None => {
            refuse(
                "anti_forgery_undeclared",
                at,
                "A write entry says where the page holds its anti-forgery token, under `anti_forgery`, or that the endpoint takes none, with `anti_forgery: none`.".to_owned(),
                "Add `anti_forgery: {from: {meta: ...}, header: ...}`, or `anti_forgery: none` if the application checks no token.",
            );
            return None;
        }
        Some(declared) if declared.as_text() == Some("none") => None,
        Some(declared) => {
            let from = declared.get("from")?;
            let (from_kind, from_name) = from.entries().first().and_then(|entry| {
                entry
                    .value
                    .as_text()
                    .map(|name| (entry.key.value.clone(), name.to_owned()))
            })?;
            let to = match (declared.text("header"), declared.text("field")) {
                (Some(header), None) if header_may_carry(header) => {
                    Some(("header".to_owned(), header.to_owned()))
                }
                (None, Some(field))
                    if a2m_pack_web_browser::plain_write_name(field)
                        && !fields.contains_key(field) =>
                {
                    Some(("field".to_owned(), field.to_owned()))
                }
                _ => None,
            };
            let Some((to_kind, to_name)) = to else {
                refuse(
                    "anti_forgery_invalid",
                    &declared.at,
                    "The token goes in exactly one `header` or one `field`: a plain name, not a header the browser or the connection sets, and not a field of the body.".to_owned(),
                    "Name the one header, or the one field, the application reads the token from.",
                );
                return None;
            };
            if !a2m_pack_web_browser::plain_write_name(&from_name) {
                refuse(
                    "anti_forgery_invalid",
                    &from.at,
                    "Where the page holds the token is named by a letter, then letters, digits, `-`, and `_`.".to_owned(),
                    "Name the meta element, the hidden input, or the cookie as the page does.",
                );
                return None;
            }
            Some(Token {
                from: from_kind,
                from_name,
                to: to_kind,
                to_name,
                refused: answers(declared.get("refused")),
            })
        }
    };
    let token_refusals = token.iter().flat_map(|token| &token.refused);
    if refused
        .iter()
        .chain(token_refusals)
        .any(|answer| ok.contains(&answer.status))
    {
        refuse(
            "responses_overlap",
            at,
            "A status is declared both as written and as refused, so an answer of it says nothing."
                .to_owned(),
            "Declare each status once.",
        );
        usable = false;
    }
    usable.then(|| WriteForm {
        method: method.to_owned(),
        encoding: doc.text("request_encoding").unwrap_or("json").to_owned(),
        fields,
        required: shape.required,
        token,
        refused,
    })
}

fn entry(doc: &Located<Doc>, surface: &str, problems: &mut Diagnostics) -> Option<PageRequest> {
    let mut path_of = |key: &str| {
        let written = doc.get(key)?;
        let text = written.as_text()?;
        if a2m_pack_web_browser::looks_like_an_action(text) {
            problems.push(problem(
                "request_path_invalid",
                Stage::Resolve,
                &written.at,
                format!(
                    "The `{key}` of a page request holds a word that names an action, and no request from inside the page goes to an address that may act on a visit (decision 0195)."
                ),
                "Make the request at an address that names the record, not an action.",
            ));
            return None;
        }
        match parse_reach(text) {
            Ok(path) => Some(path),
            Err(why) => {
                problems.push(problem(
                    "request_path_invalid",
                    Stage::Resolve,
                    &written.at,
                    format!("The `{key}` of a page request cannot be used: {why}."),
                    "Write a path that starts with `/`, each segment plain text or one whole placeholder such as `{order_id}`.",
                ));
                None
            }
        }
    };
    let path = path_of("path");
    let page = path_of("page");
    let method = doc.text("method").unwrap_or_default();
    let writes = doc.text("effect") == Some("write");
    let responses = doc.get("responses");
    let ok = statuses(responses.and_then(|r| r.get("ok")));
    let absent = statuses(responses.and_then(|r| r.get("absent")));
    // A read is a GET, and a GET only reads.
    if writes == (method == "GET") {
        problems.push(problem(
            "request_effect_mismatch",
            Stage::Analyze,
            &doc.at,
            format!(
                "A page request with `{}` is marked `effect: {}`: a GET only reads, and every other method writes.",
                shown(method),
                if writes { "write" } else { "read" }
            ),
            "Write a read with `method: GET` and `effect: read`, and a write with `method: PUT` and `effect: write`.",
        ));
        return None;
    }
    let write = if writes {
        Some(write_form(doc, method, &ok, problems))
    } else {
        for key in ["anti_forgery", "request_schema", "request_encoding"] {
            if let Some(stray) = doc.get(key) {
                problems.push(problem(
                    "read_sends",
                    Stage::Analyze,
                    &stray.at,
                    format!("A read sends no body and no token, and the entry states `{key}`."),
                    "Remove it, or write the entry as a write.",
                ));
            }
        }
        if let Some(stray) = responses.and_then(|r| r.get("refused")) {
            problems.push(problem(
                "read_sends",
                Stage::Analyze,
                &stray.at,
                "A read is refused by no answer of its own: `responses.refused` belongs to a write.",
                "Remove it.",
            ));
        }
        None
    };
    let answer = shape_of(doc.get("response_schema")?, problems)?;
    Some(PageRequest {
        surface: surface.to_owned(),
        path: path?,
        page: page?,
        properties: answer
            .properties
            .iter()
            .map(|(name, value)| (name.clone(), value.kind))
            .collect(),
        required: answer.required,
        closed: answer.closed,
        ok,
        absent,
        write: match write {
            Some(form) => Some(form?),
            None => None,
        },
    })
}

impl FileKind for RequestKind {
    fn name(&self) -> FileKindName {
        REQUEST_KIND
    }

    fn surface_kinds(&self) -> &[SurfaceKind] {
        &self.kinds
    }

    fn folder(&self) -> &str {
        "page_requests"
    }

    fn schema(&self) -> &JsonSchema {
        &self.schema
    }

    /// An entry describes a write unless it is plainly a read: `GET` and
    /// `effect: read`. A drafting rule may draft only one that is (decision
    /// 0206): a write entry is written by a person.
    fn writes(&self, doc: &Located<Doc>) -> bool {
        !(doc.text("method") == Some("GET") && doc.text("effect") == Some("read"))
    }

    fn names(&self, doc: &Located<Doc>) -> Vec<Located<DefinedName>> {
        doc.get("endpoint")
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

    fn expressions(&self, _doc: &Located<Doc>) -> Vec<ExprSite> {
        Vec::new()
    }

    fn compile(
        &self,
        docs: &[Located<Doc>],
        cx: &KindContext<'_>,
    ) -> Result<CompiledSection, Diagnostics> {
        let mut section = RequestSection::new();
        let mut problems = Diagnostics::new();
        for doc in docs {
            let name = doc.text("endpoint").unwrap_or_default();
            let Some(surface) = cx.surface_of(doc) else {
                continue;
            };
            if let Some(entry) = entry(doc, surface.name.as_str(), &mut problems) {
                section.insert(name.to_owned(), entry);
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        PackPayload::encode(&section)
            .map(|payload| CompiledSection {
                format: PAYLOAD_FORMAT,
                payload,
            })
            .map_err(|failure| {
                Diagnostics::from(problem(
                    "not_encodable",
                    Stage::Lower,
                    &docs.first().map_or_else(
                        || Place::start_of(&a2m_files::RelPath::target_file()),
                        |doc| doc.at.clone(),
                    ),
                    failure.message().to_owned(),
                    "Report this to the web pack's author.",
                ))
            })
    }
}
