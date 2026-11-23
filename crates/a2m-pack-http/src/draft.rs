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
use a2m_files::{DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath, UNTRUSTED_TEXT};

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

/// The plain properties of a record's shape: for each, the name it has as
/// an output, the property as the description writes it, and its kind.
/// A property whose name holds a dot or a star cannot be named in
/// `extract`; one whose name gives no output name, or one already taken,
/// or one that is not one plain value, is left out.
fn plain_outputs(shape: &Json) -> Vec<(String, &str, &str)> {
    let mut found: Vec<(String, &str, &str)> = Vec::new();
    let Some(Json::Object(properties)) = shape.get("properties") else {
        return found;
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
        found.push((name, property.as_str(), kind));
    }
    found
}

/// The outputs of a read: each plain property of `shape`, named in the
/// form target files use, with where it is in the answer when the name
/// differs or the answer is a list.
fn outputs(shape: &Json, listed: bool, cx: &DraftContext<'_>) -> (Pairs, Pairs) {
    let mut outputs: Pairs = Vec::new();
    let mut extract = Vec::new();
    for (name, property, kind) in plain_outputs(shape) {
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
    (outputs, extract)
}

/// The placeholders of a read's path, in order, each with what the
/// description says of it. `None` when one has no parameter stated, or the
/// read needs a parameter outside the path (a query value, a header, a
/// cookie, a body, or a form): the read is the developer's.
fn placeholders_of(operation: &DescribedOperation) -> Option<Vec<(String, &Parameter)>> {
    let mut placeholders = Vec::new();
    for part in operation.path.split('/') {
        let Some(name) = part.strip_prefix('{').and_then(|p| p.strip_suffix('}')) else {
            continue;
        };
        let parameter = operation
            .parameters
            .iter()
            .find(|known| known.place == "path" && known.name == name)?;
        placeholders.push((snake(name), parameter));
    }
    if operation
        .parameters
        .iter()
        .any(|parameter| parameter.required && parameter.place != "path")
    {
        return None;
    }
    Some(placeholders)
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
    operation.safe
        && operation.method == "GET"
        && operation.template_path().is_some()
        && object_words(&operation.path).is_some()
        && placeholders_of(operation).is_some()
        && operation
            .response
            .as_ref()
            .and_then(record_of)
            .is_some_and(|(record, _)| !plain_outputs(record).is_empty())
}

/// What a read that was given no name is asked.
const NAME_SHARED: &str = "Two or more reads of the description would share one tool name, so none was given by rule. What is this read's tool name?";
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
        // A tool is named after its target; with none there is no name.
        let Some(target) = cx.target() else {
            return Ok(Vec::new());
        };
        if !would_draft(&operation) {
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
        let (outputs, extract) = outputs(record, listed, cx);
        if outputs.is_empty() || path.is_empty() {
            return Ok(Vec::new());
        }
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
        let endpoint = operation.name();
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
            ("endpoint", DraftValue::text(&endpoint)),
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
            DraftValue::text(&if listed {
                format!(
                    "Lists what the endpoint `{endpoint}` answers, as the service's published description states it. Drafted by rule; its name is provisional."
                )
            } else {
                format!(
                    "Reads what the endpoint `{endpoint}` answers for one key, as the service's published description states it. Drafted by rule; its name is provisional, and its example is the description's own."
                )
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
