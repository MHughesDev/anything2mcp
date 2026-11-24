//! Shapes: what a value is, without the value.
//!
//! A finding keeps the names and types of what was seen and never a value
//! where a shape will do. This module turns a text, or a parsed JSON
//! document, into its shape, and merges the shapes of several observations
//! of one thing. A shape is itself JSON, with its names in one fixed
//! order, so that the same observation always gives the same bytes.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value as Json};

/// The most names one object of a shape keeps. More are counted and left
/// out.
const MAX_FIELDS: usize = 200;

/// How deep a shape goes before what is below is named `nested`.
const MAX_DEPTH: usize = 12;

/// The type of a text, by its form alone. The names are the pack's own
/// and are data: nothing is decided by them here.
#[must_use]
pub fn type_of_text(text: &str) -> &'static str {
    let text = text.trim();
    if text.is_empty() {
        return "empty";
    }
    if is_integer(text) {
        return "integer";
    }
    if is_decimal(text) {
        return "decimal";
    }
    if text == "true" || text == "false" {
        return "boolean";
    }
    if is_uuid(text) {
        return "uuid";
    }
    if is_date(text) {
        return "date";
    }
    if text.len() > 10 && is_date(&text[..10]) && matches!(text.as_bytes()[10], b'T' | b' ') {
        return "date_time";
    }
    if text.len() >= 16 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return "hex";
    }
    "text"
}

fn is_integer(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    !digits.is_empty() && digits.len() <= 18 && digits.bytes().all(|b| b.is_ascii_digit())
}

fn is_decimal(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    match digits.split_once('.') {
        Some((whole, part)) => {
            !whole.is_empty()
                && !part.is_empty()
                && whole.bytes().all(|b| b.is_ascii_digit())
                && part.bytes().all(|b| b.is_ascii_digit())
        }
        None => false,
    }
}

fn is_uuid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(at, b)| match at {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(at, b)| match at {
            4 | 7 => *b == b'-',
            _ => b.is_ascii_digit(),
        })
}

/// The shape of what was seen in one place, over every observation of it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Shape {
    /// The types seen, by name: `null`, `boolean`, `integer`, `decimal`,
    /// the types of [`type_of_text`], `list`, `object`, `nested`.
    types: BTreeSet<&'static str>,
    /// The fields of an object, with how many observations held each.
    fields: BTreeMap<String, (Shape, u64)>,
    /// How many observations were objects.
    objects: u64,
    /// The shape of a list's items, merged.
    items: Option<Box<Shape>>,
    /// How many names were left out at the limit.
    left_out: u64,
    /// Whether an object's names were withheld because they read as
    /// values: a dictionary's keys (decision 0195).
    withheld: bool,
}

impl Shape {
    /// The shape of one JSON document. Every value is replaced by its
    /// type; names are kept.
    #[must_use]
    pub fn of(json: &Json) -> Self {
        Self::at(json, 0)
    }

    fn at(json: &Json, depth: usize) -> Self {
        let mut shape = Self::default();
        if depth >= MAX_DEPTH {
            shape.types.insert("nested");
            return shape;
        }
        match json {
            Json::Null => {
                shape.types.insert("null");
            }
            Json::Bool(_) => {
                shape.types.insert("boolean");
            }
            Json::Number(number) => {
                shape.types.insert(if number.is_i64() || number.is_u64() {
                    "integer"
                } else {
                    "decimal"
                });
            }
            Json::String(text) => {
                shape.types.insert(type_of_text(text));
            }
            Json::Array(items) => {
                shape.types.insert("list");
                let mut merged: Option<Self> = None;
                for item in items {
                    let one = Self::at(item, depth + 1);
                    merged = Some(match merged {
                        Some(all) => all.merged(&one),
                        None => one,
                    });
                }
                shape.items = merged.map(Box::new);
            }
            Json::Object(fields) => {
                shape.objects = 1;
                // An object whose keys are all identifiers by form (numbers,
                // identifiers, dates) is a dictionary, whatever its size: one
                // entry of type `map`, its keys never named, and the shape
                // of its values merged (decision 0322 point 6).
                if !fields.is_empty()
                    && fields
                        .keys()
                        .all(|key| crate::infer::is_identifier_by_form(key))
                {
                    shape.types.insert("map");
                    let mut merged: Option<Self> = None;
                    for value in fields.values() {
                        let one = Self::at(value, depth + 1);
                        merged = Some(match merged {
                            Some(all) => all.merged(&one),
                            None => one,
                        });
                    }
                    shape.items = merged.map(Box::new);
                    return shape;
                }
                shape.types.insert("object");
                // An object with more names than a record has, or with a
                // name that is not a plain one, is a dictionary: its names
                // are values, and none is kept.
                if !a2m_pack_web_browser::names_are_structure(fields.keys()) {
                    shape.withheld = true;
                    return shape;
                }
                let mut names: Vec<&String> = fields.keys().collect();
                names.sort();
                for name in names {
                    if shape.fields.len() >= MAX_FIELDS {
                        shape.left_out += 1;
                        continue;
                    }
                    if let Some(value) = fields.get(name) {
                        shape
                            .fields
                            .insert(name.clone(), (Self::at(value, depth + 1), 1));
                    }
                }
            }
        }
        shape
    }

    /// The shape of a JSON answer that only its names and the kinds of
    /// their values were kept of, to one level inside an object or a
    /// list's items, as the browser's observing tab records it: `outer`,
    /// `outer.inner`, `[]`, and `[].name`. What is deeper is not known, and
    /// a text is `text`, its form not seen.
    #[must_use]
    pub fn of_fields(fields: &BTreeMap<String, String>) -> Self {
        fn kind(name: &str) -> &'static str {
            match name {
                "string" => "text",
                "integer" => "integer",
                "number" => "decimal",
                "boolean" => "boolean",
                "null" => "null",
                "object" => "object",
                "array" => "list",
                _ => "mixed",
            }
        }
        fn one(name: &str) -> Shape {
            let mut shape = Shape::default();
            let kind = kind(name);
            shape.types.insert(kind);
            if kind == "object" {
                shape.objects = 1;
            }
            shape
        }
        fn object(fields: &BTreeMap<String, String>, prefix: &str) -> Shape {
            let mut shape = Shape::default();
            shape.types.insert("object");
            shape.objects = 1;
            for (name, value) in fields {
                let Some(rest) = name.strip_prefix(prefix) else {
                    continue;
                };
                match rest.split_once('.') {
                    None if !rest.is_empty() => {
                        let entry = shape
                            .fields
                            .entry(rest.to_owned())
                            .or_insert_with(|| (Shape::default(), 1));
                        let own = one(value);
                        entry.0.types.extend(own.types);
                        entry.0.objects = entry.0.objects.max(own.objects);
                    }
                    Some((outer, inner)) if !outer.is_empty() && !inner.is_empty() => {
                        let entry = shape
                            .fields
                            .entry(outer.to_owned())
                            .or_insert_with(|| (Shape::default(), 1));
                        entry.0.objects = 1;
                        entry.0.fields.insert(inner.to_owned(), (one(value), 1));
                    }
                    _ => {}
                }
            }
            shape
        }
        match fields.get("[]") {
            Some(items) => {
                let mut shape = Self::default();
                shape.types.insert("list");
                let mut inner = object(fields, "[].");
                if items != "object" {
                    inner = one(items);
                }
                shape.items = Some(Box::new(inner));
                shape
            }
            None => object(fields, ""),
        }
    }

    /// This shape and another observation of the same place, as one.
    #[must_use]
    pub fn merged(&self, other: &Self) -> Self {
        let mut out = self.clone();
        out.types.extend(other.types.iter().copied());
        out.objects += other.objects;
        out.left_out += other.left_out;
        out.withheld |= other.withheld;
        for (name, (shape, seen)) in &other.fields {
            match out.fields.get_mut(name) {
                Some((mine, count)) => {
                    *mine = mine.merged(shape);
                    *count += seen;
                }
                None => {
                    out.fields.insert(name.clone(), (shape.clone(), *seen));
                }
            }
        }
        out.items = match (&self.items, &other.items) {
            (Some(mine), Some(theirs)) => Some(Box::new(mine.merged(theirs))),
            (Some(one), None) | (None, Some(one)) => Some(one.clone()),
            (None, None) => None,
        };
        out
    }

    /// Whether a field was in some observations of its object and not in
    /// others, anywhere in the shape.
    #[must_use]
    pub fn has_optional(&self) -> bool {
        self.fields
            .values()
            .any(|(shape, seen)| *seen < self.objects || shape.has_optional())
            || self
                .items
                .as_ref()
                .is_some_and(|items| items.has_optional())
    }

    /// Whether names were left out at the limit.
    #[must_use]
    pub fn cut(&self) -> bool {
        self.left_out > 0
            || self.fields.values().any(|(shape, _)| shape.cut())
            || self.items.as_ref().is_some_and(|items| items.cut())
    }

    /// Whether the names of an object were withheld as a dictionary's,
    /// anywhere in the shape.
    #[must_use]
    pub fn withheld(&self) -> bool {
        self.withheld
            || self.fields.values().any(|(shape, _)| shape.withheld())
            || self.items.as_ref().is_some_and(|items| items.withheld())
    }

    /// The shape as JSON: `type` is one name or a sorted list of names;
    /// an object has `fields`, each with `optional` when some observations
    /// lacked it; a list has `items`.
    #[must_use]
    pub fn to_json(&self) -> Json {
        let mut out = Map::new();
        if !self.fields.is_empty() {
            let mut fields = Map::new();
            for (name, (shape, seen)) in &self.fields {
                let mut field = match shape.to_json() {
                    Json::Object(field) => field,
                    _ => Map::new(),
                };
                if *seen < self.objects {
                    field.insert("optional".to_owned(), Json::Bool(true));
                } else if self.objects >= 2 {
                    // Seen in every answer: a count, never `required`,
                    // because presence in every sample is no contract
                    // (decision 0322 point 1).
                    field.insert("seen_in_all".to_owned(), Json::from(self.objects));
                }
                fields.insert(name.clone(), Json::Object(sorted(field)));
            }
            out.insert("fields".to_owned(), Json::Object(fields));
        }
        if let Some(items) = &self.items {
            out.insert("items".to_owned(), items.to_json());
        }
        if self.left_out > 0 {
            out.insert("left_out".to_owned(), Json::from(self.left_out));
        }
        if self.withheld {
            out.insert("names_withheld".to_owned(), Json::Bool(true));
        }
        let mut types: Vec<Json> = self.types.iter().map(|name| Json::from(*name)).collect();
        out.insert(
            "type".to_owned(),
            if types.len() == 1 {
                types.remove(0)
            } else {
                Json::Array(types)
            },
        );
        Json::Object(sorted(out))
    }
}

/// The same map with its names in order, whatever order the JSON library
/// keeps.
pub(crate) fn sorted(map: Map<String, Json>) -> Map<String, Json> {
    let ordered: BTreeMap<String, Json> = map.into_iter().collect();
    ordered.into_iter().collect()
}

/// An object of the given names and values, in the order of the names.
pub(crate) fn object<const N: usize>(fields: [(&str, Json); N]) -> Json {
    Json::Object(sorted(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
    ))
}
