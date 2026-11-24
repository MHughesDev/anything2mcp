//! The shape of a body: a small, closed subset of JSON Schema, written in a
//! target file, compiled with the entry that writes it, and checked against
//! a value a system answers or a page sends (decision 0394, point 1).
//!
//! The words a shape takes are `type`, `nullable`, `properties`, `required`,
//! `additionalProperties`, `items`, and `enum`. Any other word is refused
//! where it is written: a rule nobody checks must not look checked. The
//! HTTP pack's endpoint entries and the web pack's page requests both read
//! their shapes here, and a pack may take fewer of these words than the
//! module does; it never takes more.
//!
//! A value is checked through [`ShapeValue`], so that each caller keeps its
//! own reading of JSON: the HTTP pack keeps a number as the text it was
//! written with, and a `serde_json::Value` is read as it stands. A failed
//! check is a [`Misfit`]: where the value does not fit and how, and never
//! the value itself, so that each pack words the failure in its own terms.

use std::borrow::Cow;
use std::collections::BTreeMap;

use a2m_files::{Doc, Located, Place};
use serde::{Deserialize, Serialize};

use crate::codes::shown;

/// What a value must be: the word of a shape's `type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Any value, `null` among them.
    Any,
    /// `null`.
    Null,
    /// `true` or `false`.
    Boolean,
    /// A whole number, written with no fraction and no exponent, from
    /// `-2^63` to `2^63 - 1`, and not `-0`.
    Integer,
    /// Any number.
    Number,
    /// Text.
    String,
    /// A list.
    Array,
    /// A map of names to values.
    Object,
}

impl Kind {
    /// Every word of `type`, in the order a message lists them.
    pub const WORDS: &'static [&'static str] = &[
        "object", "array", "string", "integer", "number", "boolean", "null", "any",
    ];

    /// The kind a word of `type` names, if it names one.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        Some(match word {
            "any" => Self::Any,
            "null" => Self::Null,
            "boolean" => Self::Boolean,
            "integer" => Self::Integer,
            "number" => Self::Number,
            "string" => Self::String,
            "array" => Self::Array,
            "object" => Self::Object,
            _ => return None,
        })
    }

    /// The word of `type` that names the kind.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Null => "null",
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::String => "string",
            Self::Array => "array",
            Self::Object => "object",
        }
    }
}

/// The declared shape of a body, or of one value inside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shape {
    /// What the value must be.
    pub kind: Kind,
    /// Whether `null` fits as well.
    pub nullable: bool,
    /// The shapes of an object's named values.
    pub properties: BTreeMap<String, Shape>,
    /// The names an object must hold; each is listed under `properties`.
    pub required: Vec<String>,
    /// Whether an object may hold only the names listed: `additionalProperties:
    /// false`.
    pub closed: bool,
    /// The shape of each item of a list; any item when `None`.
    pub items: Option<Box<Shape>>,
    /// The texts a string may be; any text when empty.
    pub one_of: Vec<String>,
}

/// Why a shape as written is not one this module checks: the place of the
/// word or value at fault, and a sentence that quotes no more than a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeProblem {
    /// Where it is written.
    pub at: Place,
    /// What is wrong with it.
    pub message: String,
}

/// A value as [`Shape::check`] sees it, one level at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueView<'a, V> {
    /// `null`.
    Null,
    /// `true` or `false`.
    Boolean,
    /// A number, as JSON writes it: a fraction or an exponent makes it no
    /// integer.
    Number(Cow<'a, str>),
    /// Text.
    Text(&'a str),
    /// A list, in its order.
    List(&'a [V]),
    /// A map, in the order its reader keeps.
    Object(Vec<(&'a str, &'a V)>),
}

impl<V> ValueView<'_, V> {
    /// The word of the value's kind, for a misfit.
    #[must_use]
    pub const fn kind_word(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Boolean => "boolean",
            Self::Number(_) => "number",
            Self::Text(_) => "string",
            Self::List(_) => "array",
            Self::Object(_) => "object",
        }
    }
}

/// A value that can be checked against a shape.
pub trait ShapeValue: Sized {
    /// The value, one level deep.
    fn view(&self) -> ValueView<'_, Self>;
}

impl ShapeValue for serde_json::Value {
    fn view(&self) -> ValueView<'_, Self> {
        use serde_json::Value as Json;
        match self {
            Json::Null => ValueView::Null,
            Json::Bool(_) => ValueView::Boolean,
            Json::Number(number) => ValueView::Number(Cow::Owned(number.to_string())),
            Json::String(text) => ValueView::Text(text),
            Json::Array(items) => ValueView::List(items),
            Json::Object(entries) => ValueView::Object(
                entries
                    .iter()
                    .map(|(name, value)| (name.as_str(), value))
                    .collect(),
            ),
        }
    }
}

/// One step of the way from the top of a value to where it does not fit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathStep {
    /// The named value of an object.
    Name(String),
    /// The item of a list at this position, from 0.
    Item(usize),
}

/// How a value does not fit its shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MisfitKind {
    /// The value is of another kind: the word of the kind it is, and the
    /// kind declared.
    Kind {
        /// The word of the value's kind.
        found: &'static str,
        /// The kind the shape declares.
        declared: Kind,
    },
    /// The text is not one the shape's `enum` lists.
    NotOneOf,
    /// The object lacks this required name.
    Lacks(String),
    /// The object holds a name a closed shape does not list.
    Undeclared,
}

/// Where a value does not fit its shape, and how. It never holds the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Misfit {
    /// The way from the top of the value; empty for the top itself.
    pub path: Vec<PathStep>,
    /// How it does not fit.
    pub kind: MisfitKind,
}

impl Misfit {
    /// The path written after `root`: `.name` for a named value and
    /// `[n]` for an item, as in `the body.lines[1].n`.
    #[must_use]
    pub fn path_from(&self, root: &str) -> String {
        let mut out = root.to_owned();
        for step in &self.path {
            match step {
                PathStep::Name(name) => {
                    out.push('.');
                    out.push_str(name);
                }
                PathStep::Item(position) => out.push_str(&format!("[{position}]")),
            }
        }
        out
    }
}

impl Shape {
    /// A shape of `kind` that says nothing more.
    #[must_use]
    pub fn of(kind: Kind) -> Self {
        Self {
            kind,
            nullable: false,
            properties: BTreeMap::new(),
            required: Vec::new(),
            closed: false,
            items: None,
            one_of: Vec::new(),
        }
    }

    /// Reads a shape written in a target file.
    ///
    /// # Errors
    ///
    /// The first problem, at its place: a shape that is not a map, a
    /// `type` that is missing or not one of [`Kind::WORDS`], a word other
    /// than the seven or one its `type` does not take, a value of the wrong
    /// form under a word, or a required name not listed under `properties`.
    pub fn from_doc(doc: &Located<Doc>) -> Result<Self, ShapeProblem> {
        let problem = |at: &Place, message: String| ShapeProblem {
            at: at.clone(),
            message,
        };
        if !matches!(doc.value, Doc::Map(_)) {
            return Err(problem(
                &doc.at,
                format!(
                    "A shape is a map with a `type`; this is a {}.",
                    doc.value.kind()
                ),
            ));
        }
        let Some(kind_doc) = doc.get("type") else {
            return Err(problem(&doc.at, "A shape states its `type`.".to_owned()));
        };
        let Some(kind) = kind_doc.as_text().and_then(Kind::from_word) else {
            return Err(problem(
                &kind_doc.at,
                "A shape's `type` is one of `object`, `array`, `string`, `integer`, `number`, `boolean`, `null`, `any`.".to_owned(),
            ));
        };
        let mut shape = Self::of(kind);
        let mut required_at = Vec::new();
        for entry in doc.entries() {
            let value = &entry.value;
            match entry.key.value.as_str() {
                "type" => {}
                "nullable" => {
                    shape.nullable = value.as_bool().ok_or_else(|| {
                        problem(&value.at, "`nullable` is true or false.".to_owned())
                    })?;
                }
                "additionalProperties" => {
                    shape.closed = !value.as_bool().ok_or_else(|| {
                        problem(
                            &value.at,
                            "`additionalProperties` is true or false.".to_owned(),
                        )
                    })?;
                }
                "properties" if kind == Kind::Object => {
                    if !matches!(value.value, Doc::Map(_)) {
                        return Err(problem(
                            &value.at,
                            "`properties` is a map of names to shapes.".to_owned(),
                        ));
                    }
                    for property in value.entries() {
                        shape
                            .properties
                            .insert(property.key.value.clone(), Self::from_doc(&property.value)?);
                    }
                }
                "required" if kind == Kind::Object => {
                    if !matches!(value.value, Doc::List(_)) {
                        return Err(problem(
                            &value.at,
                            "`required` lists property names.".to_owned(),
                        ));
                    }
                    for item in value.items() {
                        let name = item.as_text().ok_or_else(|| {
                            problem(&item.at, "`required` lists property names.".to_owned())
                        })?;
                        shape.required.push(name.to_owned());
                        required_at.push(item.at.clone());
                    }
                }
                "items" if kind == Kind::Array => {
                    shape.items = Some(Box::new(Self::from_doc(value)?));
                }
                "enum" if kind == Kind::String => {
                    if !matches!(value.value, Doc::List(_)) {
                        return Err(problem(&value.at, "`enum` lists texts.".to_owned()));
                    }
                    for item in value.items() {
                        let word = item
                            .as_text()
                            .ok_or_else(|| problem(&item.at, "`enum` lists texts.".to_owned()))?;
                        shape.one_of.push(word.to_owned());
                    }
                }
                other => {
                    return Err(problem(
                        &entry.key.at,
                        format!(
                            "The word `{}` is not one a shape of type `{}` takes, so it would not be checked.",
                            shown(other),
                            kind.word()
                        ),
                    ));
                }
            }
        }
        if let Some((missing, at)) = shape
            .required
            .iter()
            .zip(&required_at)
            .find(|(name, _)| !shape.properties.contains_key(*name))
        {
            return Err(problem(
                at,
                format!(
                    "The shape requires `{}`, which it does not list under `properties`.",
                    shown(missing)
                ),
            ));
        }
        Ok(shape)
    }

    /// The shape at the end of a path of property names, where `*` steps
    /// into the items of an array; with whether every step of the path is
    /// certain to be there.
    #[must_use]
    pub fn at_path(&self, path: &[String]) -> Option<(&Self, bool)> {
        let mut shape = self;
        let mut certain = self.never_null();
        for part in path {
            if part == "*" {
                shape = shape.items.as_deref()?;
            } else {
                certain = certain && shape.required.contains(part);
                shape = shape.properties.get(part)?;
            }
            certain = certain && shape.never_null();
        }
        Some((shape, certain))
    }

    /// Whether a value of this shape can never be `null`: one that is not
    /// `nullable` and is of a type other than `any`. A shape of the type
    /// `any`, as an untyped `{}` is drafted, admits any value, `null` among
    /// them, as [`Shape::check`] does (decision 0305).
    fn never_null(&self) -> bool {
        !self.nullable && self.kind != Kind::Any
    }

    /// Checks a value against the shape: the first place it does not fit,
    /// looking at an object's required names before its values.
    ///
    /// # Errors
    ///
    /// The [`Misfit`]: where, and how. It never holds the value.
    pub fn check<V: ShapeValue>(&self, value: &V) -> Result<(), Misfit> {
        let mut path = Vec::new();
        self.check_at(value, &mut path)
    }

    fn check_at<V: ShapeValue>(&self, value: &V, path: &mut Vec<PathStep>) -> Result<(), Misfit> {
        let view = value.view();
        let misfit = |path: &[PathStep], kind: MisfitKind| Misfit {
            path: path.to_vec(),
            kind,
        };
        if self.kind == Kind::Any || (self.nullable && matches!(view, ValueView::Null)) {
            return Ok(());
        }
        let fits = match (self.kind, &view) {
            (Kind::Null, ValueView::Null)
            | (Kind::Boolean, ValueView::Boolean)
            | (Kind::Number, ValueView::Number(_))
            | (Kind::String, ValueView::Text(_))
            | (Kind::Array, ValueView::List(_))
            | (Kind::Object, ValueView::Object(_)) => true,
            (Kind::Integer, ValueView::Number(text)) => is_integer(text),
            _ => false,
        };
        if !fits {
            return Err(misfit(
                path,
                MisfitKind::Kind {
                    found: view.kind_word(),
                    declared: self.kind,
                },
            ));
        }
        match view {
            ValueView::Text(text)
                if !self.one_of.is_empty() && !self.one_of.iter().any(|one| one == text) =>
            {
                Err(misfit(path, MisfitKind::NotOneOf))
            }
            ValueView::List(items) => {
                let Some(shape) = &self.items else {
                    return Ok(());
                };
                for (position, item) in items.iter().enumerate() {
                    path.push(PathStep::Item(position));
                    shape.check_at(item, path)?;
                    path.pop();
                }
                Ok(())
            }
            ValueView::Object(entries) => {
                if let Some(missing) = self
                    .required
                    .iter()
                    .find(|name| !entries.iter().any(|(key, _)| key == *name))
                {
                    return Err(misfit(path, MisfitKind::Lacks(missing.clone())));
                }
                for (name, value) in entries {
                    match self.properties.get(name) {
                        Some(shape) => {
                            path.push(PathStep::Name(name.to_owned()));
                            shape.check_at(value, path)?;
                            path.pop();
                        }
                        None if self.closed => {
                            return Err(misfit(path, MisfitKind::Undeclared));
                        }
                        None => {}
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// Whether a number, as JSON writes it, is an integer: no fraction, no
/// exponent, not `-0`, and within 64 bits. A reader that keeps a number's
/// text and one that parses it into a machine number agree on each.
fn is_integer(text: &str) -> bool {
    text != "-0" && text.parse::<i64>().is_ok()
}

#[cfg(test)]
mod tests {
    use a2m_files::{RelPath, parse_yaml};
    use serde_json::{Value, json};

    use super::{Kind, Misfit, MisfitKind, PathStep, Shape, ShapeProblem};

    fn shape(text: &str) -> Result<Shape, ShapeProblem> {
        let docs = parse_yaml(&RelPath::new("endpoints/x.yaml").unwrap(), text).unwrap();
        Shape::from_doc(&docs[0])
    }

    fn parsed(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    const ITEM: &str = "
type: object
required: [item_code, quantity]
properties:
  item_code: {type: string}
  quantity: {type: integer}
  length_in: {type: string, nullable: true}
  status: {type: string, enum: [draft, submitted]}
  lines: {type: array, items: {type: object, required: [n], properties: {n: {type: number}}}}
";

    #[test]
    fn a_body_that_fits_passes_and_one_that_does_not_is_named_without_its_content() {
        let shape = shape(ITEM).unwrap();
        let fits = |text: &str| shape.check(&parsed(text));
        assert_eq!(
            fits(
                r#"{"item_code":"L-1","quantity":2,"length_in":null,"extra":true,"lines":[{"n":1.5}]}"#
            ),
            Ok(())
        );
        let wrong = fits(r#"{"item_code":"PLANTED","quantity":"2"}"#).unwrap_err();
        assert_eq!(wrong.path_from("the body"), "the body.quantity");
        assert_eq!(
            wrong.kind,
            MisfitKind::Kind {
                found: "string",
                declared: Kind::Integer
            }
        );
        assert!(!format!("{wrong:?}").contains("PLANTED"), "{wrong:?}");
        assert_eq!(
            fits(r#"{"quantity":2}"#).unwrap_err().kind,
            MisfitKind::Lacks("item_code".to_owned())
        );
        assert!(fits(r#"{"item_code":"L","quantity":2.5}"#).is_err());
        let nested = fits(r#"{"item_code":"L","quantity":2,"lines":[{"n":1},{"n":"x"}]}"#);
        assert_eq!(
            nested.unwrap_err().path_from("the body"),
            "the body.lines[1].n"
        );
        assert!(fits("[]").is_err());
    }

    /// The row's proof: an unnamed field under `additionalProperties:
    /// false`, a field of another kind, and a value outside an `enum` are
    /// each refused.
    #[test]
    fn a_closed_shape_refuses_an_unnamed_field_another_kind_and_a_text_outside_its_enum() {
        let shape = shape(
            "type: object
additionalProperties: false
required: [status, op]
properties:
  status: {type: string}
  op: {type: string, enum: [search]}
",
        )
        .unwrap();
        assert_eq!(
            shape.check(&json!({"status": "open", "op": "search"})),
            Ok(())
        );
        let refused = |value: Value| shape.check(&value).unwrap_err();
        assert_eq!(
            refused(json!({"status": "open", "op": "search", "limit": 5})),
            Misfit {
                path: Vec::new(),
                kind: MisfitKind::Undeclared
            }
        );
        assert_eq!(
            refused(json!({"status": 7, "op": "search"})),
            Misfit {
                path: vec![PathStep::Name("status".to_owned())],
                kind: MisfitKind::Kind {
                    found: "number",
                    declared: Kind::String
                }
            }
        );
        assert_eq!(
            refused(json!({"status": "open", "op": "archive"})),
            Misfit {
                path: vec![PathStep::Name("op".to_owned())],
                kind: MisfitKind::NotOneOf
            }
        );
    }

    /// The row's proof: a word outside the seven is refused where it is
    /// written.
    #[test]
    fn a_word_outside_the_seven_is_refused_where_it_is_written() {
        let problem = shape("type: string\npattern: '^a'\n").unwrap_err();
        assert_eq!((problem.at.line, problem.at.column), (2, 1));
        assert!(problem.message.contains("`pattern`"), "{problem:?}");
        let nested =
            shape("type: object\nproperties:\n  a: {type: string, format: date}\n").unwrap_err();
        assert_eq!(nested.at.line, 3);
        assert!(nested.message.contains("`format`"), "{nested:?}");
        assert!(shape("type: text\n").is_err());
        assert!(shape("properties: {}\n").is_err());
        assert!(shape("type: object\nrequired: [a]\n").is_err());
    }

    /// Where the two copies differed (decision 0999): the HTTP pack's took
    /// a whole number of any size, and `-0`, as an integer; the web pack's
    /// only one that fits in 64 bits, and not `-0`, which its reader makes
    /// a float. The stricter holds, for every reader.
    #[test]
    fn an_integer_is_one_that_fits_in_sixty_four_bits_with_no_fraction() {
        let integer = Shape::of(Kind::Integer);
        for fits in ["0", "-7", "9223372036854775807", "-9223372036854775808"] {
            assert_eq!(integer.check(&parsed(fits)), Ok(()), "{fits}");
        }
        for refused in [
            "-0",
            "9223372036854775808",
            "-9223372036854775809",
            "1.0",
            "1e3",
        ] {
            assert!(integer.check(&parsed(refused)).is_err(), "{refused}");
        }
        assert_eq!(Shape::of(Kind::Number).check(&parsed("1e3")), Ok(()));
    }

    /// Where the two copies differed (decision 0999): the HTTP pack's read
    /// `properties` that is not a map, and `required` or `enum` that is not
    /// a list, as if nothing were written; the web pack's refused each. The
    /// stricter holds, and each is refused at the value.
    #[test]
    fn a_word_whose_value_is_of_the_wrong_form_is_refused_at_the_value() {
        for (text, line) in [
            ("type: object\nproperties: [a]\n", 2),
            (
                "type: object\nproperties: {a: {type: string}}\nrequired: a\n",
                3,
            ),
            ("type: string\nenum: draft\n", 2),
        ] {
            let problem = shape(text).unwrap_err();
            assert_eq!(problem.at.line, line, "{text}: {problem:?}");
        }
    }

    /// Where the two copies differed (decision 0999): a required name not
    /// listed under `properties` was refused at the shape by one and at the
    /// name by the other. It is refused at the name.
    #[test]
    fn a_required_name_not_listed_is_refused_at_the_name() {
        let problem =
            shape("type: object\nproperties: {a: {type: string}}\nrequired:\n  - a\n  - b\n")
                .unwrap_err();
        assert_eq!(problem.at.line, 5, "{problem:?}");
        assert!(problem.message.contains("`b`"), "{problem:?}");
    }

    #[test]
    fn a_path_finds_its_shape_and_says_whether_it_is_certain() {
        let shape = shape(ITEM).unwrap();
        let path = |text: &str| -> Vec<String> { text.split('.').map(str::to_owned).collect() };
        assert!(shape.at_path(&path("item_code")).unwrap().1);
        assert!(!shape.at_path(&path("length_in")).unwrap().1);
        assert!(!shape.at_path(&path("status")).unwrap().1);
        assert!(shape.at_path(&path("lines.*.n")).is_some());
        assert!(shape.at_path(&path("missing")).is_none());
    }

    /// Decision 0305: a required property of the type `any`, the shape
    /// of an untyped `{}`, admits `null` as the check does, so it is not
    /// certain to be there.
    #[test]
    fn a_required_property_with_no_type_is_not_certain_because_it_admits_null() {
        let untyped = shape("type: any\n").unwrap();
        let shape = shape(
            "type: object
required: [any, code]
properties: {any: {type: any}, code: {type: string}}
",
        )
        .unwrap();
        let path = |text: &str| -> Vec<String> { vec![text.to_owned()] };
        assert!(!shape.at_path(&path("any")).unwrap().1);
        assert!(shape.at_path(&path("code")).unwrap().1);
        assert_eq!(shape.check(&json!({"any": null, "code": "L"})), Ok(()));
        assert!(!untyped.at_path(&[]).unwrap().1);
    }
}
