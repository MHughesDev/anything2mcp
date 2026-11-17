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

use a2m_core::{Failure, FailureClass, FileKindName, Finding, FindingKind, OperationName};
use a2m_files::{DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath, UNTRUSTED_TEXT};

use crate::json::Json;
use crate::openapi::{DescribedOperation, Parameter, snake};

/// Named values of a draft, in order.
type Pairs = Vec<(String, DraftValue)>;

/// The kernel's file kind for operations.
const OPERATION: FileKindName = FileKindName::fixed("operation");

/// The part of a tool's name made from the description's path: its last
/// two plain words, lowercase. `None` when the path has no plain word.
fn object_words(path: &str) -> Option<String> {
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

/// The outputs of a read: each plain property of `shape`, named in the
/// form target files use, with where it is in the answer when the name
/// differs or the answer is a list.
fn outputs(
    shape: &Json,
    listed: bool,
    cx: &DraftContext<'_>,
) -> (Pairs, Pairs) {
    let mut outputs: Pairs = Vec::new();
    let mut extract = Vec::new();
    let Some(Json::Object(properties)) = shape.get("properties") else {
        return (outputs, extract);
    };
    for (property, inner) in properties {
        // A property whose name holds a dot or a star cannot be named in
        // `extract`; one whose name gives no output name is left out.
        if property.contains(['.', '*']) {
            continue;
        }
        let name = snake(property);
        if !name.starts_with(|c: char| c.is_ascii_lowercase())
            || outputs.iter().any(|(known, _)| *known == name)
        {
            continue;
        }
        let kind = inner.get("type").and_then(Json::as_text).unwrap_or("any");
        let Some(ty) = output_type(kind, &name, cx) else {
            continue;
        };
        let mut output = vec![("type", ty)];
        if listed {
            output.push(("list", DraftValue::Bool(true)));
            extract.push((name.clone(), DraftValue::text(&format!("*.{property}"))));
        } else if name != *property {
            extract.push((name.clone(), DraftValue::text(property)));
        }
        outputs.push((name, DraftValue::map(output)));
    }
    (outputs, extract)
}

fn unreadable() -> Failure {
    Failure::new(
        FailureClass::Input,
        a2m_core::FailureCode::FINDING_INVALID,
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
        if !operation.safe || operation.method != "GET" {
            return Ok(Vec::new());
        }
        let (Some(path), Some(shape), Some(object)) = (
            operation.template_path(),
            operation.response.as_ref(),
            object_words(&operation.path),
        ) else {
            return Ok(Vec::new());
        };
        // The placeholders, in the order of the path, each with what the
        // description says of it. A required parameter outside the path is
        // a value the rule has no place for: the read is the developer's.
        let mut placeholders = Vec::new();
        for part in operation.path.split('/') {
            let Some(name) = part.strip_prefix('{').and_then(|p| p.strip_suffix('}')) else {
                continue;
            };
            let Some(parameter) = operation
                .parameters
                .iter()
                .find(|known| known.place == "path" && known.name == name)
            else {
                return Ok(Vec::new());
            };
            placeholders.push((snake(name), parameter));
        }
        if operation
            .parameters
            .iter()
            .any(|parameter| parameter.required && parameter.place != "path")
        {
            return Ok(Vec::new());
        }
        let listed = shape.get("type").and_then(Json::as_text) == Some("array");
        let record = if listed {
            match shape.get("items") {
                Some(items) => items,
                None => return Ok(Vec::new()),
            }
        } else {
            shape
        };
        let (outputs, extract) = outputs(record, listed, cx);
        if outputs.is_empty() {
            return Ok(Vec::new());
        }
        let verb = if listed { "list" } else { "get" };
        let name = format!("{}_{object}_{verb}", target.as_str());
        if OperationName::new(&name).is_err() || path.is_empty() {
            return Ok(Vec::new());
        }
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
            ("operation", DraftValue::text(&name)),
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
            path: RelPath::new(&format!("operations/{object}_{verb}.yaml"))?,
            body: DraftValue::map(doc),
        }])
    }
}
