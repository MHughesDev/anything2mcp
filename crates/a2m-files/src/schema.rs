//! Schemas for target files: a small, closed subset of JSON Schema that this
//! crate checks against located documents, so that every finding has a line
//! and a column, and that is published as ordinary JSON Schema (decision
//! 0078).

use a2m_core::Decimal;

use crate::codes::{self, kernel};
use crate::diag::{Diagnostic, Stage};
use crate::doc::{Doc, Located, Place};

/// The key the drafter leaves where the system did not state a value.
pub const OPEN_QUESTION: &str = "open_question";

/// What a text value refers to. The kernel resolves each against what is
/// registered or declared; a schema only says which it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Reference {
    /// A control an enabled pack registered.
    Control,
    /// A field type, built in or from `types.yaml`.
    FieldType,
    /// A surface the target declares.
    Surface,
}

impl Reference {
    const fn name(self) -> &'static str {
        match self {
            Self::Control => "control",
            Self::FieldType => "field_type",
            Self::Surface => "surface",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Any,
    Bool,
    Int {
        min: Option<i64>,
        max: Option<i64>,
    },
    Text,
    Scalar,
    Decimal,
    Name,
    Words(Vec<String>),
    Refers(Reference),
    Expression,
    List {
        item: Box<Node>,
        min: usize,
    },
    Object {
        fields: Vec<Field>,
        rest: Option<Box<Node>>,
    },
    Either(Vec<Node>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Field {
    name: String,
    required: bool,
    schema: Node,
}

/// The schema of one file kind, of the fields a mode adds to a binding, or
/// of the fields a surface kind adds to a surface.
///
/// It is built from the constructors below, checked by this crate, and
/// published through [`JsonSchema::to_json`]. An object lists its keys, and
/// a key it does not list is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonSchema(Node);

impl JsonSchema {
    /// Any value.
    #[must_use]
    pub const fn any() -> Self {
        Self(Node::Any)
    }

    /// True or false.
    #[must_use]
    pub const fn bool() -> Self {
        Self(Node::Bool)
    }

    /// A whole number.
    #[must_use]
    pub const fn int() -> Self {
        Self(Node::Int {
            min: None,
            max: None,
        })
    }

    /// A whole number from `min` to `max`.
    #[must_use]
    pub const fn int_between(min: i64, max: i64) -> Self {
        Self(Node::Int {
            min: Some(min),
            max: Some(max),
        })
    }

    /// Text.
    #[must_use]
    pub const fn text() -> Self {
        Self(Node::Text)
    }

    /// Text, a whole number, or a truth value.
    #[must_use]
    pub const fn scalar() -> Self {
        Self(Node::Scalar)
    }

    /// An exact number: a whole number, or text such as `0.0625`.
    #[must_use]
    pub const fn decimal() -> Self {
        Self(Node::Decimal)
    }

    /// A name: lowercase letters, digits, and underscores, starting with a
    /// letter; dots may join several.
    #[must_use]
    pub const fn name() -> Self {
        Self(Node::Name)
    }

    /// One of a closed list of words.
    #[must_use]
    pub fn words(words: &[&str]) -> Self {
        Self(Node::Words(
            words.iter().map(|word| (*word).to_owned()).collect(),
        ))
    }

    /// A control an enabled pack registered.
    #[must_use]
    pub const fn control() -> Self {
        Self(Node::Refers(Reference::Control))
    }

    /// A field type.
    #[must_use]
    pub const fn field_type() -> Self {
        Self(Node::Refers(Reference::FieldType))
    }

    /// A surface of the target.
    #[must_use]
    pub const fn surface() -> Self {
        Self(Node::Refers(Reference::Surface))
    }

    /// An expression: text, or a whole number or truth value standing for
    /// itself.
    #[must_use]
    pub const fn expression() -> Self {
        Self(Node::Expression)
    }

    /// A list of `item`.
    #[must_use]
    pub fn list(item: Self) -> Self {
        Self(Node::List {
            item: Box::new(item.0),
            min: 0,
        })
    }

    /// A list of at least one `item`.
    #[must_use]
    pub fn non_empty_list(item: Self) -> Self {
        Self(Node::List {
            item: Box::new(item.0),
            min: 1,
        })
    }

    /// A map from any key to `value`.
    #[must_use]
    pub fn map_of(value: Self) -> Self {
        Self(Node::Object {
            fields: Vec::new(),
            rest: Some(Box::new(value.0)),
        })
    }

    /// A map with the keys added by [`JsonSchema::required`] and
    /// [`JsonSchema::optional`], and no others.
    #[must_use]
    pub const fn object() -> Self {
        Self(Node::Object {
            fields: Vec::new(),
            rest: None,
        })
    }

    /// Adds a key that must be given. No effect on a schema that is not an
    /// object.
    #[must_use]
    pub fn required(self, name: &str, schema: Self) -> Self {
        self.field(name, true, schema)
    }

    /// Adds a key that may be given. No effect on a schema that is not an
    /// object.
    #[must_use]
    pub fn optional(self, name: &str, schema: Self) -> Self {
        self.field(name, false, schema)
    }

    fn field(mut self, name: &str, required: bool, schema: Self) -> Self {
        if let Node::Object { fields, .. } = &mut self.0 {
            fields.retain(|field| field.name != name);
            fields.push(Field {
                name: name.to_owned(),
                required,
                schema: schema.0,
            });
        }
        self
    }

    /// A value that fits any one of `choices`.
    #[must_use]
    pub fn either(choices: Vec<Self>) -> Self {
        Self(Node::Either(
            choices.into_iter().map(|choice| choice.0).collect(),
        ))
    }

    /// The keys an object schema lists, in the order they were added.
    #[must_use]
    pub fn keys(&self) -> Vec<&str> {
        match &self.0 {
            Node::Object { fields, .. } => fields.iter().map(|field| field.name.as_str()).collect(),
            _ => Vec::new(),
        }
    }

    /// The schema as JSON Schema, draft 2020-12, for publishing.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut schema = to_json(&self.0);
        if let serde_json::Value::Object(map) = &mut schema {
            map.insert(
                "$schema".to_owned(),
                serde_json::Value::from("https://json-schema.org/draft/2020-12/schema"),
            );
        }
        schema
    }

    /// Checks a located document against the schema.
    #[must_use]
    pub fn check(&self, doc: &Located<Doc>) -> Checked {
        let mut checked = Checked::default();
        check(&self.0, doc, &mut checked);
        checked
    }
}

/// What checking a document found: the problems, and the references the
/// kernel still has to resolve.
#[derive(Debug, Clone, Default)]
pub struct Checked {
    /// What does not fit the schema.
    pub diagnostics: Vec<Diagnostic>,
    /// Every value the schema says is a reference, with its place.
    pub references: Vec<(Reference, Located<String>)>,
}

fn to_json(node: &Node) -> serde_json::Value {
    use serde_json::json;
    match node {
        Node::Any => json!({}),
        Node::Bool => json!({"type": "boolean"}),
        Node::Int { min, max } => {
            let mut schema = json!({"type": "integer"});
            if let (Some(min), Some(map)) = (min, schema.as_object_mut()) {
                map.insert("minimum".to_owned(), json!(min));
            }
            if let (Some(max), Some(map)) = (max, schema.as_object_mut()) {
                map.insert("maximum".to_owned(), json!(max));
            }
            schema
        }
        Node::Text => json!({"type": "string"}),
        Node::Scalar => json!({"type": ["string", "integer", "boolean"]}),
        Node::Decimal => {
            json!({"type": ["string", "integer"], "pattern": "^-?[0-9]+(\\.[0-9]+)?$"})
        }
        Node::Name => {
            json!({"type": "string", "pattern": "^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)*$"})
        }
        Node::Words(words) => json!({"enum": words}),
        Node::Refers(reference) => json!({"type": "string", "x-a2m-refers-to": reference.name()}),
        Node::Expression => {
            json!({"type": ["string", "integer", "boolean"], "x-a2m-expression": true})
        }
        Node::List { item, min } => {
            let mut schema = json!({"type": "array", "items": to_json(item)});
            if let (true, Some(map)) = (*min > 0, schema.as_object_mut()) {
                map.insert("minItems".to_owned(), json!(min));
            }
            schema
        }
        Node::Object { fields, rest } => {
            let properties: serde_json::Map<String, serde_json::Value> = fields
                .iter()
                .map(|field| (field.name.clone(), to_json(&field.schema)))
                .collect();
            let required: Vec<&str> = fields
                .iter()
                .filter(|field| field.required)
                .map(|field| field.name.as_str())
                .collect();
            let additional = rest
                .as_ref()
                .map_or(serde_json::Value::Bool(false), |rest| to_json(rest));
            json!({
                "type": "object",
                "properties": properties,
                "required": required,
                "additionalProperties": additional,
            })
        }
        Node::Either(choices) => {
            json!({"anyOf": choices.iter().map(to_json).collect::<Vec<_>>()})
        }
    }
}

fn describe(node: &Node) -> String {
    match node {
        Node::Any => "any value".to_owned(),
        Node::Bool => "true or false".to_owned(),
        Node::Int { .. } => "a whole number".to_owned(),
        Node::Text => "text".to_owned(),
        Node::Scalar => "text, a whole number, or true or false".to_owned(),
        Node::Decimal => {
            "a number, written as a whole number or as text such as \"0.25\"".to_owned()
        }
        Node::Name => "a name".to_owned(),
        Node::Words(words) => format!("one of: {}", words.join(", ")),
        Node::Refers(Reference::Control) => "the name of a control".to_owned(),
        Node::Refers(Reference::FieldType) => "the name of a field type".to_owned(),
        Node::Refers(Reference::Surface) => "the name of a surface".to_owned(),
        Node::Expression => "an expression".to_owned(),
        Node::List { .. } => "a list".to_owned(),
        Node::Object { .. } => "a map".to_owned(),
        Node::Either(choices) => choices
            .iter()
            .map(describe)
            .collect::<Vec<_>>()
            .join(", or "),
    }
}

/// Whether `text` is one name, or several joined by dots.
pub(crate) fn is_dotted_name(text: &str) -> bool {
    !text.is_empty()
        && text.split('.').all(|part| {
            let mut bytes = part.bytes();
            bytes.next().is_some_and(|first| first.is_ascii_lowercase())
                && bytes
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
}

fn wrong_type(node: &Node, doc: &Located<Doc>, out: &mut Checked) {
    out.diagnostics.push(Diagnostic::error(
        kernel(codes::WRONG_TYPE),
        Stage::Validate,
        &doc.at,
        format!(
            "This value is a {}, and the schema needs {}.",
            doc.value.kind(),
            describe(node)
        ),
        format!("Write {} here.", describe(node)),
    ));
}

/// The known word nearest to `word`, when one is near enough to be meant.
pub(crate) fn nearest<'a>(word: &str, known: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    known
        .map(|candidate| (distance(word, candidate), candidate))
        .filter(|(distance, candidate)| *distance <= 2.min(candidate.len() / 2).max(1))
        .min()
        .map(|(_, candidate)| candidate)
}

fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, left) in a.iter().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, right) in b.iter().enumerate() {
            let cost = usize::from(left != right);
            let next = (previous + cost).min(row[j] + 1).min(row[j + 1] + 1);
            previous = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

/// The fix for a word that is not one of `known`.
pub(crate) fn fix_for_unknown(word: &str, known: &[&str]) -> String {
    match nearest(word, known.iter().copied()) {
        Some(meant) => format!("Write `{meant}`."),
        None if known.is_empty() => "Remove it; nothing is accepted here.".to_owned(),
        None => format!("Use one of: {}.", known.join(", ")),
    }
}

/// The diagnostic for an open question left by the drafter, when `doc` is
/// one.
pub(crate) fn open_question(doc: &Located<Doc>) -> Option<Diagnostic> {
    let entries = doc.entries();
    let [entry] = entries else { return None };
    (entry.key.value == OPEN_QUESTION).then(|| {
        let question = entry
            .value
            .as_text()
            .unwrap_or("the system did not state this value");
        Diagnostic::error(
            kernel(codes::OPEN_QUESTION),
            Stage::Validate,
            &doc.at,
            format!("An open question stands where a value is needed: {question}"),
            "Answer the question from the system or its owner, and write the value in its place.",
        )
    })
}

fn check(node: &Node, doc: &Located<Doc>, out: &mut Checked) {
    if let Some(question) = open_question(doc) {
        out.diagnostics.push(question);
        return;
    }
    match (node, &doc.value) {
        (Node::Any, _)
        | (Node::Bool, Doc::Bool(_))
        | (Node::Text, Doc::Text(_))
        | (Node::Scalar | Node::Expression, Doc::Text(_) | Doc::Int(_) | Doc::Bool(_))
        | (Node::Decimal, Doc::Int(_)) => {}
        (Node::Int { min, max }, Doc::Int(number)) => {
            if min.is_some_and(|min| *number < min) || max.is_some_and(|max| *number > max) {
                let range = match (min, max) {
                    (Some(min), Some(max)) => format!("from {min} to {max}"),
                    (Some(min), None) => format!("at least {min}"),
                    (None, Some(max)) => format!("at most {max}"),
                    (None, None) => "any whole number".to_owned(),
                };
                out.diagnostics.push(Diagnostic::error(
                    kernel(codes::UNKNOWN_VALUE),
                    Stage::Validate,
                    &doc.at,
                    format!("The number {number} is outside what this field takes: {range}."),
                    format!("Write a whole number {range}."),
                ));
            }
        }
        (Node::Decimal, Doc::Text(text)) => {
            if Decimal::parse(text).is_err() {
                wrong_type(node, doc, out);
            }
        }
        (Node::Name, Doc::Text(text)) => {
            if !is_dotted_name(text) {
                out.diagnostics.push(Diagnostic::error(
                    kernel(codes::NAME_MALFORMED),
                    Stage::Validate,
                    &doc.at,
                    format!("`{}` is not a name.", shown(text)),
                    "Write the name in lowercase letters, digits, and underscores, starting with a letter, such as `order_id`.",
                ));
            }
        }
        (Node::Words(words), Doc::Text(text)) => unknown_word(words, text, &doc.at, out),
        (Node::Words(words), Doc::Bool(flag)) => {
            // `movable: false` is the word `false`, not a truth value.
            unknown_word(words, if *flag { "true" } else { "false" }, &doc.at, out);
        }
        (Node::Refers(reference), Doc::Text(text)) => {
            out.references
                .push((*reference, Located::new(text.clone(), doc.at.clone())));
        }
        (Node::List { item, min }, Doc::List(items)) => {
            if items.len() < *min {
                out.diagnostics.push(Diagnostic::error(
                    kernel(codes::MISSING_FIELD),
                    Stage::Validate,
                    &doc.at,
                    format!(
                        "This list has {} items and needs at least {min}.",
                        items.len()
                    ),
                    "Add an item to the list.",
                ));
            }
            for entry in items {
                check(item, entry, out);
            }
        }
        (Node::Object { fields, rest }, Doc::Map(entries)) => {
            for entry in entries {
                if let Some(field) = fields.iter().find(|field| field.name == entry.key.value) {
                    check(&field.schema, &entry.value, out);
                } else if let Some(rest) = rest {
                    check(rest, &entry.value, out);
                } else {
                    let known: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
                    out.diagnostics.push(Diagnostic::error(
                        kernel(codes::UNKNOWN_KEY),
                        Stage::Validate,
                        &entry.key.at,
                        format!(
                            "The key `{}` is not one this map takes.",
                            shown(&entry.key.value)
                        ),
                        fix_for_unknown(&entry.key.value, &known),
                    ));
                }
            }
            for field in fields.iter().filter(|field| field.required) {
                if !entries.iter().any(|entry| entry.key.value == field.name) {
                    out.diagnostics.push(Diagnostic::error(
                        kernel(codes::MISSING_FIELD),
                        Stage::Validate,
                        &doc.at,
                        format!("The required field `{}` is missing.", field.name),
                        format!("Add `{}:` with {}.", field.name, describe(&field.schema)),
                    ));
                }
            }
        }
        (Node::Either(choices), _) => {
            let mut best: Option<Checked> = None;
            for choice in choices {
                let mut tried = Checked::default();
                check(choice, doc, &mut tried);
                if tried.diagnostics.is_empty() {
                    out.references.extend(tried.references);
                    return;
                }
                if best
                    .as_ref()
                    .is_none_or(|best| tried.diagnostics.len() < best.diagnostics.len())
                {
                    best = Some(tried);
                }
            }
            match best {
                Some(best) if best.diagnostics.len() > 1 || choices.len() == 1 => {
                    out.diagnostics.extend(best.diagnostics);
                }
                _ => wrong_type(node, doc, out),
            }
        }
        _ => wrong_type(node, doc, out),
    }
}

fn unknown_word(words: &[String], text: &str, at: &Place, out: &mut Checked) {
    if words.iter().any(|word| word == text) {
        return;
    }
    let known: Vec<&str> = words.iter().map(String::as_str).collect();
    out.diagnostics.push(Diagnostic::error(
        kernel(codes::UNKNOWN_VALUE),
        Stage::Validate,
        at,
        format!(
            "`{}` is not one of the words this field takes.",
            shown(text)
        ),
        fix_for_unknown(text, &known),
    ));
}

/// At most 64 characters of a word from a file, for a message.
pub(crate) fn shown(text: &str) -> String {
    if text.chars().count() <= 64 {
        text.to_owned()
    } else {
        let mut cut: String = text.chars().take(64).collect();
        cut.push('…');
        cut
    }
}

#[cfg(test)]
mod tests {
    use super::{JsonSchema, Reference};
    use crate::doc::RelPath;
    use crate::yaml::parse;

    fn schema() -> JsonSchema {
        JsonSchema::object()
            .required("screen", JsonSchema::name())
            .required("control", JsonSchema::control())
            .optional("status", JsonSchema::words(&["draft", "verified"]))
            .optional("fields", JsonSchema::map_of(JsonSchema::expression()))
            .optional("step", JsonSchema::decimal())
            .optional(
                "key",
                JsonSchema::either(vec![
                    JsonSchema::name(),
                    JsonSchema::non_empty_list(JsonSchema::name()),
                ]),
            )
    }

    fn check(text: &str) -> super::Checked {
        let file = RelPath::new("screens/order.yaml").unwrap();
        schema().check(&parse(&file, text).unwrap()[0])
    }

    #[test]
    fn a_document_that_fits_passes_and_gives_its_references() {
        let checked = check(
            "screen: order.form\ncontrol: text\nstatus: draft\nfields: {a: input.a, b: 2}\nstep: \"0.0625\"\nkey: [order_id, item_code]\n",
        );
        assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
        assert_eq!(checked.references.len(), 1);
        assert_eq!(checked.references[0].0, Reference::Control);
        assert_eq!(checked.references[0].1.at.line, 2);
    }

    #[test]
    fn an_unknown_key_a_missing_field_and_a_wrong_value_each_have_a_place_and_a_fix() {
        let checked =
            check("screen: order.form\nstatuss: draft\nstatus: live\nstep: abc\nkey: {a: 1}\n");
        let codes: Vec<(&str, u32)> = checked
            .diagnostics
            .iter()
            .map(|item| (item.code.as_str(), item.at.line))
            .collect();
        assert!(codes.contains(&("kernel.unknown_key", 2)), "{codes:?}");
        assert!(codes.contains(&("kernel.unknown_value", 3)), "{codes:?}");
        assert!(codes.contains(&("kernel.wrong_type", 4)), "{codes:?}");
        assert!(codes.contains(&("kernel.wrong_type", 5)), "{codes:?}");
        assert!(codes.contains(&("kernel.missing_field", 1)), "{codes:?}");
        let unknown = checked
            .diagnostics
            .iter()
            .find(|item| item.at.line == 2)
            .unwrap();
        assert_eq!(unknown.fix, "Write `status`.");
        assert!(checked.diagnostics.iter().all(|item| !item.fix.is_empty()));
    }

    #[test]
    fn an_open_question_is_refused_wherever_it_stands() {
        let checked =
            check("screen: order.form\ncontrol: {open_question: \"Which control is this?\"}\n");
        assert_eq!(checked.diagnostics.len(), 1);
        assert_eq!(checked.diagnostics[0].code.as_str(), "kernel.open_question");
    }

    #[test]
    fn a_schema_is_published_as_json_schema_that_refuses_unknown_keys() {
        let json = schema().to_json();
        assert_eq!(json["type"], "object");
        assert_eq!(json["additionalProperties"], false);
        assert_eq!(json["required"], serde_json::json!(["screen", "control"]));
        assert_eq!(
            json["properties"]["status"]["enum"],
            serde_json::json!(["draft", "verified"])
        );
    }
}
