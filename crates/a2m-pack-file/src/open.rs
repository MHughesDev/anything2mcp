//! Reading a file once, parsing it through `a2m-sources`, and finding the
//! named places of a record kind in the parsed form, each with where its
//! value lies in the bytes.
//!
//! Nothing here builds a path: a file is reached by its own name in the
//! surface's checked folder, through `place`. Nothing here calculates a
//! formula or opens a macro.

use a2m_core::{Digest, Failure, FailureClass, FailureCode, Value};
use a2m_provider::StepFailure;
use a2m_sources::json::{JsonNode, JsonValue, parse_document};
use a2m_sources::{
    ByteSpan, Cell, CellContext, CellRange, CellRef, FieldTyper, Limits, Parsed, ParsedPart,
    RawCell, ReaderOptions, SourceInput, Sources, typed,
};

use crate::codes::{self, stop, unsupported};
use crate::model::{DocumentSpec, FieldSpec, Format, Layout, PlaceSpec, RecordSpec};
use crate::place::Named;
use crate::settings::{self, Quotes, Settings};

/// The byte-order mark of UTF-8.
const UTF8_MARK: &[u8] = &[0xEF, 0xBB, 0xBF];

/// Windows: another program holds the file and shares it with nobody, or
/// holds a lock on part of it.
const SHARING_VIOLATION: i32 = 32;
const LOCK_VIOLATION: i32 = 33;

/// Whether the operating system refused because another program holds the
/// file. Only Windows refuses this way; elsewhere no error has these codes
/// with this meaning, so the check is made there alone.
pub(crate) fn held_by_another(error: &std::io::Error) -> bool {
    cfg!(windows)
        && matches!(
            error.raw_os_error(),
            Some(SHARING_VIOLATION | LOCK_VIOLATION)
        )
}

pub(crate) fn locked() -> Failure {
    codes::failure(
        codes::FILE_LOCKED,
        "Another program holds the file, so it cannot be read or replaced now.",
        "Close the file in the other program, then try again.",
    )
}

pub(crate) fn not_allowed(why: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        a2m_provider::codes::PATH_NOT_ALLOWED,
        format!("The file is refused: {why}."),
        "Point the document at a regular file with one name, inside the surface's folder.",
    )
}

pub(crate) fn unavailable(what: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        a2m_provider::codes::FILE_UNAVAILABLE,
        format!("The file {what}."),
        "Check that the engine's user may read and write the folder.",
    )
}

pub(crate) fn missing() -> StepFailure {
    stop(
        codes::FILE_MISSING,
        "The file is not at the place its document declares.",
        "Put the file back, or correct `file` in the document.",
    )
}

/// The whole file, read once and never changed. Blocking. The file is the
/// regular file under its own name, with one name: see [`Named::open`].
pub(crate) fn read_file(file: &Named, max_bytes: u64) -> Result<Vec<u8>, StepFailure> {
    file.read(max_bytes)?.ok_or_else(|| {
        stop(
            codes::FILE_TOO_LARGE,
            format!("The file has more than {max_bytes} bytes, the most the file pack reads."),
            "Raise `packs.file.max_bytes`, or point the document at a smaller file.",
        )
    })
}

/// A file as parsed.
#[derive(Debug, Clone)]
pub(crate) enum Content {
    /// Delimited text or a workbook: cells in parts.
    Cells(Parsed),
    /// A JSON document. Spans count from `offset`, after a byte-order mark.
    Json { root: JsonNode, offset: u64 },
    /// A settings file: an INI file or a property list. Spans count from
    /// the file's first byte.
    Settings(Settings),
}

/// One file, read and parsed.
#[derive(Debug, Clone)]
pub(crate) struct OpenFile {
    pub bytes: Vec<u8>,
    pub digest: Digest,
    pub content: Content,
}

fn unreadable(failure: &Failure) -> StepFailure {
    let code: &FailureCode = failure.code();
    if code == &a2m_sources::SOURCE_PROTECTED {
        stop(
            codes::FILE_PROTECTED,
            "The file is encrypted or protected by a password, and the file pack reads past no protection.",
            "Point the document at a file that is not protected.",
        )
    } else if code == &a2m_sources::SOURCE_TOO_LARGE {
        stop(
            codes::FILE_TOO_LARGE,
            failure.message().to_owned(),
            "Raise the limits under `packs.file`, or point the document at a smaller file.",
        )
    } else {
        stop(
            codes::FILE_UNREADABLE,
            format!(
                "The file does not parse as the format its document declares. {}",
                failure.message()
            ),
            "Correct the file, or correct `format` and `dialect` in the document.",
        )
    }
}

/// Parses the bytes as the document's format.
pub(crate) fn parse(
    bytes: Vec<u8>,
    document: &DocumentSpec,
    limits: &Limits,
) -> Result<OpenFile, StepFailure> {
    let digest = Digest::of(&bytes);
    let content = if document.format == Format::Ini {
        let separator = document
            .separator
            .as_deref()
            .and_then(|text| text.chars().next())
            .unwrap_or('=');
        let quotes = Quotes::from_name(document.quotes.as_deref());
        Content::Settings(settings::parse_ini(&bytes, separator, quotes, limits)?)
    } else if document.format == Format::PropertyList {
        Content::Settings(settings::parse_plist(&bytes, limits)?)
    } else if document.format == Format::Json {
        let offset = if bytes.starts_with(UTF8_MARK) {
            UTF8_MARK.len()
        } else {
            0
        };
        let text = bytes.get(offset..).unwrap_or(&[]);
        let root = parse_document(text, limits).map_err(|failure| unreadable(&failure))?;
        Content::Json {
            root,
            offset: offset as u64,
        }
    } else {
        let input = SourceInput {
            bytes: &bytes,
            limits: *limits,
            options: ReaderOptions {
                delimiter: document
                    .delimiter
                    .as_deref()
                    .and_then(|text| text.chars().next()),
            },
        };
        let parsed = Sources::builtin()
            .parse(document.format.reader(), &input)
            .map_err(|failure| unreadable(&failure))?;
        Content::Cells(parsed)
    };
    Ok(OpenFile {
        bytes,
        digest,
        content,
    })
}

fn layout_differs(what: &str) -> StepFailure {
    stop(
        codes::FINGERPRINT_MISMATCH,
        format!("The file's {what} are not the ones its document declares under `fingerprint`."),
        "Look at what changed in the file, and correct the document or put the file's layout back.",
    )
}

/// Checks the file's layout against what the document declares.
pub(crate) fn check_layout(open: &OpenFile, layout: &Layout) -> Result<(), StepFailure> {
    let differs = |declared: &Option<Vec<String>>, found: Vec<&str>| {
        declared
            .as_ref()
            .is_some_and(|declared| declared.iter().map(String::as_str).ne(found))
    };
    match &open.content {
        Content::Settings(file) => {
            if differs(
                &layout.keys,
                file.root_keys.iter().map(String::as_str).collect(),
            ) || differs(
                &layout.sections,
                file.sections.iter().map(String::as_str).collect(),
            ) {
                return Err(layout_differs("keys or sections"));
            }
            if layout.sheets.is_some()
                || layout.tables.is_some()
                || layout.names.is_some()
                || layout.headers.is_some()
            {
                return Err(layout_differs("sheets, tables, names, or headers"));
            }
        }
        Content::Cells(parsed) => {
            let structure = &parsed.structure;
            let sheets = structure.sheets.iter().map(|s| s.name.as_str()).collect();
            if differs(&layout.sheets, sheets) {
                return Err(layout_differs("sheets"));
            }
            let tables = structure
                .sheets
                .iter()
                .flat_map(|sheet| sheet.tables.iter().map(|table| table.name.as_str()))
                .collect();
            if differs(&layout.tables, tables) {
                return Err(layout_differs("tables"));
            }
            let names = structure.names.iter().map(|n| n.name.as_str()).collect();
            if differs(&layout.names, names) {
                return Err(layout_differs("defined names"));
            }
            let headers = structure
                .sheets
                .first()
                .map(|sheet| sheet.headers.iter().map(String::as_str).collect())
                .unwrap_or_default();
            if differs(&layout.headers, headers) {
                return Err(layout_differs("headers"));
            }
            if layout.keys.is_some() || layout.sections.is_some() {
                return Err(layout_differs("keys or sections"));
            }
        }
        Content::Json { root, .. } => {
            let keys = match &root.value {
                JsonValue::Record(members) => members.iter().map(|(k, _)| k.as_str()).collect(),
                _ => Vec::new(),
            };
            if differs(&layout.keys, keys) {
                return Err(layout_differs("keys"));
            }
            if layout.sheets.is_some()
                || layout.tables.is_some()
                || layout.names.is_some()
                || layout.headers.is_some()
                || layout.sections.is_some()
            {
                return Err(layout_differs(
                    "sheets, tables, names, headers, or sections",
                ));
            }
        }
    }
    Ok(())
}

/// One value's place: what the format says it holds, and where its literal
/// lies in the file's bytes. A workbook cell has no span in the file.
#[derive(Debug, Clone)]
pub(crate) struct Slot {
    pub cell: Cell,
    pub span: Option<ByteSpan>,
    /// Why the value is not read: a settings file's form that means
    /// different things to different programs (decision 0246).
    pub unread: Option<&'static str>,
}

/// Where one record is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowAt {
    /// A record of delimited text, by its row, from one.
    Line(u32),
    /// An element of a JSON list, by its position, from zero.
    Element(usize),
    /// A row of a sheet, or one single place.
    Other,
}

/// One record: a slot for each declared field, in declared order. A slot is
/// absent where the file holds nothing there.
#[derive(Debug, Clone)]
pub(crate) struct Row {
    pub at: RowAt,
    pub slots: Vec<Option<Slot>>,
}

/// A JSON list whose elements are the records.
#[derive(Debug, Clone)]
pub(crate) struct ListAt {
    /// The list, brackets included.
    pub span: ByteSpan,
    /// Each element.
    pub elements: Vec<ByteSpan>,
}

/// The records of one kind, as found in the parsed file.
#[derive(Debug, Clone)]
pub(crate) struct Table {
    pub rows: Vec<Row>,
    /// For delimited text: the column of each declared field, from one, and
    /// how many columns the header has.
    pub columns: Vec<u32>,
    pub width: u32,
    /// For a JSON list of records.
    pub list: Option<ListAt>,
}

fn not_found(what: impl std::fmt::Display) -> StepFailure {
    stop(
        codes::PLACE_NOT_FOUND,
        format!("A place the document names is not in the file: {what}."),
        "Look at what changed in the file, and correct the document or put the place back.",
    )
}

fn text_of(part: &ParsedPart, row: u32, column: u32) -> Option<&str> {
    match part.cell(row, column)?.cell.raw.settled()? {
        RawCell::Text(text) => Some(text),
        _ => None,
    }
}

fn header_of(field: &FieldSpec) -> &str {
    field.header.as_deref().unwrap_or_default()
}

fn rows_of_text(parsed: &Parsed, record: &RecordSpec) -> Result<Table, StepFailure> {
    let part = parsed.part(0).ok_or_else(|| not_found("the file's rows"))?;
    let headers = parsed
        .structure
        .sheets
        .first()
        .map(|sheet| sheet.headers.as_slice())
        .unwrap_or_default();
    let mut columns = Vec::new();
    for field in &record.fields {
        let wanted = header_of(field);
        let mut found = headers.iter().zip(1_u32..).filter(|(h, _)| *h == wanted);
        match (found.next(), found.next()) {
            (Some((_, column)), None) => columns.push(column),
            (None, _) => return Err(not_found(format!("the header `{wanted}`"))),
            _ => {
                return Err(not_found(format!(
                    "the header `{wanted}`, which appears twice"
                )));
            }
        }
    }
    let width = u32::try_from(headers.len()).unwrap_or(u32::MAX);
    let mut rows = Vec::new();
    for row in 2..=part.rows {
        let found = part.widths.get(row as usize - 1).copied().unwrap_or(0);
        if found != width {
            return Err(stop(
                codes::FILE_UNREADABLE,
                format!("Row {row} of the file has {found} fields where its header has {width}."),
                "Correct the row in the file.",
            ));
        }
        let slots = columns
            .iter()
            .map(|&column| {
                part.cell(row, column).map(|placed| Slot {
                    cell: placed.cell.clone(),
                    span: placed.value_span,
                    unread: None,
                })
            })
            .collect();
        rows.push(Row {
            at: RowAt::Line(row),
            slots,
        });
    }
    Ok(Table {
        rows,
        columns,
        width,
        list: None,
    })
}

fn sheet_rows(part: &ParsedPart, header_row: u32, columns: &[u32], last_row: u32) -> Vec<Row> {
    (header_row + 1..=last_row)
        .filter_map(|row| {
            let slots: Vec<Option<Slot>> = columns
                .iter()
                .map(|&column| {
                    part.cell(row, column)
                        .filter(|placed| placed.cell.raw != RawCell::Empty)
                        .map(|placed| Slot {
                            cell: placed.cell.clone(),
                            span: None,
                            unread: None,
                        })
                })
                .collect();
            // A row that holds nothing in the declared columns is no record.
            slots.iter().any(Option::is_some).then_some(Row {
                at: RowAt::Other,
                slots,
            })
        })
        .collect()
}

fn of_sheet(table: Vec<Row>) -> Table {
    Table {
        rows: table,
        columns: Vec::new(),
        width: 0,
        list: None,
    }
}

fn sheet_named<'a>(
    parsed: &'a Parsed,
    sheet: &str,
) -> Result<(usize, &'a ParsedPart), StepFailure> {
    parsed
        .part_named(sheet)
        .and_then(|index| Some((index as usize, parsed.part(index)?)))
        .ok_or_else(|| not_found(format!("the sheet `{sheet}`")))
}

fn rows_of_workbook(parsed: &Parsed, record: &RecordSpec) -> Result<Table, StepFailure> {
    match &record.place {
        PlaceSpec::Table { sheet, name } => {
            let (index, part) = sheet_named(parsed, sheet)?;
            let range = parsed
                .structure
                .sheets
                .get(index)
                .and_then(|sheet| sheet.tables.iter().find(|table| &table.name == name))
                .map(|table| table.range)
                .ok_or_else(|| not_found(format!("the table `{name}`")))?;
            let mut columns = Vec::new();
            for field in &record.fields {
                let wanted = header_of(field);
                let column = (range.left..=range.right)
                    .find(|&column| text_of(part, range.top, column) == Some(wanted))
                    .ok_or_else(|| not_found(format!("the header `{wanted}`")))?;
                columns.push(column);
            }
            Ok(of_sheet(sheet_rows(
                part,
                range.top,
                &columns,
                range.bottom,
            )))
        }
        PlaceSpec::Sheet { sheet } => {
            let (_, part) = sheet_named(parsed, sheet)?;
            let column_of = |row: u32, wanted: &str| {
                part.row(row)
                    .iter()
                    .find(|placed| text_of(part, row, placed.cell.at.column) == Some(wanted))
                    .map(|placed| placed.cell.at.column)
            };
            // The header row is the first row that holds every declared header.
            let header_row = (1..=part.rows)
                .find(|&row| {
                    record
                        .fields
                        .iter()
                        .all(|field| column_of(row, header_of(field)).is_some())
                })
                .ok_or_else(|| not_found("a row that holds every declared header"))?;
            let columns: Vec<u32> = record
                .fields
                .iter()
                .filter_map(|field| column_of(header_row, header_of(field)))
                .collect();
            Ok(of_sheet(sheet_rows(part, header_row, &columns, part.rows)))
        }
        PlaceSpec::Cell { name } => {
            let target = parsed
                .structure
                .names
                .iter()
                .find(|defined| &defined.name == name)
                .map(|defined| defined.target.as_str())
                .ok_or_else(|| not_found(format!("the defined name `{name}`")))?;
            // A defined name points at one cell of one sheet, such as
            // `Notes!$B$1`. Anything else is not a place.
            let one_cell = target.rsplit_once('!').and_then(|(sheet, range)| {
                let sheet = match sheet.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
                    Some(quoted) => quoted.replace("''", "'"),
                    None => sheet.to_owned(),
                };
                let range = CellRange::from_a1(range)?;
                (range.top == range.bottom && range.left == range.right)
                    .then_some((sheet, range.top, range.left))
            });
            let Some((sheet, row, column)) = one_cell else {
                return Err(unsupported(
                    "a defined name that does not point at one cell of one sheet",
                ));
            };
            let (_, part) = sheet_named(parsed, &sheet)?;
            let slot = part.cell(row, column).map(|placed| Slot {
                cell: placed.cell.clone(),
                span: None,
                unread: None,
            });
            Ok(of_sheet(vec![Row {
                at: RowAt::Other,
                slots: vec![slot],
            }]))
        }
        PlaceSpec::Rows | PlaceSpec::Path { .. } => {
            Err(unsupported("a place of another format in a workbook"))
        }
    }
}

fn member<'a>(node: &'a JsonNode, key: &str) -> Option<&'a JsonNode> {
    match &node.value {
        JsonValue::Record(members) => members
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value),
        _ => None,
    }
}

fn shifted(span: ByteSpan, offset: u64) -> ByteSpan {
    ByteSpan {
        start: span.start + offset,
        end: span.end + offset,
    }
}

fn json_slot(node: &JsonNode, offset: u64, row: u32, column: u32) -> Slot {
    let raw = match &node.value {
        JsonValue::Null => RawCell::Empty,
        JsonValue::Bool(flag) => RawCell::Bool(*flag),
        JsonValue::Number(text) => RawCell::Number(text.clone()),
        JsonValue::Text(text) if text.is_empty() => RawCell::Empty,
        JsonValue::Text(text) => RawCell::Text(text.clone()),
        JsonValue::List(_) | JsonValue::Record(_) => RawCell::Nested,
    };
    Slot {
        cell: Cell {
            raw,
            at: CellRef::new(0, row, column),
            format: None,
            hidden: false,
            merged: None,
        },
        span: Some(shifted(node.span, offset)),
        unread: None,
    }
}

fn json_row(element: &JsonNode, record: &RecordSpec, offset: u64, at: RowAt, row: u32) -> Row {
    let slots = record
        .fields
        .iter()
        .zip(1_u32..)
        .map(|(field, column)| {
            field
                .path
                .iter()
                .try_fold(element, |node, key| member(node, key))
                .map(|node| json_slot(node, offset, row, column))
        })
        .collect();
    Row { at, slots }
}

fn rows_of_json(root: &JsonNode, offset: u64, record: &RecordSpec) -> Result<Table, StepFailure> {
    let PlaceSpec::Path { keys } = &record.place else {
        return Err(unsupported("a place of another format in a JSON document"));
    };
    let mut node = root;
    for key in keys {
        node = member(node, key).ok_or_else(|| not_found(format!("the key `{key}`")))?;
    }
    if record.key.is_empty() {
        if !matches!(node.value, JsonValue::Record(_)) {
            return Err(not_found("a record at the declared path"));
        }
        return Ok(Table {
            rows: vec![json_row(node, record, offset, RowAt::Other, 1)],
            columns: Vec::new(),
            width: 0,
            list: None,
        });
    }
    let JsonValue::List(elements) = &node.value else {
        return Err(not_found("a list at the declared path"));
    };
    let mut rows = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        if !matches!(element.value, JsonValue::Record(_)) {
            return Err(unsupported(
                "a list of records that holds something other than a record",
            ));
        }
        let row = u32::try_from(index + 1).unwrap_or(u32::MAX);
        rows.push(json_row(
            element,
            record,
            offset,
            RowAt::Element(index),
            row,
        ));
    }
    Ok(Table {
        rows,
        columns: Vec::new(),
        width: 0,
        list: Some(ListAt {
            span: shifted(node.span, offset),
            elements: elements
                .iter()
                .map(|element| shifted(element.span, offset))
                .collect(),
        }),
    })
}

/// The one record of a settings file: each field's value, at the record's
/// path and then the field's own.
fn record_of_settings(file: &Settings, record: &RecordSpec) -> Result<Table, StepFailure> {
    let PlaceSpec::Path { keys } = &record.place else {
        return Err(unsupported("a place of another format in a settings file"));
    };
    let mut slots = Vec::new();
    for (field, column) in record.fields.iter().zip(1_u32..) {
        let path: Vec<String> = keys.iter().chain(&field.path).cloned().collect();
        slots.push(file.find(&path)?.map(|setting| Slot {
            cell: Cell {
                raw: setting.raw(),
                at: CellRef::new(0, 1, column),
                format: None,
                hidden: false,
                merged: None,
            },
            span: Some(setting.span),
            unread: setting.unread,
        }));
    }
    Ok(Table {
        rows: vec![Row {
            at: RowAt::Other,
            slots,
        }],
        columns: Vec::new(),
        width: 0,
        list: None,
    })
}

/// The setting at a slot's span, for its editor.
pub(crate) fn setting_at(open: &OpenFile, span: ByteSpan) -> Option<&settings::Setting> {
    match &open.content {
        Content::Settings(file) => file.entries.iter().find(|setting| setting.span == span),
        _ => None,
    }
}

/// Finds the records of one kind in the parsed file.
pub(crate) fn table(open: &OpenFile, record: &RecordSpec) -> Result<Table, StepFailure> {
    match &open.content {
        Content::Settings(file) => record_of_settings(file, record),
        Content::Json { root, offset } => rows_of_json(root, *offset, record),
        Content::Cells(parsed) if record.place == PlaceSpec::Rows => rows_of_text(parsed, record),
        Content::Cells(parsed) => rows_of_workbook(parsed, record),
    }
}

/// The typed value at a slot, by its field's type. An absent slot is `Null`.
pub(crate) fn value_at(
    slot: Option<&Slot>,
    field: &FieldSpec,
    typer: &FieldTyper,
    open: &OpenFile,
) -> Result<Value, StepFailure> {
    let Some(slot) = slot else {
        return Ok(Value::Null);
    };
    if let Some(why) = slot.unread {
        return Err(unsupported(&format!(
            "the value of the field `{}`, because {why}",
            field.name
        )));
    }
    if matches!(slot.cell.raw, RawCell::Formula { .. }) && !field.formula {
        return Err(stop(
            codes::FORMULA_AT_PLACE,
            format!(
                "The place of the field `{}` holds a formula, and the document declares a value there.",
                field.name
            ),
            "Mark the field `formula: true` to read its kept result, or point it at the cell that holds the value.",
        ));
    }
    let context = CellContext {
        date_system: match &open.content {
            Content::Cells(parsed) => parsed.date_system(),
            Content::Json { .. } | Content::Settings(_) => None,
        },
        missing: &[],
    };
    typed(&slot.cell, typer, &context).map_err(|problem| {
        stop(
            codes::VALUE_UNREADABLE,
            format!(
                "The place of the field `{}` holds something its field type cannot read. {}",
                field.name, problem.message
            ),
            "Correct the value in the file, or the field's type in the document.",
        )
    })
}
