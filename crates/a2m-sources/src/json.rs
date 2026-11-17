//! The reader of JSON documents. A number is kept as the text of its token,
//! so it never passes through a float; a key written twice in one record is
//! refused; every value carries the span of its token.
//!
//! It is this crate's own reader for those three reasons. The document tree
//! is public so that the file pack reads a JSON file the same way.

use a2m_core::Failure;

use crate::csv::text_start;
use crate::error::{malformed, mismatch, too_large};
use crate::fingerprint::{SheetStructure, Structure};
use crate::model::{
    ByteSpan, Cell, CellRef, ContentKind, Limits, Parsed, ParsedPart, PartBytes, PlacedCell,
    RawCell, ReaderName, SourceInput, SourceReader,
};

/// How deep lists and records may nest.
const DEPTH: u32 = 64;

/// One value of a JSON document, with where its token lies in the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonNode {
    /// The bytes of the whole value: a token, or a list or record with its
    /// brackets.
    pub span: ByteSpan,
    /// What it is.
    pub value: JsonValue,
}

/// What a JSON value is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonValue {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number, as the text of its token: `2.50` stays `2.50`.
    Number(String),
    /// A string, with its escapes read.
    Text(String),
    /// A list.
    List(Vec<JsonNode>),
    /// A record: its keys, in the order written, each with its value. No key
    /// is written twice.
    Record(Vec<(String, JsonNode)>),
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
    values: u32,
    limits: &'a Limits,
}

impl Parser<'_> {
    fn error(&self, what: &str) -> Failure {
        malformed(format!(
            "{what}, at byte {} of the JSON document",
            self.at + 1
        ))
    }

    fn space(&mut self) {
        while matches!(self.bytes.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn word(&mut self, word: &[u8]) -> Result<(), Failure> {
        if self.bytes.get(self.at..self.at + word.len()) == Some(word) {
            self.at += word.len();
            Ok(())
        } else {
            Err(self.error("a value is not one JSON allows"))
        }
    }

    fn value(&mut self, depth: u32) -> Result<JsonNode, Failure> {
        self.space();
        let start = self.at;
        self.values += 1;
        if self.values > self.limits.cells {
            return Err(too_large(format!("more than {} values", self.limits.cells)));
        }
        let value = match self.bytes.get(self.at) {
            Some(b'n') => self.word(b"null").map(|()| JsonValue::Null)?,
            Some(b't') => self.word(b"true").map(|()| JsonValue::Bool(true))?,
            Some(b'f') => self.word(b"false").map(|()| JsonValue::Bool(false))?,
            Some(b'"') => JsonValue::Text(self.string()?),
            Some(b'-' | b'0'..=b'9') => JsonValue::Number(self.number()?),
            Some(b'[') => {
                if depth >= DEPTH {
                    return Err(self.error("lists and records nest too deep"));
                }
                self.at += 1;
                let mut items = Vec::new();
                self.space();
                if self.bytes.get(self.at) == Some(&b']') {
                    self.at += 1;
                } else {
                    loop {
                        items.push(self.value(depth + 1)?);
                        self.space();
                        match self.bytes.get(self.at) {
                            Some(b',') => self.at += 1,
                            Some(b']') => {
                                self.at += 1;
                                break;
                            }
                            _ => return Err(self.error("a list is not closed")),
                        }
                    }
                }
                JsonValue::List(items)
            }
            Some(b'{') => {
                if depth >= DEPTH {
                    return Err(self.error("lists and records nest too deep"));
                }
                self.at += 1;
                let mut members: Vec<(String, JsonNode)> = Vec::new();
                self.space();
                if self.bytes.get(self.at) == Some(&b'}') {
                    self.at += 1;
                } else {
                    loop {
                        self.space();
                        if self.bytes.get(self.at) != Some(&b'"') {
                            return Err(self.error("a record's key is not a string"));
                        }
                        let key = self.string()?;
                        if members.iter().any(|(known, _)| *known == key) {
                            return Err(self.error("a key is written twice in one record"));
                        }
                        self.space();
                        if self.bytes.get(self.at) != Some(&b':') {
                            return Err(self.error("a record's key has no colon after it"));
                        }
                        self.at += 1;
                        let value = self.value(depth + 1)?;
                        members.push((key, value));
                        self.space();
                        match self.bytes.get(self.at) {
                            Some(b',') => self.at += 1,
                            Some(b'}') => {
                                self.at += 1;
                                break;
                            }
                            _ => return Err(self.error("a record is not closed")),
                        }
                    }
                }
                JsonValue::Record(members)
            }
            _ => return Err(self.error("a value is missing or is not one JSON allows")),
        };
        Ok(JsonNode {
            span: ByteSpan::new(start, self.at),
            value,
        })
    }

    fn number(&mut self) -> Result<String, Failure> {
        let start = self.at;
        let digits = |parser: &mut Self| {
            let from = parser.at;
            while matches!(parser.bytes.get(parser.at), Some(b'0'..=b'9')) {
                parser.at += 1;
            }
            parser.at - from
        };
        if self.bytes.get(self.at) == Some(&b'-') {
            self.at += 1;
        }
        let whole_start = self.at;
        let whole = digits(self);
        if whole == 0 || (whole > 1 && self.bytes.get(whole_start) == Some(&b'0')) {
            return Err(self.error("a number is not written as JSON allows"));
        }
        if self.bytes.get(self.at) == Some(&b'.') {
            self.at += 1;
            if digits(self) == 0 {
                return Err(self.error("a number has no digit after its point"));
            }
        }
        if matches!(self.bytes.get(self.at), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.bytes.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if digits(self) == 0 {
                return Err(self.error("a number has no digit in its exponent"));
            }
        }
        let token = self.bytes.get(start..self.at).unwrap_or(&[]);
        if token.len() > self.limits.cell_bytes as usize {
            return Err(too_large(format!(
                "a number is longer than {} bytes",
                self.limits.cell_bytes
            )));
        }
        Ok(String::from_utf8_lossy(token).into_owned())
    }

    fn hex4(&mut self) -> Result<u32, Failure> {
        let digits = self
            .bytes
            .get(self.at..self.at + 4)
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .filter(|digits| digits.bytes().all(|b| b.is_ascii_hexdigit()))
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            .ok_or_else(|| self.error("an escape is not four hexadecimal digits"))?;
        self.at += 4;
        Ok(digits)
    }

    fn string(&mut self) -> Result<String, Failure> {
        self.at += 1;
        let mut text: Vec<u8> = Vec::new();
        loop {
            let Some(&byte) = self.bytes.get(self.at) else {
                return Err(self.error("a string is not closed"));
            };
            self.at += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    let Some(&escape) = self.bytes.get(self.at) else {
                        return Err(self.error("a string is not closed"));
                    };
                    self.at += 1;
                    let character = match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&first) {
                                if self.bytes.get(self.at..self.at + 2) != Some(b"\\u") {
                                    return Err(self.error("half of a character pair stands alone"));
                                }
                                self.at += 2;
                                let second = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&second) {
                                    return Err(self.error("half of a character pair stands alone"));
                                }
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                            } else {
                                first
                            };
                            char::from_u32(code).ok_or_else(|| {
                                self.error("half of a character pair stands alone")
                            })?
                        }
                        _ => return Err(self.error("a string has an escape JSON does not allow")),
                    };
                    let mut buffer = [0_u8; 4];
                    text.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
                }
                0..=0x1F => {
                    return Err(
                        self.error("a string holds a control character that is not escaped")
                    );
                }
                other => text.push(other),
            }
            if text.len() > self.limits.cell_bytes as usize {
                return Err(too_large(format!(
                    "a string is longer than {} bytes",
                    self.limits.cell_bytes
                )));
            }
        }
        // The document is UTF-8 and a string is cut only at ASCII bytes.
        String::from_utf8(text).map_err(|_| self.error("a string is not UTF-8"))
    }
}

/// Reads a whole JSON document: one value, and nothing after it but space.
/// The bytes are UTF-8 with no byte-order mark; spans count from their start.
///
/// # Errors
///
/// `source_malformed` when the bytes are not one JSON document, with the
/// place and never the content; `source_too_large` when it has more values
/// than `limits.cells` or a string or number longer than `limits.cell_bytes`.
pub fn parse_document(bytes: &[u8], limits: &Limits) -> Result<JsonNode, Failure> {
    let mut parser = Parser {
        bytes,
        at: 0,
        values: 0,
        limits,
    };
    let root = parser.value(0)?;
    parser.space();
    if parser.at != bytes.len() {
        return Err(parser.error("something follows the document's one value"));
    }
    Ok(root)
}

/// The built-in reader of JSON documents: a list of records at the root, or
/// a record whose members include lists of records, each list a part named
/// by its key.
#[derive(Debug, Clone)]
pub struct JsonReader {
    name: ReaderName,
}

impl JsonReader {
    /// The reader, named `json`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            name: ReaderName::builtin(ReaderName::JSON),
        }
    }
}

impl Default for JsonReader {
    fn default() -> Self {
        Self::new()
    }
}

fn document(input: &SourceInput<'_>) -> Result<(usize, JsonNode), Failure> {
    let start = text_start(input.bytes)?;
    let text = input.bytes.get(start..).unwrap_or(&[]);
    let first = text.iter().find(|byte| !byte.is_ascii_whitespace());
    if !matches!(first, Some(b'{' | b'[')) {
        return Err(mismatch(
            "the content does not start as a JSON list or record",
        ));
    }
    Ok((start, parse_document(text, &input.limits)?))
}

fn scalar(node: &JsonNode) -> RawCell {
    match &node.value {
        JsonValue::Null => RawCell::Empty,
        JsonValue::Bool(flag) => RawCell::Bool(*flag),
        JsonValue::Number(token) => RawCell::Number(token.clone()),
        JsonValue::Text(text) => RawCell::Text(text.clone()),
        JsonValue::List(_) | JsonValue::Record(_) => RawCell::Nested,
    }
}

/// One list of records as a part: a row for each element, a column for each
/// key in the order the keys first appear.
fn list_part(
    name: &str,
    items: &[JsonNode],
    index: u32,
    offset: usize,
    limits: &Limits,
) -> Result<(ParsedPart, Vec<String>), Failure> {
    let mut keys: Vec<String> = Vec::new();
    let mut cells = Vec::new();
    let mut widths = Vec::new();
    if items.len() > limits.rows as usize {
        return Err(too_large(format!("more than {} records", limits.rows)));
    }
    for (position, item) in items.iter().enumerate() {
        let row = u32::try_from(position + 1).unwrap_or(u32::MAX);
        let JsonValue::Record(members) = &item.value else {
            return Err(malformed(format!(
                "element {row} of a list of records is not a record"
            )));
        };
        let mut placed_row = Vec::with_capacity(members.len());
        for (key, node) in members {
            let column = match keys.iter().position(|known| known == key) {
                Some(found) => found + 1,
                None => {
                    keys.push(key.clone());
                    keys.len()
                }
            };
            placed_row.push(PlacedCell {
                cell: Cell {
                    raw: scalar(node),
                    at: CellRef::new(index, row, u32::try_from(column).unwrap_or(u32::MAX)),
                    format: None,
                    hidden: false,
                    merged: None,
                },
                value_span: Some(ByteSpan {
                    start: node.span.start + offset as u64,
                    end: node.span.end + offset as u64,
                }),
                shared: None,
            });
        }
        placed_row.sort_by_key(|placed| placed.cell.at.column);
        widths.push(u32::try_from(members.len()).unwrap_or(u32::MAX));
        cells.extend(placed_row);
    }
    let part = ParsedPart {
        name: name.to_owned(),
        path: String::new(),
        bytes: PartBytes::Input,
        cells,
        rows: u32::try_from(items.len()).unwrap_or(u32::MAX),
        widths,
    };
    Ok((part, keys))
}

impl SourceReader for JsonReader {
    fn name(&self) -> &ReaderName {
        &self.name
    }

    fn check(&self, input: &SourceInput<'_>) -> Result<(), Failure> {
        document(input).map(|_| ())
    }

    fn parse(&self, input: &SourceInput<'_>) -> Result<Parsed, Failure> {
        let (offset, root) = document(input)?;
        let mut parts = Vec::new();
        let mut sheets = Vec::new();
        let mut add = |name: &str, items: &[JsonNode]| -> Result<(), Failure> {
            let index = u32::try_from(parts.len()).unwrap_or(u32::MAX);
            let (part, headers) = list_part(name, items, index, offset, &input.limits)?;
            parts.push(part);
            sheets.push(SheetStructure {
                name: name.to_owned(),
                headers,
                ..SheetStructure::default()
            });
            Ok(())
        };
        match &root.value {
            JsonValue::List(items) => add("", items)?,
            JsonValue::Record(members) => {
                for (key, node) in members {
                    if let JsonValue::List(items) = &node.value
                        && items
                            .iter()
                            .all(|item| matches!(item.value, JsonValue::Record(_)))
                    {
                        add(key, items)?;
                    }
                }
            }
            _ => return Err(mismatch("the document is not a list or a record")),
        }
        Ok(Parsed {
            kind: ContentKind::Json,
            structure: Structure {
                sheets,
                ..Structure::default()
            },
            parts,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{FILE_TYPE_MISMATCH, SOURCE_MALFORMED, SOURCE_TOO_LARGE};
    use crate::model::ReaderOptions;
    use crate::testing::LIMITS;

    fn input(bytes: &[u8]) -> SourceInput<'_> {
        SourceInput {
            bytes,
            limits: LIMITS,
            options: ReaderOptions::default(),
        }
    }

    #[test]
    fn a_number_keeps_the_text_of_its_token() {
        let bytes = br#"[{"code": "EX-1", "price": 2.50, "big": 12345678901234567890.123456789, "e": 1.9e1, "zero": -0, "ok": true, "note": null, "tags": ["a"]}]"#;
        let parsed = JsonReader::new().parse(&input(bytes)).unwrap();
        let raws: Vec<&RawCell> = parsed.parts[0].cells.iter().map(|p| &p.cell.raw).collect();
        assert_eq!(
            raws,
            [
                &RawCell::Text("EX-1".to_owned()),
                &RawCell::Number("2.50".to_owned()),
                &RawCell::Number("12345678901234567890.123456789".to_owned()),
                &RawCell::Number("1.9e1".to_owned()),
                &RawCell::Number("-0".to_owned()),
                &RawCell::Bool(true),
                &RawCell::Empty,
                &RawCell::Nested,
            ]
        );
        assert_eq!(
            parsed.structure.sheets[0].headers,
            ["code", "price", "big", "e", "zero", "ok", "note", "tags"]
        );
    }

    #[test]
    fn every_span_covers_exactly_the_token_of_its_value() {
        let bytes = b"\xEF\xBB\xBF{\"lines\": [ {\"code\":\"a\\u00e9\\n\" , \"qty\" : 1.50},\n {\"qty\":2,\"code\":\"b\"} ], \"other\": 1}";
        let parsed = JsonReader::new().parse(&input(bytes)).unwrap();
        assert_eq!(parsed.parts.len(), 1);
        assert_eq!(parsed.parts[0].name, "lines");
        let literals: Vec<&[u8]> = parsed.parts[0]
            .cells
            .iter()
            .map(|placed| placed.value_span.unwrap().of(bytes).unwrap())
            .collect();
        assert_eq!(literals, [&b"\"a\\u00e9\\n\""[..], b"1.50", b"\"b\"", b"2"]);
        assert_eq!(
            parsed.parts[0].cells[0].cell.raw,
            RawCell::Text("a\u{e9}\n".to_owned())
        );
        // Each literal reads back, alone, as the value of its cell.
        for placed in &parsed.parts[0].cells {
            let literal = placed.value_span.unwrap().of(bytes).unwrap();
            let node = parse_document(literal, &LIMITS).unwrap();
            assert_eq!(scalar(&node), placed.cell.raw);
        }
    }

    #[test]
    fn what_json_does_not_allow_is_refused_by_name() {
        let code = |bytes: &[u8]| {
            JsonReader::new()
                .parse(&input(bytes))
                .unwrap_err()
                .code()
                .clone()
        };
        assert_eq!(code(br#"[{"a": 1, "a": 2}]"#), SOURCE_MALFORMED);
        assert_eq!(code(br#"[{"a": 1}"#), SOURCE_MALFORMED);
        assert_eq!(code(br#"[{"a": 01}]"#), SOURCE_MALFORMED);
        assert_eq!(code(br#"[{"a": 1.}]"#), SOURCE_MALFORMED);
        assert_eq!(code(br#"[{"a": NaN}]"#), SOURCE_MALFORMED);
        assert_eq!(code(br#"[{"a": 1}] x"#), SOURCE_MALFORMED);
        assert_eq!(code(br#"[1, 2]"#), SOURCE_MALFORMED);
        assert_eq!(code(b"a,b\n1,2\n"), FILE_TYPE_MISMATCH);
        assert_eq!(code(b"42"), FILE_TYPE_MISMATCH);
        assert_eq!(code(b"PK\x03\x04"), FILE_TYPE_MISMATCH);
        let deep = "[".repeat(100);
        assert_eq!(code(deep.as_bytes()), SOURCE_MALFORMED);
        let limited = SourceInput {
            limits: Limits { rows: 1, ..LIMITS },
            ..input(br#"[{"a": 1}, {"a": 2}]"#)
        };
        assert_eq!(
            JsonReader::new().parse(&limited).unwrap_err().code(),
            &SOURCE_TOO_LARGE
        );
    }
}
