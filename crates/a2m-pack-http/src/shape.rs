//! The shape of a request or response body: a small, closed subset of JSON
//! Schema, written inside an endpoint entry, compiled into the endpoint's
//! template, and checked by the driver against what the system answers.
//!
//! The words accepted are `type`, `nullable`, `properties`, `required`,
//! `additionalProperties`, `items`, and `enum`. Any other word is refused
//! where it is written: a rule this pack does not check must not look
//! checked.

use std::collections::BTreeMap;

use a2m_files::{Doc, Located, Place};
use serde::{Deserialize, Serialize};

use crate::json::Json;

/// What a value must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    Any,
    Null,
    Boolean,
    Integer,
    Number,
    String,
    Array,
    Object,
}

impl Kind {
    fn from_word(word: &str) -> Option<Self> {
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

    pub(crate) const fn word(self) -> &'static str {
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
pub(crate) struct Shape {
    pub kind: Kind,
    pub nullable: bool,
    pub properties: BTreeMap<String, Shape>,
    pub required: Vec<String>,
    /// Whether an object may hold only the properties listed.
    pub closed: bool,
    pub items: Option<Box<Shape>>,
    /// The texts a string may be; empty for any.
    pub one_of: Vec<String>,
}

/// Why a shape as written is not one this pack checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShapeProblem {
    pub at: Place,
    pub message: String,
}

impl Shape {
    pub(crate) fn of(kind: Kind) -> Self {
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
    pub(crate) fn from_doc(doc: &Located<Doc>) -> Result<Self, ShapeProblem> {
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
                    for property in value.entries() {
                        shape
                            .properties
                            .insert(property.key.value.clone(), Self::from_doc(&property.value)?);
                    }
                }
                "required" if kind == Kind::Object => {
                    for item in value.items() {
                        let name = item.as_text().ok_or_else(|| {
                            problem(&item.at, "`required` lists property names.".to_owned())
                        })?;
                        shape.required.push(name.to_owned());
                    }
                }
                "items" if kind == Kind::Array => {
                    shape.items = Some(Box::new(Self::from_doc(value)?));
                }
                "enum" if kind == Kind::String => {
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
                            "The word `{}` is not one this pack checks in a shape of type `{}`.",
                            crate::codes::shown(other),
                            kind.word()
                        ),
                    ));
                }
            }
        }
        if let Some(missing) = shape
            .required
            .iter()
            .find(|name| !shape.properties.contains_key(*name))
        {
            return Err(problem(
                &doc.at,
                format!(
                    "The shape requires `{}`, which it does not list under `properties`.",
                    crate::codes::shown(missing)
                ),
            ));
        }
        Ok(shape)
    }

    /// The shape at the end of a path of property names, where `*` steps
    /// into the items of an array; with whether every step of the path is
    /// certain to be there.
    pub(crate) fn at_path(&self, path: &[String]) -> Option<(&Self, bool)> {
        let mut shape = self;
        let mut certain = !self.nullable;
        for part in path {
            if part == "*" {
                shape = shape.items.as_deref()?;
            } else {
                certain = certain && shape.required.contains(part);
                shape = shape.properties.get(part)?;
            }
            certain = certain && !shape.nullable;
        }
        Some((shape, certain))
    }

    /// Checks a value against the shape. The error says where the value
    /// does not fit and what was expected; it never quotes the value.
    pub(crate) fn check(&self, json: &Json) -> Result<(), String> {
        let mut path = String::from("the body");
        self.check_at(json, &mut path)
    }

    fn check_at(&self, json: &Json, path: &mut String) -> Result<(), String> {
        if self.kind == Kind::Any || (self.nullable && *json == Json::Null) {
            return Ok(());
        }
        let fits = match (self.kind, json) {
            (Kind::Null, Json::Null)
            | (Kind::Boolean, Json::Bool(_))
            | (Kind::Number, Json::Number(_))
            | (Kind::String, Json::Text(_))
            | (Kind::Array, Json::List(_))
            | (Kind::Object, Json::Object(_)) => true,
            (Kind::Integer, number @ Json::Number(_)) => number.is_whole(),
            _ => false,
        };
        if !fits {
            return Err(format!(
                "{path} is a {}, and the endpoint declares a {}",
                json.kind(),
                self.kind.word()
            ));
        }
        match json {
            Json::Text(text) if !self.one_of.is_empty() && !self.one_of.contains(text) => Err(
                format!("{path} is not one of the texts the endpoint declares"),
            ),
            Json::List(items) => {
                let Some(shape) = &self.items else {
                    return Ok(());
                };
                let length = path.len();
                for (position, item) in items.iter().enumerate() {
                    path.push_str(&format!("[{position}]"));
                    shape.check_at(item, path)?;
                    path.truncate(length);
                }
                Ok(())
            }
            Json::Object(entries) => {
                if let Some(missing) = self
                    .required
                    .iter()
                    .find(|name| !entries.iter().any(|(key, _)| key == *name))
                {
                    return Err(format!(
                        "{path} lacks `{missing}`, which the endpoint declares as required"
                    ));
                }
                let length = path.len();
                for (name, value) in entries {
                    match self.properties.get(name) {
                        Some(shape) => {
                            path.push('.');
                            path.push_str(name);
                            shape.check_at(value, path)?;
                            path.truncate(length);
                        }
                        None if self.closed => {
                            return Err(format!(
                                "{path} holds a property the endpoint does not declare"
                            ));
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

#[cfg(test)]
mod tests {
    use a2m_files::{RelPath, parse_yaml};

    use super::Shape;
    use crate::json::Json;

    fn shape(text: &str) -> Result<Shape, super::ShapeProblem> {
        let docs = parse_yaml(&RelPath::new("endpoints/x.yaml").unwrap(), text).unwrap();
        Shape::from_doc(&docs[0])
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
        let fits = |text: &str| shape.check(&Json::parse(text.as_bytes()).unwrap());
        assert_eq!(
            fits(
                r#"{"item_code":"L-1","quantity":2,"length_in":null,"extra":true,"lines":[{"n":1.5}]}"#
            ),
            Ok(())
        );
        let wrong = fits(r#"{"item_code":"PLANTED","quantity":"2"}"#).unwrap_err();
        assert!(wrong.contains("the body.quantity is a string"), "{wrong}");
        assert!(!wrong.contains("PLANTED"), "{wrong}");
        assert!(
            fits(r#"{"quantity":2}"#)
                .unwrap_err()
                .contains("lacks `item_code`")
        );
        assert!(fits(r#"{"item_code":"L","quantity":2.5}"#).is_err());
        assert!(fits(r#"{"item_code":"L","quantity":2,"status":"other"}"#).is_err());
        let nested = fits(r#"{"item_code":"L","quantity":2,"lines":[{"n":1},{"n":"x"}]}"#);
        assert!(nested.unwrap_err().contains("lines[1].n"));
        assert!(fits("[]").is_err());
    }

    #[test]
    fn a_word_this_pack_does_not_check_is_refused_where_it_is_written() {
        let problem = shape("type: string\npattern: '^a'\n").unwrap_err();
        assert_eq!((problem.at.line, problem.at.column), (2, 1));
        assert!(problem.message.contains("`pattern`"), "{problem:?}");
        assert!(shape("type: text\n").is_err());
        assert!(shape("properties: {}\n").is_err());
        assert!(shape("type: object\nrequired: [a]\n").is_err());
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
}
