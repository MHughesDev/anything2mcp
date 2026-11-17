//! The drafting rule from a listed tool to one pass-through read operation
//! (spec section 304, "Discovery and drafting").
//!
//! A read is drafted for a tool the server marks read-only, whose mapping
//! entry the other rule drafts as `read` with nothing left open, and for no
//! other: a tool whose effect is open is the developer's, and so is one
//! whose whole input is one string, which the server may run. The inputs
//! are the tool's input schema's plain properties, the arguments pass them
//! through by name, and the outputs are the plain properties of its output
//! schema, or its text where it states none. The example is the input
//! schema's own, for every required input, or there is none.

use a2m_core::{Failure, FailureClass, FailureCode, FileKindName, Finding, OperationName};
use a2m_files::{DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath, UNTRUSTED_TEXT};
use serde_json::Value as Json;

use crate::names::{self, Names};

/// The kernel's file kind for operations.
const OPERATION: FileKindName = FileKindName::fixed("operation");

/// Named values of a draft, in order.
type Pairs = Vec<(String, DraftValue)>;

/// A name in the form target files use: lowercase letters, digits, and
/// underscores, starting with a letter. `None` for a name that is not one
/// already, since an argument is passed under the server's own name.
fn plain(name: &str) -> Option<&str> {
    (name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
    .then_some(name)
}

/// The one type a property states, when it states one.
fn kind(property: &Json) -> Option<&str> {
    match property.get("type") {
        Some(Json::String(kind)) => Some(kind.as_str()),
        _ => None,
    }
}

/// From a `upstream_tool` finding to one read operation.
pub(crate) struct ToolToRead {
    names: Names,
}

impl ToolToRead {
    pub(crate) fn new(names: Names) -> Self {
        Self { names }
    }
}

impl DraftingRule for ToolToRead {
    fn finding_kind(&self) -> a2m_core::FindingKind {
        self.names.upstream_tool.clone()
    }

    fn file_kind(&self) -> FileKindName {
        OPERATION
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let found: Json = serde_json::from_str(finding.found()).map_err(|_| {
            Failure::new(
                FailureClass::Input,
                FailureCode::FINDING_INVALID,
                "An `upstream_tool` finding states a surface, a tool, and the digest of its definition.",
                "Correct the provider that made the finding.",
            )
        })?;
        let Some(target) = cx.target() else {
            return Ok(Vec::new());
        };
        let text = |key: &str| found.get(key).and_then(Json::as_str);
        let (Some(surface), Some(tool)) = (text("surface"), text("tool").and_then(plain)) else {
            return Ok(Vec::new());
        };
        if found.get("read_only").and_then(Json::as_bool) != Some(true)
            || found.get("free_text").and_then(Json::as_bool) == Some(true)
        {
            return Ok(Vec::new());
        }
        let name = format!("{}_{tool}", target.as_str());
        if OperationName::new(&name).is_err() {
            return Ok(Vec::new());
        }
        let schema = found.get("input_schema").unwrap_or(&Json::Null);
        let required: Vec<&str> = schema
            .get("required")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(Json::as_str)
            .collect();
        let mut inputs: Pairs = Vec::new();
        let mut args: Pairs = Vec::new();
        let mut example: Pairs = Vec::new();
        let mut every_example = true;
        for (property, inner) in schema
            .get("properties")
            .and_then(Json::as_object)
            .into_iter()
            .flatten()
        {
            let needed = required.contains(&property.as_str());
            let ty = match kind(inner) {
                Some("string") => "text",
                Some("integer") => "integer",
                Some("boolean") => "boolean",
                Some("number") => "decimal",
                // An optional input of a type the rule cannot read is left
                // out; a required one leaves the read to the developer.
                _ if !needed => continue,
                _ => return Ok(Vec::new()),
            };
            let Some(input) = plain(property) else {
                if needed {
                    return Ok(Vec::new());
                }
                continue;
            };
            let mut spec = vec![("type", DraftValue::text(ty))];
            if !needed {
                spec.push(("required", DraftValue::Bool(false)));
            }
            inputs.push((input.to_owned(), DraftValue::map(spec)));
            args.push((
                input.to_owned(),
                DraftValue::text(&format!("input.{input}")),
            ));
            if needed {
                // The schema's own example: `examples`, or `default`, of
                // the input's type. Never a value made up.
                let given = inner
                    .get("examples")
                    .and_then(Json::as_array)
                    .and_then(|list| list.first())
                    .or_else(|| inner.get("default"));
                match (ty, given) {
                    ("text", Some(Json::String(value))) => {
                        example.push((input.to_owned(), DraftValue::text(value)));
                    }
                    ("integer", Some(Json::Number(value))) if value.is_i64() => {
                        example.push((
                            input.to_owned(),
                            DraftValue::Int(value.as_i64().unwrap_or_default()),
                        ));
                    }
                    _ => every_example = false,
                }
            }
        }
        let mut outputs: Pairs = Vec::new();
        match found
            .get("output_schema")
            .filter(|schema| !schema.is_null())
        {
            None => outputs.push((
                "text".to_owned(),
                DraftValue::map(vec![("type", DraftValue::text(UNTRUSTED_TEXT))]),
            )),
            Some(schema) => {
                for (property, inner) in schema
                    .get("properties")
                    .and_then(Json::as_object)
                    .into_iter()
                    .flatten()
                {
                    let ty = match kind(inner) {
                        Some("string") => UNTRUSTED_TEXT,
                        Some("integer") => "integer",
                        Some("boolean") => "boolean",
                        Some("number") => "decimal",
                        // A nested value is not one output.
                        _ => continue,
                    };
                    let Some(output) = plain(property) else {
                        continue;
                    };
                    outputs.push((
                        output.to_owned(),
                        DraftValue::map(vec![("type", DraftValue::text(ty))]),
                    ));
                }
            }
        }
        if outputs.is_empty() {
            return Ok(Vec::new());
        }
        let mut binding = vec![
            ("mode", DraftValue::text(names::PACK)),
            ("tool", DraftValue::text(tool)),
        ];
        if !args.is_empty() {
            binding.push(("args", DraftValue::Map(args)));
        }
        let mut doc = vec![
            ("operation", DraftValue::text(&name)),
            ("effect", DraftValue::text("read")),
            ("inputs", DraftValue::Map(inputs)),
            ("outputs", DraftValue::Map(outputs)),
            ("bindings", DraftValue::List(vec![DraftValue::map(binding)])),
            (
                "description",
                DraftValue::text(&format!(
                    "Calls the tool `{tool}`, which the server marks read-only, and passes its result through. Drafted by rule from the server's tool list; its name is provisional."
                )),
            ),
        ];
        if every_example && !example.is_empty() {
            doc.push(("examples", DraftValue::List(vec![DraftValue::Map(example)])));
        }
        Ok(vec![DraftDoc {
            path: RelPath::new(&format!("operations/{surface}_{tool}.yaml"))?,
            body: DraftValue::map(doc),
        }])
    }
}
