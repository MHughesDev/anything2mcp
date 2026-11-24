//! Reading a published OpenAPI description into what it says about each
//! operation, and the drafting rule from such a finding to an endpoint
//! entry.
//!
//! A description is a claim, not an observation. Nothing here marks an
//! operation as a read unless the description's method is one HTTP marks
//! safe, and nothing is written into a draft that the description does not
//! state: what it leaves out is an open question for the developer.
//!
//! Accepted: OpenAPI 3.0 and 3.1, and Swagger 2.0, as JSON or YAML, with
//! references inside the document. A reference to another file is not
//! followed. A 2.0 document is read from its own places (`definitions`,
//! a response's `schema`, a parameter's own `type`, a body parameter) into
//! the same values; its `basePath`, like an OpenAPI 3 `servers` entry, is
//! not added to a path: the surface's address carries the prefix, and the drafter ties an endpoint to a surface only when the two agree.
//!
//! A description is text the system supplies, so reading it is bounded:
//! how deep a schema nests, how many steps the import takes, and how many
//! bytes of shapes it writes. Past a bound the import is refused by name,
//! `input`/`description_too_large`, and nothing is made of a part of it.

use std::collections::{BTreeMap, BTreeSet};

use a2m_core::{Failure, FileKindName, Finding, FindingKind, Redact};
use a2m_files::{Doc, DraftContext, DraftDoc, DraftValue, DraftingRule, Located, RelPath};

use crate::codes;
use crate::draft::would_draft;
use crate::endpoints::{ENDPOINT_KIND, HTTP_SURFACE};
use crate::json::Json;
use crate::template::parse_path;

/// The version of the shape of a `described_operation` finding's `found`.
/// Format 2 adds the operation's `parameters`; a finding of format 1 is
/// still read, as one that states none.
pub(crate) const FOUND_FORMAT: &str = "2";
/// The earlier format, still read.
const FOUND_FORMAT_1: &str = "1";

/// How far a chain of references is followed before it is taken to lead
/// nowhere.
const LONGEST_CHAIN: usize = 32;
/// The deepest a schema may nest once its references are followed.
const DEEPEST_SHAPE: usize = 32;
/// The most steps one import takes: a schema part visited, with the
/// entries it is searched through; a byte of a reference compared; and an
/// entry passed over while a reference is looked up.
const MOST_STEPS: u64 = 20_000_000;
/// The most bytes of shapes one import writes: each part, with the names
/// and the words copied into it.
const MOST_SHAPE_BYTES: u64 = 16_000_000;
/// What one part of a shape is counted as, besides the text copied into it.
const PART_BYTES: u64 = 64;

/// The finding kind this module makes and drafts from.
pub(crate) fn described_operation() -> Result<FindingKind, Failure> {
    Ok(FindingKind::new("described_operation")?)
}

fn unreadable(why: &str) -> Failure {
    codes::failure(
        &codes::row::DESCRIPTION_UNREADABLE,
        format!("The description cannot be read as OpenAPI: {why}."),
        "Give an OpenAPI 3.0 or 3.1 document or a Swagger 2.0 document, as JSON or YAML, that holds its own schemas.",
    )
}

/// A description that is past what the import reads. It is refused whole:
/// a part of it is never read as if it were all of it.
pub(crate) fn too_large(why: &str) -> Failure {
    codes::failure(
        &codes::row::DESCRIPTION_TOO_LARGE,
        format!("The description is larger than the import reads: {why}."),
        "Give a description of the part of the service the tools need, with fewer or smaller schemas.",
    )
}

fn doc_to_json(doc: &Located<Doc>) -> Json {
    match &doc.value {
        Doc::Null => Json::Null,
        Doc::Bool(flag) => Json::Bool(*flag),
        Doc::Int(number) => Json::Number(number.to_string()),
        Doc::Text(text) => Json::Text(text.clone()),
        Doc::List(items) => Json::List(items.iter().map(doc_to_json).collect()),
        Doc::Map(entries) => Json::Object(
            entries
                .iter()
                .map(|entry| (entry.key.value.clone(), doc_to_json(&entry.value)))
                .collect(),
        ),
    }
}

/// Reads a description's bytes as one document, JSON or YAML. Both readers
/// bound how deep a document nests, and the YAML reader refuses anchors
/// and aliases, so a document cannot grow as it is read.
fn document(bytes: &[u8]) -> Result<Json, Failure> {
    if let Ok(json) = Json::parse(bytes) {
        return Ok(json);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| unreadable("it is not UTF-8 text"))?;
    let path = RelPath::new("description.yaml")?;
    let docs = a2m_files::parse_yaml(&path, text)
        .map_err(|_| unreadable("it is neither JSON nor YAML"))?;
    match docs.as_slice() {
        [doc] => Ok(doc_to_json(doc)),
        _ => Err(unreadable("it is not one document")),
    }
}

/// The sign a description's security scheme is classified as, by the fixed
/// rules of decision 0367: a closed name `a2m_auth::SignInSign` knows, or
/// `unrecognized` where no rule covers the scheme. The scheme's own text is
/// never kept.
fn classify_scheme(scheme: &Json) -> &'static str {
    let word = |value: Option<&Json>| value.and_then(Json::as_text).map(str::to_ascii_lowercase);
    let has =
        |flows: Option<&Json>, name: &str| flows.is_some_and(|flows| flows.get(name).is_some());
    match word(scheme.get("type")).as_deref() {
        Some("apikey") => "api_key",
        Some("basic") => "http_basic",
        Some("mutualtls") => "client_certificate",
        Some("openidconnect") => "authorization_code",
        Some("http") => match word(scheme.get("scheme")).as_deref() {
            Some("basic") => "http_basic",
            Some("bearer") => "bearer_token",
            Some("digest") => "http_digest",
            Some("negotiate" | "ntlm") => "integrated",
            _ => "unrecognized",
        },
        Some("oauth2") => {
            let flows = scheme.get("flows");
            let flow = word(scheme.get("flow"));
            if has(flows, "clientCredentials") || flow.as_deref() == Some("application") {
                "client_credentials"
            } else if has(flows, "authorizationCode")
                || has(flows, "implicit")
                || matches!(flow.as_deref(), Some("accesscode" | "implicit"))
            {
                "authorization_code"
            } else {
                "unrecognized"
            }
        }
        _ => "unrecognized",
    }
}

/// How a description says the service is signed in to: the sign of each
/// scheme it declares (OpenAPI 3 `components.securitySchemes`, Swagger 2
/// `securityDefinitions`), each name once, in the order first met. What a
/// description declares is a claim, not a proof: nothing is sent with it.
pub(crate) fn sign_in_signs(bytes: &[u8]) -> Result<Vec<&'static str>, Failure> {
    let root = document(bytes)?;
    let declared = root
        .get("components")
        .and_then(|components| components.get("securitySchemes"))
        .or_else(|| root.get("securityDefinitions"));
    let mut signs = Vec::new();
    if let Some(Json::Object(schemes)) = declared {
        for (_, scheme) in schemes {
            let sign = classify_scheme(scheme);
            if !signs.contains(&sign) {
                signs.push(sign);
            }
        }
    }
    Ok(signs)
}

fn length(text: &str) -> u64 {
    u64::try_from(text.len()).unwrap_or(u64::MAX)
}

fn entries(json: &Json) -> u64 {
    match json {
        Json::Object(entries) => u64::try_from(entries.len()).unwrap_or(u64::MAX),
        Json::List(items) => u64::try_from(items.len()).unwrap_or(u64::MAX),
        _ => 0,
    }
}

/// One reading of a description. A description is the system's own text,
/// and a few kilobytes of it can name a schema that is millions of parts
/// once its references are followed. The walk counts what it does and
/// stops, by name, past a fixed amount: how deep a schema nests, how many
/// steps the import takes, and how many bytes of shapes it writes.
struct Walk<'a> {
    root: &'a Json,
    steps: u64,
    bytes: u64,
    /// The references being followed now, outermost first. A reference to
    /// one of them is a schema that holds itself.
    open: Vec<&'a str>,
    /// Where each reference seen so far leads, so that each is looked up
    /// once.
    seen: BTreeMap<&'a str, Option<&'a Json>>,
    /// True while an answer's shape is drafted: a property the description
    /// does not require is then written `nullable` (decision 0297). A
    /// request's shape and a parameter's are drafted without it.
    optional_may_be_null: bool,
    /// True for a Swagger 2.0 document, which states a body, a parameter's
    /// type, and a shared schema in places of its own.
    swagger: bool,
    /// Set when a reference that leads to another file was met: such a
    /// reference is never followed, and the shape it would give is left
    /// as an open question that says so.
    outside: bool,
}

impl<'a> Walk<'a> {
    fn new(root: &'a Json) -> Self {
        Self {
            root,
            steps: MOST_STEPS,
            bytes: MOST_SHAPE_BYTES,
            open: Vec::new(),
            seen: BTreeMap::new(),
            optional_may_be_null: false,
            swagger: false,
            outside: false,
        }
    }

    fn step(&mut self, count: u64) -> Result<(), Failure> {
        self.steps = self.steps.checked_sub(count.max(1)).ok_or_else(|| {
            too_large(&format!(
                "following its references takes more than {MOST_STEPS} steps"
            ))
        })?;
        Ok(())
    }

    fn write(&mut self, bytes: u64) -> Result<(), Failure> {
        self.bytes = self.bytes.checked_sub(bytes).ok_or_else(|| {
            too_large(&format!(
                "its schemas, with their references followed, are more than {MOST_SHAPE_BYTES} bytes of shapes"
            ))
        })?;
        Ok(())
    }

    /// Where a reference inside the document leads. `None` for a reference
    /// to another file, or one that leads nowhere.
    fn target(&mut self, reference: &'a str) -> Result<Option<&'a Json>, Failure> {
        if let Some(known) = self.seen.get(reference).copied() {
            return Ok(known);
        }
        let mut target = None;
        if let Some(pointer) = reference.strip_prefix("#/") {
            let mut json = Some(self.root);
            for part in pointer.split('/') {
                let Some(inner) = json else { break };
                self.step(entries(inner))?;
                let name = part.replace("~1", "/").replace("~0", "~");
                json = inner.get(&name);
            }
            target = json;
        }
        self.seen.insert(reference, target);
        Ok(target)
    }

    /// Follows the references `json` starts with, to what is not one.
    /// Each reference followed stays open until the caller closes it.
    /// `None` for a reference to another file, one that leads nowhere, a
    /// chain that does not end, and a reference to a schema that is being
    /// followed now: a schema that holds itself has no shape in this
    /// pack's words.
    fn follow(&mut self, mut json: &'a Json) -> Result<Option<&'a Json>, Failure> {
        for _ in 0..LONGEST_CHAIN {
            let Some(reference) = json.get("$ref").and_then(Json::as_text) else {
                return Ok(Some(json));
            };
            // The reference's own text is compared with each one open, and
            // looked up: its length is what that costs.
            let open = u64::try_from(self.open.len()).unwrap_or(u64::MAX);
            self.step(length(reference).saturating_mul(open.saturating_add(2)))?;
            if self.open.contains(&reference) {
                return Ok(None);
            }
            if !reference.starts_with('#') {
                self.outside = true;
                return Ok(None);
            }
            let Some(target) = self.target(reference)? else {
                return Ok(None);
            };
            self.open.push(reference);
            json = target;
        }
        Ok(None)
    }

    /// The same, for a part of the description that is not a schema: the
    /// references followed are closed again at once.
    fn resolve(&mut self, json: &'a Json) -> Result<Option<&'a Json>, Failure> {
        let opened = self.open.len();
        let found = self.follow(json);
        self.open.truncate(opened);
        found
    }

    /// A schema of the description as a shape in this pack's own words, or
    /// `None` where the description's schema cannot be followed. A rule
    /// the pack does not check is left out, so a draft never claims to
    /// check it.
    fn shape_of(&mut self, schema: &'a Json, depth: usize) -> Result<Option<Json>, Failure> {
        if depth > DEEPEST_SHAPE {
            return Err(too_large(&format!(
                "a schema nests more than {DEEPEST_SHAPE} levels deep"
            )));
        }
        let opened = self.open.len();
        let shape = match self.follow(schema) {
            Ok(Some(schema)) => self.shape_at(schema, depth),
            Ok(None) => Ok(None),
            Err(failure) => Err(failure),
        };
        self.open.truncate(opened);
        shape
    }

    fn shape_at(&mut self, schema: &'a Json, depth: usize) -> Result<Option<Json>, Failure> {
        // The part is searched for each of its words, and is one part of
        // the shape written.
        self.step(entries(schema).saturating_add(1))?;
        self.write(PART_BYTES)?;
        let text = |word: &str| Json::Text(word.to_owned());
        let mut nullable = schema.get("nullable") == Some(&Json::Bool(true));
        let kind = match schema.get("type") {
            Some(Json::Text(kind)) => kind.as_str(),
            // OpenAPI 3.1 writes a value that may be absent as a list of types.
            Some(Json::List(kinds)) => {
                self.step(entries(schema.get("type").unwrap_or(&Json::Null)))?;
                let mut named = kinds.iter().filter_map(Json::as_text);
                nullable = nullable || kinds.iter().any(|kind| kind.as_text() == Some("null"));
                let mut others = named.by_ref().filter(|kind| *kind != "null");
                match (others.next(), others.next()) {
                    (Some(one), None) => one,
                    _ => "any",
                }
            }
            _ => "any",
        };
        let known = [
            "object", "array", "string", "integer", "number", "boolean", "null",
        ];
        let kind = if known.contains(&kind) { kind } else { "any" };
        // A schema that states no type but allows only text, by `enum` or
        // `const`, holds text: JSON Schema's own rule, not a guess.
        let all_text = |json: Option<&Json>| match json {
            Some(Json::List(words)) => {
                !words.is_empty() && words.iter().all(|word| matches!(word, Json::Text(_)))
            }
            Some(Json::Text(_)) => true,
            _ => false,
        };
        let kind = if kind == "any"
            && schema.get("type").is_none()
            && (all_text(schema.get("enum")) || all_text(schema.get("const")))
        {
            "string"
        } else {
            kind
        };
        let mut shape = vec![("type".to_owned(), text(kind))];
        if nullable {
            shape.push(("nullable".to_owned(), Json::Bool(true)));
        }
        match kind {
            "object" => {
                if let Some(Json::Object(properties)) = schema.get("properties") {
                    let mut converted = Vec::new();
                    let mut names = BTreeSet::new();
                    // A property the description does not list as required
                    // may be answered as `null`, in an answer's shape.
                    let listed: BTreeSet<&str> = match schema.get("required") {
                        Some(Json::List(listed)) if self.optional_may_be_null => {
                            listed.iter().filter_map(Json::as_text).collect()
                        }
                        _ => BTreeSet::new(),
                    };
                    for (name, property) in properties {
                        self.step(1)?;
                        self.write(length(name))?;
                        let Some(mut inner) = self.shape_of(property, depth + 1)? else {
                            return Ok(None);
                        };
                        if self.optional_may_be_null
                            && !listed.contains(name.as_str())
                            && let Json::Object(parts) = &mut inner
                            && !parts.iter().any(|(word, _)| word == "nullable")
                        {
                            parts.insert(1, ("nullable".to_owned(), Json::Bool(true)));
                        }
                        names.insert(name.as_str());
                        converted.push((name.clone(), inner));
                    }
                    if let Some(Json::List(listed)) = schema.get("required") {
                        let mut required = Vec::new();
                        for word in listed {
                            self.step(1)?;
                            // A name is required once, and only when the
                            // shape holds it.
                            if let Some(name) = word.as_text()
                                && names.remove(name)
                            {
                                self.write(length(name))?;
                                required.push(word.clone());
                            }
                        }
                        if !required.is_empty() {
                            shape.push(("required".to_owned(), Json::List(required)));
                        }
                    }
                    shape.push(("properties".to_owned(), Json::Object(converted)));
                }
            }
            "array" => {
                if let Some(items) = schema.get("items") {
                    let Some(inner) = self.shape_of(items, depth + 1)? else {
                        return Ok(None);
                    };
                    shape.push(("items".to_owned(), inner));
                }
            }
            "string" => {
                if let Some(Json::List(words)) = schema.get("enum")
                    && words.iter().all(|word| matches!(word, Json::Text(_)))
                {
                    for word in words {
                        self.step(1)?;
                        self.write(word.as_text().map_or(0, length))?;
                    }
                    shape.push(("enum".to_owned(), Json::List(words.clone())));
                }
            }
            _ => {}
        }
        Ok(Some(Json::Object(shape)))
    }

    /// The JSON schema of one body of the description, as a shape. In a
    /// Swagger 2.0 document the schema is the response's own `schema`, and
    /// is JSON only when `json` says the operation speaks it.
    fn body_shape(&mut self, holder: &'a Json, json: bool) -> Result<Option<Json>, Failure> {
        let Some(holder) = self.resolve(holder)? else {
            return Ok(None);
        };
        let schema = if self.swagger {
            holder.get("schema").filter(|_| json)
        } else {
            holder
                .get("content")
                .and_then(|content| content.get("application/json"))
                .and_then(|json| json.get("schema"))
        };
        match schema {
            Some(schema) => self.shape_of(schema, 0),
            None => Ok(None),
        }
    }

    /// Whether a Swagger 2.0 list of media types (`produces`, `consumes`)
    /// lets an operation speak JSON. Only an absent word takes the default;
    /// a word that is not a list of text, or a list that does not name
    /// `application/json` (an empty one included), does not. Each entry
    /// examined is a step.
    fn speaks(&mut self, list: Option<&Json>) -> Result<bool, Failure> {
        match list {
            None => Ok(true),
            Some(Json::List(types)) => {
                self.step(u64::try_from(types.len()).unwrap_or(u64::MAX))?;
                Ok(types.iter().all(|word| matches!(word, Json::Text(_)))
                    && types
                        .iter()
                        .any(|word| word.as_text() == Some("application/json")))
            }
            Some(_) => Ok(false),
        }
    }

    /// The request body of a Swagger 2.0 operation: the schema of its
    /// `in: body` parameter, when the operation takes JSON. A form
    /// (`in: formData`) is a body this pack cannot draft, as a 3.x body that
    /// is not JSON is: it has no shape.
    fn swagger_request(
        &mut self,
        of_path: Option<&'a Json>,
        own: Option<&'a Json>,
        json: bool,
    ) -> Result<Option<Json>, Failure> {
        let mut body = None;
        for list in [of_path, own].into_iter().flatten() {
            let Some(Json::List(items)) = self.resolve(list)? else {
                continue;
            };
            for item in items {
                self.step(1)?;
                let Some(parameter) = self.resolve(item)? else {
                    continue;
                };
                match parameter.get("in").and_then(Json::as_text) {
                    Some("body") => body = Some(parameter),
                    Some("formData") => return Ok(None),
                    _ => {}
                }
            }
        }
        match body.and_then(|parameter| parameter.get("schema")) {
            Some(schema) if json => self.shape_of(schema, 0),
            _ => Ok(None),
        }
    }

    /// The parameters of one operation: the path item's, then the
    /// operation's own, one of a name and place replacing the other as the
    /// format says. A parameter that cannot be followed is left out.
    fn parameters(
        &mut self,
        of_path: Option<&'a Json>,
        own: Option<&'a Json>,
    ) -> Result<Vec<Parameter>, Failure> {
        let mut found: Vec<Parameter> = Vec::new();
        for list in [of_path, own].into_iter().flatten() {
            let Some(Json::List(items)) = self.resolve(list)? else {
                continue;
            };
            for item in items {
                self.step(1)?;
                let Some(parameter) = self.resolve(item)? else {
                    continue;
                };
                let (Some(name), Some(place)) = (
                    parameter.get("name").and_then(Json::as_text),
                    parameter.get("in").and_then(Json::as_text),
                ) else {
                    continue;
                };
                // A 2.0 body or form is a parameter too: a read that needs one
                // is not one the rule drafts.
                let body_like = self.swagger && ["body", "formData"].contains(&place);
                if !body_like && !["path", "query", "header", "cookie"].contains(&place) {
                    continue;
                }
                // A 2.0 parameter states its type, format, enum and items in
                // place, as a schema of its own.
                let schema = if body_like {
                    None
                } else if self.swagger {
                    self.shape_of(parameter, 0)?
                } else {
                    match parameter.get("schema") {
                        Some(schema) => self.shape_of(schema, 0)?,
                        None => None,
                    }
                };
                let kind = schema
                    .as_ref()
                    .and_then(|shape| shape.get("type"))
                    .and_then(Json::as_text)
                    .unwrap_or("any")
                    .to_owned();
                // The description's own example, on the parameter or on
                // its schema: plain text or a whole number, nothing else.
                let example = parameter
                    .get("example")
                    .or_else(|| parameter.get("schema").and_then(|s| s.get("example")))
                    .filter(|example| match example {
                        Json::Text(_) => true,
                        number @ Json::Number(_) => number.is_whole(),
                        _ => false,
                    })
                    .cloned();
                self.write(PART_BYTES.saturating_add(length(name)))?;
                let parameter = Parameter {
                    name: name.to_owned(),
                    place: place.to_owned(),
                    // A path parameter is always required, by the format.
                    required: place == "path"
                        || parameter.get("required") == Some(&Json::Bool(true)),
                    kind,
                    example,
                };
                found.retain(|known| {
                    !(known.name == parameter.name && known.place == parameter.place)
                });
                found.push(parameter);
            }
        }
        Ok(found)
    }
}

/// A name in the form target files use, made mechanically from the
/// system's own word: lowercase, with `_` before each capital and in place
/// of anything that is not a letter or a digit.
pub(crate) fn snake(word: &str) -> String {
    let mut out = String::new();
    let mut previous_lower = false;
    for c in word.chars() {
        if c.is_ascii_alphanumeric() {
            if c.is_ascii_uppercase() && previous_lower {
                out.push('_');
            }
            previous_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
            out.push(c.to_ascii_lowercase());
        } else {
            previous_lower = false;
            if !out.ends_with('_') && !out.is_empty() {
                out.push('_');
            }
        }
    }
    out.trim_end_matches('_').to_owned()
}

/// One parameter a description states for an operation: its name as the
/// description writes it, where it goes, whether it is required, the type
/// of its schema in this pack's words, and the example the description
/// gives for it, when it gives one that is plain text or a whole number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Parameter {
    pub name: String,
    /// `path`, `query`, `header`, or `cookie`.
    pub place: String,
    pub required: bool,
    /// `string`, `integer`, `number`, `boolean`, or `any`.
    pub kind: String,
    pub example: Option<Json>,
}

impl Parameter {
    fn to_json(&self) -> Json {
        let text = |word: &str| Json::Text(word.to_owned());
        Json::Object(vec![
            ("name".to_owned(), text(&self.name)),
            ("in".to_owned(), text(&self.place)),
            ("required".to_owned(), Json::Bool(self.required)),
            ("type".to_owned(), text(&self.kind)),
            (
                "example".to_owned(),
                self.example.clone().unwrap_or(Json::Null),
            ),
        ])
    }

    fn from_json(json: &Json) -> Option<Self> {
        Some(Self {
            name: json.get("name")?.as_text()?.to_owned(),
            place: json.get("in")?.as_text()?.to_owned(),
            required: json.get("required")? == &Json::Bool(true),
            kind: json.get("type")?.as_text()?.to_owned(),
            example: json
                .get("example")
                .filter(|example| **example != Json::Null)
                .cloned(),
        })
    }
}

/// What a description says about one operation, as the `found` of a
/// `described_operation` finding: a JSON object with the keys `format`,
/// `method`, `path`, `operation_id`, `safe`, `ok`, `absent`,
/// `request_schema`, `response_schema`, and `parameters`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DescribedOperation {
    pub method: String,
    /// The path as the description writes it.
    pub path: String,
    pub operation_id: Option<String>,
    /// Whether the description's method is one HTTP marks safe.
    pub safe: bool,
    pub ok: Vec<u16>,
    pub absent: Vec<u16>,
    pub request: Option<Json>,
    pub response: Option<Json>,
    /// The parameters it states, in the order the description lists them:
    /// those of the path first, then the operation's own.
    pub parameters: Vec<Parameter>,
    /// Whether the description says the operation needs no credential: it
    /// states no `security`, for itself or for the whole description, or
    /// an empty one, or one that allows no scheme. A finding of format 1
    /// does not say, and is read as needing one.
    pub open: bool,
    /// The part of the tool's name made from the path, when the read is
    /// one the drafter may draft: unique among the description's reads
    /// ([`assign_objects`]). A finding that states none is named from its
    /// path alone.
    pub object: Option<String>,
    /// True for a read the rule would draft whose name is still shared with
    /// another read's once every name is made: it is given no name.
    pub shared: bool,
    /// A name for the file of a read the rule would draft, from its method
    /// and path, the same in every run and the same for no other read: the
    /// file it takes when it is given no tool name.
    pub stem: Option<String>,
    /// The name of the endpoint entry, when the import has made every
    /// operation's name and this one's is its own alone ([`assign_endpoints`]).
    /// A finding that states none is named by [`Self::base_name`].
    pub endpoint: Option<String>,
    /// True when every name the operation could take is another's: it takes
    /// none, and its endpoint is drafted with its name as an open question.
    pub endpoint_shared: bool,
    /// The base path a 2.0 description states, when it states one that is
    /// not the root: leading slash, no trailing one.
    pub base_path: Option<String>,
    /// The digest of the description's text as the import read it, once
    /// redacted and with its line endings made one: what an `api` binding
    /// is pinned to (decision 0399). A finding that states none is not
    /// pinned, and its read is drafted over its endpoint entry.
    pub digest: Option<String>,
    /// The path, on the surface, the description was fetched from; none
    /// when it was handed in as a file. The `api` driver reads it again to
    /// see whether the description has changed since it was pinned.
    pub served_at: Option<String>,
    /// True when the description states the answer's shape only through a
    /// reference to another file, which the import never reads: the shape
    /// is left as an open question that says so.
    pub response_outside: bool,
}

impl DescribedOperation {
    pub(crate) fn found(&self) -> String {
        let statuses =
            |list: &[u16]| Json::List(list.iter().map(|s| Json::Number(s.to_string())).collect());
        let text = |word: &str| Json::Text(word.to_owned());
        Json::Object(vec![
            ("format".to_owned(), text(FOUND_FORMAT)),
            ("method".to_owned(), text(&self.method)),
            ("path".to_owned(), text(&self.path)),
            (
                "operation_id".to_owned(),
                self.operation_id.as_deref().map_or(Json::Null, text),
            ),
            ("safe".to_owned(), Json::Bool(self.safe)),
            ("ok".to_owned(), statuses(&self.ok)),
            ("absent".to_owned(), statuses(&self.absent)),
            (
                "request_schema".to_owned(),
                self.request.clone().unwrap_or(Json::Null),
            ),
            (
                "response_schema".to_owned(),
                self.response.clone().unwrap_or(Json::Null),
            ),
            (
                "parameters".to_owned(),
                Json::List(self.parameters.iter().map(Parameter::to_json).collect()),
            ),
            ("open".to_owned(), Json::Bool(self.open)),
            (
                "object".to_owned(),
                self.object.as_deref().map_or(Json::Null, text),
            ),
            ("shared".to_owned(), Json::Bool(self.shared)),
            (
                "stem".to_owned(),
                self.stem.as_deref().map_or(Json::Null, text),
            ),
            (
                "endpoint".to_owned(),
                self.endpoint.as_deref().map_or(Json::Null, text),
            ),
            (
                "endpoint_shared".to_owned(),
                Json::Bool(self.endpoint_shared),
            ),
            (
                "base_path".to_owned(),
                self.base_path.as_deref().map_or(Json::Null, text),
            ),
            (
                "digest".to_owned(),
                self.digest.as_deref().map_or(Json::Null, text),
            ),
            (
                "served_at".to_owned(),
                self.served_at.as_deref().map_or(Json::Null, text),
            ),
            (
                "response_outside".to_owned(),
                Json::Bool(self.response_outside),
            ),
        ])
        .to_text()
    }

    pub(crate) fn from_found(found: &str) -> Option<Self> {
        let json = Json::parse(found.as_bytes()).ok()?;
        let format = json.get("format")?.as_text()?;
        if format != FOUND_FORMAT && format != FOUND_FORMAT_1 {
            return None;
        }
        let parameters = match json.get("parameters") {
            Some(Json::List(items)) => items
                .iter()
                .map(Parameter::from_json)
                .collect::<Option<Vec<_>>>()?,
            None if format == FOUND_FORMAT_1 => Vec::new(),
            _ => return None,
        };
        let statuses = |key: &str| -> Option<Vec<u16>> {
            match json.get(key)? {
                Json::List(items) => items
                    .iter()
                    .map(|item| item.scalar_text()?.parse().ok())
                    .collect(),
                _ => None,
            }
        };
        let optional = |key: &str| json.get(key).filter(|value| **value != Json::Null).cloned();
        Some(Self {
            method: json.get("method")?.as_text()?.to_owned(),
            path: json.get("path")?.as_text()?.to_owned(),
            operation_id: json
                .get("operation_id")
                .and_then(Json::as_text)
                .map(str::to_owned),
            safe: json.get("safe")? == &Json::Bool(true),
            ok: statuses("ok")?,
            absent: statuses("absent")?,
            request: optional("request_schema"),
            response: optional("response_schema"),
            parameters,
            open: json.get("open") == Some(&Json::Bool(true)),
            object: json
                .get("object")
                .and_then(Json::as_text)
                .map(str::to_owned),
            shared: json.get("shared") == Some(&Json::Bool(true)),
            stem: json.get("stem").and_then(Json::as_text).map(str::to_owned),
            endpoint: json
                .get("endpoint")
                .and_then(Json::as_text)
                .map(str::to_owned),
            endpoint_shared: json.get("endpoint_shared") == Some(&Json::Bool(true)),
            base_path: json
                .get("base_path")
                .and_then(Json::as_text)
                .map(str::to_owned),
            digest: json
                .get("digest")
                .and_then(Json::as_text)
                .map(str::to_owned),
            served_at: json
                .get("served_at")
                .and_then(Json::as_text)
                .map(str::to_owned),
            response_outside: json.get("response_outside") == Some(&Json::Bool(true)),
        })
    }

    /// The name of the endpoint entry drafted for the operation: the one
    /// the import gave it, or, for a finding that states none, the name its
    /// own words make.
    pub(crate) fn name(&self) -> String {
        self.endpoint.clone().unwrap_or_else(|| self.base_name())
    }

    /// The name from the operation's `operationId`, when it makes one that
    /// begins with a lowercase letter; else the one from its method and path.
    pub(crate) fn base_name(&self) -> String {
        self.id_name().unwrap_or_else(|| self.path_name())
    }

    fn id_name(&self) -> Option<String> {
        let from_id = self.operation_id.as_deref().map(snake).unwrap_or_default();
        from_id
            .starts_with(|c: char| c.is_ascii_lowercase())
            .then(|| from_id.chars().take(60).collect())
    }

    fn path_name(&self) -> String {
        snake(&format!("{} {}", self.method, self.path))
            .chars()
            .take(60)
            .collect()
    }

    /// The path as a template of this pack: each placeholder a lowercase
    /// name. `None` when the description's path is not one.
    pub(crate) fn template_path(&self) -> Option<String> {
        template_path_of(&self.path)
    }
}

/// A description's path as a template of this pack: each placeholder a
/// lowercase name. `None` when the path is not one.
pub(crate) fn template_path_of(path: &str) -> Option<String> {
    let mut out = String::new();
    for part in path.split('/').skip(1) {
        out.push('/');
        match part.strip_prefix('{').and_then(|p| p.strip_suffix('}')) {
            Some(name) => {
                out.push('{');
                out.push_str(&snake(name));
                out.push('}');
            }
            None => out.push_str(part),
        }
    }
    parse_path(&out).ok().map(|_| out)
}

/// Every operation a description states, in the order of its paths.
///
/// # Errors
///
/// `input`/`description_unreadable` for bytes that are not an OpenAPI 3.0
/// or 3.1 document.
pub(crate) fn operations(bytes: &[u8]) -> Result<Vec<DescribedOperation>, Failure> {
    let root = document(bytes)?;
    let version = root.get("openapi").and_then(Json::scalar_text);
    let three = version
        .as_ref()
        .is_some_and(|version| version.starts_with("3.0") || version.starts_with("3.1"));
    let swagger =
        !three && root.get("swagger").and_then(Json::scalar_text).as_deref() == Some("2.0");
    if !three && !swagger {
        return Err(unreadable(
            "it states neither `openapi: 3.0` or `3.1` nor `swagger: 2.0`",
        ));
    }
    let Some(Json::Object(paths)) = root.get("paths") else {
        return Err(unreadable("it has no `paths`"));
    };
    let mut walk = Walk::new(&root);
    walk.swagger = swagger;
    // The document's own lists, read once.
    let document_produces = walk.speaks(root.get("produces"))?;
    let document_consumes = walk.speaks(root.get("consumes"))?;
    let base_path = root
        .get("basePath")
        .filter(|_| swagger)
        .and_then(Json::as_text)
        .map(|word| format!("/{}", word.trim_matches('/')))
        .filter(|word| word != "/");
    let mut found = Vec::new();
    for (path, item) in paths {
        walk.step(1)?;
        let Some(resolved @ Json::Object(entries)) = walk.resolve(item)? else {
            continue;
        };
        if !path.starts_with('/') {
            continue;
        }
        for (method, operation) in entries {
            walk.step(1)?;
            let known = ["get", "head", "post", "put", "patch", "delete"];
            if !known.contains(&method.as_str()) {
                continue;
            }
            let mut ok = Vec::new();
            let mut absent = Vec::new();
            let mut response = None;
            let mut response_outside = false;
            // A 2.0 operation answers, and takes, JSON when its own list of
            // types, or else the document's, names it or names none.
            let (answers_json, takes_json) = if swagger {
                (
                    match operation.get("produces") {
                        Some(own) => walk.speaks(Some(own))?,
                        None => document_produces,
                    },
                    match operation.get("consumes") {
                        Some(own) => walk.speaks(Some(own))?,
                        None => document_consumes,
                    },
                )
            } else {
                (true, true)
            };
            if let Some(Json::Object(responses)) = operation.get("responses") {
                for (status, body) in responses {
                    walk.step(1)?;
                    let Ok(status) = status.parse::<u16>() else {
                        continue;
                    };
                    if (200..300).contains(&status) {
                        if ok.is_empty() {
                            walk.optional_may_be_null = true;
                            walk.outside = false;
                            let drafted = walk.body_shape(body, answers_json);
                            walk.optional_may_be_null = false;
                            response = drafted?;
                            response_outside = response.is_none() && walk.outside;
                        }
                        ok.push(status);
                    } else if status == 404 {
                        absent.push(status);
                    }
                }
            }
            let request = if swagger {
                walk.swagger_request(
                    item_parameters(resolved),
                    operation.get("parameters"),
                    takes_json,
                )?
            } else {
                match operation.get("requestBody") {
                    Some(body) => walk.body_shape(body, true)?,
                    None => None,
                }
            };
            let mut parameters =
                walk.parameters(item_parameters(resolved), operation.get("parameters"))?;
            // A 3.x request body that is required is a parameter the read
            // needs, as a 2.0 `in: body` one is: the same check refuses it.
            if !swagger && let Some(body) = operation.get("requestBody") {
                walk.step(1)?;
                if walk.resolve(body)?.and_then(|body| body.get("required"))
                    == Some(&Json::Bool(true))
                {
                    parameters.push(Parameter {
                        name: "body".to_owned(),
                        place: "body".to_owned(),
                        required: true,
                        kind: "any".to_owned(),
                        example: None,
                    });
                }
            }
            walk.write(length(path))?;
            found.push(DescribedOperation {
                method: method.to_ascii_uppercase(),
                path: path.clone(),
                operation_id: operation
                    .get("operationId")
                    .and_then(Json::as_text)
                    .map(str::to_owned),
                // GET and HEAD, and nothing else: the rule of the format.
                safe: matches!(method.as_str(), "get" | "head"),
                ok,
                absent,
                request,
                response,
                parameters,
                open: needs_no_credential(
                    operation.get("security").or_else(|| root.get("security")),
                ),
                object: None,
                shared: false,
                stem: None,
                endpoint: None,
                endpoint_shared: false,
                base_path: base_path.clone(),
                digest: None,
                served_at: None,
                response_outside,
            });
        }
    }
    assign_objects(&mut found, &mut walk)?;
    assign_endpoints(&mut found, &mut walk)?;
    Ok(found)
}

/// The part of a tool's name made from the description's path: its last
/// two plain words, lowercase. `None` when the path has no plain word.
pub(crate) fn object_words(path: &str) -> Option<String> {
    let words: Vec<String> = path
        .split('/')
        .filter(|part| !part.starts_with('{'))
        .map(snake)
        .filter(|word| word.starts_with(|c: char| c.is_ascii_lowercase()))
        .collect();
    match words.as_slice() {
        [] => None,
        [only] => Some(only.clone()),
        [.., module, object] => Some(format!("{module}_{object}")),
    }
}

/// A word of a path for telling two paths apart: a plain word as
/// [`object_words`] takes it, or a placeholder as `by_` and its name.
fn path_tokens(path: &str) -> Vec<(String, bool)> {
    path.split('/')
        .filter_map(|part| {
            if let Some(name) = part.strip_prefix('{') {
                let name = snake(name.trim_end_matches('}'));
                let word = if name.is_empty() {
                    "by".to_owned()
                } else {
                    format!("by_{name}")
                };
                return Some((word, false));
            }
            let word = snake(part);
            word.starts_with(|c: char| c.is_ascii_lowercase())
                .then_some((word, true))
        })
        .collect()
}

/// A short name, the same in every run, made from a path alone.
fn path_digest(path: &str) -> String {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in path.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("{hash:08x}")
}

/// Whether a read of the description is one the drafter drafts an operation
/// for, as far as its name is concerned, and the kind of verb it has.
fn read_verb(operation: &DescribedOperation) -> Option<(String, bool)> {
    if !operation.safe || operation.method != "GET" || operation.template_path().is_none() {
        return None;
    }
    let shape = operation.response.as_ref()?;
    let listed = shape.get("type").and_then(Json::as_text) == Some("array");
    Some((object_words(&operation.path)?, listed))
}

/// A short name, the same in every run, made from a method and a path.
pub(crate) fn path_stem(method: &str, path: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in method.bytes().chain(*b" ").chain(path.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Gives each read the rule would draft the part of its tool's name that
/// its path makes, and gives no two the same; a read whose name would still
/// be shared is given none.
///
/// A read is named from its path's last two plain words. Two reads that
/// would get one name and one verb are told apart by the path's nearest
/// words that differ: the name is made again from the shortest end of each
/// path, placeholders included as `by_` and their name, that no other has
/// the same, and one more word is added to each that still has. Only reads
/// the rule would draft count, so one the developer must write does not
/// make another's name longer. Nothing here depends on the order of the
/// description: the names are a function of the set of paths. Paths that
/// no word tells apart (`/Orders` and `/orders`) are told apart by a short
/// digest of the path itself. When every name is made, each is checked
/// against every other of its verb, across groups; a read whose name is
/// still another's is marked `shared`, takes no name, and is drafted with
/// its name as an open question, in a file named for its path. The work is
/// counted as steps.
///
/// # Errors
///
/// `input`/`description_too_large` past the step bound.
fn assign_objects(
    operations: &mut [DescribedOperation],
    walk: &mut Walk<'_>,
) -> Result<(), Failure> {
    let mut groups: BTreeMap<(String, bool), Vec<usize>> = BTreeMap::new();
    let mut verbs: BTreeMap<usize, bool> = BTreeMap::new();
    for (index, operation) in operations.iter().enumerate() {
        let properties = operation
            .response
            .as_ref()
            .and_then(|shape| shape.get("properties"))
            .map_or(0, entries);
        walk.step(
            length(&operation.path)
                .saturating_add(properties)
                .saturating_add(u64::try_from(operation.parameters.len()).unwrap_or(u64::MAX)),
        )?;
        if !would_draft(operation) {
            continue;
        }
        if let Some((object, listed)) = read_verb(operation) {
            verbs.insert(index, listed);
            groups.entry((object, listed)).or_default().push(index);
        }
    }
    for ((object, _), members) in groups {
        if let [only] = members.as_slice() {
            operations[*only].object = Some(object);
            continue;
        }
        let tokens: Vec<Vec<(String, bool)>> = members
            .iter()
            .map(|index| path_tokens(&operations[*index].path))
            .collect();
        // The shortest end of each path that holds two plain words, or all
        // of it when it has fewer.
        let mut take: Vec<usize> = tokens
            .iter()
            .map(|words| {
                let (mut plain, mut length) = (0, 0);
                for (_, is_plain) in words.iter().rev() {
                    if plain == 2 {
                        break;
                    }
                    length += 1;
                    plain += usize::from(*is_plain);
                }
                length
            })
            .collect();
        let mut names: Vec<String>;
        loop {
            names = tokens
                .iter()
                .zip(&take)
                .map(|(words, length)| {
                    words[words.len() - length..]
                        .iter()
                        .map(|(word, _)| word.as_str())
                        .collect::<Vec<_>>()
                        .join("_")
                })
                .collect();
            walk.step(names.iter().map(|name| length(name)).sum::<u64>())?;
            let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
            for name in &names {
                *counts.entry(name.as_str()).or_default() += 1;
            }
            let colliding: Vec<usize> = (0..names.len())
                .filter(|index| counts[names[*index].as_str()] > 1)
                .collect();
            if colliding.is_empty() {
                break;
            }
            let mut grew = false;
            for index in colliding.iter().copied() {
                if take[index] < tokens[index].len() {
                    take[index] += 1;
                    grew = true;
                }
            }
            if !grew {
                for index in colliding {
                    let digest = path_digest(&operations[members[index]].path);
                    names[index] = format!("{}_{digest}", names[index]);
                }
                break;
            }
        }
        for (index, name) in members.iter().zip(names) {
            operations[*index].object = Some(name);
        }
    }
    // Every name against every other of its verb, across the groups.
    let mut named: BTreeMap<(String, bool), Vec<usize>> = BTreeMap::new();
    for (index, listed) in &verbs {
        walk.step(1)?;
        if let Some(object) = operations[*index].object.clone() {
            named.entry((object, *listed)).or_default().push(*index);
        }
    }
    for indexes in named.values().filter(|indexes| indexes.len() > 1) {
        for index in indexes {
            operations[*index].object = None;
            operations[*index].shared = true;
        }
    }
    // The file of each read that may need one: no two the same.
    let mut order: Vec<usize> = verbs.keys().copied().collect();
    order.sort_by(|a, b| operations[*a].path.cmp(&operations[*b].path));
    let mut taken: BTreeSet<String> = BTreeSet::new();
    for index in order {
        walk.step(1)?;
        let base = path_stem(&operations[index].method, &operations[index].path);
        let mut stem = base.clone();
        let mut next = 2_u32;
        while !taken.insert(stem.clone()) {
            stem = format!("{base}_{next}");
            next += 1;
        }
        operations[index].stem = Some(stem);
    }
    Ok(())
}

/// Gives each operation the name of its endpoint entry, and gives no two the
/// same, so that no two files of `endpoints/` are one.
///
/// An operation is named from its `operationId`, or from its method and
/// path where that makes no name. Operations that would share a name over
/// the whole description do not take it: each that has an `operationId`
/// falls back to the name its method and path make, and once every name is
/// made, those still shared are marked `endpoint_shared`, take none, and are
/// drafted with the name as an open question in a file named for their
/// method and path. No operation is dropped and none takes another's name.
/// The names are a function of the set of operations, never of their order.
/// The work is counted as steps.
///
/// # Errors
///
/// `input`/`description_too_large` past the step bound.
fn assign_endpoints(
    operations: &mut [DescribedOperation],
    walk: &mut Walk<'_>,
) -> Result<(), Failure> {
    fn shared_names(names: &[String]) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut shared = BTreeSet::new();
        for name in names {
            if !seen.insert(name.clone()) {
                shared.insert(name.clone());
            }
        }
        shared
    }
    let mut names: Vec<String> = Vec::with_capacity(operations.len());
    for operation in operations.iter() {
        let name = operation.base_name();
        walk.step(length(&name).saturating_add(1))?;
        names.push(name);
    }
    let shared = shared_names(&names);
    for (operation, name) in operations.iter().zip(names.iter_mut()) {
        if shared.contains(name.as_str()) && operation.id_name().is_some() {
            *name = operation.path_name();
        }
    }
    walk.step(u64::try_from(names.len()).unwrap_or(u64::MAX))?;
    let shared = shared_names(&names);
    for (operation, name) in operations.iter_mut().zip(names) {
        if shared.contains(&name) {
            operation.endpoint_shared = true;
        } else {
            operation.endpoint = Some(name);
        }
    }
    Ok(())
}

/// Whether a `security` statement lets an operation be called with no
/// credential, by the format's rule: none stated, an empty list, or a list
/// that holds an empty requirement.
fn needs_no_credential(security: Option<&Json>) -> bool {
    match security {
        None => true,
        Some(Json::List(items)) => {
            items.is_empty()
                || items
                    .iter()
                    .any(|item| matches!(item, Json::Object(entries) if entries.is_empty()))
        }
        Some(_) => false,
    }
}

/// The parameters a path item states for every operation under it.
fn item_parameters(item: &Json) -> Option<&Json> {
    item.get("parameters")
}

/// The description as text with every registered secret taken out, before
/// anything is read from it: a name, a path, or a word of a shape that a
/// finding quotes can then hold no part of one.
///
/// # Errors
///
/// `input`/`description_unreadable` for bytes that are not UTF-8 text.
pub(crate) fn redacted(bytes: &[u8], redact: &dyn Redact) -> Result<String, Failure> {
    let text = std::str::from_utf8(bytes).map_err(|_| unreadable("it is not UTF-8 text"))?;
    Ok(redact.redact(text))
}

pub(crate) fn draft_value(json: &Json) -> DraftValue {
    match json {
        Json::Null => DraftValue::Null,
        Json::Bool(flag) => DraftValue::Bool(*flag),
        Json::Number(text) => text
            .parse::<i64>()
            .map_or_else(|_| DraftValue::Text(text.clone()), DraftValue::Int),
        Json::Text(text) => DraftValue::Text(text.clone()),
        Json::List(items) => DraftValue::List(items.iter().map(draft_value).collect()),
        Json::Object(entries) => DraftValue::Map(
            entries
                .iter()
                .map(|(name, value)| (name.clone(), draft_value(value)))
                .collect(),
        ),
    }
}

/// The target's `http` surfaces that take what a finding is about, with
/// the path each one's address ends in: of kind `http`, at the finding's
/// origin, with an address whose path begins the finding's own, by whole
/// segments.
pub(crate) fn surfaces_taking<'a>(
    finding: &Finding,
    cx: &DraftContext<'a>,
) -> Vec<(&'a str, String)> {
    let (origin, at) = match finding.subject() {
        a2m_core::Subject::Address(address) => (Some(address.origin()), address.path()),
        _ => (None, "/"),
    };
    let at = at.trim_end_matches('/');
    let mut taking: Vec<(&str, String)> = Vec::new();
    if let (Some(origin), Some(surfaces)) = (
        origin,
        cx.existing()
            .and_then(|sources| sources.file("target.yaml"))
            .and_then(|file| file.docs.first())
            .and_then(|target| target.get("surfaces")),
    ) {
        for entry in surfaces.entries() {
            if entry.value.text("kind") != Some(HTTP_SURFACE.as_str()) {
                continue;
            }
            let Some(Ok(base)) = entry
                .value
                .text(crate::ADDRESS_KEY)
                .map(crate::driver::Origin::of)
            else {
                continue;
            };
            let within = base.prefix.is_empty()
                || at == base.prefix
                || at.starts_with(&format!("{}/", base.prefix));
            if base.text == origin && within {
                taking.push((entry.key.value.as_str(), base.prefix));
            }
        }
    }
    taking
}

/// The one surface an operation of a description certainly lives on: the
/// only `http` surface that takes its finding, when a 2.0 description's
/// base path, if it states one, is the path that surface's address ends
/// in. `None` when that is not certain.
pub(crate) fn certain_surface<'a>(
    finding: &Finding,
    operation: &DescribedOperation,
    cx: &DraftContext<'a>,
) -> Option<&'a str> {
    match surfaces_taking(finding, cx).as_slice() {
        [(surface, prefix)] => match &operation.base_path {
            Some(base_path) if base_path != prefix => None,
            _ => Some(surface),
        },
        _ => None,
    }
}

/// From a `described_operation` finding to an endpoint entry.
pub(crate) struct DescribedOperationRule {
    pub(crate) kind: FindingKind,
}

/// What an endpoint that was given no name is asked.
const NAME_SHARED: &str = "Two or more operations of the description would share one endpoint name, so none was given by rule. What is this endpoint's name?";

impl DraftingRule for DescribedOperationRule {
    fn finding_kind(&self) -> FindingKind {
        self.kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        ENDPOINT_KIND
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let Some(operation) = DescribedOperation::from_found(finding.found()) else {
            return Err(a2m_core::codes::row::FINDING_INVALID.failure(
                "A `described_operation` finding is not in the form the HTTP pack writes.",
                "Import the description again with the version of the HTTP pack that is enabled.",
            ));
        };
        // An operation whose name is another's takes none: its name is an
        // open question, in a file named for its method and path.
        let name = if operation.endpoint_shared {
            "(a name not yet given)".to_owned()
        } else {
            operation.name()
        };
        // The surface is the one the description was read for, and it is
        // named only when it is certain. A surface of the target that
        // takes it is of kind `http`, at the finding's origin, with an
        // address whose path begins the finding's own, by whole segments.
        // With one such surface it is that; with none, or several (the
        // longer path is not preferred), the question stays open.
        let taking = surfaces_taking(finding, cx);
        let unknown = || {
            cx.open_question(&format!(
                "Which `http` surface does the endpoint `{name}` live on?"
            ))
        };
        let surface = match taking.as_slice() {
            [(surface, prefix)] => match &operation.base_path {
                // A 2.0 description states the path its operations are
                // under; a surface whose address ends in another path is
                // not the one it was written for.
                Some(base_path) if base_path != prefix => cx.open_question_quoting(
                    "The base path the description states, quoted, and the path the address of the only `http` surface that takes it ends in, are not the same, so the endpoint is not tied to that surface. Which surface does the endpoint live on?",
                    &[base_path],
                ),
                _ => DraftValue::text(surface),
            },
            _ => unknown(),
        };
        let path = operation.template_path().map_or_else(
            || {
                cx.open_question(&format!(
                    "How is the path of `{name}` written as a template, with each placeholder as one whole segment?"
                ))
            },
            |path| DraftValue::text(&path),
        );
        let statuses = |list: &[u16]| {
            DraftValue::List(
                list.iter()
                    .map(|s| DraftValue::Int(i64::from(*s)))
                    .collect(),
            )
        };
        let mut responses = Vec::new();
        if !operation.ok.is_empty() {
            responses.push(("ok", statuses(&operation.ok)));
        }
        if !operation.absent.is_empty() && operation.safe {
            responses.push(("absent", statuses(&operation.absent)));
        }
        let mut entry = vec![
            (
                "endpoint",
                if operation.endpoint_shared {
                    cx.open_question(NAME_SHARED)
                } else {
                    DraftValue::text(&name)
                },
            ),
            ("surface", surface),
            ("method", DraftValue::text(&operation.method)),
            ("path", path),
        ];
        // The description says the operation takes no credential: none is
        // sent. Otherwise the endpoint takes the session's, the default.
        if operation.open {
            entry.push(("auth", DraftValue::text("none")));
        }
        entry.extend([
            // A read only for a method the description marks safe. Any
            // other is drafted as the write it is, and the drafter refuses
            // it: the file kind says it describes a write.
            (
                "effect",
                DraftValue::text(if operation.safe { "read" } else { "write" }),
            ),
        ]);
        if let Some(request) = &operation.request {
            entry.push(("request_schema", draft_value(request)));
        }
        entry.push((
            "response_schema",
            operation.response.as_ref().map_or_else(
                || {
                    cx.open_question(&if operation.response_outside {
                        format!(
                            "What is the shape of the answer of `{name}`? The description states it only by a reference to another file, which the import does not read."
                        )
                    } else {
                        format!(
                            "What is the shape of the answer of `{name}`? The description states none this pack can follow."
                        )
                    })
                },
                draft_value,
            ),
        ));
        if !responses.is_empty() {
            entry.push(("responses", DraftValue::map(responses)));
        }
        Ok(vec![DraftDoc {
            path: RelPath::new(&if operation.endpoint_shared {
                format!(
                    "endpoints/held_{}.yaml",
                    path_stem(&operation.method, &operation.path)
                )
            } else {
                format!("endpoints/{name}.yaml")
            })?,
            body: DraftValue::map(entry),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::{DescribedOperation, operations, path_digest, snake};
    use crate::codes;
    use crate::draft::would_draft;

    pub(crate) const DESCRIPTION: &str = r##"{
      "openapi": "3.0.3",
      "info": {"title": "Example order entry", "version": "1"},
      "paths": {
        "/api/orders/{orderId}": {
          "get": {
            "operationId": "getOrder",
            "responses": {
              "200": {"content": {"application/json": {"schema": {"$ref": "#/components/schemas/Order"}}}},
              "404": {"description": "No such order"}
            }
          }
        },
        "/api/orders/{orderId}/items/{itemCode}": {
          "put": {
            "operationId": "putItem",
            "requestBody": {"content": {"application/json": {"schema": {
              "type": "object", "required": ["description"],
              "properties": {"description": {"type": "string", "maxLength": 80}, "quantity": {"type": "integer"}}
            }}}},
            "responses": {"200": {"description": "Updated"}, "201": {"description": "Created"}}
          }
        }
      },
      "components": {"schemas": {"Order": {
        "type": "object", "required": ["order_id"],
        "properties": {
          "order_id": {"type": "string"},
          "status": {"type": "string", "enum": ["draft", "submitted"]},
          "items": {"type": "array", "items": {"$ref": "#/components/schemas/Item"}}
        }},
        "Item": {"type": "object", "properties": {"item_code": {"type": "string"}, "note": {"type": "string", "nullable": true}}}
      }}
    }"##;

    #[test]
    fn a_description_gives_its_operations_and_marks_only_get_and_head_safe() {
        let found = operations(DESCRIPTION.as_bytes()).unwrap();
        assert_eq!(found.len(), 2);
        let (get, put) = (&found[0], &found[1]);
        assert!(get.safe && !put.safe);
        assert_eq!(
            (get.ok.as_slice(), get.absent.as_slice()),
            (&[200][..], &[404][..])
        );
        assert_eq!(put.ok, vec![200, 201]);
        assert_eq!(get.name(), "get_order");
        assert_eq!(
            get.template_path().as_deref(),
            Some("/api/orders/{order_id}")
        );
        // A reference inside the document is followed; a rule this pack
        // does not check is left out of the shape.
        let response = get.response.as_ref().unwrap().to_text();
        assert!(
            response
                .contains(r#""items":{"type":"array","nullable":true,"items":{"type":"object""#),
            "{response}"
        );
        // `order_id` is required and stays as written; `status` is not, and
        // may be answered as `null` (decision 0297).
        assert!(
            response.contains(r#""order_id":{"type":"string"}"#),
            "{response}"
        );
        assert!(
            response.contains(r#""status":{"type":"string","nullable":true"#),
            "{response}"
        );
        assert!(response.contains(r#""nullable":true"#), "{response}");
        let request = put.request.as_ref().unwrap().to_text();
        assert!(!request.contains("maxLength"), "{request}");
        assert!(put.response.is_none());
        // What a finding carries reads back as what was found.
        assert_eq!(
            DescribedOperation::from_found(&get.found()).as_ref(),
            Some(get)
        );
    }

    #[test]
    fn what_is_not_an_accepted_description_is_refused_by_name() {
        for bad in [
            &b"not a description: ["[..],
            br#"{"swagger": "1.2", "paths": {}}"#,
            br#"{"swagger": "2.0"}"#,
            br#"{"openapi": "3.0.0"}"#,
            b"\xff\xfe",
        ] {
            let failure = operations(bad).unwrap_err();
            assert_eq!(failure.code(), &codes::DESCRIPTION_UNREADABLE);
        }
        let yaml = b"openapi: 3.1.0\npaths:\n  /orders:\n    get:\n      responses:\n        '200': {description: ok}\n";
        assert_eq!(operations(yaml).unwrap().len(), 1);
    }

    /// A description of reads, each answering a list of records with one
    /// plain property, at the given paths. Each placeholder is a stated
    /// path parameter, so the rule would draft each read.
    fn reads_at(paths: &[&str]) -> String {
        let items: Vec<String> = paths
            .iter()
            .map(|path| {
                let parameters: Vec<String> = path
                    .split('/')
                    .filter_map(|part| part.strip_prefix('{').and_then(|p| p.strip_suffix('}')))
                    .map(|name| {
                        format!(r#"{{"name": "{name}", "in": "path", "required": true, "schema": {{"type": "string"}}}}"#)
                    })
                    .collect();
                format!(
                    r##""{path}": {{"get": {{"parameters": [{}], "responses": {{"200": {{"content": {{"application/json": {{"schema": {{
                      "type": "array", "items": {{"type": "object", "properties": {{"id": {{"type": "string"}}}}}}}}}}}}}}}}}}}}"##,
                    parameters.join(",")
                )
            })
            .collect();
        format!(
            r#"{{"openapi": "3.0.3", "paths": {{{}}}}}"#,
            items.join(",")
        )
    }

    /// The object of each path, by path; `-` for a read given no name.
    fn objects(paths: &[&str]) -> std::collections::BTreeMap<String, String> {
        operations(reads_at(paths).as_bytes())
            .unwrap()
            .into_iter()
            .map(|operation| {
                (
                    operation.path,
                    operation.object.unwrap_or_else(|| "-".to_owned()),
                )
            })
            .collect()
    }

    #[test]
    fn two_reads_that_would_share_a_tool_name_are_told_apart_by_the_nearest_word_that_differs() {
        let paths = [
            "/repos/{owner}/issues/comments",
            "/repos/{owner}/issues/{number}/comments",
            "/repos/{owner}/pulls/{number}/comments",
            "/repos/{owner}/issues",
        ];
        let forward = objects(&paths);
        // The path with no placeholder keeps the name its words make; the
        // other is told apart by the placeholder between them; a read that
        // collides with nothing is named as before.
        assert_eq!(forward["/repos/{owner}/issues/comments"], "issues_comments");
        assert_eq!(
            forward["/repos/{owner}/issues/{number}/comments"],
            "issues_by_number_comments"
        );
        assert_eq!(
            forward["/repos/{owner}/pulls/{number}/comments"],
            "pulls_comments"
        );
        assert_eq!(forward["/repos/{owner}/issues"], "repos_issues");
        // The same names in any order of reading.
        let mut reversed = paths;
        reversed.reverse();
        assert_eq!(objects(&reversed), forward);

        // Where the last two words are the same and the words before them
        // tell them apart, one more word is added to each that still
        // collides; and where no word does, a digest of the path does.
        let deeper = objects(&["/a/x/{id}/notes", "/b/x/{id}/notes", "/x/notes"]);
        assert_eq!(deeper["/x/notes"], "x_notes");
        assert_eq!(deeper["/a/x/{id}/notes"], "a_x_by_id_notes");
        assert_eq!(deeper["/b/x/{id}/notes"], "b_x_by_id_notes");
        let alike = objects(&["/Notes/items", "/notes/items"]);
        assert_ne!(alike["/Notes/items"], alike["/notes/items"]);
        assert_eq!(alike, objects(&["/notes/items", "/Notes/items"]));
    }

    #[test]
    fn a_read_whose_name_is_still_another_reads_after_every_name_is_made_is_given_none() {
        // (a) A grown name that is another group's own name.
        let a = [
            "/issues/comments",
            "/issues/{number}/comments",
            "/issues_by_number/comments",
        ];
        let named = objects(&a);
        assert_eq!(named["/issues/comments"], "issues_comments");
        assert_eq!(named["/issues/{number}/comments"], "-");
        assert_eq!(named["/issues_by_number/comments"], "-");
        let mut reversed = a;
        reversed.reverse();
        assert_eq!(objects(&reversed), named);
        // (b) A path whose words are another's name with its digest.
        let digest = path_digest("/Notes/items");
        let tricky = format!("/notes/items_{digest}");
        let named = objects(&["/Notes/items", "/notes/items", &tricky]);
        assert_eq!(named.values().filter(|name| *name == "-").count(), 2);
        // Each read that may need a file has one of its own.
        let found = operations(reads_at(&a).as_bytes()).unwrap();
        let stems: std::collections::BTreeSet<_> = found
            .iter()
            .map(|read| read.stem.clone().unwrap())
            .collect();
        assert_eq!(stems.len(), found.len());
        assert_eq!(found.iter().filter(|read| read.shared).count(), 2);
        // The marks read back from a finding.
        for read in &found {
            assert_eq!(
                DescribedOperation::from_found(&read.found()).as_ref(),
                Some(read)
            );
        }
        // (c) A path written twice is refused with the document, so no two
        // reads share a path and a file.
        let read = |property: &str| {
            format!(
                r#""/a/b": {{"get": {{"responses": {{"200": {{"content": {{"application/json": {{"schema": {{"type": "array", "items": {{"type": "object", "properties": {{"{property}": {{"type": "string"}}{}"#,
                "}".repeat(9)
            )
        };
        let twice = format!(
            r#"{{"openapi": "3.0.3", "paths": {{{},{}}}}}"#,
            read("id"),
            read("x")
        );
        let failure = operations(twice.as_bytes()).unwrap_err();
        assert_eq!(failure.code(), &codes::DESCRIPTION_UNREADABLE);
    }

    #[test]
    fn only_a_read_the_rule_would_draft_makes_another_reads_name_longer() {
        // The first needs a required query value, so it is the developer's
        // and does not rename the second.
        let found = operations(
            br##"{"openapi": "3.0.3", "paths": {
              "/orgs/{org}/x/y": {"get": {"parameters": [
                  {"name": "org", "in": "path", "required": true, "schema": {"type": "string"}},
                  {"name": "q", "in": "query", "required": true, "schema": {"type": "string"}}],
                "responses": {"200": {"content": {"application/json": {"schema": {
                  "type": "array", "items": {"type": "object", "properties": {"id": {"type": "string"}}}}}}}}}},
              "/x/y": {"get": {"responses": {"200": {"content": {"application/json": {"schema": {
                  "type": "array", "items": {"type": "object", "properties": {"id": {"type": "string"}}}}}}}}}}}}"##,
        )
        .unwrap();
        assert_eq!(found[0].object, None);
        assert!(!found[0].shared);
        assert_eq!(found[1].object.as_deref(), Some("x_y"));
        // A read with no plain output is not drafted either.
        let found = operations(
            br##"{"openapi": "3.0.3", "paths": {
              "/orgs/{org}/x/y": {"get": {"parameters": [
                  {"name": "org", "in": "path", "required": true, "schema": {"type": "string"}}],
                "responses": {"200": {"content": {"application/json": {"schema": {
                  "type": "array", "items": {"type": "object", "properties": {"tags": {"type": "array", "items": {"type": "string"}}}}}}}}}}},
              "/x/y": {"get": {"responses": {"200": {"content": {"application/json": {"schema": {
                  "type": "array", "items": {"type": "object", "properties": {"id": {"type": "string"}}}}}}}}}}}}"##,
        )
        .unwrap();
        assert_eq!(found[1].object.as_deref(), Some("x_y"));
    }

    #[test]
    fn a_collision_of_names_is_between_reads_of_one_verb_only() {
        // A list and a read by key at one path's words are not told apart:
        // their verbs differ, so their names already do.
        let found = operations(
            br##"{"openapi": "3.0.3", "paths": {
              "/orders": {"get": {"responses": {"200": {"content": {"application/json": {"schema": {
                "type": "array", "items": {"type": "object", "properties": {"id": {"type": "string"}}}}}}}}}},
              "/orders/{id}": {"get": {"parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}],
                "responses": {"200": {"content": {"application/json": {"schema": {
                "type": "object", "properties": {"id": {"type": "string"}}}}}}}}}}}"##,
        )
        .unwrap();
        assert!(
            found
                .iter()
                .all(|read| read.object.as_deref() == Some("orders"))
        );
    }

    #[test]
    fn a_swagger_2_read_with_a_required_body_or_form_is_not_a_read_the_rule_drafts() {
        let read = |parameter: &str| {
            format!(
                r##"{{"swagger": "2.0", "paths": {{"/a/b": {{"get": {{
                  "parameters": [{parameter}],
                  "responses": {{"200": {{"description": "ok", "schema": {{
                    "type": "array", "items": {{"type": "object", "properties": {{"id": {{"type": "string"}}}}}}}}}}}}}}}}}}}}"##
            )
        };
        let body = r#"{"name": "b", "in": "body", "required": true, "schema": {"type": "object"}}"#;
        let form = r#"{"name": "f", "in": "formData", "required": true, "type": "string"}"#;
        let optional = r#"{"name": "f", "in": "formData", "type": "string"}"#;
        for (parameter, drafted) in [(body, false), (form, false), (optional, true)] {
            let found = operations(read(parameter).as_bytes()).unwrap();
            assert_eq!(would_draft(&found[0]), drafted, "{parameter}");
        }
    }

    #[test]
    fn a_3_read_with_a_required_body_is_not_a_read_the_rule_drafts() {
        let read = |body: &str| {
            format!(
                r##"{{"openapi": "3.0.3", "paths": {{"/a/b": {{"get": {{
                  {body}
                  "responses": {{"200": {{"description": "ok", "content": {{"application/json": {{"schema": {{
                    "type": "array", "items": {{"type": "object", "properties": {{"id": {{"type": "string"}}}}}}}}}}}}}}}}}}}}}}}}"##
            )
        };
        let content = r#""content": {"application/json": {"schema": {"type": "object"}}}"#;
        for (body, drafted) in [
            (
                format!(r#""requestBody": {{"required": true, {content}}},"#),
                false,
            ),
            (
                format!(r#""requestBody": {{"required": false, {content}}},"#),
                true,
            ),
            (format!(r#""requestBody": {{{content}}},"#), true),
            (String::new(), true),
        ] {
            let found = operations(read(&body).as_bytes()).unwrap();
            assert_eq!(would_draft(&found[0]), drafted, "{body}");
        }
    }

    /// A description of GETs, each `(path, operationId)`, in the order given.
    fn with_ids(operations: &[(&str, Option<&str>)]) -> Vec<DescribedOperation> {
        let reads: Vec<String> = operations
            .iter()
            .map(|(path, id)| {
                let id = id.map_or_else(String::new, |id| format!(r#""operationId": "{id}","#));
                format!(
                    r#""{path}": {{"get": {{{id} "responses": {{"200": {{"description": "ok"}}}}}}}}"#
                )
            })
            .collect();
        let text = format!(
            r#"{{"openapi": "3.0.3", "paths": {{{}}}}}"#,
            reads.join(",")
        );
        super::operations(text.as_bytes()).unwrap()
    }

    fn endpoint_names(found: &[DescribedOperation]) -> std::collections::BTreeMap<String, String> {
        found
            .iter()
            .map(|read| {
                (
                    read.path.clone(),
                    read.endpoint.clone().unwrap_or_else(|| "(held)".to_owned()),
                )
            })
            .collect()
    }

    #[test]
    fn two_operations_with_one_operation_id_take_two_endpoint_names_in_any_order() {
        let forward = with_ids(&[
            ("/a", Some("list")),
            ("/b", Some("list")),
            ("/c", Some("one")),
        ]);
        let reversed = with_ids(&[
            ("/c", Some("one")),
            ("/b", Some("list")),
            ("/a", Some("list")),
        ]);
        let names = endpoint_names(&forward);
        assert_eq!(names["/a"], "get_a");
        assert_eq!(names["/b"], "get_b");
        assert_eq!(names["/c"], "one");
        assert!(forward.iter().all(|read| !read.endpoint_shared));
        assert_eq!(names, endpoint_names(&reversed));
    }

    #[test]
    fn operations_whose_fallback_name_collides_too_are_each_held_and_none_is_merged() {
        // `/a/b`, `/a_b`, and `/a.b` all make `get_a_b` from method and path.
        let forward = with_ids(&[("/a/b", Some("x")), ("/a_b", Some("x")), ("/a.b", None)]);
        let reversed = with_ids(&[("/a.b", None), ("/a_b", Some("x")), ("/a/b", Some("x"))]);
        for found in [&forward, &reversed] {
            assert_eq!(found.len(), 3);
            assert!(found.iter().all(|read| read.endpoint_shared));
            assert!(found.iter().all(|read| read.endpoint.is_none()));
        }
        // An operation with a name of its own is not held by another's trouble.
        let mixed = with_ids(&[("/a/b", Some("x")), ("/a_b", Some("x")), ("/c", Some("y"))]);
        assert_eq!(endpoint_names(&mixed)["/c"], "y");
    }

    #[test]
    fn a_fallback_name_that_is_another_operations_is_held_for_both() {
        // `/d` falls back to `get_d`, which `/e` has by its own id.
        let found = with_ids(&[
            ("/d", Some("dup")),
            ("/f", Some("dup")),
            ("/e", Some("get_d")),
        ]);
        let names = endpoint_names(&found);
        assert_eq!(names["/d"], "(held)");
        assert_eq!(names["/e"], "(held)");
        assert_eq!(names["/f"], "get_f");
    }

    #[test]
    fn a_list_of_media_types_that_is_not_a_list_of_text_naming_json_is_not_json() {
        let with = |produces: &str| {
            format!(
                r##"{{"swagger": "2.0", "paths": {{"/a": {{"get": {{{produces}
                  "responses": {{"200": {{"description": "ok", "schema": {{"type": "string"}}}}}}}}}}}}}}"##
            )
        };
        let answered = |produces: &str| {
            operations(with(produces).as_bytes()).unwrap()[0]
                .response
                .is_some()
        };
        assert!(answered(""));
        assert!(answered(r#""produces": ["application/json"],"#));
        assert!(!answered(r#""produces": "application/json","#));
        assert!(!answered(r#""produces": {"application/json": true},"#));
        assert!(!answered(r#""produces": [],"#));
        assert!(!answered(r#""produces": ["application/json", 5],"#));
        // An empty list of the operation replaces the document's.
        let replaced = r##"{"swagger": "2.0", "produces": ["application/json"], "paths": {"/a": {"get": {
          "produces": [], "responses": {"200": {"description": "ok", "schema": {"type": "string"}}}}}}}"##;
        assert!(
            operations(replaced.as_bytes()).unwrap()[0]
                .response
                .is_none()
        );
    }

    #[test]
    fn lists_of_media_types_and_names_of_many_reads_are_counted_as_work() {
        // A document-level list of many entries, with many operations: read
        // once, so the work stays inside the bound.
        let many = vec![r#""text/plain""#; 5000].join(",");
        let paths: Vec<String> = (0..300)
            .map(|index| {
                format!(
                    r#""/p{index}": {{"get": {{"responses": {{"200": {{"description": "ok", "schema": {{"type": "string"}}}}}}}}}}"#
                )
            })
            .collect();
        let document = format!(
            r#"{{"swagger": "2.0", "produces": [{many}], "paths": {{{}}}}}"#,
            paths.join(",")
        );
        match operations(document.as_bytes()) {
            Ok(found) => assert_eq!(found.len(), 300),
            Err(failure) => assert_eq!(failure.code(), &codes::DESCRIPTION_TOO_LARGE),
        }
        // Each operation's own list is charged for each entry.
        let own: Vec<String> = (0..300)
            .map(|index| {
                format!(
                    r#""/p{index}": {{"get": {{"produces": [{many}], "responses": {{"200": {{"description": "ok", "schema": {{"type": "string"}}}}}}}}}}"#
                )
            })
            .collect();
        let document = format!(r#"{{"swagger": "2.0", "paths": {{{}}}}}"#, own.join(","));
        match operations(document.as_bytes()) {
            Ok(found) => assert_eq!(found.len(), 300),
            Err(failure) => assert_eq!(failure.code(), &codes::DESCRIPTION_TOO_LARGE),
        }
        // Many reads whose names all collide, grown one word at a time.
        let reads: Vec<String> = (0..2000).map(|index| format!("/a{index}/x/y")).collect();
        let reads: Vec<&str> = reads.iter().map(String::as_str).collect();
        match operations(reads_at(&reads).as_bytes()) {
            Ok(found) => {
                let names: std::collections::BTreeSet<_> = found
                    .iter()
                    .filter_map(|read| read.object.clone())
                    .collect();
                assert_eq!(names.len(), found.len());
            }
            Err(failure) => assert_eq!(failure.code(), &codes::DESCRIPTION_TOO_LARGE),
        }
    }

    /// A Swagger 2.0 document, small, in the form the 2.0 specification
    /// writes: `definitions`, a response's own `schema`, a parameter's own
    /// type, a body parameter, and a form.
    const SWAGGER: &str = r##"{
      "swagger": "2.0",
      "info": {"title": "Example order entry", "version": "1"},
      "basePath": "/api",
      "produces": ["application/json"],
      "paths": {
        "/orders/{orderId}": {
          "parameters": [{"name": "orderId", "in": "path", "required": true, "type": "string"}],
          "get": {
            "operationId": "getOrder",
            "parameters": [
              {"name": "unit", "in": "query", "type": "integer", "format": "int32"},
              {"name": "sort", "in": "query", "type": "array", "items": {"type": "string"}}],
            "responses": {
              "200": {"description": "ok", "schema": {"$ref": "#/definitions/Order"}},
              "404": {"$ref": "#/responses/Missing"}
            }
          },
          "put": {
            "operationId": "putOrder",
            "consumes": ["application/json"],
            "parameters": [{"name": "body", "in": "body", "required": true,
                            "schema": {"$ref": "#/definitions/Order"}}],
            "responses": {"200": {"$ref": "#/responses/Done"}}
          }
        },
        "/orders/{orderId}/upload": {
          "post": {
            "operationId": "uploadNote",
            "consumes": ["multipart/form-data"],
            "parameters": [{"name": "note", "in": "formData", "type": "string"}],
            "responses": {"200": {"description": "ok"}}
          }
        },
        "/report": {
          "get": {
            "produces": ["text/csv"],
            "responses": {"200": {"description": "ok", "schema": {"type": "string"}}}
          }
        }
      },
      "definitions": {
        "Order": {
          "type": "object", "required": ["order_id"],
          "properties": {
            "order_id": {"type": "string"},
            "status": {"type": "string", "enum": ["draft", "submitted"]},
            "items": {"type": "array", "items": {"$ref": "#/definitions/Item"}}
          }},
        "Item": {"type": "object", "properties": {"item_code": {"type": "string"}}}
      },
      "responses": {
        "Missing": {"description": "No such order"},
        "Done": {"description": "Done", "schema": {"$ref": "#/definitions/Item"}}
      }
    }"##;

    #[test]
    fn a_swagger_2_description_is_read_from_its_own_places_into_the_same_operations() {
        let found = operations(SWAGGER.as_bytes()).unwrap();
        assert_eq!(found.len(), 4);
        let (get, put, upload, report) = (&found[0], &found[1], &found[2], &found[3]);
        assert_eq!(get.name(), "get_order");
        // `basePath` is not read: the path stays as the description writes it.
        assert_eq!(get.path, "/orders/{orderId}");
        assert_eq!(get.template_path().as_deref(), Some("/orders/{order_id}"));
        // A shared response is followed, and a 404 is absent.
        assert_eq!(
            (get.ok.as_slice(), get.absent.as_slice()),
            (&[200][..], &[404][..])
        );
        // A response's own `schema` is the answer, references into
        // `definitions` followed, and decision 0297 applies as for 3.x.
        let answer = get.response.as_ref().unwrap().to_text();
        assert!(
            answer.contains(r#""order_id":{"type":"string"}"#),
            "{answer}"
        );
        assert!(
            answer.contains(r#""status":{"type":"string","nullable":true"#),
            "{answer}"
        );
        assert!(
            answer.contains(r#""item_code":{"type":"string","nullable":true}"#),
            "{answer}"
        );
        // A parameter states its type in place, the path item's and the
        // operation's own.
        let kinds: Vec<(&str, &str, bool)> = get
            .parameters
            .iter()
            .map(|p| (p.name.as_str(), p.kind.as_str(), p.required))
            .collect();
        assert_eq!(
            kinds,
            [
                ("orderId", "string", true),
                ("unit", "integer", false),
                ("sort", "array", false)
            ]
        );
        // A body parameter is the request, drafted without the answer rule.
        let request = put.request.as_ref().unwrap().to_text();
        assert!(
            request.contains(r#""order_id""#) && !request.contains("nullable"),
            "{request}"
        );
        assert!(!put.safe && get.safe);
        // A shared response with a schema is an answer too.
        assert!(put.response.is_some());
        // A form is a body this pack cannot draft: no shape, as for a 3.x
        // body that is not JSON.
        assert!(upload.request.is_none() && upload.response.is_none());
        // An answer the operation does not give as JSON has no shape.
        assert!(report.response.is_none());
        assert_eq!(
            DescribedOperation::from_found(&get.found()).as_ref(),
            Some(get)
        );
    }

    #[test]
    fn a_swagger_2_description_with_no_list_of_types_answers_json_and_yaml_is_read() {
        let yaml = "swagger: '2.0'\npaths:\n  /orders:\n    get:\n      responses:\n        '200':\n          description: ok\n          schema: {type: object, properties: {id: {type: string}}}\n";
        let found = operations(yaml.as_bytes()).unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].response.is_some());
        let plain = "swagger: 2.0\npaths:\n  /orders:\n    get:\n      responses:\n        '200': {description: ok}\n";
        assert_eq!(operations(plain.as_bytes()).unwrap().len(), 1);
    }

    #[test]
    fn a_swagger_2_description_is_held_to_the_same_bounds() {
        let swagger = |definitions: &str| {
            format!(
                r##"{{"swagger": "2.0", "paths": {{
                  "/wide": {{"get": {{"responses": {{"200": {{"description": "ok",
                    "schema": {{"$ref": "#/definitions/S0"}}}}}}}}}}
                }}, "definitions": {{{definitions}}}}}"##
            )
        };
        let doubling = |levels: usize| {
            let mut all: Vec<String> = (0..levels)
                .map(|level| {
                    let next = level + 1;
                    format!(
                        r##""S{level}": {{"type": "object", "properties": {{
                          "a": {{"$ref": "#/definitions/S{next}"}},
                          "b": {{"$ref": "#/definitions/S{next}"}}}}}}"##
                    )
                })
                .collect();
            all.push(format!(r#""S{levels}": {{"type": "string"}}"#));
            swagger(&all.join(","))
        };
        for levels in [20, 30] {
            let failure = operations(doubling(levels).as_bytes()).unwrap_err();
            assert_eq!(failure.code(), &codes::DESCRIPTION_TOO_LARGE, "{levels}");
        }
        assert!(
            operations(doubling(4).as_bytes()).unwrap()[0]
                .response
                .is_some()
        );
        let mut deep: Vec<String> = (0..40)
            .map(|level| {
                let next = level + 1;
                format!(
                    r##""S{level}": {{"type": "array", "items": {{"$ref": "#/definitions/S{next}"}}}}"##
                )
            })
            .collect();
        deep.push(r#""S40": {"type": "string"}"#.to_owned());
        let failure = operations(swagger(&deep.join(",")).as_bytes()).unwrap_err();
        assert_eq!(failure.code(), &codes::DESCRIPTION_TOO_LARGE);
        assert!(failure.message().contains("32 levels"), "{failure}");
    }

    /// A description whose one operation answers with the schema `S0`.
    fn described(schemas: &str) -> String {
        format!(
            r##"{{"openapi": "3.0.3", "paths": {{
              "/plain": {{"get": {{"responses": {{"200": {{"content": {{"application/json": {{"schema": {{"type": "string"}}}}}}}}}}}}}},
              "/wide": {{"get": {{"responses": {{"200": {{"content": {{"application/json": {{"schema": {{"$ref": "#/components/schemas/S0"}}}}}}}}}}}}}}
            }}, "components": {{"schemas": {{{schemas}}}}}}}"##
        )
    }

    /// Package 4.4: a reference to another file is never followed, and the
    /// answer it would shape is marked as such, so its draft names why;
    /// a reference inside the document is followed as before.
    #[test]
    fn a_reference_to_another_file_is_not_followed_and_is_named() {
        let text = r##"{"openapi": "3.0.3", "paths": {
          "/outside": {"get": {"responses": {"200": {"content": {"application/json": {"schema": {"$ref": "shared.json#/Order"}}}}}}},
          "/inside": {"get": {"responses": {"200": {"content": {"application/json": {"schema": {"$ref": "#/components/schemas/Order"}}}}}}}
        }, "components": {"schemas": {"Order": {"type": "object", "properties": {"order_id": {"type": "string"}}}}}}"##;
        let found = operations(text.as_bytes()).unwrap();
        let outside = found.iter().find(|op| op.path == "/outside").unwrap();
        assert!(outside.response.is_none() && outside.response_outside);
        let inside = found.iter().find(|op| op.path == "/inside").unwrap();
        assert!(inside.response.is_some() && !inside.response_outside);
        // The mark survives the finding.
        let read = DescribedOperation::from_found(&outside.found()).unwrap();
        assert!(read.response_outside);
    }

    #[test]
    fn a_description_that_grows_past_the_bounds_is_refused_by_name_and_not_expanded() {
        // Each schema names the next one twice: a few kilobytes that are a
        // million parts once the references are followed, and far more
        // with every level added.
        let doubling = |levels: usize| {
            let mut schemas: Vec<String> = (0..levels)
                .map(|level| {
                    let next = level + 1;
                    format!(
                        r##""S{level}": {{"type": "object", "properties": {{
                          "a": {{"$ref": "#/components/schemas/S{next}"}},
                          "b": {{"$ref": "#/components/schemas/S{next}"}}}}}}"##
                    )
                })
                .collect();
            schemas.push(format!(r#""S{levels}": {{"type": "string"}}"#));
            described(&schemas.join(","))
        };
        for levels in [20, 30] {
            let failure = operations(doubling(levels).as_bytes()).unwrap_err();
            assert_eq!(failure.code(), &codes::DESCRIPTION_TOO_LARGE, "{levels}");
            assert_eq!(failure.class(), a2m_core::FailureClass::Input);
        }
        // A small one of the same form is read.
        let found = operations(doubling(4).as_bytes()).unwrap();
        assert!(found[1].response.is_some());

        // A schema that nests deeper than the import follows.
        let mut schemas: Vec<String> = (0..40)
            .map(|level| {
                let next = level + 1;
                format!(
                    r##""S{level}": {{"type": "array", "items": {{"$ref": "#/components/schemas/S{next}"}}}}"##
                )
            })
            .collect();
        schemas.push(r#""S40": {"type": "string"}"#.to_owned());
        let failure = operations(described(&schemas.join(",")).as_bytes()).unwrap_err();
        assert_eq!(failure.code(), &codes::DESCRIPTION_TOO_LARGE);
        assert!(failure.message().contains("32 levels"), "{failure}");

        // A schema that holds itself has no shape in this pack's words. It
        // is found at once, costs nothing more, and only its own body is
        // left without a shape: the other operations are read.
        let holds_itself = described(
            r##""S0": {"type": "object", "properties": {
              "name": {"type": "string"},
              "left": {"$ref": "#/components/schemas/S0"},
              "right": {"$ref": "#/components/schemas/S1"}}},
            "S1": {"type": "array", "items": {"$ref": "#/components/schemas/S0"}}"##,
        );
        let found = operations(holds_itself.as_bytes()).unwrap();
        assert_eq!(found.len(), 2);
        assert!(found[0].response.is_some() && found[1].response.is_none());

        // A name is required once, however often the description lists it.
        let repeated = described(&format!(
            r#""S0": {{"type": "object", "required": [{}], "properties": {{"a": {{"type": "string"}}}}}}"#,
            vec![r#""a""#; 5000].join(",")
        ));
        let found = operations(repeated.as_bytes()).unwrap();
        let shape = found[1].response.as_ref().unwrap().to_text();
        assert_eq!(shape.matches(r#""a""#).count(), 2, "{shape}");
    }

    #[test]
    fn a_parameter_keeps_the_descriptions_own_example_and_security_is_read_by_the_formats_rule() {
        let described = r##"{"openapi": "3.1.0", "security": [{"key": []}], "paths": {
          "/stock/{itemCode}": {
            "parameters": [{"name": "itemCode", "in": "path", "required": true,
                            "schema": {"type": "string", "example": "L-101"}}],
            "get": {"security": [], "parameters": [
                {"name": "unit", "in": "query", "schema": {"type": "integer"}, "example": 2.5}],
              "responses": {"200": {"content": {"application/json": {"schema": {
                "type": "object", "properties": {"state": {"enum": ["in", "out"]}}}}}}}},
            "head": {"responses": {"200": {"description": "ok"}}}
          }}}"##;
        let found = operations(described.as_bytes()).unwrap();
        let (get, head) = (&found[0], &found[1]);
        assert_eq!(get.parameters.len(), 2);
        assert_eq!(get.parameters[0].name, "itemCode");
        assert!(get.parameters[0].required);
        assert_eq!(
            get.parameters[0].example,
            Some(crate::json::Json::Text("L-101".to_owned()))
        );
        // An example that is not plain text or a whole number is not kept.
        assert!(!get.parameters[1].required && get.parameters[1].example.is_none());
        // The operation's empty `security` lifts the description's; the HEAD
        // keeps the description's own.
        assert!(get.open && !head.open);
        // An `enum` of text with no `type` is text.
        let shape = get.response.as_ref().unwrap().to_text();
        assert!(shape.contains(r#""state":{"type":"string""#), "{shape}");
        assert_eq!(
            DescribedOperation::from_found(&get.found()).as_ref(),
            Some(get)
        );
        // A finding of the first format is still read, as stating no
        // parameter and needing a credential.
        let first = get.found().replace(r#""format":"2""#, r#""format":"1""#);
        let first = first
            .split(r#","parameters""#)
            .next()
            .map(|head| format!("{head}}}"))
            .unwrap();
        let read = DescribedOperation::from_found(&first).unwrap();
        assert!(read.parameters.is_empty() && !read.open);
    }

    /// The answer shape of a description whose one operation answers `S0`.
    fn answer_of(schemas: &str) -> String {
        let found = operations(described(schemas).as_bytes()).unwrap();
        found[1].response.as_ref().unwrap().to_text()
    }

    #[test]
    fn a_property_the_description_does_not_require_is_drafted_as_possibly_null() {
        let shape = answer_of(
            r##""S0": {"type": "object", "required": ["id", "tag"], "properties": {
              "id": {"type": "string"},
              "tag": {"type": "string", "nullable": true},
              "owner": {"type": "object", "required": ["name"], "properties": {
                "name": {"type": "string"},
                "nick": {"type": "string"},
                "address": {"type": "object", "properties": {"city": {"type": "string"}}}}},
              "labels": {"type": "array", "items": {"type": "string"}},
              "note": {"type": "string"}}}"##,
        );
        // Required, not nullable: as drafted before. Required and nullable
        // keeps it.
        assert!(shape.contains(r#""id":{"type":"string"}"#), "{shape}");
        assert!(
            shape.contains(r#""tag":{"type":"string","nullable":true}"#),
            "{shape}"
        );
        // Not required, of each kind, at depth 1.
        assert!(
            shape.contains(r#""owner":{"type":"object","nullable":true,"required":["name"]"#),
            "{shape}"
        );
        assert!(
            shape
                .contains(r#""labels":{"type":"array","nullable":true,"items":{"type":"string"}}"#),
            "{shape}"
        );
        assert!(
            shape.contains(r#""note":{"type":"string","nullable":true}"#),
            "{shape}"
        );
        // Nested: a required name stays; the others are possibly null.
        assert!(shape.contains(r#""name":{"type":"string"}"#), "{shape}");
        assert!(
            shape.contains(r#""nick":{"type":"string","nullable":true}"#),
            "{shape}"
        );
        assert!(
            shape.contains(r#""address":{"type":"object","nullable":true,"properties":{"city":{"type":"string","nullable":true}}}"#),
            "{shape}"
        );
    }

    /// What the drafted shape then says of an answer, and of a path an output
    /// is taken from: through a property the description does not require,
    /// the value is not certain to be there, so the output may be absent.
    #[test]
    fn a_drafted_shape_takes_null_and_missing_for_an_optional_property_and_not_a_wrong_kind() {
        use crate::json::Json;
        use crate::shape::Shape;
        let drafted = answer_of(
            r##""S0": {"type": "object", "required": ["id"], "properties": {
              "id": {"type": "string"},
              "owner": {"type": "object", "required": ["name"], "properties": {
                "name": {"type": "string"}}}}}"##,
        );
        let docs = a2m_files::parse_yaml(
            &a2m_files::RelPath::new("endpoints/x.yaml").unwrap(),
            &drafted,
        )
        .unwrap();
        let shape = Shape::from_doc(&docs[0]).unwrap();
        let path = |text: &str| -> Vec<String> { text.split('.').map(str::to_owned).collect() };
        assert!(shape.at_path(&path("id")).unwrap().1);
        assert!(!shape.at_path(&path("owner")).unwrap().1);
        // `name` is required of an owner, yet the owner may be null.
        assert!(!shape.at_path(&path("owner.name")).unwrap().1);
        let fits = |text: &str| shape.check(&Json::parse(text.as_bytes()).unwrap());
        assert_eq!(fits(r#"{"id":"a","owner":{"name":"n"}}"#), Ok(()));
        assert_eq!(fits(r#"{"id":"a","owner":null}"#), Ok(()));
        assert_eq!(fits(r#"{"id":"a"}"#), Ok(()));
        assert!(fits(r#"{"id":"a","owner":"text"}"#).is_err());
        assert!(fits(r#"{"id":null,"owner":null}"#).is_err());
        assert!(fits(r#"{"id":"a","owner":{"name":null}}"#).is_err());
    }

    #[test]
    fn a_request_body_and_a_parameter_are_drafted_without_the_answer_rule() {
        let found = operations(DESCRIPTION.as_bytes()).unwrap();
        let request = found[1].request.as_ref().unwrap().to_text();
        assert!(!request.contains("nullable"), "{request}");
    }

    #[test]
    fn a_name_is_made_mechanically_from_the_systems_own_word() {
        assert_eq!(snake("getOrderById"), "get_order_by_id");
        assert_eq!(
            snake("GET /api/orders/{orderId}"),
            "get_api_orders_order_id"
        );
        assert_eq!(snake("Orders.List"), "orders_list");
    }

    #[test]
    fn each_declared_security_scheme_is_classified_by_a_fixed_rule() {
        let described = r##"{"openapi": "3.0.3", "paths": {}, "components": {"securitySchemes": {
            "a": {"type": "apiKey", "in": "header", "name": "X-Key"},
            "b": {"type": "http", "scheme": "Bearer"},
            "c": {"type": "http", "scheme": "basic"},
            "d": {"type": "oauth2", "flows": {"clientCredentials": {}}},
            "e": {"type": "oauth2", "flows": {"password": {}}},
            "f": {"type": "openIdConnect"},
            "g": {"type": "mutualTLS"},
            "h": {"type": "apiKey", "in": "query", "name": "k"}
        }}}"##;
        assert_eq!(
            super::sign_in_signs(described.as_bytes()).unwrap(),
            [
                "api_key",
                "bearer_token",
                "http_basic",
                "client_credentials",
                "unrecognized",
                "authorization_code",
                "client_certificate"
            ]
        );
        let two = r##"{"swagger": "2.0", "paths": {}, "securityDefinitions": {
            "a": {"type": "basic"},
            "b": {"type": "oauth2", "flow": "application"}}}"##;
        assert_eq!(
            super::sign_in_signs(two.as_bytes()).unwrap(),
            ["http_basic", "client_credentials"]
        );
        let none = br#"{"openapi": "3.0.3", "paths": {}}"#;
        assert!(super::sign_in_signs(none).unwrap().is_empty());
    }
}
