//! Reading a published OpenAPI description into what it says about each
//! operation, and the drafting rule from such a finding to an endpoint
//! entry.
//!
//! A description is a claim, not an observation. Nothing here marks an
//! operation as a read unless the description's method is one HTTP marks
//! safe, and nothing is written into a draft that the description does not
//! state: what it leaves out is an open question for the developer.
//!
//! Accepted: OpenAPI 3.0 and 3.1, as JSON or YAML, with references inside
//! the document. A reference to another file is not followed.
//!
//! A description is text the system supplies, so reading it is bounded:
//! how deep a schema nests, how many steps the import takes, and how many
//! bytes of shapes it writes. Past a bound the import is refused by name,
//! `input`/`description_too_large`, and nothing is made of a part of it.

use std::collections::{BTreeMap, BTreeSet};

use a2m_core::{Failure, FileKindName, Finding, FindingKind, Redact};
use a2m_files::{Doc, DraftContext, DraftDoc, DraftValue, DraftingRule, Located, RelPath};

use crate::codes;
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
        "Give an OpenAPI 3.0 or 3.1 document, as JSON or YAML, that holds its own schemas.",
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
}

impl<'a> Walk<'a> {
    fn new(root: &'a Json) -> Self {
        Self {
            root,
            steps: MOST_STEPS,
            bytes: MOST_SHAPE_BYTES,
            open: Vec::new(),
            seen: BTreeMap::new(),
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
                    for (name, property) in properties {
                        self.step(1)?;
                        self.write(length(name))?;
                        let Some(inner) = self.shape_of(property, depth + 1)? else {
                            return Ok(None);
                        };
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

    /// The JSON schema of one body of the description, as a shape.
    fn body_shape(&mut self, holder: &'a Json) -> Result<Option<Json>, Failure> {
        let Some(holder) = self.resolve(holder)? else {
            return Ok(None);
        };
        let schema = holder
            .get("content")
            .and_then(|content| content.get("application/json"))
            .and_then(|json| json.get("schema"));
        match schema {
            Some(schema) => self.shape_of(schema, 0),
            None => Ok(None),
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
                if !["path", "query", "header", "cookie"].contains(&place) {
                    continue;
                }
                let schema = match parameter.get("schema") {
                    Some(schema) => self.shape_of(schema, 0)?,
                    None => None,
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
        })
    }

    /// The name of the endpoint entry drafted for the operation.
    pub(crate) fn name(&self) -> String {
        let from_id = self.operation_id.as_deref().map(snake).unwrap_or_default();
        let name = if from_id.starts_with(|c: char| c.is_ascii_lowercase()) {
            from_id
        } else {
            snake(&format!("{} {}", self.method, self.path))
        };
        name.chars().take(60).collect()
    }

    /// The path as a template of this pack: each placeholder a lowercase
    /// name. `None` when the description's path is not one.
    pub(crate) fn template_path(&self) -> Option<String> {
        let mut out = String::new();
        for part in self.path.split('/').skip(1) {
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
    if !version.is_some_and(|version| version.starts_with("3.0") || version.starts_with("3.1")) {
        return Err(unreadable("it does not state `openapi: 3.0` or `3.1`"));
    }
    let Some(Json::Object(paths)) = root.get("paths") else {
        return Err(unreadable("it has no `paths`"));
    };
    let mut walk = Walk::new(&root);
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
            if let Some(Json::Object(responses)) = operation.get("responses") {
                for (status, body) in responses {
                    walk.step(1)?;
                    let Ok(status) = status.parse::<u16>() else {
                        continue;
                    };
                    if (200..300).contains(&status) {
                        if ok.is_empty() {
                            response = walk.body_shape(body)?;
                        }
                        ok.push(status);
                    } else if status == 404 {
                        absent.push(status);
                    }
                }
            }
            let request = match operation.get("requestBody") {
                Some(body) => walk.body_shape(body)?,
                None => None,
            };
            let parameters =
                walk.parameters(item_parameters(resolved), operation.get("parameters"))?;
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
            });
        }
    }
    Ok(found)
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

fn draft_value(json: &Json) -> DraftValue {
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

/// From a `described_operation` finding to an endpoint entry.
pub(crate) struct DescribedOperationRule {
    pub(crate) kind: FindingKind,
}

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
        let name = operation.name();
        // The surface is the one whose address the description was read
        // for, when the target already declares it.
        let origin = match finding.subject() {
            a2m_core::Subject::Address(address) => Some(address.origin()),
            _ => None,
        };
        let surface = cx
            .existing()
            .and_then(|sources| sources.file("target.yaml"))
            .and_then(|file| file.docs.first())
            .and_then(|target| target.get("surfaces"))
            .and_then(|surfaces| {
                surfaces.entries().iter().find(|entry| {
                    entry.value.text("kind") == Some(HTTP_SURFACE.as_str())
                        && origin.is_some_and(|origin| {
                            entry
                                .value
                                .text(crate::ADDRESS_KEY)
                                .is_some_and(|base| base.trim_end_matches('/') == origin)
                        })
                })
            })
            .map_or_else(
                || {
                    cx.open_question(&format!(
                        "Which `http` surface does the endpoint `{name}` live on?"
                    ))
                },
                |entry| DraftValue::text(&entry.key.value),
            );
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
            ("endpoint", DraftValue::text(&name)),
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
                    cx.open_question(&format!(
                        "What is the shape of the answer of `{name}`? The description states none this pack can follow."
                    ))
                },
                draft_value,
            ),
        ));
        if !responses.is_empty() {
            entry.push(("responses", DraftValue::map(responses)));
        }
        Ok(vec![DraftDoc {
            path: RelPath::new(&format!("endpoints/{name}.yaml"))?,
            body: DraftValue::map(entry),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::{DescribedOperation, operations, snake};
    use crate::codes;

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
            response.contains(r#""items":{"type":"array","items":{"type":"object""#),
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
            br#"{"swagger": "2.0", "paths": {}}"#,
            br#"{"openapi": "3.0.0"}"#,
            b"\xff\xfe",
        ] {
            let failure = operations(bad).unwrap_err();
            assert_eq!(failure.code(), &codes::DESCRIPTION_UNREADABLE);
        }
        let yaml = b"openapi: 3.1.0\npaths:\n  /orders:\n    get:\n      responses:\n        '200': {description: ok}\n";
        assert_eq!(operations(yaml).unwrap().len(), 1);
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

    #[test]
    fn a_name_is_made_mechanically_from_the_systems_own_word() {
        assert_eq!(snake("getOrderById"), "get_order_by_id");
        assert_eq!(
            snake("GET /api/orders/{orderId}"),
            "get_api_orders_order_id"
        );
        assert_eq!(snake("Orders.List"), "orders_list");
    }
}
