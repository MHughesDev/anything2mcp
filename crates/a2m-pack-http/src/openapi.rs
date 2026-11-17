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

use a2m_core::{Failure, FailureClass, FileKindName, Finding, FindingKind};
use a2m_files::{Doc, DraftContext, DraftDoc, DraftValue, DraftingRule, Located, RelPath};

use crate::codes;
use crate::endpoints::{ENDPOINT_KIND, HTTP_SURFACE};
use crate::json::Json;
use crate::template::parse_path;

/// The version of the shape of a `described_operation` finding's `found`.
pub(crate) const FOUND_FORMAT: &str = "1";

/// How far a chain of references is followed.
const DEEPEST_REFERENCE: usize = 32;

/// The finding kind this module makes and drafts from.
pub(crate) fn described_operation() -> Result<FindingKind, Failure> {
    Ok(FindingKind::new("described_operation")?)
}

fn unreadable(why: &str) -> Failure {
    codes::failure(
        FailureClass::Input,
        codes::DESCRIPTION_UNREADABLE,
        format!("The description cannot be read as OpenAPI: {why}."),
        "Give an OpenAPI 3.0 or 3.1 document, as JSON or YAML, that holds its own schemas.",
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

/// Reads a description's bytes as one document, JSON or YAML.
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

/// Follows a reference inside the document. `None` for a reference to
/// another file, or one that leads nowhere.
fn resolve<'a>(root: &'a Json, mut json: &'a Json) -> Option<&'a Json> {
    for _ in 0..DEEPEST_REFERENCE {
        let Some(reference) = json.get("$ref").and_then(Json::as_text) else {
            return Some(json);
        };
        let pointer = reference.strip_prefix("#/")?;
        json = root;
        for part in pointer.split('/') {
            let name = part.replace("~1", "/").replace("~0", "~");
            json = json.get(&name)?;
        }
    }
    None
}

/// A schema of the description as a shape in this pack's own words, or
/// `None` where the description's schema cannot be followed. A rule the
/// pack does not check is left out, so a draft never claims to check it.
fn shape_of(root: &Json, schema: &Json, depth: usize) -> Option<Json> {
    if depth > DEEPEST_REFERENCE {
        return None;
    }
    let schema = resolve(root, schema)?;
    let text = |word: &str| Json::Text(word.to_owned());
    let mut nullable = schema.get("nullable") == Some(&Json::Bool(true));
    let kind = match schema.get("type") {
        Some(Json::Text(kind)) => kind.clone(),
        // OpenAPI 3.1 writes a value that may be absent as a list of types.
        Some(Json::List(kinds)) => {
            let named: Vec<&str> = kinds.iter().filter_map(Json::as_text).collect();
            nullable = nullable || named.contains(&"null");
            match named
                .iter()
                .filter(|kind| **kind != "null")
                .collect::<Vec<_>>()[..]
            {
                [one] => (*one).to_owned(),
                _ => "any".to_owned(),
            }
        }
        _ => "any".to_owned(),
    };
    let known = [
        "object", "array", "string", "integer", "number", "boolean", "null",
    ];
    let kind = if known.contains(&kind.as_str()) {
        kind
    } else {
        "any".to_owned()
    };
    let mut shape = vec![("type".to_owned(), text(&kind))];
    if nullable {
        shape.push(("nullable".to_owned(), Json::Bool(true)));
    }
    match kind.as_str() {
        "object" => {
            if let Some(Json::Object(properties)) = schema.get("properties") {
                let mut converted = Vec::new();
                for (name, property) in properties {
                    converted.push((name.clone(), shape_of(root, property, depth + 1)?));
                }
                let listed = |name: &Json| {
                    name.as_text()
                        .is_some_and(|name| converted.iter().any(|(known, _)| known == name))
                };
                if let Some(Json::List(required)) = schema.get("required") {
                    let required: Vec<Json> = required
                        .iter()
                        .filter(|name| listed(name))
                        .cloned()
                        .collect();
                    if !required.is_empty() {
                        shape.push(("required".to_owned(), Json::List(required)));
                    }
                }
                shape.push(("properties".to_owned(), Json::Object(converted)));
            }
        }
        "array" => {
            if let Some(items) = schema.get("items") {
                shape.push(("items".to_owned(), shape_of(root, items, depth + 1)?));
            }
        }
        "string" => {
            if let Some(Json::List(words)) = schema.get("enum")
                && words.iter().all(|word| matches!(word, Json::Text(_)))
            {
                shape.push(("enum".to_owned(), Json::List(words.clone())));
            }
        }
        _ => {}
    }
    Some(Json::Object(shape))
}

/// The JSON schema of one body of the description, as a shape.
fn body_shape(root: &Json, holder: &Json) -> Option<Json> {
    let holder = resolve(root, holder)?;
    let schema = holder
        .get("content")?
        .get("application/json")?
        .get("schema")?;
    shape_of(root, schema, 0)
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

/// What a description says about one operation, as the `found` of a
/// `described_operation` finding: a JSON object with the keys `format`,
/// `method`, `path`, `operation_id`, `safe`, `ok`, `absent`,
/// `request_schema`, and `response_schema`.
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
        ])
        .to_text()
    }

    fn from_found(found: &str) -> Option<Self> {
        let json = Json::parse(found.as_bytes()).ok()?;
        if json.get("format")?.as_text()? != FOUND_FORMAT {
            return None;
        }
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
    let mut found = Vec::new();
    for (path, item) in paths {
        let Some(Json::Object(entries)) = resolve(&root, item) else {
            continue;
        };
        if !path.starts_with('/') {
            continue;
        }
        for (method, operation) in entries {
            let known = ["get", "head", "post", "put", "patch", "delete"];
            if !known.contains(&method.as_str()) {
                continue;
            }
            let mut ok = Vec::new();
            let mut absent = Vec::new();
            let mut response = None;
            if let Some(Json::Object(responses)) = operation.get("responses") {
                for (status, body) in responses {
                    let Ok(status) = status.parse::<u16>() else {
                        continue;
                    };
                    if (200..300).contains(&status) {
                        if ok.is_empty() {
                            response = body_shape(&root, body);
                        }
                        ok.push(status);
                    } else if status == 404 {
                        absent.push(status);
                    }
                }
            }
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
                request: operation
                    .get("requestBody")
                    .and_then(|body| body_shape(&root, body)),
                response,
            });
        }
    }
    Ok(found)
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
            return Err(Failure::new(
                FailureClass::Input,
                a2m_core::FailureCode::FINDING_INVALID,
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
            // A read only for a method the description marks safe.
            (
                "effect",
                DraftValue::text(if operation.safe { "read" } else { "write" }),
            ),
        ];
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
