//! The reader of delimited text, as RFC 4180 describes it, and strict: what
//! the format does not allow is refused, never repaired.
//!
//! It is this crate's own reader because the parsed form must give the bytes
//! of every field, quotes included, and because a quote that is never closed
//! or text after a closing quote has to stop the read.

use a2m_core::Failure;

use crate::error::{encoding, malformed, mismatch, too_large};
use crate::fingerprint::{SheetStructure, Structure};
use crate::model::{
    ByteSpan, Cell, CellRef, ContentKind, Limits, Parsed, ParsedPart, PartBytes, PlacedCell,
    RawCell, ReaderName, SourceInput, SourceReader,
};

/// The byte-order mark of UTF-8.
const UTF8_MARK: &[u8] = &[0xEF, 0xBB, 0xBF];

/// Checks that bytes are UTF-8 text a reader of text can take, and gives
/// where the text starts: after a byte-order mark, if there is one.
pub(crate) fn text_start(bytes: &[u8]) -> Result<usize, Failure> {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return Err(encoding(
            "it starts with the byte-order mark of UTF-16, and only UTF-8 is read",
        ));
    }
    if looks_like_package(bytes) {
        return Err(mismatch("the content is a package, not text"));
    }
    if bytes.contains(&0) {
        return Err(mismatch("the content holds a zero byte, so it is not text"));
    }
    if let Err(error) = std::str::from_utf8(bytes) {
        return Err(encoding(format!(
            "byte {} is not UTF-8, and only UTF-8 is read",
            error.valid_up_to() + 1
        )));
    }
    Ok(if bytes.starts_with(UTF8_MARK) {
        UTF8_MARK.len()
    } else {
        0
    })
}

/// Whether the bytes start as a package or as the container an encrypted
/// workbook is kept in.
pub(crate) fn looks_like_package(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"PK\x05\x06")
        || bytes.starts_with(crate::xlsx::COMPOUND_FILE)
}

/// The built-in reader of delimited text.
#[derive(Debug, Clone)]
pub struct CsvReader {
    name: ReaderName,
}

impl CsvReader {
    /// The reader, named `csv`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            name: ReaderName::builtin(ReaderName::CSV),
        }
    }
}

impl Default for CsvReader {
    fn default() -> Self {
        Self::new()
    }
}

fn delimiter(input: &SourceInput<'_>) -> Result<u8, Failure> {
    let delimiter = input.options.delimiter.unwrap_or(',');
    match u8::try_from(delimiter) {
        Ok(byte) if byte.is_ascii() && !matches!(byte, b'"' | b'\r' | b'\n') => Ok(byte),
        _ => Err(crate::error::definition(
            "a delimiter is one ASCII character that is not a quote or a line break",
        )),
    }
}

impl SourceReader for CsvReader {
    fn name(&self) -> &ReaderName {
        &self.name
    }

    fn check(&self, input: &SourceInput<'_>) -> Result<(), Failure> {
        let start = text_start(input.bytes)?;
        // A document that is a whole JSON list or record is JSON, whatever it
        // was called.
        let text = input.bytes.get(start..).unwrap_or(&[]);
        let first = text.iter().find(|byte| !byte.is_ascii_whitespace());
        if matches!(first, Some(b'{' | b'['))
            && crate::json::parse_document(text, &input.limits).is_ok()
        {
            return Err(mismatch(
                "the content is a JSON document, not delimited text",
            ));
        }
        Ok(())
    }

    fn parse(&self, input: &SourceInput<'_>) -> Result<Parsed, Failure> {
        self.check(input)?;
        let delimiter = delimiter(input)?;
        let start = text_start(input.bytes)?;
        let part = records(input.bytes, start, delimiter, &input.limits)?;
        let headers = part
            .row(1)
            .iter()
            .map(|placed| match &placed.cell.raw {
                RawCell::Text(text) => text.clone(),
                _ => String::new(),
            })
            .collect();
        Ok(Parsed {
            kind: ContentKind::Csv,
            structure: Structure {
                sheets: vec![SheetStructure {
                    headers,
                    ..SheetStructure::default()
                }],
                ..Structure::default()
            },
            parts: vec![part],
        })
    }
}

/// Reads every record. A field is kept with the span of its bytes, quotes
/// included; an empty field is kept too, so that its place can be patched.
fn records(
    bytes: &[u8],
    start: usize,
    delimiter: u8,
    limits: &Limits,
) -> Result<ParsedPart, Failure> {
    let mut cells = Vec::new();
    let mut widths = Vec::new();
    let mut at = start;
    let mut row = 1_u32;
    let mut column = 1_u32;
    let end = bytes.len();
    // An input with no bytes after its mark has no records at all.
    while at < end {
        let field_start = at;
        let value = if bytes.get(at) == Some(&b'"') {
            let mut value = Vec::new();
            at += 1;
            loop {
                match bytes.get(at) {
                    None => {
                        return Err(malformed(format!(
                            "a quote opened in record {row}, field {column} is never closed"
                        )));
                    }
                    Some(b'"') if bytes.get(at + 1) == Some(&b'"') => {
                        value.push(b'"');
                        at += 2;
                    }
                    Some(b'"') => {
                        at += 1;
                        break;
                    }
                    Some(&byte) => {
                        value.push(byte);
                        at += 1;
                    }
                }
            }
            match bytes.get(at) {
                None | Some(b'\r' | b'\n') => {}
                Some(&byte) if byte == delimiter => {}
                Some(_) => {
                    return Err(malformed(format!(
                        "record {row}, field {column} has text after its closing quote"
                    )));
                }
            }
            value
        } else {
            while let Some(&byte) = bytes.get(at) {
                if byte == delimiter || byte == b'\r' || byte == b'\n' {
                    break;
                }
                if byte == b'"' {
                    return Err(malformed(format!(
                        "record {row}, field {column} has a quote inside a field that is not quoted"
                    )));
                }
                at += 1;
            }
            bytes.get(field_start..at).unwrap_or(&[]).to_vec()
        };
        if value.len() > limits.cell_bytes as usize {
            return Err(too_large(format!(
                "record {row}, field {column} is longer than {} bytes",
                limits.cell_bytes
            )));
        }
        if cells.len() >= limits.cells as usize {
            return Err(too_large(format!("more than {} fields", limits.cells)));
        }
        // The whole input is UTF-8 and a field is cut only at ASCII bytes, so
        // a field is UTF-8 too.
        let text = String::from_utf8(value)
            .map_err(|_| encoding("a field is not UTF-8, and only UTF-8 is read"))?;
        cells.push(PlacedCell {
            cell: Cell {
                raw: if text.is_empty() {
                    RawCell::Empty
                } else {
                    RawCell::Text(text)
                },
                at: CellRef::new(0, row, column),
                format: None,
                hidden: false,
                merged: None,
            },
            value_span: Some(ByteSpan::new(field_start, at)),
            shared: None,
        });
        // What follows a field: a delimiter, a line end, or the end.
        match bytes.get(at) {
            Some(&byte) if byte == delimiter => {
                at += 1;
                column += 1;
                if at == end {
                    // A delimiter at the very end is followed by one empty field.
                    cells.push(PlacedCell {
                        cell: Cell {
                            raw: RawCell::Empty,
                            at: CellRef::new(0, row, column),
                            format: None,
                            hidden: false,
                            merged: None,
                        },
                        value_span: Some(ByteSpan::new(at, at)),
                        shared: None,
                    });
                    widths.push(column);
                }
                continue;
            }
            Some(b'\r') => {
                if bytes.get(at + 1) != Some(&b'\n') {
                    return Err(malformed(format!(
                        "record {row} ends with a carriage return that no line feed follows"
                    )));
                }
                at += 2;
            }
            Some(b'\n') => at += 1,
            _ => {}
        }
        widths.push(column);
        if at < end {
            if row >= limits.rows {
                return Err(too_large(format!("more than {} records", limits.rows)));
            }
            row += 1;
            column = 1;
        }
    }
    let rows = u32::try_from(widths.len()).unwrap_or(u32::MAX);
    if rows > limits.rows {
        return Err(too_large(format!("more than {} records", limits.rows)));
    }
    Ok(ParsedPart {
        name: String::new(),
        path: String::new(),
        bytes: PartBytes::Input,
        cells,
        rows,
        widths,
    })
}

/// The text a field's bytes stand for: the bytes themselves, or, for a
/// quoted field, what lies between the quotes with each doubled quote made
/// one. Nothing when the bytes are not one well-formed field.
#[must_use]
pub fn field_text(literal: &[u8]) -> Option<String> {
    let inner = match literal {
        [b'"', inner @ .., b'"'] => {
            let mut text = Vec::with_capacity(inner.len());
            let mut bytes = inner.iter();
            while let Some(&byte) = bytes.next() {
                if byte == b'"' && bytes.next() != Some(&b'"') {
                    return None;
                }
                text.push(byte);
            }
            text
        }
        plain if plain.contains(&b'"') => return None,
        plain => plain.to_vec(),
    };
    String::from_utf8(inner).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{
        FILE_TYPE_MISMATCH, SOURCE_ENCODING_UNSUPPORTED, SOURCE_MALFORMED, SOURCE_TOO_LARGE,
    };
    use crate::testing::LIMITS;
    use a2m_files::ReaderOptions;

    fn input(bytes: &[u8]) -> SourceInput<'_> {
        SourceInput {
            bytes,
            limits: LIMITS,
            options: ReaderOptions::default(),
        }
    }

    fn texts(parsed: &Parsed) -> Vec<Vec<String>> {
        let part = &parsed.parts[0];
        (1..=part.rows)
            .map(|row| {
                part.row(row)
                    .iter()
                    .map(|placed| match &placed.cell.raw {
                        RawCell::Text(text) => text.clone(),
                        RawCell::Empty => String::new(),
                        other => panic!("delimited text holds only text, found {other:?}"),
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn the_awkward_cases_of_delimited_text_read_exactly() {
        // A byte-order mark, CRLF, a delimiter and a line break and a doubled
        // quote inside quotes, an empty field in quotes, a padded number, and
        // a last line with no ending.
        let bytes =
            b"\xEF\xBB\xBFcode,note,qty\r\n\"EX-1,A\",\"two\nlines \"\"x\"\"\",0040\r\nEX-2,\"\",7";
        let parsed = CsvReader::new().parse(&input(bytes)).unwrap();
        assert_eq!(
            texts(&parsed),
            vec![
                vec!["code", "note", "qty"],
                vec!["EX-1,A", "two\nlines \"x\"", "0040"],
                vec!["EX-2", "", "7"],
            ]
        );
        assert_eq!(parsed.structure.sheets[0].headers, ["code", "note", "qty"]);
        assert_eq!(parsed.parts[0].widths, [3, 3, 3]);

        // LF, the last line ended, a delimiter the definition declares.
        let mut semicolons = input(b"a;b\n1;2\n");
        semicolons.options.delimiter = Some(';');
        let parsed = CsvReader::new().parse(&semicolons).unwrap();
        assert_eq!(texts(&parsed), vec![vec!["a", "b"], vec!["1", "2"]]);
        // The same bytes read with a comma are one column.
        let parsed = CsvReader::new().parse(&input(b"a;b\n1;2\n")).unwrap();
        assert_eq!(texts(&parsed), vec![vec!["a;b"], vec!["1;2"]]);
    }

    #[test]
    fn every_span_covers_exactly_the_literal_of_its_value() {
        let bytes =
            b"\xEF\xBB\xBFcode,note,qty\r\n\"EX-1,A\",\"say \"\"x\"\"\",0040\r\nEX-2,\"\",\r\n";
        let parsed = CsvReader::new().parse(&input(bytes)).unwrap();
        let mut rebuilt = bytes.to_vec();
        let mut last_end = 0;
        for placed in &parsed.parts[0].cells {
            let span = placed.value_span.unwrap();
            let literal = span.of(bytes).unwrap();
            let text = field_text(literal).unwrap();
            match &placed.cell.raw {
                RawCell::Text(value) => assert_eq!(&text, value),
                RawCell::Empty => assert_eq!(text, ""),
                other => panic!("unexpected {other:?}"),
            }
            assert!(span.start >= last_end, "spans are in order and apart");
            last_end = span.end;
            // Writing each span's own bytes back changes nothing.
            rebuilt.splice(
                span.start as usize..span.end as usize,
                literal.iter().copied(),
            );
        }
        assert_eq!(rebuilt, bytes);
    }

    #[test]
    fn what_the_format_does_not_allow_is_refused_by_name() {
        let code = |bytes: &[u8]| {
            CsvReader::new()
                .parse(&input(bytes))
                .unwrap_err()
                .code()
                .clone()
        };
        assert_eq!(code(b"a,b\n\"open,2\n"), SOURCE_MALFORMED);
        assert_eq!(code(b"a,b\n\"x\"y,2\n"), SOURCE_MALFORMED);
        assert_eq!(code(b"a,b\nx\"y,2\n"), SOURCE_MALFORMED);
        assert_eq!(code(b"a,b\r1,2\r"), SOURCE_MALFORMED);
        assert_eq!(code(b"a,b\n\xE9,2\n"), SOURCE_ENCODING_UNSUPPORTED);
        assert_eq!(code(b"\xFF\xFEa\x00,\x00"), SOURCE_ENCODING_UNSUPPORTED);
        assert_eq!(code(b"PK\x03\x04rest"), FILE_TYPE_MISMATCH);
        assert_eq!(code(b"[{\"a\": 1}]"), FILE_TYPE_MISMATCH);
    }

    #[test]
    fn limits_stop_an_input_that_is_too_large() {
        let read = |bytes: &[u8], limits: Limits| {
            CsvReader::new()
                .parse(&SourceInput {
                    bytes,
                    limits,
                    options: ReaderOptions::default(),
                })
                .map(|parsed| parsed.parts[0].rows)
                .map_err(|failure| failure.code().clone())
        };
        let bytes = b"a,b\n1,2\n3,4\n";
        assert_eq!(read(bytes, LIMITS), Ok(3));
        assert_eq!(read(bytes, Limits { rows: 3, ..LIMITS }), Ok(3));
        assert_eq!(
            read(bytes, Limits { rows: 2, ..LIMITS }),
            Err(SOURCE_TOO_LARGE)
        );
        assert_eq!(
            read(bytes, Limits { cells: 5, ..LIMITS }),
            Err(SOURCE_TOO_LARGE)
        );
        assert_eq!(
            read(
                b"a\nlonger than four\n",
                Limits {
                    cell_bytes: 4,
                    ..LIMITS
                }
            ),
            Err(SOURCE_TOO_LARGE)
        );
    }

    #[test]
    fn an_empty_input_has_no_records() {
        let parsed = CsvReader::new().parse(&input(b"")).unwrap();
        assert_eq!(parsed.parts[0].rows, 0);
        assert!(parsed.structure.sheets[0].headers.is_empty());
    }
}
