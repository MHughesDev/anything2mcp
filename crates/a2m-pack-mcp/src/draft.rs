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
use a2m_files::{
    DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath, UNTRUSTED_TEXT, left_out_sentence,
    names_a_secret,
};
use serde_json::Value as Json;

use crate::names::{self, Names};

/// The kernel's file kind for operations.
const OPERATION: FileKindName = FileKindName::fixed("operation");

/// Named values of a draft, in order.
type Pairs = Vec<(String, DraftValue)>;

/// Why the rule drafts no read for a tool: the pack's own sentences, which
/// quote nothing the server wrote (field test 6, finding 41).
mod why {
    pub(super) const NO_TARGET: &str =
        "The folder has no `target.yaml` yet, so the read has no name to be drafted under.";
    pub(super) const NAME_NOT_PLAIN: &str = "The tool's name is not a plain name of lowercase letters, digits, and underscores, and an operation's name is made from it.";
    pub(super) const NOT_READ_ONLY: &str = "The server does not mark the tool read-only. Whether it reads, writes, or does something that cannot be taken back is a person's answer in its mapping entry, and an operation over it is the developer's.";
    pub(super) const FREE_TEXT: &str = "The tool's whole input is one string, which the server may run as a command, a query, or a prompt. Its read is drafted when the surface's mapping states `free_text: false` for this definition of it and the scout is run again.";
    pub(super) const NAME_REFUSED: &str =
        "The operation's name made from the target and the tool is not one an operation may have.";
    pub(super) const INPUT_SECRET: &str =
        "A required input is named as a secret's, and a secret is never an input.";
    pub(super) const INPUT_TYPE: &str = "A required input states no type the rule reads: a string, a whole number, a number, or a truth value.";
    pub(super) const INPUT_NAME: &str = "A required input's name is not a plain name, and an argument is passed under the server's own name.";
    pub(super) const NO_OUTPUT: &str =
        "The tool's output schema states no plain property the rule can make an output of.";
}

/// Whether a person answered, in the target's mapping, that the server does
/// not run the one string of this tool, for the definition the finding
/// reports: an entry of the surface's mapping, outside the holding folder,
/// with the finding's digest and `free_text: false` (decision 0365).
fn answered_not_free_text(cx: &DraftContext<'_>, surface: &str, tool: &str, digest: &str) -> bool {
    cx.existing().is_some_and(|sources| {
        sources
            .docs_of(names::MAPPING_KIND.as_str())
            .filter(|doc| doc.text("surface") == Some(surface))
            .filter_map(|doc| doc.get("tools"))
            .flat_map(|tools| tools.entries())
            .filter(|entry| entry.key.value == tool)
            .any(|entry| {
                entry.value.text("digest") == Some(digest)
                    && entry.value.get("free_text").and_then(|flag| flag.as_bool()) == Some(false)
            })
    })
}

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

/// Whether the compiler takes a name for a secret's, as the server writes
/// it or as the input or output name it becomes (decision 0302).
fn named_as_secret(property: &str) -> bool {
    names_a_secret(property) || plain(property).is_some_and(names_a_secret)
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
        let none = |reason: &str| {
            cx.not_drafted(reason);
            Ok(Vec::new())
        };
        let Some(target) = cx.target() else {
            return none(why::NO_TARGET);
        };
        let text = |key: &str| found.get(key).and_then(Json::as_str);
        let (Some(surface), Some(tool)) = (text("surface"), text("tool").and_then(plain)) else {
            return none(why::NAME_NOT_PLAIN);
        };
        if found.get("read_only").and_then(Json::as_bool) != Some(true) {
            return none(why::NOT_READ_ONLY);
        }
        if found.get("free_text").and_then(Json::as_bool) == Some(true)
            && !text("digest")
                .is_some_and(|digest| answered_not_free_text(cx, surface, tool, digest))
        {
            return none(why::FREE_TEXT);
        }
        let name = format!("{}_{tool}", target.as_str());
        if OperationName::new(&name).is_err() {
            return none(why::NAME_REFUSED);
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
        let mut left_out = 0_usize;
        for (property, inner) in schema
            .get("properties")
            .and_then(Json::as_object)
            .into_iter()
            .flatten()
        {
            let needed = required.contains(&property.as_str());
            // A secret is never an input: left out where optional, and the
            // read is the developer's where required (decision 0302).
            if named_as_secret(property) {
                if needed {
                    return none(why::INPUT_SECRET);
                }
                left_out += 1;
                continue;
            }
            let ty = match kind(inner) {
                Some("string") => "text",
                Some("integer") => "integer",
                Some("boolean") => "boolean",
                Some("number") => "decimal",
                // An optional input of a type the rule cannot read is left
                // out; a required one leaves the read to the developer.
                _ if !needed => continue,
                _ => return none(why::INPUT_TYPE),
            };
            let Some(input) = plain(property) else {
                if needed {
                    return none(why::INPUT_NAME);
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
        let mut secrets: Vec<String> = Vec::new();
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
                    if named_as_secret(property) {
                        secrets.push(output.to_owned());
                        continue;
                    }
                    outputs.push((
                        output.to_owned(),
                        DraftValue::map(vec![("type", DraftValue::text(ty))]),
                    ));
                }
            }
        }
        if outputs.is_empty() {
            return none(why::NO_OUTPUT);
        }
        cx.left_out_as_secret(left_out + secrets.len());
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
                    "Calls the tool `{tool}`, which the server marks read-only, and passes its result through. Drafted by rule from the server's tool list; its name is provisional.{}",
                    left_out_sentence(&secrets)
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
