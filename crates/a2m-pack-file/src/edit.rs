//! The patch editors for delimited text and JSON, and the untouched check.
//!
//! An editor never writes a file again from its parsed form. It replaces
//! the bytes of the places named, adds one row, or removes one, and leaves
//! every other byte where it was. The untouched check then parses the new
//! bytes and compares every place outside the named ones with what it was.

use a2m_core::{DecimalMark, FieldFormat, FieldTypePlan, Rounding, Value};
use a2m_provider::StepFailure;
use a2m_sources::json::{JsonNode, JsonValue};
use a2m_sources::{ByteSpan, ParsedPart};

use crate::codes::unsupported;
use crate::model::DocumentSpec;
use crate::open::{Content, ListAt, OpenFile, Table};

/// One run of bytes replaced by others. An empty run is an insertion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Patch {
    pub start: usize,
    pub end: usize,
    pub with: Vec<u8>,
}

impl Patch {
    pub(crate) fn over(span: ByteSpan, with: Vec<u8>) -> Self {
        Self {
            start: usize::try_from(span.start).unwrap_or(usize::MAX),
            end: usize::try_from(span.end).unwrap_or(usize::MAX),
            with,
        }
    }
}

/// The bytes with every patch applied. Nothing when two patches overlap or
/// one lies outside the bytes.
pub(crate) fn apply(bytes: &[u8], mut patches: Vec<Patch>) -> Option<Vec<u8>> {
    patches.sort_by_key(|patch| (patch.start, patch.end));
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    for patch in &patches {
        if patch.start < at || patch.end < patch.start {
            return None;
        }
        out.extend_from_slice(bytes.get(at..patch.start)?);
        out.extend_from_slice(&patch.with);
        at = patch.end;
    }
    out.extend_from_slice(bytes.get(at..)?);
    Some(out)
}

/// The text a value is written as, by its field type. Nothing for no value.
/// A date, a time, a list, and a record have no spelling here yet.
pub(crate) fn spell(
    value: &Value,
    field_type: &FieldTypePlan,
) -> Result<Option<String>, StepFailure> {
    let comma = matches!(
        field_type.format,
        FieldFormat::Number {
            decimal_mark: DecimalMark::Comma
        }
    );
    Ok(Some(match value {
        Value::Null => return Ok(None),
        Value::Text(text) => text.clone(),
        Value::Int(number) => number.to_string(),
        Value::Decimal(number) => {
            let places = field_type.places.or_else(|| {
                field_type
                    .currency
                    .as_deref()
                    .map(a2m_expr::currency_places)
            });
            let text = match places {
                Some(places) => {
                    number.to_places(places, field_type.rounding.unwrap_or(Rounding::HalfEven))
                }
                None => number.to_string(),
            };
            if comma { text.replace('.', ",") } else { text }
        }
        Value::Bool(flag) => match (&field_type.format, flag) {
            (FieldFormat::Boolean { true_text, .. }, true) => true_text.clone(),
            (FieldFormat::Boolean { false_text, .. }, false) => false_text.clone(),
            (_, flag) => flag.to_string(),
        },
        _ => {
            return Err(unsupported(
                "writing a date, a time, a list, or a record into a file",
            ));
        }
    }))
}

/// One field of delimited text. It is quoted when it was quoted before, or
/// when its text would otherwise end the field or the row.
pub(crate) fn csv_field(text: &str, quoted_before: bool, delimiter: u8) -> Vec<u8> {
    let needs = text
        .bytes()
        .any(|byte| byte == delimiter || matches!(byte, b'"' | b'\r' | b'\n'));
    if !(needs || quoted_before) {
        return text.as_bytes().to_vec();
    }
    let mut out = vec![b'"'];
    for byte in text.bytes() {
        if byte == b'"' {
            out.push(b'"');
        }
        out.push(byte);
    }
    out.push(b'"');
    out
}

fn json_string(text: &str) -> Vec<u8> {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", other as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out.into_bytes()
}

/// The JSON token a value is written as. `existing` is the token it
/// replaces, when it replaces one: a value kept as a string stays a string.
pub(crate) fn json_literal(
    value: &Value,
    field_type: &FieldTypePlan,
    existing: Option<&[u8]>,
) -> Result<Vec<u8>, StepFailure> {
    let Some(text) = spell(value, field_type)? else {
        return Ok(b"null".to_vec());
    };
    let was_string = existing.is_some_and(|token| token.first() == Some(&b'"'));
    Ok(match value {
        Value::Text(_) => json_string(&text),
        _ if was_string => json_string(&text),
        Value::Bool(flag) => flag.to_string().into_bytes(),
        Value::Int(_) | Value::Decimal(_) => {
            // A decimal comma has no place in a JSON number.
            if text.contains(',') {
                return Err(unsupported(
                    "writing a number with a decimal comma as a JSON number",
                ));
            }
            text.into_bytes()
        }
        _ => json_string(&text),
    })
}

fn at(position: u64) -> usize {
    usize::try_from(position).unwrap_or(usize::MAX)
}

/// Where a row of delimited text starts and where its last field ends.
fn line_of(part: &ParsedPart, row: u32) -> Option<(usize, usize)> {
    let cells = part.row(row);
    let start = cells.first()?.value_span?.start;
    let end = cells.last()?.value_span?.end;
    Some((at(start), at(end)))
}

/// Removes one row of delimited text, with its line ending.
pub(crate) fn csv_remove_row(bytes: &[u8], part: &ParsedPart, row: u32) -> Option<Patch> {
    let (start, _) = line_of(part, row)?;
    let next = if row < part.rows {
        line_of(part, row + 1)?.0
    } else {
        bytes.len()
    };
    let last_unended = row == part.rows && !bytes.ends_with(b"\n");
    let from = if last_unended && row > 1 {
        // The last line had no ending: the line before it must not gain one.
        line_of(part, row - 1)?.1
    } else {
        start
    };
    Some(Patch {
        start: from,
        end: next,
        with: Vec::new(),
    })
}

/// Adds one row at the end of delimited text. `values` gives the text of
/// each column that has one, by column from one.
pub(crate) fn csv_add_row(
    bytes: &[u8],
    part: &ParsedPart,
    document: &DocumentSpec,
    table: &Table,
    values: &[(u32, String)],
    delimiter: u8,
) -> Result<Patch, StepFailure> {
    let shown = if part.rows >= 2 {
        line_of(part, 1)
            .zip(line_of(part, 2))
            .and_then(|((_, end), (next, _))| bytes.get(end..next))
            .map(<[u8]>::to_vec)
    } else if bytes.ends_with(b"\r\n") {
        Some(b"\r\n".to_vec())
    } else if bytes.ends_with(b"\n") {
        Some(b"\n".to_vec())
    } else {
        None
    };
    let ending = match (shown, document.line_ending.as_deref()) {
        (Some(ending), _) if ending == b"\n" || ending == b"\r\n" => ending,
        (None, Some("lf")) => b"\n".to_vec(),
        (None, Some("crlf")) => b"\r\n".to_vec(),
        _ => {
            return Err(unsupported(
                "adding a row to a file that shows no line ending, when its dialect states none",
            ));
        }
    };
    let mut row = Vec::new();
    for column in 1..=table.width {
        if column > 1 {
            row.push(delimiter);
        }
        if let Some((_, text)) = values.iter().find(|(known, _)| *known == column) {
            row.extend(csv_field(text, false, delimiter));
        }
    }
    let with = if bytes.ends_with(b"\n") {
        [row, ending].concat()
    } else {
        [ending, row].concat()
    };
    Ok(Patch {
        start: bytes.len(),
        end: bytes.len(),
        with,
    })
}

/// Removes one element of a JSON list, with the comma that set it apart.
pub(crate) fn json_remove(list: &ListAt, index: usize) -> Option<Patch> {
    let element = list.elements.get(index)?;
    let (start, end) = if index > 0 {
        (list.elements.get(index - 1)?.end, element.end)
    } else if let Some(next) = list.elements.get(1) {
        (element.start, next.start)
    } else {
        (element.start, element.end)
    };
    Some(Patch {
        start: at(start),
        end: at(end),
        with: Vec::new(),
    })
}

/// Adds one record at the end of a JSON list. The new record is written on
/// one line, set apart as the last element is.
pub(crate) fn json_add(
    bytes: &[u8],
    list: &ListAt,
    members: &[(String, Vec<u8>)],
) -> Option<Patch> {
    let mut record = vec![b'{'];
    for (position, (name, literal)) in members.iter().enumerate() {
        if position > 0 {
            record.extend_from_slice(b", ");
        }
        record.extend(json_string(name));
        record.extend_from_slice(b": ");
        record.extend_from_slice(literal);
    }
    record.push(b'}');
    let Some(last) = list.elements.last() else {
        let inside = at(list.span.start) + 1;
        return Some(Patch {
            start: inside,
            end: inside,
            with: record,
        });
    };
    // What stands between the element before and the last one, after its
    // comma, is what sets the new element apart.
    let before = match list.elements.len().checked_sub(2) {
        Some(previous) => at(list.elements.get(previous)?.end),
        None => at(list.span.start),
    };
    let gap = bytes.get(before + 1..at(last.start))?;
    let mut with = vec![b','];
    with.extend_from_slice(gap);
    with.extend(record);
    Some(Patch {
        start: at(last.end),
        end: at(last.end),
        with,
    })
}

/// What a patch was meant to change, for the untouched check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Meant {
    /// These cells of delimited text, by row and column.
    Cells(Vec<(u32, u32)>),
    /// One row of delimited text, removed.
    RowRemoved(u32),
    /// One row of delimited text, added at the end.
    RowAdded,
    /// These JSON values, by their spans in the old file.
    Nodes(Vec<ByteSpan>),
    /// One element of the JSON list at this span in the old file, removed.
    ElementRemoved { list: ByteSpan, index: usize },
    /// One element added at the end of the JSON list at this span.
    ElementAdded { list: ByteSpan },
}

type Literals<'a> = Vec<Vec<&'a [u8]>>;

/// Every field of delimited text as its bytes, by row, and every run of
/// bytes that lies between two fields.
fn literals<'a>(bytes: &'a [u8], part: &ParsedPart) -> Option<(Literals<'a>, Vec<&'a [u8]>)> {
    let mut rows: Literals<'a> = Vec::new();
    let mut gaps = Vec::new();
    let mut last = 0;
    for placed in &part.cells {
        let span = placed.value_span?;
        let row = placed.cell.at.row as usize;
        if rows.len() < row {
            rows.resize(row, Vec::new());
        }
        gaps.push(bytes.get(last..at(span.start))?);
        rows.get_mut(row - 1)?.push(span.of(bytes)?);
        last = at(span.end);
    }
    gaps.push(bytes.get(last..)?);
    Some((rows, gaps))
}

fn same_cells(old: &OpenFile, new: &OpenFile, meant: &Meant) -> Option<bool> {
    let (Content::Cells(old_parsed), Content::Cells(new_parsed)) = (&old.content, &new.content)
    else {
        return None;
    };
    let (old_rows, old_gaps) = literals(&old.bytes, old_parsed.part(0)?)?;
    let (new_rows, new_gaps) = literals(&new.bytes, new_parsed.part(0)?)?;
    Some(match meant {
        Meant::Cells(cells) => {
            old_gaps == new_gaps
                && old_rows.len() == new_rows.len()
                && old_rows
                    .iter()
                    .zip(&new_rows)
                    .zip(1_u32..)
                    .all(|((old, new), row)| {
                        old.len() == new.len()
                            && old
                                .iter()
                                .zip(new)
                                .zip(1_u32..)
                                .all(|((old, new), column)| {
                                    old == new || cells.contains(&(row, column))
                                })
                    })
        }
        Meant::RowRemoved(row) => {
            let mut kept = old_rows;
            if (*row as usize) > kept.len() || *row == 0 {
                return Some(false);
            }
            kept.remove(*row as usize - 1);
            kept == new_rows
        }
        Meant::RowAdded => {
            new_rows.len() == old_rows.len() + 1 && new_rows.get(..old_rows.len())? == old_rows
        }
        _ => return None,
    })
}

/// The text of a JSON document, after its byte-order mark.
fn json_text(file: &OpenFile) -> Option<(&JsonNode, &[u8], u64)> {
    match &file.content {
        Content::Json { root, offset } => Some((root, file.bytes.get(at(*offset)..)?, *offset)),
        Content::Cells(_) => None,
    }
}

struct Walk<'a> {
    old_text: &'a [u8],
    new_text: &'a [u8],
    offset: u64,
    meant: &'a Meant,
}

impl Walk<'_> {
    fn is(&self, node: &JsonNode, span: &ByteSpan) -> bool {
        node.span.start + self.offset == span.start && node.span.end + self.offset == span.end
    }

    fn all(&self, old: &[JsonNode], new: &[JsonNode]) -> bool {
        old.len() == new.len() && old.iter().zip(new).all(|(old, new)| self.same(old, new))
    }

    /// Whether two values are the same, token for token, outside what the
    /// patch was meant to change.
    fn same(&self, old: &JsonNode, new: &JsonNode) -> bool {
        match (&old.value, &new.value) {
            (JsonValue::Record(old_members), JsonValue::Record(new_members)) => {
                old_members.len() == new_members.len()
                    && old_members.iter().zip(new_members).all(
                        |((old_key, old_value), (new_key, new_value))| {
                            old_key == new_key && self.same(old_value, new_value)
                        },
                    )
            }
            (JsonValue::List(old_items), JsonValue::List(new_items)) => match self.meant {
                Meant::ElementRemoved { list, index } if self.is(old, list) => {
                    let mut kept: Vec<JsonNode> = old_items.clone();
                    if *index >= kept.len() {
                        return false;
                    }
                    kept.remove(*index);
                    self.all(&kept, new_items)
                }
                Meant::ElementAdded { list } if self.is(old, list) => {
                    new_items.len() == old_items.len() + 1
                        && self.all(old_items, &new_items[..old_items.len()])
                }
                _ => self.all(old_items, new_items),
            },
            (JsonValue::Record(_) | JsonValue::List(_), _)
            | (_, JsonValue::Record(_) | JsonValue::List(_)) => false,
            _ => {
                let named = matches!(self.meant, Meant::Nodes(spans) if spans.iter().any(|span| self.is(old, span)));
                named || old.span.of(self.old_text) == new.span.of(self.new_text)
            }
        }
    }
}

/// The untouched check: whether the new file, parsed again, holds at every
/// place outside the named ones exactly what the old file held.
pub(crate) fn untouched(old: &OpenFile, new: &OpenFile, meant: &Meant) -> bool {
    if let Some(same) = same_cells(old, new, meant) {
        return same;
    }
    let (Some((old_root, old_text, offset)), Some((new_root, new_text, new_offset))) =
        (json_text(old), json_text(new))
    else {
        return false;
    };
    // The byte-order mark, when there is one, stays as it was.
    offset == new_offset
        && old.bytes.get(..at(offset)) == new.bytes.get(..at(offset))
        && Walk {
            old_text,
            new_text,
            offset,
            meant,
        }
        .same(old_root, new_root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_keeps_its_quotes_and_gains_them_only_when_it_must() {
        assert_eq!(csv_field("38", false, b','), b"38");
        assert_eq!(csv_field("38", true, b','), b"\"38\"");
        assert_eq!(csv_field("a,b", false, b','), b"\"a,b\"");
        assert_eq!(csv_field("a;b", false, b','), b"a;b");
        assert_eq!(csv_field("say \"hi\"", false, b','), b"\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("two\nlines", false, b';'), b"\"two\nlines\"");
    }

    #[test]
    fn patches_replace_only_their_own_bytes() {
        let bytes = b"abcdef";
        let patched = apply(
            bytes,
            vec![
                Patch {
                    start: 4,
                    end: 5,
                    with: b"XY".to_vec(),
                },
                Patch {
                    start: 1,
                    end: 2,
                    with: Vec::new(),
                },
                Patch {
                    start: 6,
                    end: 6,
                    with: b"!".to_vec(),
                },
            ],
        );
        assert_eq!(patched.as_deref(), Some(&b"acdXYf!"[..]));
        // Two patches over the same bytes are refused, never merged.
        let overlapping = vec![
            Patch {
                start: 1,
                end: 3,
                with: Vec::new(),
            },
            Patch {
                start: 2,
                end: 4,
                with: Vec::new(),
            },
        ];
        assert_eq!(apply(bytes, overlapping), None);
        assert_eq!(
            apply(
                bytes,
                vec![Patch {
                    start: 5,
                    end: 9,
                    with: Vec::new()
                }]
            ),
            None
        );
    }

    #[test]
    fn a_json_string_escapes_what_it_must_and_nothing_else() {
        assert_eq!(json_string("13:00"), b"\"13:00\"");
        assert_eq!(json_string("a\"b\\c\n"), b"\"a\\\"b\\\\c\\n\"");
        assert_eq!(json_string("caf\u{e9}"), "\"caf\u{e9}\"".as_bytes());
        assert_eq!(json_string("\u{1}"), b"\"\\u0001\"");
    }
}
