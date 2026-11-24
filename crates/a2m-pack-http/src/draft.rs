//! The drafting rule from a described operation to one read operation over
//! the endpoint entry the other rule drafts for it (spec section 303,
//! "Discovery and drafting").
//!
//! A read is drafted for a GET the description marks safe, and for nothing
//! else: a HEAD answers with no body to read, and every other method may
//! write. Its inputs are the path's placeholders, its outputs the plain
//! values the description's answer shape states at its top, or in each
//! item of a list, and its example the one the description itself gives
//! for every placeholder. What the description does not state is not
//! drafted: a parameter it requires outside the path leaves the operation
//! to the developer, an output of a type the rule cannot judge is an open
//! question, and a nested value is left out, since an output is one value
//! or one list of values.

use a2m_core::{Failure, FileKindName, Finding, FindingKind, OperationName};
use a2m_files::{
    DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath, UNTRUSTED_TEXT, left_out_sentence,
    names_a_secret,
};

use crate::json::Json;
use crate::openapi::{DescribedOperation, Parameter, object_words, snake};

/// Named values of a draft, in order.
type Pairs = Vec<(String, DraftValue)>;

/// The kernel's file kind for operations.
const OPERATION: FileKindName = FileKindName::fixed("operation");

/// The field type of a plain value of the answer, from the kind its shape
/// states. Text the system gives is untrusted. `None` for a value that is
/// not one plain value (an object, a list, a null); an open question for a
/// kind the description leaves open.
fn output_type(kind: &str, name: &str, cx: &DraftContext<'_>) -> Option<DraftValue> {
    match kind {
        "string" => Some(DraftValue::text(UNTRUSTED_TEXT)),
        "integer" => Some(DraftValue::text("integer")),
        "number" => Some(DraftValue::text("decimal")),
        "boolean" => Some(DraftValue::text("boolean")),
        "any" => Some(cx.open_question(&format!(
            "Which field type reads the output `{name}`? The description states no type for it."
        ))),
        _ => None,
    }
}

/// The input type of a placeholder, from its schema's kind.
fn input_type(parameter: &Parameter, name: &str, cx: &DraftContext<'_>) -> DraftValue {
    match parameter.kind.as_str() {
        "string" => DraftValue::text("text"),
        "integer" => DraftValue::text("integer"),
        _ => cx.open_question(&format!(
            "Which field type does the input `{name}` take? The description states none this pack can follow."
        )),
    }
}

/// The description's example of a placeholder, when it is of the
/// placeholder's own type.
fn example_of(parameter: &Parameter) -> Option<DraftValue> {
    match (parameter.kind.as_str(), parameter.example.as_ref()?) {
        ("string", Json::Text(text)) => Some(DraftValue::text(text)),
        ("integer", number @ Json::Number(text)) if number.is_whole() => {
            text.parse::<i64>().ok().map(DraftValue::Int)
        }
        _ => None,
    }
}

/// Whether a name the description writes is a certain secret's by the
/// compiler's own rule, as the description writes it or as an output or
/// input is named from it (decision 0302).
fn named_as_secret(word: &str, name: &str) -> bool {
    names_a_secret(word) || names_a_secret(name)
}

/// The plain properties of a record's shape: for each, the name it has as
/// an output, the property as the description writes it, and its kind.
/// A property whose name holds a dot or a star cannot be named in
/// `extract`; one whose name gives no output name, or one already taken,
/// or one that is not one plain value, is left out. Apart, the names of
/// those left out because the compiler takes them for a secret's: a value
/// such a property holds is never an output.
fn plain_outputs_and_secrets(shape: &Json) -> (Vec<(String, &str, &str)>, Vec<String>) {
    let mut found: Vec<(String, &str, &str)> = Vec::new();
    let mut secrets: Vec<String> = Vec::new();
    let Some(Json::Object(properties)) = shape.get("properties") else {
        return (found, secrets);
    };
    for (property, inner) in properties {
        if property.contains(['.', '*']) {
            continue;
        }
        let name = snake(property);
        if !name.starts_with(|c: char| c.is_ascii_lowercase())
            || found.iter().any(|(known, _, _)| *known == name)
        {
            continue;
        }
        let kind = inner.get("type").and_then(Json::as_text).unwrap_or("any");
        if !matches!(kind, "string" | "integer" | "number" | "boolean" | "any") {
            continue;
        }
        if named_as_secret(property, &name) {
            secrets.push(name);
            continue;
        }
        found.push((name, property.as_str(), kind));
    }
    (found, secrets)
}

/// The outputs of a read: each plain property of `shape`, named in the
/// form target files use, with where it is in the answer when the name
/// differs or the answer is a list.
fn outputs(shape: &Json, listed: bool, cx: &DraftContext<'_>) -> (Pairs, Pairs, Vec<String>) {
    let mut outputs: Pairs = Vec::new();
    let mut extract = Vec::new();
    let (plain, secrets) = plain_outputs_and_secrets(shape);
    for (name, property, kind) in plain {
        let Some(ty) = output_type(kind, &name, cx) else {
            continue;
        };
        let mut output = vec![("type", ty)];
        if listed {
            output.push(("list", DraftValue::Bool(true)));
            extract.push((name.clone(), DraftValue::text(&format!("*.{property}"))));
        } else if name != property {
            extract.push((name.clone(), DraftValue::text(property)));
        }
        outputs.push((name, DraftValue::map(output)));
    }
    (outputs, extract, secrets)
}

/// Why a read is not drafted, in this pack's own words: they quote nothing
/// of the description (roadmap package FT11).
mod why {
    pub(super) const NO_TARGET: &str =
        "The target has no `target.yaml` yet, so a tool has no name to be given.";
    pub(super) const PATH_NOT_BINDABLE: &str = "The read's path is not one this rule can bind: every placeholder must be a whole segment of a path that begins with `/`.";
    pub(super) const NO_NAME: &str = "No tool name can be made from the words of the read's path.";
    pub(super) const NO_PARAMETER: &str = "A placeholder of the read's path has no parameter the description states, so its type is not known.";
    pub(super) const SECRET_PLACEHOLDER: &str = "A placeholder of the read's path is named as a secret's. A secret is never an input, so the read is left to a person.";
    pub(super) const SECRET_PARAMETER: &str = "The read requires a parameter outside the path whose name is a secret's. A secret is never an input, so the read is left to a person.";
    pub(super) const OUTSIDE_PATH: &str = "The read requires a parameter outside the path (a query value, a header, a cookie, a body, or a form), which this rule does not draft as an input.";
    pub(super) const NO_ANSWER: &str = "The description states no answer shape for the read.";
    pub(super) const NO_PLAIN_OUTPUT: &str =
        "The answer has no property of one plain value this rule can name as an output.";
    pub(super) const ONLY_SECRETS: &str = "Every plain property of the answer is named as a secret's, so the read would have no output. A secret is never an output.";
}

/// The placeholders of a read's path, in order, each with what the
/// description says of it. `None` when one has no parameter stated, or the
/// read needs a parameter outside the path (a query value, a header, a
/// cookie, a body, or a form): the read is the developer's.
fn placeholders_of(operation: &DescribedOperation) -> Option<Vec<(String, &Parameter)>> {
    placeholders_or_why(operation).ok()
}

/// As [`placeholders_of`], with the reason it gives none.
fn placeholders_or_why(
    operation: &DescribedOperation,
) -> Result<Vec<(String, &Parameter)>, &'static str> {
    let mut placeholders = Vec::new();
    for part in operation.path.split('/') {
        let Some(name) = part.strip_prefix('{').and_then(|p| p.strip_suffix('}')) else {
            continue;
        };
        let parameter = operation
            .parameters
            .iter()
            .find(|known| known.place == "path" && known.name == name)
            .ok_or(why::NO_PARAMETER)?;
        // A placeholder named as a secret makes the read the developer's: a
        // secret is never an input (decision 0302).
        if named_as_secret(name, &snake(name)) {
            return Err(why::SECRET_PLACEHOLDER);
        }
        placeholders.push((snake(name), parameter));
    }
    let outside: Vec<&Parameter> = operation
        .parameters
        .iter()
        .filter(|parameter| parameter.required && parameter.place != "path")
        .collect();
    if outside
        .iter()
        .any(|parameter| named_as_secret(&parameter.name, &snake(&parameter.name)))
    {
        return Err(why::SECRET_PARAMETER);
    }
    if !outside.is_empty() {
        return Err(why::OUTSIDE_PATH);
    }
    Ok(placeholders)
}

/// The record of an answer, and whether the answer is a list of them.
fn record_of(shape: &Json) -> Option<(&Json, bool)> {
    if shape.get("type").and_then(Json::as_text) == Some("array") {
        shape.get("items").map(|items| (items, true))
    } else {
        Some((shape, false))
    }
}

/// Whether the rule drafts a read operation for this description's
/// operation, whatever the target is named. The conditions are those
/// `draft` goes by; the import asks it, so that a read the rule leaves to
/// the developer does not make another read's name longer.
pub(crate) fn would_draft(operation: &DescribedOperation) -> bool {
    is_read(operation) && why_not_drafted(operation).is_none()
}

/// Whether the description's operation is one the rule reads at all: a GET
/// it marks safe. Any other is not a read, and is not reported as one left
/// undrafted.
fn is_read(operation: &DescribedOperation) -> bool {
    operation.safe && operation.method == "GET"
}

/// Why a read is not drafted, when it is not; `None` when it is. The
/// conditions of [`would_draft`], one by one, in the order they are met.
fn why_not_drafted(operation: &DescribedOperation) -> Option<&'static str> {
    if operation.template_path().is_none() {
        return Some(why::PATH_NOT_BINDABLE);
    }
    if object_words(&operation.path).is_none() {
        return Some(why::NO_NAME);
    }
    if let Err(reason) = placeholders_or_why(operation) {
        return Some(reason);
    }
    let Some((record, _)) = operation.response.as_ref().and_then(record_of) else {
        return Some(why::NO_ANSWER);
    };
    let (plain, secrets) = plain_outputs_and_secrets(record);
    if plain.is_empty() {
        return Some(if secrets.is_empty() {
            why::NO_PLAIN_OUTPUT
        } else {
            why::ONLY_SECRETS
        });
    }
    None
}

/// What a read that was given no name is asked.
const NAME_SHARED: &str = "Two or more reads of the description would share one tool name, so none was given by rule. What is this read's tool name?";
const NAME_ENDPOINT_SHARED: &str = "Two or more operations of the description would share one endpoint name, so none was given by rule. Which endpoint does this read bind?";
const NAME_TOO_LONG: &str = "The tool name made from this read's path is longer than a tool name may be, so none was given by rule. What is this read's tool name?";

fn unreadable() -> Failure {
    a2m_core::codes::row::FINDING_INVALID.failure(
        "A `described_operation` finding is not in the form the HTTP pack writes.",
        "Import the description again with the version of the HTTP pack that is enabled.",
    )
}

/// From a `described_operation` finding to one read operation.
pub(crate) struct DescribedReadRule {
    pub(crate) kind: FindingKind,
}

impl DraftingRule for DescribedReadRule {
    fn finding_kind(&self) -> FindingKind {
        self.kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        OPERATION
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let operation = DescribedOperation::from_found(finding.found()).ok_or_else(unreadable)?;
        // Only a safe GET is a read; any other is not this rule's, and is
        // not reported as left undrafted.
        if !is_read(&operation) {
            return Ok(Vec::new());
        }
        // A tool is named after its target; with none there is no name.
        let Some(target) = cx.target() else {
            cx.not_drafted(why::NO_TARGET);
            return Ok(Vec::new());
        };
        if let Some(reason) = why_not_drafted(&operation) {
            cx.not_drafted(reason);
            // A read with nothing left but secrets is not drafted for them:
            // they are the properties left out.
            if reason == why::ONLY_SECRETS
                && let Some((record, _)) = operation.response.as_ref().and_then(record_of)
            {
                cx.left_out_as_secret(plain_outputs_and_secrets(record).1.len());
            }
            return Ok(Vec::new());
        }
        let (Some(path), Some(shape), Some(placeholders)) = (
            operation.template_path(),
            operation.response.as_ref(),
            placeholders_of(&operation),
        ) else {
            return Ok(Vec::new());
        };
        let Some((record, listed)) = record_of(shape) else {
            return Ok(Vec::new());
        };
        let (outputs, extract, secrets) = outputs(record, listed, cx);
        if outputs.is_empty() || path.is_empty() {
            cx.not_drafted(why::NO_PLAIN_OUTPUT);
            return Ok(Vec::new());
        }
        cx.left_out_as_secret(secrets.len());
        let verb = if listed { "list" } else { "get" };
        // A read whose name is shared with another's, or is too long, is
        // drafted with its name as an open question, in a file of its own.
        let object = operation
            .object
            .clone()
            .or_else(|| object_words(&operation.path))
            .unwrap_or_default();
        let name = format!("{}_{object}_{verb}", target.as_str());
        let why = if operation.shared {
            Some(NAME_SHARED)
        } else if OperationName::new(&name).is_err() {
            Some(NAME_TOO_LONG)
        } else {
            None
        };
        let (name_value, file) = match why {
            Some(why) => (
                cx.open_question(why),
                format!(
                    "operations/held_{}.yaml",
                    operation
                        .stem
                        .clone()
                        .unwrap_or_else(|| crate::openapi::path_stem(
                            &operation.method,
                            &operation.path
                        ))
                ),
            ),
            None => (
                DraftValue::text(&name),
                format!("operations/{object}_{verb}.yaml"),
            ),
        };
        let endpoint = if operation.endpoint_shared {
            "(a name not yet given)".to_owned()
        } else {
            operation.name()
        };
        let inputs: Vec<(String, DraftValue)> = placeholders
            .iter()
            .map(|(name, parameter)| {
                (
                    name.clone(),
                    DraftValue::map(vec![("type", input_type(parameter, name, cx))]),
                )
            })
            .collect();
        let mut binding = vec![
            ("mode", DraftValue::text(crate::HTTP_MODE)),
            (
                "verb",
                DraftValue::text(if listed { "list" } else { "read" }),
            ),
            (
                "endpoint",
                if operation.endpoint_shared {
                    cx.open_question(NAME_ENDPOINT_SHARED)
                } else {
                    DraftValue::text(&endpoint)
                },
            ),
        ];
        if !extract.is_empty() {
            binding.push(("extract", DraftValue::Map(extract)));
        }
        let mut doc = vec![
            ("operation", name_value),
            ("effect", DraftValue::text("read")),
        ];
        match placeholders.as_slice() {
            [] => {}
            [(only, _)] => doc.push(("key", DraftValue::text(only))),
            many => doc.push((
                "key",
                DraftValue::List(
                    many.iter()
                        .map(|(name, _)| DraftValue::text(name))
                        .collect(),
                ),
            )),
        }
        doc.push(("inputs", DraftValue::Map(inputs)));
        doc.push(("outputs", DraftValue::Map(outputs)));
        doc.push(("bindings", DraftValue::List(vec![DraftValue::map(binding)])));
        doc.push((
            "description",
            DraftValue::text(&{
                let mut text = if listed {
                    format!(
                        "Lists what the endpoint `{endpoint}` answers, as the service's published description states it. Drafted by rule; its name is provisional."
                    )
                } else {
                    format!(
                        "Reads what the endpoint `{endpoint}` answers for one key, as the service's published description states it. Drafted by rule; its name is provisional, and its example is the description's own."
                    )
                };
                if !secrets.is_empty() {
                    text.push_str(&left_out_sentence(&secrets));
                }
                text
            }),
        ));
        // The example is the description's own, for every placeholder, or
        // there is none: a value is never made up.
        let example: Option<Vec<(String, DraftValue)>> = placeholders
            .iter()
            .map(|(name, parameter)| example_of(parameter).map(|value| (name.clone(), value)))
            .collect();
        if let Some(example) = example.filter(|example| !example.is_empty()) {
            doc.push(("examples", DraftValue::List(vec![DraftValue::Map(example)])));
        }
        Ok(vec![DraftDoc {
            path: RelPath::new(&file)?,
            body: DraftValue::map(doc),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::{
        left_out_sentence, placeholders_of, plain_outputs_and_secrets, why, why_not_drafted,
        would_draft,
    };
    use crate::json::Json;
    use crate::openapi::{DescribedOperation, operations};

    /// One GET at `path` with `parameters` (JSON text) answering `schema`.
    fn read(path: &str, parameters: &str, schema: &str) -> DescribedOperation {
        let text = r#"{"openapi": "3.0.3", "paths": {"PATH": {"get": {"parameters": [PARAMS],
              "responses": {"200": {"content": {"application/json": {"schema": SCHEMA}}}}}}}}"#
            .replace("PATH", path)
            .replace("PARAMS", parameters)
            .replace("SCHEMA", schema);
        operations(text.as_bytes()).unwrap().remove(0)
    }

    const RECORD: &str = r#"{"type": "object", "properties": {
        "id": {"type": "string"}, "client_secret": {"type": "string"},
        "accessToken": {"type": "string"}, "total": {"type": "integer"},
        "inner": {"type": "object", "properties": {"password": {"type": "string"}}}}}"#;

    fn names(found: &[(String, &str, &str)]) -> Vec<String> {
        found.iter().map(|(name, _, _)| name.clone()).collect()
    }

    #[test]
    fn a_property_named_as_a_secret_is_left_out_at_the_top_and_in_a_lists_items() {
        let top = Json::parse(RECORD.as_bytes()).unwrap();
        let list =
            Json::parse(format!(r#"{{"type": "array", "items": {RECORD}}}"#).as_bytes()).unwrap();
        let items = list.get("items").unwrap();
        for record in [&top, items] {
            let (kept, secrets) = plain_outputs_and_secrets(record);
            assert_eq!(names(&kept), ["id", "total"]);
            assert_eq!(secrets, ["client_secret", "access_token"]);
        }
        let sentence = left_out_sentence(&["client_secret".to_owned(), "access_token".to_owned()]);
        assert!(
            sentence.ends_with("`client_secret`, `access_token`."),
            "{sentence}"
        );
        assert!(sentence.contains("secret"), "{sentence}");
    }

    #[test]
    fn a_nested_property_named_as_a_secret_changes_nothing() {
        let record = Json::parse(RECORD.as_bytes()).unwrap();
        let (_, secrets) = plain_outputs_and_secrets(&record);
        // `inner.password` is nested: it was never an output, so it is
        // neither kept nor counted as left out.
        assert!(
            !secrets
                .iter()
                .any(|name| name == "password" || name == "inner")
        );
    }

    #[test]
    fn a_read_with_nothing_but_secrets_to_give_is_not_drafted() {
        let only = r#"{"type": "object", "properties": {"client_secret": {"type": "string"}}}"#;
        assert!(!would_draft(&read("/a/b", "", only)));
        assert!(would_draft(&read("/a/b", "", RECORD)));
    }

    #[test]
    fn an_optional_parameter_named_as_a_secret_is_no_input_and_a_required_one_is_not_drafted() {
        let optional = r#"{"name": "access_token", "in": "query", "schema": {"type": "string"}}"#;
        let found = read("/a/b", optional, RECORD);
        assert_eq!(placeholders_of(&found).unwrap().len(), 0);
        assert!(would_draft(&found));
        for place in ["query", "header", "cookie"] {
            let required = format!(
                r#"{{"name": "api_key", "in": "{place}", "required": true, "schema": {{"type": "string"}}}}"#
            );
            let found = read("/a/b", &required, RECORD);
            assert!(placeholders_of(&found).is_none(), "{place}");
            assert!(!would_draft(&found), "{place}");
        }
    }

    #[test]
    fn a_path_placeholder_named_as_a_secret_makes_the_read_not_drafted() {
        let secret =
            r#"{"name": "api_key", "in": "path", "required": true, "schema": {"type": "string"}}"#;
        assert!(!would_draft(&read("/a/{api_key}", secret, RECORD)));
        let plain =
            r#"{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}"#;
        assert!(would_draft(&read("/a/{id}", plain, RECORD)));
    }

    #[test]
    fn a_read_the_rule_does_not_draft_says_why_and_a_read_it_drafts_says_nothing() {
        let id = r#"{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}"#;
        let required = |place: &str, name: &str| {
            format!(
                r#"{{"name": "{name}", "in": "{place}", "required": true, "schema": {{"type": "string"}}}}"#
            )
        };
        let secrets_only =
            r#"{"type": "object", "properties": {"client_secret": {"type": "string"}}}"#;
        let nothing_plain = r#"{"type": "object", "properties": {"tags": {"type": "array", "items": {"type": "string"}}}}"#;
        let cases = [
            (read("/a/{id}", id, RECORD), None),
            (
                read(
                    "/a/{id}",
                    &format!("{id}, {}", required("query", "api_key")),
                    RECORD,
                ),
                Some(why::SECRET_PARAMETER),
            ),
            (
                read(
                    "/a/{id}",
                    &format!("{id}, {}", required("header", "x")),
                    RECORD,
                ),
                Some(why::OUTSIDE_PATH),
            ),
            (
                read("/a/{api_key}", &required("path", "api_key"), RECORD),
                Some(why::SECRET_PLACEHOLDER),
            ),
            (read("/a/{id}", "", RECORD), Some(why::NO_PARAMETER)),
            (read("/a/b", "", secrets_only), Some(why::ONLY_SECRETS)),
            (read("/a/b", "", nothing_plain), Some(why::NO_PLAIN_OUTPUT)),
        ];
        for (operation, expected) in &cases {
            assert_eq!(why_not_drafted(operation), *expected, "{}", operation.path);
            assert_eq!(would_draft(operation), expected.is_none());
        }
        // A reason quotes nothing of the description.
        for (_, expected) in &cases {
            if let Some(reason) = expected {
                assert!(!reason.contains("api_key") && !reason.contains("client_secret"));
            }
        }
    }
}
