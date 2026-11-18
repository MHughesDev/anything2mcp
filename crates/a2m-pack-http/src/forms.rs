//! The form of an endpoint's answer: JSON, which is the default and is read
//! as it always was, or XML, text, or bytes (decision 0217).
//!
//! An answer that is not JSON is never turned into JSON as a whole. The
//! endpoint declares, for each value it wants, where the value is; reading
//! the answer gives one record of exactly those values, typed by the field
//! types the catalog already uses, and that record is what the endpoint's
//! shape is checked against and what the `extract` step takes values from.
//! So the shape of the record, the extraction, the version, and the
//! read-back are the same machinery for every form.

use std::collections::BTreeMap;

use a2m_core::{Digest, Redact};
use a2m_files::{Diagnostics, Doc, Located, Place, Stage};
use serde::{Deserialize, Serialize};

use crate::endpoints::problem;
use crate::json::{Json, is_plain_number};
use crate::shape::{Kind, Shape};

/// The name of the output that holds the whole body of a `text` answer.
pub(crate) const WHOLE_TEXT: &str = "text";
/// The outputs of a `bytes` answer.
pub(crate) const LENGTH: &str = "length";
pub(crate) const DIGEST: &str = "digest";

/// The most elements an XML answer may be nested, unless the endpoint says.
pub(crate) const DEFAULT_MAX_DEPTH: u32 = 64;
/// The most elements an XML answer may hold, unless the endpoint says.
pub(crate) const DEFAULT_MAX_ELEMENTS: u64 = 100_000;
/// The longest namespace address an XML answer may declare, in bytes
/// (decision 0249).
pub(crate) const MAX_NAMESPACE_BYTES: usize = 2_048;
/// The longest name of an element or an attribute, with its prefix, in
/// bytes.
pub(crate) const MAX_NAME_BYTES: usize = 256;
/// The longest value of one attribute, as written, in bytes.
pub(crate) const MAX_ATTRIBUTE_BYTES: usize = 65_536;
/// The most namespaces an XML answer may have declared at once.
pub(crate) const MAX_NAMESPACES: usize = 64;
/// What reading an answer may cost, in units of work, for each byte of it
/// and each declared field: an event, an attribute, a byte of text taken,
/// and a comparison with a field each cost one. A reader that would spend
/// more stops (decision 0249).
pub(crate) const WORK_PER_BYTE: u64 = 4;
/// What reading any answer may cost besides.
pub(crate) const WORK_BASE: u64 = 65_536;
/// The most memory a compiled pattern may take, as the command pack's.
const PATTERN_SIZE_LIMIT: usize = 1 << 20;

/// The words of `answer.form`.
pub(crate) const FORMS: &[&str] = &["json", "xml", "text", "bytes"];
/// The words of `answer.charset`.
pub(crate) const CHARSETS: &[&str] = &["utf-8", "us-ascii", "iso-8859-1"];

/// How an answer is read.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Form {
    /// The body is one JSON document, checked whole against the endpoint's
    /// `response_schema`.
    #[default]
    Json,
    /// The body is an XML document; the declared paths give the record.
    Xml(XmlForm),
    /// The body is text; the whole of it, and the named groups of an
    /// optional pattern, give the record.
    Text(TextForm),
    /// The body is not read: its length and its digest are the record.
    Bytes,
}

/// The form of an endpoint's answer, and the media types its
/// `Content-Type` must be one of.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnswerSpec {
    pub form: Form,
    /// Lowercase media types, with no parameters. `type/*+suffix` stands
    /// for any type of that family and suffix. Empty only for JSON with
    /// none declared, which is not checked, as before this form existed.
    pub content_types: Vec<String>,
}

impl AnswerSpec {
    /// Whether this is JSON read as it always was, with nothing declared:
    /// such an endpoint's payload is written as it was before.
    pub(crate) fn is_plain_json(&self) -> bool {
        self.form == Form::Json && self.content_types.is_empty()
    }
}

/// One name in an XML document: the namespace it is in, by its URI, and
/// its local part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct XmlName {
    pub namespace: Option<String>,
    pub local: String,
}

/// Where one value is: child elements step by step, and the attribute of
/// the last one, or its text when no attribute is named.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct XmlPath {
    pub elements: Vec<XmlName>,
    pub attribute: Option<XmlName>,
}

/// What one declared field takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum XmlTake {
    /// One value, from the one place the path matches.
    One(XmlPath),
    /// A list: one entry for each element the path matches. With fields,
    /// each entry is a record of them, by paths from that element; with
    /// none, each entry is the element's text.
    Each {
        elements: Vec<XmlName>,
        fields: Vec<XmlField>,
    },
}

/// One field of the record an XML answer gives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct XmlField {
    pub name: String,
    pub take: XmlTake,
    /// The type of the value, or of each entry of a list of texts.
    pub kind: Kind,
    /// Whether an answer where the path matches nothing is refused. A list
    /// may be empty.
    pub required: bool,
}

/// How an XML answer is read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct XmlForm {
    pub fields: Vec<XmlField>,
    pub max_depth: u32,
    pub max_elements: u64,
}

/// The character set a text answer is decoded by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Charset {
    Utf8,
    UsAscii,
    Iso8859_1,
}

impl Charset {
    pub(crate) fn from_word(word: &str) -> Option<Self> {
        Some(match word.to_ascii_lowercase().as_str() {
            "utf-8" | "utf8" => Self::Utf8,
            "us-ascii" | "ascii" => Self::UsAscii,
            "iso-8859-1" | "latin1" | "iso_8859-1" => Self::Iso8859_1,
            _ => return None,
        })
    }

    /// The body as text, or `None` when it does not decode. No character
    /// is ever replaced. A byte-order mark is the UTF-8 one, and only for
    /// UTF-8: any other, or one before a body declared in another set, is
    /// a disagreement about the character set, and the body is refused.
    fn decode(self, body: &[u8]) -> Option<String> {
        if self != Self::Utf8 && byte_order_mark(body) {
            return None;
        }
        match self {
            Self::Utf8 => {
                let body = body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(body);
                std::str::from_utf8(body).ok().map(str::to_owned)
            }
            Self::UsAscii => body
                .is_ascii()
                .then(|| body.iter().map(|byte| char::from(*byte)).collect()),
            Self::Iso8859_1 => Some(body.iter().map(|byte| char::from(*byte)).collect()),
        }
    }
}

/// Whether a body starts with a byte-order mark of UTF-8, UTF-16, or
/// UTF-32.
fn byte_order_mark(body: &[u8]) -> bool {
    [&b"\xEF\xBB\xBF"[..], b"\xFE\xFF", b"\xFF\xFE", b"\x00\x00\xFE\xFF"]
        .iter()
        .any(|mark| body.starts_with(mark))
}

/// Whether text holds a control character this pack never returns: any
/// of Unicode's control characters but the tab, the line feed, and the
/// carriage return, which are ordinary in text (decision 0249). The one
/// check of every value of every form, made where a value is taken.
pub(crate) fn refused_control(text: &str) -> bool {
    text.chars()
        .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r'))
}

/// The digest of a `bytes` answer's body. The one place it is made: it is
/// the plain SHA-256 of the body, because no keyed digest reaches a driver
/// (decision 0249 says what is owed).
pub(crate) fn body_digest(body: &[u8]) -> Digest {
    Digest::of(body)
}

/// A bounded count of the work one read of an answer does, and the flag
/// the step's deadline sets. A reader spends before it works, and stops
/// past its budget or once the flag is set.
pub(crate) struct Work<'a> {
    used: u64,
    budget: u64,
    stop: &'a AtomicBool,
}

impl<'a> Work<'a> {
    /// The budget of an answer of `bytes` bytes read for `fields` declared
    /// fields.
    pub(crate) fn for_answer(bytes: usize, fields: usize, stop: &'a AtomicBool) -> Self {
        let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
        let fields = u64::try_from(fields).unwrap_or(u64::MAX);
        Self {
            used: 0,
            budget: bytes
                .saturating_add(1)
                .saturating_mul(WORK_PER_BYTE)
                .saturating_mul(fields.saturating_add(1))
                .saturating_add(WORK_BASE),
            stop,
        }
    }

    /// Spends `units`, or says why the read stops.
    pub(crate) fn spend(&mut self, units: u64) -> Result<(), Unread> {
        self.used = self.used.saturating_add(units);
        if self.used > self.budget {
            return Err(Unread::TooLarge(
                "reading it takes more work than an answer of its size may".to_owned(),
            ));
        }
        if self.stop.load(Ordering::Relaxed) {
            return Err(Unread::Stopped);
        }
        Ok(())
    }

    /// The units spent so far.
    #[cfg(test)]
    pub(crate) const fn used(&self) -> u64 {
        self.used
    }
}

/// One named group of a text answer's pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TextField {
    pub name: String,
    pub kind: Kind,
    pub required: bool,
}

/// How a text answer is read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TextForm {
    pub charset: Charset,
    /// A pattern the whole text must match, from its first character to
    /// its last; its named groups are the fields.
    pub pattern: Option<String>,
    pub fields: Vec<TextField>,
}

/// Compiles a text answer's pattern so that it covers the whole text: an
/// alternative inside it cannot slip past the anchors. The matcher takes
/// time linear in the text.
pub(crate) fn whole_pattern(text: &str) -> Result<regex::Regex, String> {
    regex::RegexBuilder::new(&format!(r"\A(?:{text})\z"))
        .size_limit(PATTERN_SIZE_LIMIT)
        .build()
        .map_err(|error| error.to_string())
}

/// Why an answer could not be read in its declared form. A message names a
/// field or a byte position, never a value from the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Unread {
    /// The answer is not of the declared form, or does not hold what the
    /// endpoint declares: the response-shape failure.
    Shape(String),
    /// An XML answer that declares a document type.
    DocumentType,
    /// An answer past a bound on what is read: an XML answer's depth, its
    /// number of elements or of namespaces, the length of a name, of a
    /// namespace's address, or of an attribute's value, or the work its
    /// reading takes.
    TooLarge(String),
    /// A `bytes` answer whose body holds a registered secret: neither its
    /// length nor its digest is returned.
    HoldsSecret,
    /// The step's deadline passed while the answer was read.
    Stopped,
}

/// A field type's text as the JSON value of that type. `None` when the
/// text is not of the type.
pub(crate) fn typed(text: &str, kind: Kind) -> Option<Json> {
    match kind {
        Kind::Integer => {
            let digits = text.strip_prefix('-').unwrap_or(text);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            text.parse::<i64>()
                .ok()
                .map(|number| Json::Number(number.to_string()))
        }
        Kind::Number => is_plain_number(text).then(|| Json::Number(text.to_owned())),
        Kind::Boolean => match text {
            "true" => Some(Json::Bool(true)),
            "false" => Some(Json::Bool(false)),
            _ => None,
        },
        _ => Some(Json::Text(text.to_owned())),
    }
}

/// The media type of a `Content-Type` header, lowercase, and its charset
/// parameter, when it has one.
fn media_type(header: &str) -> (String, Option<String>) {
    let mut parts = header.split(';');
    let media = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    let charset = parts.find_map(|part| {
        let (name, value) = part.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("charset")
            .then(|| value.trim().trim_matches('"').to_ascii_lowercase())
    });
    (media, charset)
}

fn type_matches(declared: &str, media: &str) -> bool {
    if let Some((family, suffix)) = declared.split_once("/*+") {
        return media
            .split_once('/')
            .is_some_and(|(kind, sub)| kind == family && sub.ends_with(&format!("+{suffix}")));
    }
    declared == media
}

impl AnswerSpec {
    /// Checks the answer's `Content-Type` against the declared media types,
    /// and its charset against the one the form decodes by.
    pub(crate) fn check_content_type(&self, header: Option<&str>) -> Result<(), String> {
        if self.content_types.is_empty() {
            return Ok(());
        }
        let Some(header) = header else {
            return Err(
                "the answer states no content type, and the endpoint declares one".to_owned(),
            );
        };
        let (media, charset) = media_type(header);
        if !self
            .content_types
            .iter()
            .any(|declared| type_matches(declared, &media))
        {
            return Err(
                "the answer's content type is not one the endpoint declares for its form"
                    .to_owned(),
            );
        }
        let decoded_by = match &self.form {
            Form::Text(text) => Some(text.charset),
            Form::Xml(_) => Some(Charset::Utf8),
            Form::Json | Form::Bytes => None,
        };
        if let (Some(expected), Some(charset)) = (decoded_by, charset) {
            let fits = match Charset::from_word(&charset) {
                Some(said) => {
                    said == expected || (said == Charset::UsAscii && expected == Charset::Utf8)
                }
                None => false,
            };
            if !fits {
                return Err(
                    "the answer's content type names a character set other than the one the endpoint declares"
                        .to_owned(),
                );
            }
        }
        Ok(())
    }

    /// Reads a body that is not JSON into its record. The text of a `text`
    /// answer passes the redactor whole before it is matched; every value
    /// passes it again when the `extract` step returns it.
    pub(crate) fn read(&self, body: &[u8], redact: &dyn Redact) -> Result<Json, Unread> {
        match &self.form {
            Form::Json => Err(Unread::Shape(
                "the endpoint's answer is JSON, and was read as another form".to_owned(),
            )),
            Form::Xml(form) => crate::xml::read(body, form),
            Form::Text(form) => read_text(body, form, redact),
            Form::Bytes => Ok(Json::Object(vec![
                (LENGTH.to_owned(), Json::Number(body.len().to_string())),
                (DIGEST.to_owned(), Json::Text(Digest::of(body).to_string())),
            ])),
        }
    }
}

fn read_text(body: &[u8], form: &TextForm, redact: &dyn Redact) -> Result<Json, Unread> {
    let text = form.charset.decode(body).ok_or_else(|| {
        Unread::Shape(
            "the body does not decode by the character set the endpoint declares".to_owned(),
        )
    })?;
    let text = redact.redact(&text);
    let mut record = vec![(WHOLE_TEXT.to_owned(), Json::Text(text.clone()))];
    if let Some(pattern) = &form.pattern {
        let compiled = whole_pattern(pattern)
            .map_err(|_| Unread::Shape("the endpoint's pattern does not compile".to_owned()))?;
        let found = compiled.captures(&text).ok_or_else(|| {
            Unread::Shape("the body does not match the pattern the endpoint declares".to_owned())
        })?;
        for field in &form.fields {
            let value = match found.name(&field.name) {
                Some(group) => typed(group.as_str(), field.kind).ok_or_else(|| {
                    Unread::Shape(format!(
                        "the group `{}` is not a {}",
                        field.name,
                        field.kind.word()
                    ))
                })?,
                None if field.required => {
                    return Err(Unread::Shape(format!(
                        "the group `{}` matched nothing, and the endpoint declares it required",
                        field.name
                    )));
                }
                None => Json::Null,
            };
            record.push((field.name.clone(), value));
        }
    }
    Ok(Json::Object(record))
}

/// The shape of the record a form gives, which the endpoint's `extract`
/// paths are checked against when the target compiles.
pub(crate) fn record_shape(form: &Form) -> Shape {
    let mut shape = Shape::of(Kind::Object);
    shape.closed = true;
    let mut add = |name: &str, value: Shape, required: bool| {
        let mut value = value;
        value.nullable = !required;
        shape.properties.insert(name.to_owned(), value);
        shape.required.push(name.to_owned());
    };
    match form {
        Form::Json => {}
        Form::Bytes => {
            add(LENGTH, Shape::of(Kind::Integer), true);
            add(DIGEST, Shape::of(Kind::String), true);
        }
        Form::Text(text) => {
            add(WHOLE_TEXT, Shape::of(Kind::String), true);
            for field in &text.fields {
                add(&field.name, Shape::of(field.kind), field.required);
            }
        }
        Form::Xml(xml) => {
            for field in &xml.fields {
                add(&field.name, xml_field_shape(field), field.required);
            }
        }
    }
    shape
}

fn xml_field_shape(field: &XmlField) -> Shape {
    match &field.take {
        XmlTake::One(_) => Shape::of(field.kind),
        XmlTake::Each { fields, .. } => {
            let mut list = Shape::of(Kind::Array);
            let item = if fields.is_empty() {
                Shape::of(field.kind)
            } else {
                let mut record = Shape::of(Kind::Object);
                record.closed = true;
                for inner in fields {
                    let mut value = xml_field_shape(inner);
                    value.nullable = !inner.required;
                    record.properties.insert(inner.name.clone(), value);
                    record.required.push(inner.name.clone());
                }
                record
            };
            list.items = Some(Box::new(item));
            list
        }
    }
}

// ---- reading the declaration from an endpoint entry ----

fn is_field_name(text: &str) -> bool {
    text.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn is_xml_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// The types a value of an answer that is not JSON may be declared as.
fn kind_of(word: Option<&str>) -> Option<Kind> {
    Some(match word {
        None | Some("string") => Kind::String,
        Some("integer") => Kind::Integer,
        Some("number") => Kind::Number,
        Some("boolean") => Kind::Boolean,
        Some(_) => return None,
    })
}

const FIX: &str = "Write the answer as decision 0217 and the file formats reference give it.";

struct Reading<'a> {
    problems: &'a mut Diagnostics,
    namespaces: BTreeMap<String, String>,
}

impl Reading<'_> {
    fn refuse(&mut self, at: &Place, message: impl Into<String>) {
        self.problems.push(problem(
            "answer_invalid",
            Stage::Lower,
            at,
            message.into(),
            FIX,
        ));
    }

    fn name(&mut self, step: &str, at: &Place) -> Option<XmlName> {
        let (prefix, local) = match step.split_once(':') {
            Some((prefix, local)) => (Some(prefix), local),
            None => (None, step),
        };
        if !is_xml_name(local) || prefix.is_some_and(|p| !is_xml_name(p)) {
            self.refuse(
                at,
                format!(
                    "`{}` is not a name: a step is a name, or a declared prefix, a colon, and a name.",
                    crate::codes::shown(step)
                ),
            );
            return None;
        }
        let namespace = match prefix {
            None => None,
            Some(prefix) => match self.namespaces.get(prefix) {
                Some(uri) => Some(uri.clone()),
                None => {
                    self.refuse(
                        at,
                        format!(
                            "The prefix `{}` is not declared under `namespaces`; the document's own prefixes are never used.",
                            crate::codes::shown(prefix)
                        ),
                    );
                    return None;
                }
            },
        };
        Some(XmlName {
            namespace,
            local: local.to_owned(),
        })
    }

    /// A path: steps joined by `/`, each a name; the last may be `@` and a
    /// name, for an attribute. Nothing else: no function, no predicate, no
    /// axis, no wildcard.
    fn path(&mut self, doc: &Located<Doc>, relative: bool) -> Option<XmlPath> {
        let Some(text) = doc.as_text() else {
            self.refuse(&doc.at, "A path is text.");
            return None;
        };
        let steps: Vec<&str> = text.split('/').collect();
        let mut elements = Vec::new();
        let mut attribute = None;
        for (position, step) in steps.iter().enumerate() {
            let last = position + 1 == steps.len();
            if let Some(name) = step.strip_prefix('@') {
                if !last {
                    self.refuse(&doc.at, "An attribute can only be the last step of a path.");
                    return None;
                }
                attribute = Some(self.name(name, &doc.at)?);
            } else {
                elements.push(self.name(step, &doc.at)?);
            }
        }
        if elements.is_empty() && (!relative || attribute.is_none()) {
            self.refuse(
                &doc.at,
                if relative {
                    "A path inside `each` names at least an attribute of the repeated element."
                } else {
                    "A path starts at the document's root element, by its name."
                },
            );
            return None;
        }
        Some(XmlPath {
            elements,
            attribute,
        })
    }

    fn fields(&mut self, doc: &Located<Doc>, inside_each: bool) -> Vec<XmlField> {
        let mut out = Vec::new();
        if doc.entries().is_empty() {
            self.refuse(&doc.at, "`fields` is a map of at least one field, by name.");
        }
        for entry in doc.entries() {
            let name = &entry.key.value;
            if !is_field_name(name) {
                self.refuse(
                    &entry.key.at,
                    "A field's name is lowercase letters, digits, and `_`, starting with a letter.",
                );
                continue;
            }
            let spec = &entry.value;
            for key in spec.entries() {
                if !matches!(
                    key.key.value.as_str(),
                    "path" | "each" | "fields" | "type" | "required"
                ) {
                    self.refuse(
                        &key.key.at,
                        format!(
                            "`{}` is not a word of an XML field: `path` or `each`, `fields`, `type`, `required`.",
                            crate::codes::shown(&key.key.value)
                        ),
                    );
                }
            }
            let Some(kind) = kind_of(spec.text("type")) else {
                self.refuse(
                    &spec.at,
                    "A field's `type` is `string`, `integer`, `number`, or `boolean`.",
                );
                continue;
            };
            let required = spec.get("required").and_then(Located::as_bool);
            let take = match (spec.get("path"), spec.get("each")) {
                (Some(path), None) => {
                    if spec.get("fields").is_some() {
                        self.refuse(&spec.at, "`fields` belongs with `each`, not with `path`.");
                        continue;
                    }
                    match self.path(path, inside_each) {
                        Some(path) => XmlTake::One(path),
                        None => continue,
                    }
                }
                (None, Some(each)) => {
                    if inside_each {
                        self.refuse(&each.at, "An `each` inside an `each` is not built.");
                        continue;
                    }
                    if required.is_some() {
                        self.refuse(
                            &spec.at,
                            "A list may be empty; `required` belongs with `path`.",
                        );
                        continue;
                    }
                    let Some(path) = self.path(each, false) else {
                        continue;
                    };
                    if path.attribute.is_some() {
                        self.refuse(&each.at, "`each` names elements, not an attribute.");
                        continue;
                    }
                    let fields = match spec.get("fields") {
                        Some(fields) => self.fields(fields, true),
                        None => Vec::new(),
                    };
                    XmlTake::Each {
                        elements: path.elements,
                        fields,
                    }
                }
                _ => {
                    self.refuse(&spec.at, "A field has exactly one of `path` and `each`.");
                    continue;
                }
            };
            out.push(XmlField {
                name: name.clone(),
                take,
                kind,
                required: required.unwrap_or(true),
            });
        }
        out
    }
}

/// Reads an endpoint's `answer`, and refuses what the form does not take.
/// `None` when nothing is declared or something was refused.
pub(crate) fn answer_of(doc: &Located<Doc>, problems: &mut Diagnostics) -> Option<AnswerSpec> {
    let written = doc.get("answer")?;
    let before = problems.len();
    let form_word = written.text("form").unwrap_or("json");
    let mut reading = Reading {
        problems,
        namespaces: BTreeMap::new(),
    };
    let allowed: &[&str] = match form_word {
        "xml" => &[
            "form",
            "content_type",
            "namespaces",
            "fields",
            "max_depth",
            "max_elements",
        ],
        "text" => &["form", "content_type", "charset", "pattern", "fields"],
        _ => &["form", "content_type"],
    };
    for entry in written.entries() {
        if !allowed.contains(&entry.key.value.as_str()) {
            reading.refuse(
                &entry.key.at,
                format!(
                    "`{}` is not a word of an answer of the form `{form_word}`.",
                    crate::codes::shown(&entry.key.value)
                ),
            );
        }
    }
    let mut content_types = Vec::new();
    for item in written
        .get("content_type")
        .map(Located::items)
        .unwrap_or_default()
    {
        let text = item
            .as_text()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let shaped = text.split_once('/').is_some_and(|(kind, sub)| {
            !kind.is_empty()
                && !sub.is_empty()
                && text.bytes().all(|b| {
                    b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'+' | b'.' | b'*')
                })
        });
        if shaped {
            content_types.push(text);
        } else {
            reading.refuse(
                &item.at,
                "A content type is a media type such as `application/xml`, with no parameters.",
            );
        }
    }
    let defaults: &[&str] = match form_word {
        "xml" => &["application/xml", "text/xml", "application/*+xml"],
        "text" => &["text/plain"],
        "bytes" => &["application/octet-stream"],
        _ => &[],
    };
    if content_types.is_empty() {
        content_types = defaults.iter().map(|text| (*text).to_owned()).collect();
    }
    if form_word != "json" && doc.get("response_schema").is_some() {
        reading.refuse(
            &written.at,
            "An answer that is not JSON has no `response_schema`: its record is the fields the answer declares.",
        );
    }

    let form = match form_word {
        "xml" => {
            for entry in written
                .get("namespaces")
                .map(Located::entries)
                .unwrap_or_default()
            {
                let uri = entry.value.as_text().unwrap_or_default();
                if !is_xml_name(&entry.key.value) || uri.is_empty() {
                    reading.refuse(
                        &entry.key.at,
                        "A namespace is a prefix and the URI it stands for.",
                    );
                    continue;
                }
                reading
                    .namespaces
                    .insert(entry.key.value.clone(), uri.to_owned());
            }
            let fields = match written.get("fields") {
                Some(fields) => reading.fields(fields, false),
                None => {
                    reading.refuse(&written.at, "An XML answer declares its `fields`.");
                    Vec::new()
                }
            };
            let bound = |key: &str, default: u64| {
                written
                    .get(key)
                    .and_then(Located::as_int)
                    .and_then(|number| u64::try_from(number).ok())
                    .unwrap_or(default)
            };
            Form::Xml(XmlForm {
                fields,
                max_depth: u32::try_from(bound("max_depth", u64::from(DEFAULT_MAX_DEPTH)))
                    .unwrap_or(DEFAULT_MAX_DEPTH),
                max_elements: bound("max_elements", DEFAULT_MAX_ELEMENTS),
            })
        }
        "text" => {
            let charset = written
                .text("charset")
                .and_then(Charset::from_word)
                .unwrap_or(Charset::Utf8);
            let pattern = written.text("pattern").map(str::to_owned);
            let mut fields = Vec::new();
            let declared = written.get("fields");
            match (&pattern, declared) {
                (None, Some(declared)) => {
                    reading.refuse(
                        &declared.at,
                        "A text answer's `fields` are the named groups of its `pattern`.",
                    );
                }
                (Some(text), _) => match whole_pattern(text) {
                    Err(_) => {
                        let at = written
                            .get("pattern")
                            .map_or(&written.at, |p| &p.at)
                            .clone();
                        reading.refuse(&at, "The pattern does not compile.");
                    }
                    Ok(compiled) => {
                        let groups: Vec<&str> = compiled.capture_names().flatten().collect();
                        let at = written
                            .get("pattern")
                            .map_or(&written.at, |p| &p.at)
                            .clone();
                        if groups.is_empty() {
                            reading.refuse(
                                &at,
                                "A pattern names its groups, such as `(?P<count>[0-9]+)`.",
                            );
                        }
                        for group in &groups {
                            if !is_field_name(group) || *group == WHOLE_TEXT {
                                reading.refuse(
                                    &at,
                                    format!(
                                        "The group `{}` is not a field's name, or is `text`, which holds the whole body.",
                                        crate::codes::shown(group)
                                    ),
                                );
                            }
                        }
                        for entry in declared.map(Located::entries).unwrap_or_default() {
                            if !groups.contains(&entry.key.value.as_str()) {
                                reading.refuse(
                                    &entry.key.at,
                                    "The pattern has no group of this name.",
                                );
                            }
                        }
                        for group in groups {
                            let spec = declared.and_then(|fields| fields.get(group));
                            let Some(kind) = kind_of(spec.and_then(|spec| spec.text("type")))
                            else {
                                reading.refuse(
                                    spec.map_or(&at, |spec| &spec.at),
                                    "A field's `type` is `string`, `integer`, `number`, or `boolean`.",
                                );
                                continue;
                            };
                            fields.push(TextField {
                                name: group.to_owned(),
                                kind,
                                required: spec
                                    .and_then(|spec| spec.get("required"))
                                    .and_then(Located::as_bool)
                                    .unwrap_or(true),
                            });
                        }
                        fields.sort_by(|a, b| a.name.cmp(&b.name));
                    }
                },
                (None, None) => {}
            }
            Form::Text(TextForm {
                charset,
                pattern,
                fields,
            })
        }
        "bytes" => Form::Bytes,
        _ => Form::Json,
    };
    if problems.len() > before {
        return None;
    }
    Some(AnswerSpec {
        form,
        content_types,
    })
}

#[cfg(test)]
mod tests {
    use a2m_core::NoSecrets;

    use super::{AnswerSpec, Charset, Form, TextField, TextForm, Unread, typed};
    use crate::json::Json;
    use crate::shape::Kind;

    #[test]
    fn a_text_is_typed_exactly_or_not_at_all() {
        assert_eq!(
            typed("007", Kind::Integer),
            Some(Json::Number("7".to_owned()))
        );
        assert_eq!(
            typed("-3", Kind::Integer),
            Some(Json::Number("-3".to_owned()))
        );
        for wrong in ["", "1.5", "+1", "1e3", " 1", "99999999999999999999"] {
            assert_eq!(typed(wrong, Kind::Integer), None, "{wrong}");
        }
        assert_eq!(
            typed("150.5", Kind::Number),
            Some(Json::Number("150.5".to_owned()))
        );
        assert_eq!(typed("1e3", Kind::Number), None);
        assert_eq!(typed("true", Kind::Boolean), Some(Json::Bool(true)));
        assert_eq!(typed("1", Kind::Boolean), None);
    }

    #[test]
    fn the_content_type_and_its_charset_are_held_to_the_form() {
        let text = AnswerSpec {
            form: Form::Text(TextForm {
                charset: Charset::Utf8,
                pattern: None,
                fields: Vec::new(),
            }),
            content_types: vec!["text/plain".to_owned()],
        };
        assert!(text.check_content_type(Some("text/plain")).is_ok());
        assert!(
            text.check_content_type(Some("Text/Plain; charset=UTF-8"))
                .is_ok()
        );
        assert!(
            text.check_content_type(Some("text/plain; charset=iso-8859-1"))
                .is_err()
        );
        assert!(text.check_content_type(Some("text/html")).is_err());
        assert!(text.check_content_type(None).is_err());
        let xml = AnswerSpec {
            form: Form::Bytes,
            content_types: vec!["application/*+xml".to_owned()],
        };
        assert!(xml.check_content_type(Some("application/atom+xml")).is_ok());
        assert!(xml.check_content_type(Some("application/json")).is_err());
        // JSON with nothing declared is not checked, as before.
        assert!(AnswerSpec::default().check_content_type(None).is_ok());
    }

    #[test]
    fn a_text_that_does_not_decode_is_refused_and_never_replaced() {
        let form = |charset| AnswerSpec {
            form: Form::Text(TextForm {
                charset,
                pattern: Some(r"size (?P<size>[0-9]+)".to_owned()),
                fields: vec![TextField {
                    name: "size".to_owned(),
                    kind: Kind::Integer,
                    required: true,
                }],
            }),
            content_types: Vec::new(),
        };
        let latin = b"size 12\xE9";
        assert!(matches!(
            form(Charset::Utf8).read(latin, &NoSecrets),
            Err(Unread::Shape(_))
        ));
        assert!(matches!(
            form(Charset::UsAscii).read(latin, &NoSecrets),
            Err(Unread::Shape(_))
        ));
        // Anchored: the trailing character is outside the pattern.
        assert!(form(Charset::Iso8859_1).read(latin, &NoSecrets).is_err());
        let read = form(Charset::Utf8).read(b"size 12", &NoSecrets).unwrap();
        assert_eq!(read.get("size"), Some(&Json::Number("12".to_owned())));
        assert_eq!(read.get("text"), Some(&Json::Text("size 12".to_owned())));
    }
}
