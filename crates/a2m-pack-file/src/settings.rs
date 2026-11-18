//! Settings files: INI files and property lists in their XML form, read as
//! named places of one value each, by a path of keys, and written by
//! replacing the bytes of one value and nothing else (decision 0246).
//!
//! Both readers are the pack's own and small. Each reads a fixed set of
//! forms and stops by name at any other, never guessing what a form means
//! (rule 7). Every value keeps where it lies in the file's bytes, so that a
//! write patches that span and leaves every other byte as it was: comments,
//! order, blank lines, line endings, the byte-order mark, and the last
//! line's ending.
//!
//! The property-list reader takes the rules of the HTTP pack's XML answers
//! (decision 0217): no document type is processed. The one declaration a
//! property list carries, a document type named `plist` with no
//! declarations of its own, is recognized by that form and never fetched;
//! any other is refused. No entity is expanded beyond the five the format
//! defines and character references; depth and size are bounded. A binary
//! property list is a named stop: not built.

use a2m_core::{FieldTypePlan, Value};
use a2m_provider::StepFailure;
use a2m_sources::{ByteSpan, Limits, RawCell};

use crate::codes::{self, stop, unsupported};
use crate::edit;

/// The byte-order mark of UTF-8.
const UTF8_MARK: &[u8] = &[0xEF, 0xBB, 0xBF];
/// The deepest a property list's values may nest.
const MAX_DEPTH: usize = 32;
/// The first bytes of a property list in its binary form.
const BINARY_PLIST: &[u8] = b"bplist";

/// What one value of a settings file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Text: an INI value, or a property list's `string`.
    Text,
    /// A property list's `integer`.
    Integer,
    /// A property list's `real`.
    Real,
    /// A property list's `true` or `false`.
    Bool,
    /// A property list's `date`.
    Date,
    /// A property list's `data`, which is not read.
    Data,
    /// A property list's `dict` or `array`.
    Nested,
}

impl Kind {
    /// The element a property list writes a value of this kind in.
    const fn element(self) -> &'static str {
        match self {
            Self::Text => "string",
            Self::Integer => "integer",
            Self::Real => "real",
            Self::Bool => "true",
            Self::Date => "date",
            Self::Data => "data",
            Self::Nested => "dict",
        }
    }
}

/// One part of a value's path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Part {
    /// A section of an INI file, a key, or a key of a dictionary.
    Key(String),
    /// An element of a property list's array, from zero. No place names one.
    Index(usize),
}

/// One value of a settings file, and where it lies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Setting {
    /// Its path: for an INI file, the section and the key, or the key alone
    /// before the first section.
    pub path: Vec<Part>,
    /// What it is.
    pub kind: Kind,
    /// Its text: as written in an INI file, inside its quotes where they are
    /// stripped; with the format's references read in a property list.
    pub text: String,
    /// The bytes a write replaces.
    pub span: ByteSpan,
    /// Whether the span is the whole element (`<true/>`, `<string/>`) and
    /// not its content.
    pub whole_element: bool,
    /// The quote around an INI value whose quotes are stripped.
    pub quote: Option<char>,
    /// Why the value is not read, when a form makes it mean different things
    /// to different programs.
    pub unread: Option<&'static str>,
}

impl Setting {
    /// What the value holds, for its field type to read.
    pub(crate) fn raw(&self) -> RawCell {
        match self.kind {
            Kind::Bool => RawCell::Bool(self.text == "true"),
            Kind::Integer | Kind::Real => RawCell::Number(self.text.trim().to_owned()),
            Kind::Nested => RawCell::Nested,
            Kind::Text | Kind::Date | Kind::Data if self.text.is_empty() => RawCell::Empty,
            Kind::Text | Kind::Date | Kind::Data => RawCell::Text(self.text.clone()),
        }
    }
}

/// Which settings format a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Which {
    Ini,
    PropertyList,
}

/// A settings file, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Settings {
    pub which: Which,
    /// Every value, in the order of the file.
    pub entries: Vec<Setting>,
    /// An INI file's section headers, in order, each as often as written.
    pub sections: Vec<String>,
    /// A property list's root dictionary's keys, in order.
    pub root_keys: Vec<String>,
}

/// What quotes around an INI value are, as the document says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Quotes {
    /// Not said: a value that begins with a quote is not read.
    Unsaid,
    /// Part of the value.
    Literal,
    /// Around the value, and not part of it.
    Strip,
}

impl Quotes {
    pub(crate) fn from_name(name: Option<&str>) -> Self {
        match name {
            Some("literal") => Self::Literal,
            Some("strip") => Self::Strip,
            _ => Self::Unsaid,
        }
    }
}

fn unreadable(why: &str) -> StepFailure {
    stop(
        codes::FILE_UNREADABLE,
        format!("The file does not parse as the format its document declares: {why}."),
        "Correct the file, or correct `format` and `dialect` in the document.",
    )
}

fn not_read(why: &str) -> StepFailure {
    unsupported(&format!(
        "a settings file that {why}; what it means differs between the programs that read such files"
    ))
}

fn too_large(what: &str) -> StepFailure {
    stop(
        codes::FILE_TOO_LARGE,
        format!("The file holds {what} than the file pack reads."),
        "Raise the limits under `packs.file`, or point the document at a smaller file.",
    )
}

fn span(start: usize, end: usize) -> ByteSpan {
    ByteSpan {
        start: start as u64,
        end: end as u64,
    }
}

/// The text after a UTF-8 byte-order mark, and where it starts. UTF-16 and
/// text that is not UTF-8 are not read.
fn text_of(bytes: &[u8]) -> Result<(&str, usize), StepFailure> {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return Err(unreadable("it is UTF-16, and only UTF-8 is read"));
    }
    let offset = if bytes.starts_with(UTF8_MARK) {
        UTF8_MARK.len()
    } else {
        0
    };
    let text = std::str::from_utf8(bytes.get(offset..).unwrap_or_default())
        .map_err(|_| unreadable("it is not UTF-8"))?;
    Ok((text, offset))
}

const BLANKS: [char; 2] = [' ', '\t'];

/// Whether text holds what an INI reader may take as a comment after the
/// value: `;` or `#` at its start, or after a space or a tab.
fn holds_inline_comment(text: &str) -> bool {
    text.starts_with([';', '#'])
        || [" ;", "\t;", " #", "\t#"]
            .iter()
            .any(|mark| text.contains(mark))
}

/// Reads an INI file.
///
/// The rules, each recorded in decision 0246: a line is blank, a comment
/// (`;` or `#` first, after any blanks), a section header (`[name]`), or a
/// key, the separator, and a value. A value is trimmed of spaces and tabs.
/// Keys before the first header have no section. A key or a section is the
/// same for two spellings that differ only in ASCII case when it comes to
/// telling whether a place is there twice. These stop the whole file: a
/// line continued with a backslash or by indenting, a header that does not
/// close, a line with no separator, a carriage return that ends no line.
/// These stop the read of one value and leave the rest readable: a comment
/// after the value; a value in quotes when the document does not say what
/// quotes are; a key that holds the other separator.
pub(crate) fn parse_ini(
    bytes: &[u8],
    separator: char,
    quotes: Quotes,
    limits: &Limits,
) -> Result<Settings, StepFailure> {
    let (text, offset) = text_of(bytes)?;
    let other = if separator == '=' { ':' } else { '=' };
    let mut section: Option<String> = None;
    let mut sections = Vec::new();
    let mut entries = Vec::new();
    let mut at = 0_usize;
    for line in text.split_inclusive('\n') {
        let start = offset + at;
        at += line.len();
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        let trimmed = content.trim_matches(BLANKS);
        if content.contains('\r') {
            return Err(not_read("holds a carriage return that ends no line"));
        }
        if trimmed.is_empty() || trimmed.starts_with([';', '#']) {
            continue;
        }
        if content.starts_with(BLANKS) {
            return Err(not_read(
                "holds an indented line, which some programs read as the value above continued",
            ));
        }
        if content.trim_end_matches(BLANKS).ends_with('\\') {
            return Err(not_read("continues a line with a backslash"));
        }
        if let Some(header) = trimmed.strip_prefix('[') {
            let Some(name) = header
                .strip_suffix(']')
                .map(|name| name.trim_matches(BLANKS))
            else {
                return Err(not_read(
                    "holds a section header that does not end with `]`",
                ));
            };
            if name.is_empty() || name.contains(['[', ']']) {
                return Err(not_read("holds a section header with no plain name"));
            }
            if sections
                .iter()
                .any(|seen: &String| seen.eq_ignore_ascii_case(name))
            {
                return Err(not_read(
                    "holds one section header twice, which some programs read as one section and others as its first or its last block alone",
                ));
            }
            sections.push(name.to_owned());
            section = Some(name.to_owned());
            continue;
        }
        let Some(split) = content.find(separator) else {
            return Err(not_read(
                "holds a line that is not a section, a key with a value, or a comment",
            ));
        };
        let key = content
            .get(..split)
            .unwrap_or_default()
            .trim_matches(BLANKS);
        if key.is_empty() {
            return Err(not_read("holds a value with no key"));
        }
        let after = content.get(split + 1..).unwrap_or_default();
        let value = after.trim_matches(BLANKS);
        let lead = after.len() - after.trim_start_matches(BLANKS).len();
        let mut value_start = split + 1 + lead;
        let mut value_end = value_start + value.len();
        if value.is_empty() {
            // A value written later goes at the end of the line.
            value_start = content.len();
            value_end = content.len();
        }
        let mut written = value;
        let mut quote = None;
        let mut unread = None;
        if key.contains(other) {
            unread = Some("its key holds the other separator, which some programs split at");
        } else if holds_inline_comment(value) {
            unread = Some(
                "its value holds `;` or `#` after a blank, which some programs read as a comment",
            );
        } else if let Some(first) = value.chars().next().filter(|c| matches!(c, '"' | '\'')) {
            match quotes {
                Quotes::Unsaid => {
                    unread = Some(
                        "its value is in quotes, and the document does not say whether quotes are part of the value (`dialect: {quotes: literal}` or `{quotes: strip}`)",
                    );
                }
                Quotes::Literal => {}
                Quotes::Strip => {
                    let inner = value
                        .strip_prefix(first)
                        .and_then(|rest| rest.strip_suffix(first))
                        .filter(|inner| !inner.contains(first));
                    match inner {
                        Some(inner) if value.len() >= 2 => {
                            written = inner;
                            quote = Some(first);
                            value_start += first.len_utf8();
                            value_end -= first.len_utf8();
                        }
                        _ => unread = Some("its value has quotes that do not close it"),
                    }
                }
            }
        }
        if written.len() > limits.cell_bytes as usize {
            return Err(too_large("a longer value"));
        }
        let mut path = Vec::new();
        if let Some(section) = &section {
            path.push(Part::Key(section.clone()));
        }
        path.push(Part::Key(key.to_owned()));
        entries.push(Setting {
            path,
            kind: Kind::Text,
            text: written.to_owned(),
            span: span(start + value_start, start + value_end),
            whole_element: false,
            quote,
            unread,
        });
        if entries.len() > limits.cells as usize {
            return Err(too_large("more values"));
        }
    }
    Ok(Settings {
        which: Which::Ini,
        entries,
        sections,
        root_keys: Vec::new(),
    })
}

/// A reader over the XML form of a property list.
struct Plist<'a> {
    text: &'a str,
    at: usize,
    offset: usize,
    entries: Vec<Setting>,
    root_keys: Vec<String>,
    limits: &'a Limits,
}

/// One tag, as read.
struct Tag<'a> {
    name: &'a str,
    /// It closes itself: `<true/>`.
    closed: bool,
    start: usize,
    end: usize,
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':')
}

impl<'a> Plist<'a> {
    fn rest(&self) -> &'a str {
        self.text.get(self.at..).unwrap_or_default()
    }

    fn absolute(&self, at: usize) -> usize {
        self.offset + at
    }

    /// Blanks and comments.
    fn skip_between(&mut self) -> Result<(), StepFailure> {
        loop {
            let rest = self.rest();
            let blanks = rest.len() - rest.trim_start().len();
            self.at += blanks;
            if self.rest().starts_with("<!--") {
                let end = self
                    .rest()
                    .find("-->")
                    .ok_or_else(|| unreadable("a comment does not close"))?;
                self.at += end + 3;
            } else {
                return Ok(());
            }
        }
    }

    /// The prolog: the declaration, comments, and the one document type a
    /// property list carries, which is recognized and never fetched.
    fn prolog(&mut self) -> Result<(), StepFailure> {
        if self.rest().starts_with("<?xml") {
            let end = self
                .rest()
                .find("?>")
                .ok_or_else(|| unreadable("the declaration does not close"))?;
            let declaration = self
                .rest()
                .get(..end)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if let Some(at) = declaration.find("encoding") {
                let named = declaration.get(at..).unwrap_or_default();
                if !(named.contains("\"utf-8\"") || named.contains("'utf-8'")) {
                    return Err(unreadable("it declares an encoding other than UTF-8"));
                }
            }
            self.at += end + 2;
        }
        self.skip_between()?;
        if self.rest().starts_with("<!DOCTYPE") {
            let end = self
                .rest()
                .find('>')
                .ok_or_else(|| unreadable("the document type does not close"))?;
            let declared = self.rest().get("<!DOCTYPE".len()..end).unwrap_or_default();
            // Declarations of its own could define entities: never read.
            if declared.contains('[') {
                return Err(unreadable(
                    "it declares a document type with declarations of its own, which the file pack never reads",
                ));
            }
            let root = declared.trim_start();
            let names_plist = root
                .strip_prefix("plist")
                .is_some_and(|after| after.is_empty() || after.starts_with(char::is_whitespace));
            if !names_plist {
                return Err(unreadable(
                    "it declares a document type other than a property list's",
                ));
            }
            self.at += end + 1;
        }
        self.skip_between()
    }

    fn tag(&mut self) -> Result<Tag<'a>, StepFailure> {
        let start = self.at;
        let rest = self.rest();
        if rest.starts_with("<![CDATA[") {
            return Err(not_read("holds a literal section"));
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            return Err(not_read(
                "holds a processing instruction or a declaration inside its values",
            ));
        }
        let Some(after) = rest.strip_prefix('<') else {
            return Err(unreadable("text stands where an element is expected"));
        };
        let name_len = after
            .find(|c: char| !is_name_char(c))
            .unwrap_or(after.len());
        let name = after.get(..name_len).unwrap_or_default();
        if name.is_empty() {
            return Err(unreadable("an element has no name"));
        }
        // The attributes, honouring quotes, up to the end of the tag.
        let mut quote: Option<char> = None;
        let mut end = None;
        for (index, c) in after.char_indices().skip(name_len) {
            match (quote, c) {
                (Some(open), c) if c == open => quote = None,
                (Some(_), _) => {}
                (None, '"' | '\'') => quote = Some(c),
                (None, '<') => return Err(unreadable("a tag does not close")),
                (None, '>') => {
                    end = Some(index);
                    break;
                }
                _ => {}
            }
        }
        let end = end.ok_or_else(|| unreadable("a tag does not close"))?;
        let inside = after.get(name_len..end).unwrap_or_default();
        let closed = inside.trim_end().ends_with('/');
        self.at += 1 + end + 1;
        Ok(Tag {
            name,
            closed,
            start,
            end: self.at,
        })
    }

    fn close(&mut self, name: &str) -> Result<(), StepFailure> {
        let rest = self.rest();
        let Some(after) = rest.strip_prefix("</").and_then(|r| r.strip_prefix(name)) else {
            return Err(unreadable("an element does not close where it should"));
        };
        let blanks = after.len() - after.trim_start().len();
        if !after.get(blanks..).is_some_and(|r| r.starts_with('>')) {
            return Err(unreadable("a closing tag is not written as one"));
        }
        self.at += 2 + name.len() + blanks + 1;
        Ok(())
    }

    /// The text up to the next tag, its start, and its end.
    fn text(&mut self) -> Result<(String, usize, usize), StepFailure> {
        let start = self.at;
        let length = self.rest().find('<').unwrap_or(self.rest().len());
        let raw = self.rest().get(..length).unwrap_or_default();
        self.at += length;
        if !self.rest().starts_with("</") {
            return Err(not_read(
                "holds a comment, a literal section, or an element inside a value's text",
            ));
        }
        if raw.len() > self.limits.cell_bytes as usize {
            return Err(too_large("a longer value"));
        }
        Ok((decoded(raw)?, start, self.at))
    }

    fn push(&mut self, setting: Setting) -> Result<(), StepFailure> {
        self.entries.push(setting);
        if self.entries.len() > self.limits.cells as usize {
            return Err(too_large("more values"));
        }
        Ok(())
    }

    fn value(&mut self, path: Vec<Part>, depth: usize) -> Result<(), StepFailure> {
        if depth > MAX_DEPTH {
            return Err(too_large("values nested deeper"));
        }
        let tag = self.tag()?;
        let scalar = match tag.name {
            "dict" | "array" => None,
            "string" => Some(Kind::Text),
            "integer" => Some(Kind::Integer),
            "real" => Some(Kind::Real),
            "date" => Some(Kind::Date),
            "data" => Some(Kind::Data),
            "true" | "false" => Some(Kind::Bool),
            _ => {
                return Err(unreadable(
                    "an element stands where a property list has none",
                ));
            }
        };
        if let Some(kind) = scalar {
            let (text, value_span, whole) = if kind == Kind::Bool {
                if !tag.closed {
                    self.close(tag.name)?;
                }
                (tag.name.to_owned(), span(tag.start, self.at), true)
            } else if tag.closed {
                (String::new(), span(tag.start, tag.end), true)
            } else {
                let (text, start, end) = self.text()?;
                self.close(tag.name)?;
                (text, span(start, end), false)
            };
            let value_span = ByteSpan {
                start: self.absolute(value_span.start as usize) as u64,
                end: self.absolute(value_span.end as usize) as u64,
            };
            return self.push(Setting {
                path,
                kind,
                text,
                span: value_span,
                whole_element: whole,
                quote: None,
                unread: (kind == Kind::Data)
                    .then_some("it is binary data, which the file pack does not read"),
            });
        }
        let start = self.absolute(tag.start);
        self.push(Setting {
            path: path.clone(),
            kind: Kind::Nested,
            text: String::new(),
            span: span(start, start),
            whole_element: true,
            quote: None,
            unread: None,
        })?;
        if tag.closed {
            return Ok(());
        }
        let mut keys: Vec<String> = Vec::new();
        let mut index = 0;
        loop {
            self.skip_between()?;
            if self.rest().starts_with("</") {
                self.close(tag.name)?;
                break;
            }
            let mut inner = path.clone();
            if tag.name == "dict" {
                let key = self.tag()?;
                if key.name != "key" || key.closed {
                    return Err(unreadable(
                        "a dictionary holds a value with no key before it",
                    ));
                }
                let (name, _, _) = self.text()?;
                self.close("key")?;
                if keys.contains(&name) {
                    return Err(unreadable("a dictionary holds one key twice"));
                }
                keys.push(name.clone());
                self.skip_between()?;
                inner.push(Part::Key(name));
            } else {
                inner.push(Part::Index(index));
                index += 1;
            }
            self.value(inner, depth + 1)?;
        }
        if path.is_empty() && tag.name == "dict" {
            self.root_keys = keys;
        }
        Ok(())
    }
}

/// Text with the format's own references read: the five names and
/// character references. Any other is refused.
fn decoded(raw: &str) -> Result<String, StepFailure> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(rest.get(..at).unwrap_or_default());
        let after = rest.get(at + 1..).unwrap_or_default();
        let end = after
            .find(';')
            .ok_or_else(|| unreadable("a reference does not end"))?;
        let name = after.get(..end).unwrap_or_default();
        let read = match name {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => {
                let number = name
                    .strip_prefix("#x")
                    .map(|hex| u32::from_str_radix(hex, 16))
                    .or_else(|| name.strip_prefix('#').map(str::parse::<u32>));
                number.and_then(Result::ok).and_then(char::from_u32)
            }
        };
        let Some(read) = read else {
            return Err(unreadable("it names an entity the format does not define"));
        };
        out.push(read);
        rest = after.get(end + 1..).unwrap_or_default();
    }
    out.push_str(rest);
    Ok(out)
}

/// Reads a property list in its XML form.
pub(crate) fn parse_plist(bytes: &[u8], limits: &Limits) -> Result<Settings, StepFailure> {
    if bytes.starts_with(BINARY_PLIST) {
        return Err(unsupported(
            "a property list in its binary form, which is not built; only the XML form is read",
        ));
    }
    let (text, offset) = text_of(bytes)?;
    let mut reader = Plist {
        text,
        at: 0,
        offset,
        entries: Vec::new(),
        root_keys: Vec::new(),
        limits,
    };
    reader.prolog()?;
    let root = reader.tag()?;
    if root.name != "plist" || root.closed {
        return Err(unreadable("its root element is not `plist`"));
    }
    reader.skip_between()?;
    reader.value(Vec::new(), 0)?;
    reader.skip_between()?;
    reader.close("plist")?;
    reader.skip_between()?;
    if !reader.rest().is_empty() {
        return Err(unreadable("something follows the property list"));
    }
    Ok(Settings {
        which: Which::PropertyList,
        entries: reader.entries,
        sections: Vec::new(),
        root_keys: reader.root_keys,
    })
}

impl Settings {
    /// The value at a path of keys, when the file has it once. A path an
    /// INI file has twice, two spellings that differ only in ASCII case
    /// included, is `key_not_unique`; one it has only in another case is not
    /// taken for it.
    pub(crate) fn find(&self, path: &[String]) -> Result<Option<&Setting>, StepFailure> {
        let is = |setting: &Setting, fold: bool| {
            setting.path.len() == path.len()
                && setting
                    .path
                    .iter()
                    .zip(path)
                    .all(|(part, wanted)| match part {
                        Part::Key(key) if fold => key.eq_ignore_ascii_case(wanted),
                        Part::Key(key) => key == wanted,
                        Part::Index(_) => false,
                    })
        };
        if self.which == Which::PropertyList {
            return Ok(self.entries.iter().find(|setting| is(setting, false)));
        }
        let mut found = self.entries.iter().filter(|setting| is(setting, true));
        match (found.next(), found.next()) {
            (None, _) => Ok(None),
            (Some(one), None) if is(one, false) => Ok(Some(one)),
            (Some(_), None) => Err(stop(
                codes::PLACE_NOT_FOUND,
                "The file holds this key only in another case, and programs differ on whether that is the same key.",
                "Write the key in the document as the file spells it.",
            )),
            (Some(_), Some(_)) => Err(stop(
                codes::KEY_NOT_UNIQUE,
                "The file holds this key twice in one section, or in two spellings that differ only in case, so the place cannot be told.",
                "Remove the second key from the file.",
            )),
        }
    }

    /// The untouched check: the same values at the same paths, in the same
    /// order, with the same sections and root keys, and only the values at
    /// `changed`, spans of the old file, other.
    pub(crate) fn same_except(&self, new: &Self, changed: &[ByteSpan]) -> bool {
        self.which == new.which
            && self.sections == new.sections
            && self.root_keys == new.root_keys
            && self.entries.len() == new.entries.len()
            && self.entries.iter().zip(&new.entries).all(|(old, new)| {
                old.path == new.path
                    && old.kind == new.kind
                    && old.quote == new.quote
                    && old.unread == new.unread
                    && (old.text == new.text || changed.contains(&old.span))
            })
    }
}

fn refused(why: &str) -> StepFailure {
    unsupported(&format!("writing a value that {why}"))
}

/// The bytes a value is written as, at a setting's span.
pub(crate) fn literal(
    setting: &Setting,
    which: Which,
    quotes: Quotes,
    value: &Value,
    field_type: &FieldTypePlan,
) -> Result<Vec<u8>, StepFailure> {
    if setting.unread.is_some() || matches!(setting.kind, Kind::Data | Kind::Nested) {
        return Err(refused("stands at a place the file pack does not read"));
    }
    if which == Which::PropertyList && setting.kind == Kind::Bool {
        return match value {
            Value::Bool(true) => Ok(b"<true/>".to_vec()),
            Value::Bool(false) => Ok(b"<false/>".to_vec()),
            _ => Err(refused("is not true or false, where the file holds one")),
        };
    }
    if setting.kind == Kind::Date {
        return Err(refused("is a date, which the file pack does not write yet"));
    }
    let text = edit::spell(value, field_type)?.unwrap_or_default();
    // A line feed and a carriage return end a line everywhere; a next-line
    // character and the line and paragraph separators end one for some
    // readers.
    if text.contains(['\r', '\n', '\u{85}', '\u{2028}', '\u{2029}']) {
        return Err(refused("holds a line break"));
    }
    match which {
        Which::Ini => {
            if text.starts_with(BLANKS) || text.ends_with(BLANKS) {
                return Err(refused(
                    "begins or ends with a blank, which the file would lose",
                ));
            }
            // A tab, a null, and every other control character: some
            // readers stop or split at them.
            if text.chars().any(char::is_control) {
                return Err(refused(
                    "holds a control character, at which some programs stop or split a line",
                ));
            }
            if holds_inline_comment(&text) {
                return Err(refused(
                    "holds `;` or `#` at its start or after a blank, which a program may read as a comment",
                ));
            }
            match (setting.quote, quotes) {
                (Some(quote), _) if text.contains(quote) => {
                    return Err(refused("holds the quote it is written inside"));
                }
                (None, Quotes::Unsaid | Quotes::Strip) if text.starts_with(['"', '\'']) => {
                    return Err(refused(
                        "begins with a quote, which a program may take as around the value",
                    ));
                }
                _ => {}
            }
            Ok(text.into_bytes())
        }
        Which::PropertyList => {
            let fits = match setting.kind {
                Kind::Integer => {
                    let digits = text.strip_prefix(['-', '+']).unwrap_or(&text);
                    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
                }
                Kind::Real => {
                    let digits = text.strip_prefix(['-', '+']).unwrap_or(&text);
                    let mut parts = digits.splitn(2, '.');
                    let whole = parts.next().unwrap_or_default();
                    let fraction = parts.next();
                    !whole.is_empty()
                        && whole.bytes().all(|b| b.is_ascii_digit())
                        && fraction
                            .is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
                }
                _ => true,
            };
            if !fits {
                return Err(refused(&format!(
                    "is not spelled as the `{}` the file holds there",
                    setting.kind.element()
                )));
            }
            if text
                .chars()
                .any(|c| (c.is_control() && c != '\t') || matches!(c, '\u{FFFE}' | '\u{FFFF}'))
            {
                return Err(refused("holds a character the XML form cannot hold"));
            }
            let escaped = text
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            Ok(if setting.whole_element {
                let element = setting.kind.element();
                format!("<{element}>{escaped}</{element}>").into_bytes()
            } else {
                escaped.into_bytes()
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> Limits {
        Limits {
            bytes: 1 << 20,
            unpacked_bytes: 1 << 24,
            rows: 1000,
            cells: 1000,
            cell_bytes: 4096,
        }
    }

    fn text_type() -> FieldTypePlan {
        FieldTypePlan {
            name: "text".to_owned(),
            base: a2m_core::FieldBase::Text,
            ty: a2m_core::ValueType::Text,
            places: None,
            rounding: None,
            step: None,
            unit: None,
            display_unit: None,
            currency: None,
            zone: None,
            codes: Vec::new(),
            sensitivity: a2m_core::Sensitivity::Internal,
            untrusted: false,
            format: a2m_core::FieldFormat::Text {
                case_sensitive: true,
                whitespace: a2m_core::WhitespaceRule::Exact,
            },
            normalizer: None,
        }
    }

    fn keys(path: &[&str]) -> Vec<String> {
        path.iter().map(|part| (*part).to_owned()).collect()
    }

    #[test]
    fn an_ini_file_gives_each_value_with_its_span() {
        let bytes = b"; top\r\nname = example\r\n\r\n[orders]\r\nprefix=A-\r\nempty =\r\n";
        let file = parse_ini(bytes, '=', Quotes::Unsaid, &limits()).unwrap();
        assert_eq!(file.sections, ["orders"]);
        let name = file.find(&keys(&["name"])).unwrap().unwrap();
        assert_eq!(name.text, "example");
        assert_eq!(name.span.of(bytes), Some(&b"example"[..]));
        let prefix = file.find(&keys(&["orders", "prefix"])).unwrap().unwrap();
        assert_eq!(prefix.span.of(bytes), Some(&b"A-"[..]));
        let empty = file.find(&keys(&["orders", "empty"])).unwrap().unwrap();
        assert_eq!(empty.raw(), RawCell::Empty);
        assert_eq!(empty.span.start, empty.span.end);
        assert!(file.find(&keys(&["prefix"])).unwrap().is_none());
    }

    #[test]
    fn an_ini_form_with_two_meanings_is_a_named_stop() {
        for (bytes, whole) in [
            (&b"[a]\nkey = one \\\n  two\n"[..], true),
            (b"[a]\nkey = one\n  two\n", true),
            (b"[a\nkey = one\n", true),
            (b"just words\n", true),
            (b"key = one\rkey2 = two\n", true),
            (b"[a]\nkey = one\n[A]\nother = two\n", true),
            (b"key = one ; a comment\n", false),
            (b"key = \"quoted\"\n", false),
            (b"a:b = c\n", false),
        ] {
            match parse_ini(bytes, '=', Quotes::Unsaid, &limits()) {
                Err(failure) => {
                    assert!(whole, "{bytes:?}");
                    assert_eq!(failure.failure.code(), &codes::SHAPE_UNSUPPORTED);
                }
                Ok(file) => {
                    assert!(!whole, "{bytes:?}");
                    assert!(file.entries[0].unread.is_some(), "{bytes:?}");
                }
            }
        }
        // Two keys of one section, in any case, are one place twice.
        let twice = parse_ini(b"[a]\nKey = 1\nkey = 2\n", '=', Quotes::Unsaid, &limits()).unwrap();
        let failure = twice.find(&keys(&["a", "key"])).unwrap_err();
        assert_eq!(failure.failure.code(), &codes::KEY_NOT_UNIQUE);
        let other_case = parse_ini(b"[a]\nKey = 1\n", '=', Quotes::Unsaid, &limits()).unwrap();
        assert!(other_case.find(&keys(&["a", "key"])).is_err());
    }

    #[test]
    fn quotes_are_read_as_the_document_says() {
        let bytes = b"a = \"x y\"\nb = 'p'\n";
        let strip = parse_ini(bytes, '=', Quotes::Strip, &limits()).unwrap();
        let a = strip.find(&keys(&["a"])).unwrap().unwrap();
        assert_eq!((a.text.as_str(), a.quote), ("x y", Some('"')));
        assert_eq!(a.span.of(bytes), Some(&b"x y"[..]));
        let literal = parse_ini(bytes, '=', Quotes::Literal, &limits()).unwrap();
        assert_eq!(literal.find(&keys(&["b"])).unwrap().unwrap().text, "'p'");
    }

    const PLIST: &str = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<!DOCTYPE plist PUBLIC \"-//Example//DTD PLIST 1.0//EN\" \"http://example.invalid/plist.dtd\">\n",
        "<plist version=\"1.0\">\n<dict>\n",
        "  <!-- a comment -->\n",
        "  <key>Name</key><string>Rails &amp; brackets</string>\n",
        "  <key>Count</key><integer>3</integer>\n",
        "  <key>Open</key><true/>\n",
        "  <key>Empty</key><string/>\n",
        "  <key>Inner</key><dict><key>Rate</key><real>1.5</real></dict>\n",
        "  <key>List</key><array><string>a</string></array>\n",
        "</dict>\n</plist>\n",
    );

    #[test]
    fn a_property_list_gives_each_value_by_its_keys() {
        let file = parse_plist(PLIST.as_bytes(), &limits()).unwrap();
        assert_eq!(
            file.root_keys,
            ["Name", "Count", "Open", "Empty", "Inner", "List"]
        );
        let name = file.find(&keys(&["Name"])).unwrap().unwrap();
        assert_eq!(name.text, "Rails & brackets");
        assert_eq!(
            name.span.of(PLIST.as_bytes()),
            Some(&b"Rails &amp; brackets"[..])
        );
        let open = file.find(&keys(&["Open"])).unwrap().unwrap();
        assert_eq!(open.raw(), RawCell::Bool(true));
        assert_eq!(open.span.of(PLIST.as_bytes()), Some(&b"<true/>"[..]));
        let rate = file.find(&keys(&["Inner", "Rate"])).unwrap().unwrap();
        assert_eq!(rate.raw(), RawCell::Number("1.5".to_owned()));
        // An element of an array is no place.
        assert!(file.find(&keys(&["List", "0"])).unwrap().is_none());
    }

    #[test]
    fn a_property_list_reads_no_document_type_and_no_entity() {
        for text in [
            "<!DOCTYPE plist [<!ENTITY a \"b\">]><plist><string>&a;</string></plist>",
            "<!DOCTYPE other><plist><string>x</string></plist>",
            "<plist><string>&a;</string></plist>",
            "<plist><dict><key>a</key><true/><key>a</key><false/></dict></plist>",
            "<plist><unknown/></plist>",
        ] {
            let failure = parse_plist(text.as_bytes(), &limits()).unwrap_err();
            assert_eq!(failure.failure.code(), &codes::FILE_UNREADABLE, "{text}");
        }
        let binary = parse_plist(b"bplist00\x01\x02", &limits()).unwrap_err();
        assert_eq!(binary.failure.code(), &codes::SHAPE_UNSUPPORTED);
        let deep = format!(
            "<plist>{}{}</plist>",
            "<array>".repeat(40),
            "</array>".repeat(40)
        );
        let failure = parse_plist(deep.as_bytes(), &limits()).unwrap_err();
        assert_eq!(failure.failure.code(), &codes::FILE_TOO_LARGE);
        let cdata = "<plist><string><![CDATA[x]]></string></plist>";
        assert!(parse_plist(cdata.as_bytes(), &limits()).is_err());
    }

    #[test]
    fn a_value_is_written_only_as_the_file_can_hold_it() {
        let text = text_type();
        let ini = parse_ini(b"a = x\n", '=', Quotes::Unsaid, &limits()).unwrap();
        let a = &ini.entries[0];
        let write =
            |value: &str| literal(a, Which::Ini, Quotes::Unsaid, &Value::from(value), &text);
        assert_eq!(write("y z").unwrap(), b"y z");
        for refused in [
            " y",
            "y ",
            "y ; z",
            "#y",
            "\"y\"",
            "two\nlines",
            "a\tb",
            "a\0b",
            "a\u{b}b",
            "a\u{c}b",
            "a\u{85}b",
            "a\u{2028}b",
            "a\u{2029}b",
        ] {
            assert!(write(refused).is_err(), "{refused:?}");
        }
        let plist = parse_plist(PLIST.as_bytes(), &limits()).unwrap();
        let empty = plist.find(&keys(&["Empty"])).unwrap().unwrap();
        let out = literal(
            empty,
            Which::PropertyList,
            Quotes::Unsaid,
            &Value::from("a<b"),
            &text,
        );
        assert_eq!(out.unwrap(), b"<string>a&lt;b</string>");
        for refused in ["a\u{FFFE}b", "a\u{FFFF}b", "a\u{2028}b", "a\0b"] {
            let out = literal(
                empty,
                Which::PropertyList,
                Quotes::Unsaid,
                &Value::from(refused),
                &text,
            );
            assert!(out.is_err(), "{refused:?}");
        }
        let count = plist.find(&keys(&["Count"])).unwrap().unwrap();
        let out = literal(
            count,
            Which::PropertyList,
            Quotes::Unsaid,
            &Value::from("x"),
            &text,
        );
        assert!(out.is_err());
        let open = plist.find(&keys(&["Open"])).unwrap().unwrap();
        let out = literal(
            open,
            Which::PropertyList,
            Quotes::Unsaid,
            &Value::Bool(false),
            &text,
        );
        assert_eq!(out.unwrap(), b"<false/>");
    }
}
