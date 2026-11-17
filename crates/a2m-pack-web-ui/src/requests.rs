//! The `page_requests/` file kind for surfaces of the kind `web`: the reads
//! a page makes from inside itself, which the `page_http` mode makes again
//! with the page's own session (decision 0190).
//!
//! An entry is written in the form of an entry of the endpoint catalog,
//! and holds a part of it: the name under `endpoint`, the surface, the
//! method, which is `GET`, the path, `effect: read`, and the shape of the
//! answer under `response_schema`, in the same closed words. It adds one
//! field the catalog has no word for: `page`, the path of the page that
//! makes the request, whose session the read uses. The entries live here,
//! in a kind this pack owns, because the drafter lets a pack draft only
//! the kinds it owns, and the catalog is the HTTP pack's.

use std::collections::BTreeMap;

use a2m_core::{FileKindName, PackPayload, SurfaceKind};
use a2m_files::{
    CompiledSection, DefinedName, Diagnostics, Doc, ExprSite, FileKind, JsonSchema, KindContext,
    Located, Place, Stage,
};
use serde::{Deserialize, Serialize};

use crate::codes::shown;
use crate::model::{PAYLOAD_FORMAT, Segment};
use crate::screens::{WEB_SURFACE, parse_reach, problem};

/// The name of the file kind.
pub const REQUEST_KIND: FileKindName = FileKindName::fixed("page_request");

/// The kinds a value of an answer may be declared as, in the words of the
/// endpoint catalog's shapes.
const KINDS: &[&str] = &[
    "string", "integer", "number", "boolean", "null", "object", "array",
];

/// What one declared value of an answer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    String,
    Integer,
    Number,
    Boolean,
    Null,
    Object,
    Array,
}

impl Kind {
    fn from_word(word: &str) -> Option<Self> {
        Some(match word {
            "string" => Self::String,
            "integer" => Self::Integer,
            "number" => Self::Number,
            "boolean" => Self::Boolean,
            "null" => Self::Null,
            "object" => Self::Object,
            "array" => Self::Array,
            _ => return None,
        })
    }

    pub(crate) const fn word(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Null => "null",
            Self::Object => "object",
            Self::Array => "array",
        }
    }

    /// Whether a JSON value is of this kind. An integer is a number with
    /// no fraction that fits in 64 bits.
    pub(crate) fn fits(self, value: &serde_json::Value) -> bool {
        use serde_json::Value as Json;
        match (self, value) {
            (Self::String, Json::String(_))
            | (Self::Boolean, Json::Bool(_))
            | (Self::Null, Json::Null)
            | (Self::Object, Json::Object(_))
            | (Self::Array, Json::Array(_))
            | (Self::Number, Json::Number(_)) => true,
            (Self::Integer, Json::Number(number)) => number.is_i64(),
            _ => false,
        }
    }
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

fn schema() -> JsonSchema {
    let value = JsonSchema::object().required("type", JsonSchema::words(KINDS));
    let shape = JsonSchema::object()
        .required("type", JsonSchema::words(&["object"]))
        .required("properties", JsonSchema::map_of(value))
        .optional("required", JsonSchema::list(JsonSchema::text()))
        .optional("additionalProperties", JsonSchema::bool());
    JsonSchema::object()
        .required("endpoint", JsonSchema::name())
        .optional("surface", JsonSchema::surface())
        .required("method", JsonSchema::words(&["GET"]))
        .required("path", JsonSchema::text())
        .required("page", JsonSchema::text())
        .required("effect", JsonSchema::words(&["read"]))
        .required("response_schema", shape)
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

fn entry(doc: &Located<Doc>, surface: &str, problems: &mut Diagnostics) -> Option<PageRequest> {
    let mut path_of = |key: &str| {
        let written = doc.get(key)?;
        match parse_reach(written.as_text()?) {
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
    let shape = doc.get("response_schema")?;
    let mut properties = BTreeMap::new();
    for property in shape
        .get("properties")
        .map(Located::entries)
        .unwrap_or_default()
    {
        let kind = property.value.text("type").and_then(Kind::from_word)?;
        properties.insert(property.key.value.clone(), kind);
    }
    let mut required = Vec::new();
    for item in shape
        .get("required")
        .map(Located::items)
        .unwrap_or_default()
    {
        let name = item.as_text()?;
        if !properties.contains_key(name) {
            problems.push(problem(
                "request_shape_invalid",
                Stage::Resolve,
                &item.at,
                format!(
                    "The answer's shape requires `{}`, which it does not list under `properties`.",
                    shown(name)
                ),
                "List it under `properties`, or take it out of `required`.",
            ));
        }
        required.push(name.to_owned());
    }
    let closed = shape
        .get("additionalProperties")
        .and_then(Located::as_bool)
        .is_some_and(|open| !open);
    Some(PageRequest {
        surface: surface.to_owned(),
        path: path?,
        page: page?,
        properties,
        required,
        closed,
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

    /// An entry is a read: its schema takes `GET` and `effect: read` only.
    fn writes(&self, _doc: &Located<Doc>) -> bool {
        false
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
