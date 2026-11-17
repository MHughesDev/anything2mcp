//! What a reader takes and what it gives: the input as bytes, the parsed
//! form with the position of every value, and the `SourceReader` contract.

use std::fmt;

use a2m_core::Failure;
use a2m_expr::DateSystem;
use serde::{Deserialize, Serialize};

use crate::error::definition;
use crate::fingerprint::Structure;

/// The name of a reader: `csv`, `xlsx`, `json`, or a plug-in's. Lowercase
/// letters, digits, and underscores, starting with a letter, at most 32.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ReaderName(String);

impl ReaderName {
    /// The built-in reader of delimited text.
    pub const CSV: &'static str = "csv";
    /// The built-in reader of workbooks.
    pub const XLSX: &'static str = "xlsx";
    /// The built-in reader of JSON documents.
    pub const JSON: &'static str = "json";

    /// Checks a reader's name.
    ///
    /// # Errors
    ///
    /// `source_definition_invalid` when the name has another shape.
    pub fn new(name: &str) -> Result<Self, Failure> {
        let mut bytes = name.bytes();
        let shaped = name.len() <= 32
            && bytes.next().is_some_and(|b| b.is_ascii_lowercase())
            && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if shaped {
            Ok(Self(name.to_owned()))
        } else {
            Err(definition(
                "a reader's name is lowercase letters, digits, and underscores",
            ))
        }
    }

    /// The name of a built-in reader, which is known to be well formed.
    pub(crate) fn builtin(name: &'static str) -> Self {
        Self(name.to_owned())
    }

    /// The name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ReaderName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a reader may use up. Every number comes from the caller; a reader
/// that would pass one stops with `source_too_large`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most bytes an input may have.
    pub bytes: u64,
    /// The most bytes all the unpacked parts of a package may have together.
    pub unpacked_bytes: u64,
    /// The most rows one part may have.
    pub rows: u32,
    /// The most cells the whole input may have.
    pub cells: u32,
    /// The most bytes one cell's value may have.
    pub cell_bytes: u32,
}

/// What a source definition tells its reader about the format's dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderOptions {
    /// For delimited text: the character between fields. A comma when absent.
    /// One ASCII character that is not a quote or a line break.
    pub delimiter: Option<char>,
}

/// One input: bytes the caller has already read, once, and stored. There is
/// no path here and no way to give one, so no input can choose which file is
/// read.
#[derive(Debug, Clone, Copy)]
pub struct SourceInput<'a> {
    /// The content.
    pub bytes: &'a [u8],
    /// What reading it may use up.
    pub limits: Limits,
    /// The dialect the source definition declares.
    pub options: ReaderOptions,
}

/// The format of an input, as its content states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    /// A workbook: a package of parts.
    Xlsx,
    /// Delimited text.
    Csv,
    /// A JSON document.
    Json,
}

/// The extension point for a format: bytes in, the parsed form out.
///
/// A reader is a pure function of its input. It never opens a path, never
/// reaches a network, and reads the bytes once. It does not type a value and
/// does not apply a mapping: [`crate::Sources`] does both for every reader,
/// so a reader, built in or a plug-in's, cannot pass validation by.
pub trait SourceReader: Send + Sync {
    /// The reader's name, as a source definition writes it.
    fn name(&self) -> &ReaderName;

    /// Refuses content that is not the format this reader reads, whatever
    /// the input was called.
    ///
    /// # Errors
    ///
    /// `file_type_mismatch`; `source_protected` for an encrypted workbook;
    /// `source_encoding_unsupported` for text that is not UTF-8.
    fn check(&self, input: &SourceInput<'_>) -> Result<(), Failure>;

    /// The whole input as parsed: its structure, and every cell with where
    /// its value lies in the bytes.
    ///
    /// # Errors
    ///
    /// As for [`SourceReader::check`], and `source_malformed` when the
    /// content breaks its format's rules, `source_too_large` when it passes
    /// a limit.
    fn parse(&self, input: &SourceInput<'_>) -> Result<Parsed, Failure>;
}

/// A cell's place: which part, which row, which column. Rows and columns
/// count from one. In delimited text a row is a record, the header being
/// row 1; in a JSON list a row is an element, and a column is a key in the
/// order the keys first appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CellRef {
    /// The part's position in [`Parsed::parts`].
    pub part: u32,
    /// The row, from one.
    pub row: u32,
    /// The column, from one.
    pub column: u32,
}

impl CellRef {
    /// The cell at `row` and `column` of part `part`.
    #[must_use]
    pub const fn new(part: u32, row: u32, column: u32) -> Self {
        Self { part, row, column }
    }

    /// The cell as a workbook writes it, such as `C7`.
    #[must_use]
    pub fn a1(self) -> String {
        format!("{}{}", column_letters(self.column), self.row)
    }
}

impl fmt::Display for CellRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "part {}, row {}, column {} ({})",
            self.part + 1,
            self.row,
            self.column,
            self.a1()
        )
    }
}

/// A column's letters, as a workbook writes them: 1 is `A`, 27 is `AA`.
pub(crate) fn column_letters(column: u32) -> String {
    let mut letters = Vec::new();
    let mut rest = column;
    while rest > 0 {
        let digit = u8::try_from((rest - 1) % 26).unwrap_or(0);
        letters.push(b'A' + digit);
        rest = (rest - 1) / 26;
    }
    letters.reverse();
    String::from_utf8(letters).unwrap_or_default()
}

/// A rectangle of cells in one part, both corners included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CellRange {
    /// The first row.
    pub top: u32,
    /// The first column.
    pub left: u32,
    /// The last row.
    pub bottom: u32,
    /// The last column.
    pub right: u32,
}

impl CellRange {
    /// Whether the cell at `row` and `column` lies inside.
    #[must_use]
    pub const fn contains(&self, row: u32, column: u32) -> bool {
        row >= self.top && row <= self.bottom && column >= self.left && column <= self.right
    }

    /// Reads a range as a workbook writes it: `A1:B2`, `$A$1:$B$2`, or one
    /// cell, `C3`. Nothing else: a whole column or a whole row is not read.
    #[must_use]
    pub fn from_a1(text: &str) -> Option<Self> {
        let (first, last) = text.split_once(':').unwrap_or((text, text));
        let (top, left) = cell_from_a1(first)?;
        let (bottom, right) = cell_from_a1(last)?;
        (top <= bottom && left <= right).then_some(Self {
            top,
            left,
            bottom,
            right,
        })
    }

    /// The range as a workbook writes it, `A1:B2`, or `A1` for one cell.
    #[must_use]
    pub fn a1(&self) -> String {
        let first = format!("{}{}", column_letters(self.left), self.top);
        if self.top == self.bottom && self.left == self.right {
            first
        } else {
            format!("{first}:{}{}", column_letters(self.right), self.bottom)
        }
    }
}

/// Reads one cell reference, `C7` or `$C$7`, as a row and a column.
pub(crate) fn cell_from_a1(text: &str) -> Option<(u32, u32)> {
    let text = text.strip_prefix('$').unwrap_or(text);
    let letters = text.bytes().take_while(u8::is_ascii_uppercase).count();
    let (column_text, row_text) = text.split_at(letters);
    let row_text = row_text.strip_prefix('$').unwrap_or(row_text);
    if column_text.is_empty()
        || column_text.len() > 3
        || row_text.is_empty()
        || row_text.len() > 7
        || !row_text.bytes().all(|b| b.is_ascii_digit())
        || row_text.starts_with('0')
    {
        return None;
    }
    let column = column_text
        .bytes()
        .fold(0_u32, |sum, b| sum * 26 + u32::from(b - b'A') + 1);
    let row = row_text.parse().ok()?;
    Some((row, column))
}

/// A run of bytes in a part, from `start` up to and not including `end`.
/// For a workbook the part is a part of the package after unpacking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ByteSpan {
    /// The first byte.
    pub start: u64,
    /// One past the last byte.
    pub end: u64,
}

impl ByteSpan {
    pub(crate) const fn new(start: usize, end: usize) -> Self {
        Self {
            start: start as u64,
            end: end as u64,
        }
    }

    /// The bytes of the span in `part`, or nothing when the span does not
    /// lie inside it.
    #[must_use]
    pub fn of<'a>(&self, part: &'a [u8]) -> Option<&'a [u8]> {
        let start = usize::try_from(self.start).ok()?;
        let end = usize::try_from(self.end).ok()?;
        part.get(start..end)
    }
}

/// What a format says one cell holds, before any field type is applied.
/// A number is its text as stored: it has not been through a float and never
/// will be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawCell {
    /// Nothing: an empty cell, an empty field, a JSON `null`.
    Empty,
    /// Text. Delimited text holds nothing else, so `0040` stays `0040`.
    Text(String),
    /// A number, as the digits the format stores.
    Number(String),
    /// True or false.
    Bool(bool),
    /// A number in a cell whose number format is a date or a time: a serial
    /// in the workbook's date system, as the digits the workbook stores.
    DateSerial(String),
    /// A formula, with the result the workbook kept from its last
    /// calculation, if it kept one. The formula is never calculated here.
    Formula {
        /// The kept result. Nothing when the workbook has none.
        cached: Option<Box<RawCell>>,
    },
    /// An error value a workbook shows, such as a division by zero, by its
    /// name in the format.
    Error(String),
    /// A list or a record where one value is expected. Never typed.
    Nested,
}

impl RawCell {
    /// The cell with a formula replaced by its kept result. Nothing when the
    /// cell is a formula that kept none.
    #[must_use]
    pub fn settled(&self) -> Option<&Self> {
        match self {
            Self::Formula { cached } => cached.as_deref(),
            other => Some(other),
        }
    }
}

/// One cell: what it holds, where it is, and what the format says about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    /// What it holds.
    pub raw: RawCell,
    /// Where it is.
    pub at: CellRef,
    /// Its number format, when the format gives it one.
    pub format: Option<String>,
    /// True when its row or its column is hidden.
    pub hidden: bool,
    /// The merged range it lies in, if any. Only the top-left cell of a
    /// merged range holds a value.
    pub merged: Option<CellRange>,
}

/// A cell and where its value's literal lies in its part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacedCell {
    /// The cell.
    pub cell: Cell,
    /// The bytes of the value's literal: a field of delimited text with its
    /// quotes, a JSON token, the content of a workbook cell's value element.
    /// Nothing where the format stores no literal.
    pub value_span: Option<ByteSpan>,
    /// For a workbook cell that holds text through the shared strings: which
    /// entry. The span then covers the entry's number, not the text.
    pub shared: Option<u32>,
}

/// The bytes of a part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartBytes {
    /// The part is the whole input: spans point into the input's bytes.
    Input,
    /// The part was unpacked from a package: spans point into these bytes.
    Unpacked(Vec<u8>),
}

/// One part of an input: a sheet of a workbook, a delimited file, a list of
/// records in a JSON document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPart {
    /// Its name: the sheet's name, the key of a JSON list, or nothing for a
    /// delimited file or a JSON list at the root.
    pub name: String,
    /// Where it is kept in a package, such as `xl/worksheets/sheet1.xml`.
    /// Empty when the part is the whole input.
    pub path: String,
    /// Its bytes.
    pub bytes: PartBytes,
    /// Its cells that hold something, by row and then by column.
    pub cells: Vec<PlacedCell>,
    /// How many rows it has, counting the last row that holds a cell.
    pub rows: u32,
    /// For delimited text and JSON: how many fields each row has, by row.
    /// Empty for a workbook, whose rows have no length.
    pub widths: Vec<u32>,
}

impl ParsedPart {
    /// The cell at `row` and `column`, if the part holds one there.
    #[must_use]
    pub fn cell(&self, row: u32, column: u32) -> Option<&PlacedCell> {
        self.cells
            .binary_search_by(|placed| {
                (placed.cell.at.row, placed.cell.at.column).cmp(&(row, column))
            })
            .ok()
            .and_then(|found| self.cells.get(found))
    }

    /// The cells of one row, by column.
    #[must_use]
    pub fn row(&self, row: u32) -> &[PlacedCell] {
        let start = self
            .cells
            .partition_point(|placed| placed.cell.at.row < row);
        let end = self
            .cells
            .partition_point(|placed| placed.cell.at.row <= row);
        self.cells.get(start..end).unwrap_or(&[])
    }

    /// The part's bytes: its own, or the input's when the part is the input.
    #[must_use]
    pub fn bytes<'a>(&'a self, input: &'a [u8]) -> &'a [u8] {
        match &self.bytes {
            PartBytes::Input => input,
            PartBytes::Unpacked(bytes) => bytes,
        }
    }
}

/// An input as parsed: its structure, and every cell with its position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// The format the content was read as.
    pub kind: ContentKind,
    /// What the input is made of, without its values.
    pub structure: Structure,
    /// Its parts, in the order the input declares them.
    pub parts: Vec<ParsedPart>,
}

impl Parsed {
    /// The date system of a workbook; nothing for a format that has none.
    #[must_use]
    pub const fn date_system(&self) -> Option<DateSystem> {
        self.structure.date_system
    }

    /// The position of the part with that name.
    #[must_use]
    pub fn part_named(&self, name: &str) -> Option<u32> {
        self.parts
            .iter()
            .position(|part| part.name == name)
            .and_then(|found| u32::try_from(found).ok())
    }

    /// The part at that position.
    #[must_use]
    pub fn part(&self, index: u32) -> Option<&ParsedPart> {
        self.parts.get(usize::try_from(index).ok()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_and_cells_read_as_a_workbook_writes_them() {
        assert_eq!(cell_from_a1("C7"), Some((7, 3)));
        assert_eq!(cell_from_a1("$AA$10"), Some((10, 27)));
        assert_eq!(cell_from_a1("7"), None);
        assert_eq!(cell_from_a1("A0"), None);
        let range = CellRange::from_a1("$A$1:$E$4").unwrap();
        assert_eq!(
            (range.top, range.left, range.bottom, range.right),
            (1, 1, 4, 5)
        );
        assert_eq!(range.a1(), "A1:E4");
        assert_eq!(CellRange::from_a1("B2").unwrap().a1(), "B2");
        assert!(CellRange::from_a1("A:A").is_none());
        assert_eq!(CellRef::new(0, 12, 28).a1(), "AB12");
    }

    #[test]
    fn a_reader_name_is_a_checked_name() {
        assert!(ReaderName::new("csv").is_ok());
        assert!(ReaderName::new("my_reader2").is_ok());
        assert!(ReaderName::new("../etc").is_err());
        assert!(ReaderName::new("").is_err());
    }
}
