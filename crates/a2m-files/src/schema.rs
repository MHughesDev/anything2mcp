//! Schemas for target files: a small, closed subset of JSON Schema that this
//! crate checks against located documents, so that every finding has a line
//! and a column, and that is published as ordinary JSON Schema (decision
//! 0078).

use std::fmt;

use a2m_core::{Decimal, Failure, FailureClass, FailureCode, LintCode, Value, Values};

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
        let mut found = Found::default();
        check(&self.0, doc, &doc.at, &mut found);
        Checked {
            diagnostics: found
                .problems
                .into_iter()
                .map(|problem| {
                    Diagnostic::error(
                        kernel(problem.code),
                        Stage::Validate,
                        &problem.at,
                        problem.message,
                        problem.fix,
                    )
                })
                .collect(),
            references: found
                .references
                .into_iter()
                .map(|(reference, name, at)| (reference, Located::new(name, at)))
                .collect(),
        }
    }

    /// Checks named run-time values, such as the input of an action,
    /// against the schema: the rules of [`JsonSchema::check`], with a path
    /// in place of a line and a column.
    ///
    /// Three things differ from a file, and nothing else. No message holds
    /// a value, only its kind and its path. An open question is a drafter's
    /// mark in a file and means nothing here. A value the schema says is a
    /// reference is taken as text: only the compiler can resolve it.
    ///
    /// # Errors
    ///
    /// Every place that does not fit, in the order the values are walked:
    /// a map's members in order of name, then its missing fields.
    pub fn check_values(&self, values: &Values) -> Result<(), Misfits> {
        self.check_value(&Value::Record(values.clone()))
    }

    /// Checks one run-time value against the schema, as
    /// [`JsonSchema::check_values`] does. A record is a map; a decimal is
    /// taken where the schema needs an exact number; a date, a moment, and
    /// a length of time fit only where any value does.
    ///
    /// # Errors
    ///
    /// Every place that does not fit.
    pub fn check_value(&self, value: &Value) -> Result<(), Misfits> {
        self.check_at_run_time(value)
    }

    /// Checks JSON as a front door received it against the schema, as
    /// [`JsonSchema::check_values`] does. It takes what the document from
    /// [`JsonSchema::to_json`] describes: a number with a fraction fits
    /// nowhere but where any value does, and an exact number is written as
    /// a whole number or as text.
    ///
    /// # Errors
    ///
    /// Every place that does not fit.
    pub fn check_json(&self, value: &serde_json::Value) -> Result<(), Misfits> {
        self.check_at_run_time(value)
    }

    fn check_at_run_time<V: Checkable<At = String>>(&self, value: &V) -> Result<(), Misfits> {
        let mut found = Found::default();
        check(&self.0, value, &String::new(), &mut found);
        if found.problems.is_empty() {
            return Ok(());
        }
        Err(Misfits(
            found
                .problems
                .into_iter()
                .map(|problem| Misfit {
                    path: problem.at,
                    code: kernel(problem.code),
                    message: problem.message,
                    fix: problem.fix,
                })
                .collect(),
        ))
    }
}

/// One place where a run-time value does not fit its schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Misfit {
    /// The path of the value from the root: field names joined by dots,
    /// with the position of a list's item in brackets, such as
    /// `lines[2].quantity`. Empty for the root itself.
    pub path: String,
    /// The rule that was broken, under the code a file that broke it would
    /// be reported with, such as `kernel.wrong_type`.
    pub code: LintCode,
    /// What is wrong, in one plain sentence. It never holds the value.
    pub message: String,
    /// What to do about it.
    pub fix: String,
}

impl fmt::Display for Misfit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "The value does not fit its schema. {}", self.message)
        } else {
            write!(
                f,
                "The value at `{}` does not fit its schema. {}",
                self.path, self.message
            )
        }
    }
}

/// Run-time values that do not fit their schema: at least one place, in the
/// order the values were walked.
///
/// It maps to the failure class `input`, code `schema_invalid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Misfits(Vec<Misfit>);

impl Misfits {
    /// Every place that does not fit. Never empty.
    #[must_use]
    pub fn as_slice(&self) -> &[Misfit] {
        &self.0
    }

    /// The paths that do not fit, in order.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|misfit| misfit.path.as_str())
    }
}

impl fmt::Display for Misfits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(first) = self.0.first() {
            write!(f, "{first}")?;
        }
        match self.0.len().saturating_sub(1) {
            0 => Ok(()),
            1 => write!(f, " One more place does not fit."),
            more => write!(f, " {more} more places do not fit."),
        }
    }
}

impl std::error::Error for Misfits {}

impl From<Misfits> for Failure {
    fn from(misfits: Misfits) -> Self {
        let fix = misfits
            .0
            .first()
            .map_or_else(String::new, |first| first.fix.clone());
        Self::new(
            FailureClass::Input,
            FailureCode::SCHEMA_INVALID,
            misfits.to_string(),
            fix,
        )
    }
}

/// One thing the checker found, at a place of the kind its value has.
struct Problem<At> {
    code: &'static str,
    at: At,
    message: String,
    fix: String,
}

/// What the checker found, before it is given the form its caller reads.
struct Found<At> {
    problems: Vec<Problem<At>>,
    references: Vec<(Reference, String, At)>,
}

impl<At> Default for Found<At> {
    fn default() -> Self {
        Self {
            problems: Vec::new(),
            references: Vec::new(),
        }
    }
}

/// One member of a map, as the checker sees it.
struct Member<'a, V: Checkable> {
    key: &'a str,
    key_at: V::At,
    at: V::At,
    value: &'a V,
}

/// What the checker sees of one value, whatever holds it.
enum View<'a, V: Checkable> {
    Bool(bool),
    Int(i64),
    Text(&'a str),
    /// An exact number held as one, which only a run-time value can be.
    Decimal,
    List(Vec<(V::At, &'a V)>),
    Map(Vec<Member<'a, V>>),
    /// Nothing, or a kind that fits only where any value does.
    Other,
}

/// A value the one checker can walk: a located document, a run-time value,
/// or JSON. The rules are written once, over what this trait shows.
trait Checkable: Sized {
    /// Where a value is: a place in a file, or a path from the root.
    type At: Clone;
    /// True for a document from a file. A message may then quote what the
    /// file wrote, and a drafter's open question is looked for.
    const FROM_FILE: bool;
    /// What kind of value this is, for a message.
    fn kind(&self) -> &'static str;
    /// What the checker sees of the value, which is at `at`.
    fn view<'a>(&'a self, at: &Self::At) -> View<'a, Self>;
}

impl Checkable for Located<Doc> {
    type At = Place;
    const FROM_FILE: bool = true;

    fn kind(&self) -> &'static str {
        self.value.kind()
    }

    fn view<'a>(&'a self, _at: &Place) -> View<'a, Self> {
        match &self.value {
            Doc::Null => View::Other,
            Doc::Bool(flag) => View::Bool(*flag),
            Doc::Int(number) => View::Int(*number),
            Doc::Text(text) => View::Text(text),
            Doc::List(items) => {
                View::List(items.iter().map(|item| (item.at.clone(), item)).collect())
            }
            Doc::Map(entries) => View::Map(
                entries
                    .iter()
                    .map(|entry| Member {
                        key: &entry.key.value,
                        key_at: entry.key.at.clone(),
                        at: entry.value.at.clone(),
                        value: &entry.value,
                    })
                    .collect(),
            ),
        }
    }
}

/// The path of the member `key` of the map at `at`.
fn member_path(at: &str, key: &str) -> String {
    if at.is_empty() {
        shown(key)
    } else {
        format!("{at}.{}", shown(key))
    }
}

/// What the checker sees of a run-time list and of a run-time map.
fn list_view<'a, V: Checkable<At = String>>(at: &str, items: &'a [V]) -> View<'a, V> {
    View::List(
        items
            .iter()
            .enumerate()
            .map(|(position, item)| (format!("{at}[{position}]"), item))
            .collect(),
    )
}

fn map_view<'a, V: Checkable<At = String> + 'a>(
    at: &str,
    members: impl Iterator<Item = (&'a str, &'a V)>,
) -> View<'a, V> {
    View::Map(
        members
            .map(|(key, value)| {
                let path = member_path(at, key);
                Member {
                    key,
                    key_at: path.clone(),
                    at: path,
                    value,
                }
            })
            .collect(),
    )
}

impl Checkable for Value {
    type At = String;
    const FROM_FILE: bool = false;

    fn kind(&self) -> &'static str {
        match self {
            Self::Null => "nothing",
            Self::Bool(_) => "true or false",
            Self::Int(_) => "whole number",
            Self::Text(_) => "text",
            Self::Decimal(_) => "decimal number",
            Self::Instant(_) => "moment",
            Self::Date(_) => "date",
            Self::Duration(_) => "length of time",
            Self::List(_) => "list",
            Self::Record(_) => "map",
        }
    }

    fn view<'a>(&'a self, at: &String) -> View<'a, Self> {
        match self {
            Self::Bool(flag) => View::Bool(*flag),
            Self::Int(number) => View::Int(*number),
            Self::Text(text) => View::Text(text),
            Self::Decimal(_) => View::Decimal,
            Self::List(items) => list_view(at, items),
            Self::Record(values) => map_view(at, values.iter()),
            Self::Null | Self::Instant(_) | Self::Date(_) | Self::Duration(_) => View::Other,
        }
    }
}

impl Checkable for serde_json::Value {
    type At = String;
    const FROM_FILE: bool = false;

    fn kind(&self) -> &'static str {
        match self {
            Self::Null => "nothing",
            Self::Bool(_) => "true or false",
            Self::Number(number) if number.as_i64().is_some() => "whole number",
            Self::Number(_) => "number that is not a whole number this engine holds",
            Self::String(_) => "text",
            Self::Array(_) => "list",
            Self::Object(_) => "map",
        }
    }

    fn view<'a>(&'a self, at: &String) -> View<'a, Self> {
        match self {
            Self::Bool(flag) => View::Bool(*flag),
            Self::Number(number) => number.as_i64().map_or(View::Other, View::Int),
            Self::String(text) => View::Text(text),
            Self::Array(items) => list_view(at, items),
            Self::Object(members) => {
                // In order of name, as named values are, whichever order the
                // JSON library keeps: the order of the findings is the same
                // in every build.
                let mut sorted: Vec<(&str, &Self)> = members
                    .iter()
                    .map(|(key, value)| (key.as_str(), value))
                    .collect();
                sorted.sort_by_key(|(key, _)| *key);
                map_view(at, sorted.into_iter())
            }
            Self::Null => View::Other,
        }
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

fn wrong_type<V: Checkable>(node: &Node, value: &V, at: &V::At, out: &mut Found<V::At>) {
    out.problems.push(Problem {
        code: codes::WRONG_TYPE,
        at: at.clone(),
        message: format!(
            "This value is a {}, and the schema needs {}.",
            value.kind(),
            describe(node)
        ),
        fix: format!("Write {} here.", describe(node)),
    });
}

/// How a message names text a value holds: as the file wrote it, or, for a
/// run-time value, not at all.
fn named<V: Checkable>(text: &str) -> String {
    if V::FROM_FILE {
        format!("`{}`", shown(text))
    } else {
        "This text".to_owned()
    }
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

/// The question, when `members` are an open question left by the drafter.
fn open_question<'a, V: Checkable>(members: &'a [Member<'a, V>]) -> Option<&'a str> {
    let [only] = members else { return None };
    (only.key == OPEN_QUESTION).then(|| match only.value.view(&only.at) {
        View::Text(question) => question,
        _ => "the system did not state this value",
    })
}

fn check<V: Checkable>(node: &Node, value: &V, at: &V::At, out: &mut Found<V::At>) {
    let view = value.view(at);
    if V::FROM_FILE
        && let View::Map(members) = &view
        && let Some(question) = open_question(members)
    {
        out.problems.push(Problem {
            code: codes::OPEN_QUESTION,
            at: at.clone(),
            message: format!("An open question stands where a value is needed: {question}"),
            fix: "Answer the question from the system or its owner, and write the value in its place.".to_owned(),
        });
        return;
    }
    match (node, view) {
        // Any value fits, and an open question is still refused wherever it
        // stands inside one.
        (Node::Any, View::List(items)) => {
            for (item_at, entry) in &items {
                check(node, *entry, item_at, out);
            }
        }
        (Node::Any, View::Map(members)) => {
            for member in &members {
                check(node, member.value, &member.at, out);
            }
        }
        (Node::Any, _)
        | (Node::Bool, View::Bool(_))
        | (Node::Text, View::Text(_))
        | (Node::Scalar | Node::Expression, View::Text(_) | View::Int(_) | View::Bool(_))
        | (Node::Decimal, View::Int(_) | View::Decimal) => {}
        (Node::Int { min, max }, View::Int(number)) => {
            if min.is_some_and(|min| number < min) || max.is_some_and(|max| number > max) {
                let range = match (min, max) {
                    (Some(min), Some(max)) => format!("from {min} to {max}"),
                    (Some(min), None) => format!("at least {min}"),
                    (None, Some(max)) => format!("at most {max}"),
                    (None, None) => "any whole number".to_owned(),
                };
                let subject = if V::FROM_FILE {
                    format!("The number {number}")
                } else {
                    "This number".to_owned()
                };
                out.problems.push(Problem {
                    code: codes::UNKNOWN_VALUE,
                    at: at.clone(),
                    message: format!("{subject} is outside what this field takes: {range}."),
                    fix: format!("Write a whole number {range}."),
                });
            }
        }
        (Node::Decimal, View::Text(text)) => {
            if Decimal::parse(text).is_err() {
                wrong_type(node, value, at, out);
            }
        }
        (Node::Name, View::Text(text)) => {
            if !is_dotted_name(text) {
                out.problems.push(Problem {
                    code: codes::NAME_MALFORMED,
                    at: at.clone(),
                    message: format!("{} is not a name.", named::<V>(text)),
                    fix: "Write the name in lowercase letters, digits, and underscores, starting with a letter, such as `order_id`.".to_owned(),
                });
            }
        }
        (Node::Words(words), View::Text(text)) => unknown_word::<V>(words, text, at, out),
        // In a file `movable: false` is the word `false`, not a truth value.
        (Node::Words(words), View::Bool(flag)) if V::FROM_FILE => {
            unknown_word::<V>(words, if flag { "true" } else { "false" }, at, out);
        }
        (Node::Refers(reference), View::Text(text)) => {
            out.references
                .push((*reference, text.to_owned(), at.clone()));
        }
        (Node::List { item, min }, View::List(items)) => {
            if items.len() < *min {
                out.problems.push(Problem {
                    code: codes::MISSING_FIELD,
                    at: at.clone(),
                    message: format!(
                        "This list has {} items and needs at least {min}.",
                        items.len()
                    ),
                    fix: "Add an item to the list.".to_owned(),
                });
            }
            for (item_at, entry) in &items {
                check(item, *entry, item_at, out);
            }
        }
        (Node::Object { fields, rest }, View::Map(members)) => {
            for member in &members {
                if let Some(field) = fields.iter().find(|field| field.name == member.key) {
                    check(&field.schema, member.value, &member.at, out);
                } else if let Some(rest) = rest {
                    check(rest, member.value, &member.at, out);
                } else {
                    let known: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
                    out.problems.push(Problem {
                        code: codes::UNKNOWN_KEY,
                        at: member.key_at.clone(),
                        message: format!(
                            "The key `{}` is not one this map takes.",
                            shown(member.key)
                        ),
                        fix: fix_for_unknown(member.key, &known),
                    });
                }
            }
            for field in fields.iter().filter(|field| field.required) {
                if !members.iter().any(|member| member.key == field.name) {
                    out.problems.push(Problem {
                        code: codes::MISSING_FIELD,
                        at: at.clone(),
                        message: format!("The required field `{}` is missing.", field.name),
                        fix: format!("Add `{}:` with {}.", field.name, describe(&field.schema)),
                    });
                }
            }
        }
        (Node::Either(choices), _) => {
            let mut best: Option<Found<V::At>> = None;
            for choice in choices {
                let mut tried = Found::default();
                check(choice, value, at, &mut tried);
                if tried.problems.is_empty() {
                    out.references.extend(tried.references);
                    return;
                }
                if best
                    .as_ref()
                    .is_none_or(|best| tried.problems.len() < best.problems.len())
                {
                    best = Some(tried);
                }
            }
            match best {
                Some(best) if best.problems.len() > 1 || choices.len() == 1 => {
                    out.problems.extend(best.problems);
                }
                _ => wrong_type(node, value, at, out),
            }
        }
        _ => wrong_type(node, value, at, out),
    }
}

fn unknown_word<V: Checkable>(words: &[String], text: &str, at: &V::At, out: &mut Found<V::At>) {
    if words.iter().any(|word| word == text) {
        return;
    }
    let known: Vec<&str> = words.iter().map(String::as_str).collect();
    out.problems.push(Problem {
        code: codes::UNKNOWN_VALUE,
        at: at.clone(),
        message: format!(
            "{} is not one of the words this field takes.",
            named::<V>(text)
        ),
        fix: fix_for_unknown(text, &known),
    });
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
    use a2m_core::{Decimal, Failure, FailureClass, FailureCode, Value, Values};

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

    /// The input of an imaginary action that enters an order.
    fn input() -> JsonSchema {
        let line = JsonSchema::object()
            .required("sku", JsonSchema::name())
            .required("quantity", JsonSchema::int_between(1, 99))
            .optional("price", JsonSchema::decimal());
        JsonSchema::object()
            .required("order_id", JsonSchema::text())
            .required("lines", JsonSchema::non_empty_list(line))
            .optional("status", JsonSchema::words(&["draft", "verified"]))
            .optional("urgent", JsonSchema::bool())
            .optional("surface", JsonSchema::surface())
            .optional("labels", JsonSchema::map_of(JsonSchema::text()))
            .optional(
                "key",
                JsonSchema::either(vec![
                    JsonSchema::name(),
                    JsonSchema::non_empty_list(JsonSchema::name()),
                ]),
            )
    }

    fn line(sku: &str, quantity: i64) -> Value {
        Value::Record(Values::new().with("sku", sku).with("quantity", quantity))
    }

    fn fitting() -> Values {
        let priced = Values::new()
            .with("sku", "widget")
            .with("quantity", 2_i64)
            .with("price", Decimal::parse("12.50").unwrap());
        Values::new()
            .with("order_id", "SO-1001")
            .with("lines", vec![Value::Record(priced), line("gadget", 99)])
            .with("status", "draft")
            .with("urgent", true)
            .with("surface", "desk")
            .with("labels", Values::new().with("open_question", "none"))
            .with("key", vec![Value::from("order_id")])
    }

    fn fitting_json() -> serde_json::Value {
        serde_json::json!({
            "order_id": "SO-1001",
            "lines": [
                {"sku": "widget", "quantity": 2, "price": "12.50"},
                {"sku": "gadget", "quantity": 99, "price": 12},
            ],
            "status": "verified",
            "urgent": false,
            "labels": {"colour": "red"},
            "key": "order_id",
        })
    }

    #[test]
    fn run_time_values_and_json_that_fit_a_schema_pass() {
        assert_eq!(input().check_values(&fitting()), Ok(()));
        assert_eq!(input().check_json(&fitting_json()), Ok(()));
        assert_eq!(
            JsonSchema::text().check_value(&Value::from("SO-1001")),
            Ok(())
        );
        assert_eq!(
            JsonSchema::any().check_value(&Value::Date(a2m_core::Date::new(2026, 11, 17).unwrap())),
            Ok(())
        );
    }

    #[test]
    fn each_kind_of_misfit_in_run_time_values_is_refused_with_the_path_of_the_field() {
        type Change = fn(&mut Values);
        fn lines(values: &mut Values, change: fn(&mut Vec<Value>)) {
            let Some(Value::List(mut items)) = values.remove("lines") else {
                panic!("no lines");
            };
            change(&mut items);
            values.insert("lines", items);
        }
        let cases: [(Change, &str, &str, &str); 10] = [
            (
                |values| {
                    values.remove("order_id");
                },
                "",
                "kernel.missing_field",
                "`order_id`",
            ),
            (
                |values| {
                    values.insert("order_id", 1001_i64);
                },
                "order_id",
                "kernel.wrong_type",
                "whole number",
            ),
            (
                |values| lines(values, Vec::clear),
                "lines",
                "kernel.missing_field",
                "at least 1",
            ),
            (
                |values| lines(values, |items| items[0] = line("widget", 100)),
                "lines[0].quantity",
                "kernel.unknown_value",
                "from 1 to 99",
            ),
            (
                |values| lines(values, |items| items[1] = line("Not A Name", 1)),
                "lines[1].sku",
                "kernel.name_malformed",
                "not a name",
            ),
            (
                |values| {
                    lines(values, |items| {
                        let day = a2m_core::Date::new(2026, 11, 17).unwrap();
                        items[1] = Value::Record(
                            Values::new()
                                .with("sku", "gadget")
                                .with("quantity", 1_i64)
                                .with("price", day),
                        );
                    });
                },
                "lines[1].price",
                "kernel.wrong_type",
                "a date",
            ),
            (
                |values| {
                    values.insert("status", "secret-word");
                },
                "status",
                "kernel.unknown_value",
                "one of the words",
            ),
            (
                |values| {
                    values.insert("statuss", "draft");
                },
                "statuss",
                "kernel.unknown_key",
                "`statuss`",
            ),
            (
                |values| {
                    values.insert("labels", Values::new().with("colour", 7_i64));
                },
                "labels.colour",
                "kernel.wrong_type",
                "needs text",
            ),
            (
                |values| {
                    values.insert("key", Values::new());
                },
                "key",
                "kernel.wrong_type",
                "a name, or a list",
            ),
        ];
        for (change, path, code, says) in cases {
            let mut values = fitting();
            change(&mut values);
            let misfits = input().check_values(&values).unwrap_err();
            let [misfit] = misfits.as_slice() else {
                panic!("{path}: {misfits}");
            };
            assert_eq!((misfit.path.as_str(), misfit.code.as_str()), (path, code));
            assert!(misfit.message.contains(says), "{misfit}");
            assert!(!misfit.fix.is_empty(), "{path}");
            // A message names the kind and the path, never the value.
            for value in ["Not A Name", "secret-word", "100", "1001"] {
                assert!(!misfits.to_string().contains(value), "{misfits}");
            }
            let failure = Failure::from(misfits);
            assert_eq!(failure.class(), FailureClass::Input);
            assert_eq!(failure.code(), &FailureCode::SCHEMA_INVALID);
            assert!(
                path.is_empty() || failure.message().contains(path),
                "{failure}"
            );
            assert!(!failure.next_step().is_empty());
        }
    }

    #[test]
    fn json_is_checked_as_the_published_schema_describes_it() {
        type Change = fn(&mut serde_json::Value);
        let cases: [(Change, &str, &str); 6] = [
            // A number with a fraction is no exact number and no whole one.
            (
                |json| json["lines"][0]["price"] = serde_json::json!(12.5),
                "lines[0].price",
                "kernel.wrong_type",
            ),
            (
                |json| json["lines"][1]["quantity"] = serde_json::json!(1.0),
                "lines[1].quantity",
                "kernel.wrong_type",
            ),
            (
                |json| json["lines"][1]["quantity"] = serde_json::json!("2"),
                "lines[1].quantity",
                "kernel.wrong_type",
            ),
            // In JSON a truth value is never one of a list of words.
            (
                |json| json["status"] = serde_json::json!(true),
                "status",
                "kernel.wrong_type",
            ),
            (
                |json| json["urgent"] = serde_json::Value::Null,
                "urgent",
                "kernel.wrong_type",
            ),
            (
                |json| json["lines"][0]["note"] = serde_json::json!("x"),
                "lines[0].note",
                "kernel.unknown_key",
            ),
        ];
        for (change, path, code) in cases {
            let mut json = fitting_json();
            change(&mut json);
            let misfits = input().check_json(&json).unwrap_err();
            let found: Vec<(&str, &str)> = misfits
                .as_slice()
                .iter()
                .map(|misfit| (misfit.path.as_str(), misfit.code.as_str()))
                .collect();
            assert_eq!(found, vec![(path, code)]);
        }
        // The root itself, and every place at once, in the order walked.
        let misfits = input().check_json(&serde_json::json!([])).unwrap_err();
        assert_eq!(misfits.paths().collect::<Vec<_>>(), vec![""]);
        let misfits = input()
            .check_json(
                &serde_json::json!({"order_id": 1, "lines": [{"sku": "a"}], "urgent": "no"}),
            )
            .unwrap_err();
        assert_eq!(
            misfits.paths().collect::<Vec<_>>(),
            vec!["lines[0]", "order_id", "urgent"]
        );
        let failure = Failure::from(misfits);
        assert!(failure.message().contains("`lines[0]`"), "{failure}");
        assert!(failure.message().contains("`quantity`"), "{failure}");
        assert!(failure.message().contains("2 more places"), "{failure}");
    }

    #[test]
    fn a_file_and_run_time_values_break_the_same_rule_in_the_same_words() {
        let checked = check("screen: order.form\ncontrol: text\nfields: [a]\n");
        let from_file = &checked.diagnostics[0];
        let values = Values::new()
            .with("screen", "order.form")
            .with("control", "text")
            .with("fields", vec![Value::from("a")]);
        let misfits = schema().check_values(&values).unwrap_err();
        let at_run_time = &misfits.as_slice()[0];
        assert_eq!(from_file.code, at_run_time.code);
        assert_eq!(from_file.message, at_run_time.message);
        assert_eq!(from_file.fix, at_run_time.fix);
        assert_eq!(at_run_time.path, "fields");
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
