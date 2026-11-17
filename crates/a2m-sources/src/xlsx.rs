//! The reader of workbooks. It reads the package's parts directly, because a
//! number must be taken as the digits the workbook stores and a cell's value
//! must come with the place of those digits in its part.
//!
//! A workbook keeps a formula and the result of its last calculation in the
//! same cell element, so both are read in one walk over a sheet: the formula's
//! presence, and the kept result or its absence. A formula is never
//! calculated, a macro is never run, and a link to another file is never
//! followed.

use std::collections::BTreeMap;

use a2m_core::Failure;
use a2m_expr::DateSystem;

use crate::error::{malformed, mismatch, protected, too_large};
use crate::fingerprint::{DefinedName, DropdownList, SheetStructure, Structure, TableStructure};
use crate::model::{
    ByteSpan, Cell, CellRange, CellRef, ContentKind, Limits, Parsed, ParsedPart, PartBytes,
    PlacedCell, RawCell, ReaderName, SourceInput, SourceReader, cell_from_a1,
};
use crate::package::Package;
use crate::xml::{Event, Xml};

/// The first bytes of a compound file: the container of an encrypted
/// workbook, and of the older binary workbook format.
pub(crate) const COMPOUND_FILE: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

const CONTENT_TYPES: &str = "[Content_Types].xml";
const WORKBOOK: &str = "xl/workbook.xml";

/// The built-in reader of workbooks.
#[derive(Debug, Clone)]
pub struct XlsxReader {
    name: ReaderName,
}

impl XlsxReader {
    /// The reader, named `xlsx`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            name: ReaderName::builtin(ReaderName::XLSX),
        }
    }
}

impl Default for XlsxReader {
    fn default() -> Self {
        Self::new()
    }
}

fn open<'a>(input: &SourceInput<'a>) -> Result<Package<'a>, Failure> {
    if input.bytes.starts_with(COMPOUND_FILE) {
        return Err(protected());
    }
    if !input.bytes.starts_with(b"PK\x03\x04") {
        return Err(mismatch(
            "the content is not a package, so it is not a workbook",
        ));
    }
    let package = Package::open(input.bytes, input.limits.unpacked_bytes)?;
    if !package.has(CONTENT_TYPES) || !package.has(WORKBOOK) {
        return Err(mismatch("the package does not hold a workbook"));
    }
    Ok(package)
}

impl SourceReader for XlsxReader {
    fn name(&self) -> &ReaderName {
        &self.name
    }

    fn check(&self, input: &SourceInput<'_>) -> Result<(), Failure> {
        open(input).map(|_| ())
    }

    fn parse(&self, input: &SourceInput<'_>) -> Result<Parsed, Failure> {
        let mut package = open(input)?;
        let macros = package.names().any(|name| name.ends_with("vbaProject.bin"));
        let signed = package
            .names()
            .any(|name| name.starts_with("_xmlsignatures/"));
        let workbook_bytes = required(&mut package, WORKBOOK)?;
        let workbook = read_workbook(&workbook_bytes)?;
        let relations = relations_of(&mut package, WORKBOOK)?;
        let shared = match relations
            .values()
            .find(|r| r.kind.ends_with("/sharedStrings"))
        {
            Some(relation) => {
                read_shared(&required(&mut package, &relation.target)?, &input.limits)?
            }
            None => Vec::new(),
        };
        let styles = match relations.values().find(|r| r.kind.ends_with("/styles")) {
            Some(relation) => read_styles(&required(&mut package, &relation.target)?)?,
            None => Vec::new(),
        };
        let mut parts = Vec::new();
        let mut sheets = Vec::new();
        let mut cells_left = input.limits.cells;
        for (position, sheet) in workbook.sheets.iter().enumerate() {
            let index = u32::try_from(position).unwrap_or(u32::MAX);
            let relation = relations
                .get(&sheet.relation)
                .ok_or_else(|| malformed("a sheet names a part the workbook does not list"))?;
            let mut structure = SheetStructure {
                name: sheet.name.clone(),
                hidden: sheet.hidden,
                ..SheetStructure::default()
            };
            // A sheet that is not a grid of cells, such as a chart sheet, is
            // listed by name and has no cells.
            if !relation.kind.ends_with("/worksheet") {
                parts.push(ParsedPart {
                    name: sheet.name.clone(),
                    path: relation.target.clone(),
                    bytes: PartBytes::Unpacked(Vec::new()),
                    cells: Vec::new(),
                    rows: 0,
                    widths: Vec::new(),
                });
                sheets.push(structure);
                continue;
            }
            let bytes = required(&mut package, &relation.target)?;
            let context = SheetContext {
                index,
                shared: &shared,
                styles: &styles,
                limits: &input.limits,
            };
            let sheet_read = read_sheet(&bytes, &context, &mut cells_left)?;
            structure.headers = first_row_headers(&sheet_read.cells);
            structure.merged = sheet_read.merged;
            structure.lists = sheet_read.lists;
            if !sheet_read.tables.is_empty() {
                let sheet_relations = relations_of(&mut package, &relation.target)?;
                for id in &sheet_read.tables {
                    let table = sheet_relations
                        .get(id)
                        .ok_or_else(|| malformed("a sheet names a table part it does not list"))?;
                    let table_bytes = required(&mut package, &table.target)?;
                    let (name, range) = read_table(&table_bytes)?;
                    let headers = (range.left..=range.right)
                        .map(|column| text_at(&sheet_read.cells, range.top, column))
                        .collect();
                    structure.tables.push(TableStructure {
                        name,
                        range,
                        headers,
                    });
                }
            }
            let rows = sheet_read
                .cells
                .last()
                .map_or(0, |placed| placed.cell.at.row);
            parts.push(ParsedPart {
                name: sheet.name.clone(),
                path: relation.target.clone(),
                bytes: PartBytes::Unpacked(bytes),
                cells: sheet_read.cells,
                rows,
                widths: Vec::new(),
            });
            sheets.push(structure);
        }
        let names = workbook
            .names
            .into_iter()
            .map(|(name, local, target)| DefinedName {
                name,
                sheet: local.and_then(|index| workbook.sheets.get(index).map(|s| s.name.clone())),
                target,
            })
            .collect();
        Ok(Parsed {
            kind: ContentKind::Xlsx,
            structure: Structure {
                date_system: Some(workbook.date_system),
                sheets,
                names,
                macros,
                signed,
            },
            parts,
        })
    }
}

fn required(package: &mut Package<'_>, name: &str) -> Result<Vec<u8>, Failure> {
    package
        .part(name)?
        .ok_or_else(|| malformed("the workbook names a part the package does not hold"))
}

/// One relation of a part: what kind of part it points at, and which.
#[derive(Debug)]
struct Relation {
    kind: String,
    target: String,
}

/// The relations of the part at `path`, by their id. A part with no
/// relations part has none. A relation that points outside the package is
/// kept out: it is never followed.
fn relations_of(
    package: &mut Package<'_>,
    path: &str,
) -> Result<BTreeMap<String, Relation>, Failure> {
    let (folder, file) = path.rsplit_once('/').unwrap_or(("", path));
    let relations_path = if folder.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{folder}/_rels/{file}.rels")
    };
    let mut relations = BTreeMap::new();
    let Some(bytes) = package.part(&relations_path)? else {
        return Ok(relations);
    };
    let mut xml = Xml::new(&bytes)?;
    while let Some(event) = xml.next()? {
        if let Event::Open {
            name: "Relationship",
            attributes,
            ..
        } = event
        {
            if attributes.get("TargetMode").as_deref() == Some("External") {
                continue;
            }
            let (Some(id), Some(kind), Some(target)) = (
                attributes.get("Id"),
                attributes.get("Type"),
                attributes.get("Target"),
            ) else {
                return Err(malformed(
                    "a relation lacks its id, its type, or its target",
                ));
            };
            let target = resolve(folder, &target)
                .ok_or_else(|| malformed("a relation points outside the package"))?;
            relations.insert(
                id.into_owned(),
                Relation {
                    kind: kind.into_owned(),
                    target,
                },
            );
        }
    }
    Ok(relations)
}

/// The path of a part named from a part in `folder`. Nothing when the name
/// climbs out of the package.
fn resolve(folder: &str, target: &str) -> Option<String> {
    let mut segments: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        folder.split('/').filter(|s| !s.is_empty()).collect()
    };
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    Some(segments.join("/"))
}

#[derive(Debug)]
struct SheetEntry {
    name: String,
    hidden: bool,
    relation: String,
}

#[derive(Debug)]
struct Workbook {
    date_system: DateSystem,
    sheets: Vec<SheetEntry>,
    /// Each defined name, the position of the sheet it belongs to, if any,
    /// and its target.
    names: Vec<(String, Option<usize>, String)>,
}

fn read_workbook(bytes: &[u8]) -> Result<Workbook, Failure> {
    let mut xml = Xml::new(bytes)?;
    let mut workbook = Workbook {
        date_system: DateSystem::From1900,
        sheets: Vec::new(),
        names: Vec::new(),
    };
    while let Some(event) = xml.next()? {
        let Event::Open {
            name,
            attributes,
            closed,
        } = event
        else {
            continue;
        };
        match name {
            "workbookPr" => {
                if attributes.flag("date1904")? {
                    workbook.date_system = DateSystem::From1904;
                }
            }
            "sheet" => {
                let (Some(name), Some(relation)) = (attributes.get("name"), attributes.get("r:id"))
                else {
                    return Err(malformed("a sheet lacks its name or its part"));
                };
                let hidden = match attributes.get("state").as_deref() {
                    None | Some("visible") => false,
                    Some("hidden" | "veryHidden") => true,
                    Some(_) => {
                        return Err(malformed(
                            "a sheet states a visibility the format does not define",
                        ));
                    }
                };
                workbook.sheets.push(SheetEntry {
                    name: name.into_owned(),
                    hidden,
                    relation: relation.into_owned(),
                });
            }
            "definedName" => {
                let name = attributes
                    .get("name")
                    .ok_or_else(|| malformed("a defined name has no name"))?
                    .into_owned();
                let local = attributes
                    .get("localSheetId")
                    .and_then(|index| index.parse::<usize>().ok());
                let mut target = String::new();
                if !closed {
                    loop {
                        match xml.next()? {
                            Some(Event::Text { text, .. }) => target.push_str(&text),
                            Some(Event::Close {
                                name: "definedName",
                            }) => break,
                            Some(_) => {}
                            None => return Err(malformed("a defined name is not closed")),
                        }
                    }
                }
                // Names the spreadsheet program keeps for itself, such as a
                // print area or the range of a filter, change as the file is
                // used and are not part of its layout.
                if !name.starts_with("_xlnm.") {
                    workbook.names.push((name, local, target));
                }
            }
            _ => {}
        }
    }
    if workbook.sheets.is_empty() {
        return Err(malformed("the workbook lists no sheet"));
    }
    Ok(workbook)
}

/// Reads the characters a workbook writes as `_xHHHH_`, such as a carriage
/// return inside a cell.
fn unescape_text(text: &str) -> String {
    if !text.contains("_x") {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("_x") {
        let code = rest
            .get(at + 2..at + 6)
            .filter(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit()))
            .filter(|_| rest.get(at + 6..at + 7) == Some("_"))
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .and_then(char::from_u32);
        out.push_str(rest.get(..at).unwrap_or(""));
        if let Some(character) = code {
            out.push(character);
            rest = rest.get(at + 7..).unwrap_or("");
        } else {
            out.push_str("_x");
            rest = rest.get(at + 2..).unwrap_or("");
        }
    }
    out.push_str(rest);
    out
}

/// Collects the text of a string item: every `t` inside it, except the
/// phonetic guide. Leaves the reader after the item's close.
fn string_item(xml: &mut Xml<'_>, close: &str) -> Result<(String, Option<ByteSpan>), Failure> {
    let mut text = String::new();
    let mut spans = Vec::new();
    loop {
        match xml.next()? {
            Some(Event::Open {
                name: "rPh" | "phoneticPr",
                closed: false,
                ..
            }) => xml.skip()?,
            Some(Event::Open {
                name: "t",
                closed: false,
                ..
            }) => loop {
                match xml.next()? {
                    Some(Event::Text {
                        text: piece,
                        start,
                        end,
                    }) => {
                        text.push_str(&piece);
                        spans.push(ByteSpan::new(start, end));
                    }
                    Some(Event::Close { name: "t" }) => break,
                    _ => return Err(malformed("a text element holds something that is not text")),
                }
            },
            Some(Event::Close { name }) if name == close => break,
            Some(_) => {}
            None => return Err(malformed("a string is not closed")),
        }
    }
    let span = match spans.as_slice() {
        [only] => Some(*only),
        _ => None,
    };
    Ok((unescape_text(&text), span))
}

fn read_shared(bytes: &[u8], limits: &Limits) -> Result<Vec<String>, Failure> {
    let mut xml = Xml::new(bytes)?;
    let mut strings = Vec::new();
    while let Some(event) = xml.next()? {
        if let Event::Open {
            name: "si", closed, ..
        } = event
        {
            if strings.len() >= limits.cells as usize {
                return Err(too_large(format!(
                    "more than {} shared strings",
                    limits.cells
                )));
            }
            let text = if closed {
                String::new()
            } else {
                string_item(&mut xml, "si")?.0
            };
            if text.len() > limits.cell_bytes as usize {
                return Err(too_large(format!(
                    "a shared string is longer than {} bytes",
                    limits.cell_bytes
                )));
            }
            strings.push(text);
        }
    }
    Ok(strings)
}

/// One cell style: its number format, and whether that format shows a date
/// or a time.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Style {
    format: Option<String>,
    date: bool,
}

/// The number formats every workbook has without declaring them.
fn builtin_format(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "mm-dd-yy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yy h:mm",
        37 => "#,##0 ;(#,##0)",
        38 => "#,##0 ;[Red](#,##0)",
        39 => "#,##0.00;(#,##0.00)",
        40 => "#,##0.00;[Red](#,##0.00)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

/// Whether a number format shows a date or a time: by its number for the
/// formats every workbook has, and for a declared format by whether its code
/// holds a day, month, year, hour, or second letter outside quotes, brackets,
/// and escapes.
pub(crate) fn is_date_format(id: u32, code: Option<&str>) -> bool {
    if matches!(id, 14..=22 | 27..=36 | 45..=47 | 50..=58) {
        return true;
    }
    let Some(code) = code else {
        return false;
    };
    if id < 164 && builtin_format(id).is_some() {
        return false;
    }
    let mut characters = code.chars();
    while let Some(character) = characters.next() {
        match character {
            '"' => {
                for inner in characters.by_ref() {
                    if inner == '"' {
                        break;
                    }
                }
            }
            '[' => {
                let mut inside = String::new();
                for inner in characters.by_ref() {
                    if inner == ']' {
                        break;
                    }
                    inside.push(inner);
                }
                // Elapsed hours, minutes, and seconds are times.
                if matches!(
                    inside.to_ascii_lowercase().as_str(),
                    "h" | "hh" | "m" | "mm" | "s" | "ss"
                ) {
                    return true;
                }
            }
            '\\' | '_' | '*' => {
                characters.next();
            }
            'd' | 'D' | 'm' | 'M' | 'y' | 'Y' | 'h' | 'H' | 's' | 'S' => return true,
            _ => {}
        }
    }
    false
}

fn read_styles(bytes: &[u8]) -> Result<Vec<Style>, Failure> {
    let mut xml = Xml::new(bytes)?;
    let mut declared: BTreeMap<u32, String> = BTreeMap::new();
    let mut ids = Vec::new();
    let mut in_cell_styles = false;
    while let Some(event) = xml.next()? {
        match event {
            Event::Open {
                name: "numFmt",
                attributes,
                ..
            } => {
                if let (Some(id), Some(code)) =
                    (attributes.get("numFmtId"), attributes.get("formatCode"))
                    && let Ok(id) = id.parse::<u32>()
                {
                    declared.insert(id, code.into_owned());
                }
            }
            Event::Open {
                name: "cellXfs",
                closed,
                ..
            } => in_cell_styles = !closed,
            Event::Close { name: "cellXfs" } => in_cell_styles = false,
            Event::Open {
                name: "xf",
                attributes,
                ..
            } if in_cell_styles => {
                let id = match attributes.get("numFmtId") {
                    None => 0,
                    Some(id) => id
                        .parse::<u32>()
                        .map_err(|_| malformed("a cell style's number format is not a number"))?,
                };
                ids.push(id);
            }
            _ => {}
        }
    }
    Ok(ids
        .into_iter()
        .map(|id| {
            let format = declared
                .get(&id)
                .cloned()
                .or_else(|| builtin_format(id).map(str::to_owned));
            Style {
                date: is_date_format(id, declared.get(&id).map(String::as_str)),
                format,
            }
        })
        .collect())
}

fn read_table(bytes: &[u8]) -> Result<(String, CellRange), Failure> {
    let mut xml = Xml::new(bytes)?;
    while let Some(event) = xml.next()? {
        if let Event::Open {
            name: "table",
            attributes,
            ..
        } = event
        {
            let name = attributes
                .get("displayName")
                .or_else(|| attributes.get("name"))
                .ok_or_else(|| malformed("a table has no name"))?;
            let range = attributes
                .get("ref")
                .and_then(|range| CellRange::from_a1(&range))
                .ok_or_else(|| malformed("a table does not say where it lies"))?;
            return Ok((name.into_owned(), range));
        }
    }
    Err(malformed("a table part holds no table"))
}

struct SheetContext<'a> {
    index: u32,
    shared: &'a [String],
    styles: &'a [Style],
    limits: &'a Limits,
}

#[derive(Debug, Default)]
struct SheetRead {
    cells: Vec<PlacedCell>,
    merged: Vec<CellRange>,
    lists: Vec<DropdownList>,
    /// The relation ids of the sheet's table parts.
    tables: Vec<String>,
}

/// The text of the cell at `row` and `column`, or nothing when it holds no
/// text.
fn text_at(cells: &[PlacedCell], row: u32, column: u32) -> String {
    cells
        .binary_search_by(|placed| (placed.cell.at.row, placed.cell.at.column).cmp(&(row, column)))
        .ok()
        .and_then(|found| cells.get(found))
        .and_then(|placed| match placed.cell.raw.settled() {
            Some(RawCell::Text(text)) => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// The text cells of the first row that holds anything, by column.
fn first_row_headers(cells: &[PlacedCell]) -> Vec<String> {
    let Some(first) = cells.first() else {
        return Vec::new();
    };
    let row = first.cell.at.row;
    let last = cells
        .iter()
        .take_while(|placed| placed.cell.at.row == row)
        .map(|placed| placed.cell.at.column)
        .max()
        .unwrap_or(0);
    (1..=last)
        .map(|column| text_at(cells, row, column))
        .collect()
}

/// What one cell element holds, as the walk collects it.
#[derive(Default)]
struct CellParts {
    formula: bool,
    value: Option<(String, ByteSpan)>,
    inline: Option<(String, Option<ByteSpan>)>,
}

fn read_sheet(
    bytes: &[u8],
    context: &SheetContext<'_>,
    cells_left: &mut u32,
) -> Result<SheetRead, Failure> {
    let mut xml = Xml::new(bytes)?;
    let mut read = SheetRead::default();
    let mut hidden_columns: Vec<(u32, u32)> = Vec::new();
    let mut row = 0_u32;
    let mut row_hidden = false;
    let mut column = 0_u32;
    let mut rows_seen = 0_u32;
    let mut last = (0_u32, 0_u32);
    while let Some(event) = xml.next()? {
        let Event::Open {
            name,
            attributes,
            closed,
        } = event
        else {
            continue;
        };
        match name {
            "col" => {
                if attributes.flag("hidden")? {
                    let bound =
                        |name: &str| attributes.get(name).and_then(|n| n.parse::<u32>().ok());
                    let (Some(min), Some(max)) = (bound("min"), bound("max")) else {
                        return Err(malformed(
                            "a hidden column does not say which columns it covers",
                        ));
                    };
                    hidden_columns.push((min, max));
                }
            }
            "row" => {
                row = match attributes.get("r") {
                    Some(number) => number
                        .parse()
                        .ok()
                        .filter(|&number: &u32| number > row)
                        .ok_or_else(|| malformed("a sheet's rows are not in order"))?,
                    None => row + 1,
                };
                row_hidden = attributes.flag("hidden")?;
                column = 0;
                rows_seen += 1;
                if rows_seen > context.limits.rows {
                    return Err(too_large(format!(
                        "a sheet has more than {} rows",
                        context.limits.rows
                    )));
                }
            }
            "c" => {
                let at = match attributes.get("r") {
                    Some(reference) => cell_from_a1(&reference)
                        .filter(|&(cell_row, _)| cell_row == row)
                        .ok_or_else(|| malformed("a cell's reference is not in its row"))?,
                    None => (row, column + 1),
                };
                if at <= last {
                    return Err(malformed("a sheet's cells are not in order"));
                }
                last = at;
                column = at.1;
                if closed {
                    continue;
                }
                let parts = cell_parts(&mut xml)?;
                // A style the workbook lacks is never read as no style: a
                // date would then be read as a plain number.
                let style = match attributes.get("s") {
                    None => None,
                    Some(index) => Some(
                        index
                            .parse::<usize>()
                            .ok()
                            .and_then(|index| context.styles.get(index))
                            .ok_or_else(|| malformed("a cell names a style the workbook lacks"))?,
                    ),
                };
                let kind = attributes.get("t");
                let (raw, span, shared) =
                    cell_value(kind.as_deref(), &parts, style, context.shared)?;
                let raw = match (parts.formula, raw) {
                    (true, None) => RawCell::Formula { cached: None },
                    (true, Some(raw)) => RawCell::Formula {
                        cached: Some(Box::new(raw)),
                    },
                    (false, None) => continue,
                    (false, Some(raw)) => raw,
                };
                let length = match raw.settled() {
                    Some(
                        RawCell::Text(text) | RawCell::Number(text) | RawCell::DateSerial(text),
                    ) => text.len(),
                    _ => 0,
                };
                if length > context.limits.cell_bytes as usize {
                    return Err(too_large(format!(
                        "a cell's value is longer than {} bytes",
                        context.limits.cell_bytes
                    )));
                }
                if *cells_left == 0 {
                    return Err(too_large(format!(
                        "more than {} cells",
                        context.limits.cells
                    )));
                }
                *cells_left -= 1;
                let hidden = row_hidden
                    || hidden_columns
                        .iter()
                        .any(|&(min, max)| at.1 >= min && at.1 <= max);
                read.cells.push(PlacedCell {
                    cell: Cell {
                        raw,
                        at: CellRef::new(context.index, at.0, at.1),
                        format: style.and_then(|style| style.format.clone()),
                        hidden,
                        merged: None,
                    },
                    value_span: span,
                    shared,
                });
            }
            "mergeCell" => {
                let range = attributes
                    .get("ref")
                    .and_then(|range| CellRange::from_a1(&range))
                    .ok_or_else(|| malformed("a merged range does not say where it lies"))?;
                read.merged.push(range);
            }
            "dataValidation" if attributes.get("type").as_deref() == Some("list") => {
                let cells = attributes.get("sqref").unwrap_or_default().into_owned();
                let mut choices = String::new();
                if !closed {
                    let mut inside = false;
                    loop {
                        match xml.next()? {
                            Some(Event::Open {
                                name: "formula1",
                                closed: false,
                                ..
                            }) => inside = true,
                            Some(Event::Close { name: "formula1" }) => inside = false,
                            Some(Event::Text { text, .. }) if inside => choices.push_str(&text),
                            Some(Event::Close {
                                name: "dataValidation",
                            }) => break,
                            Some(_) => {}
                            None => return Err(malformed("a dropdown list is not closed")),
                        }
                    }
                }
                read.lists.push(DropdownList { cells, choices });
            }
            "tablePart" => {
                if let Some(id) = attributes.get("r:id") {
                    read.tables.push(id.into_owned());
                }
            }
            _ => {}
        }
    }
    if !read.merged.is_empty() {
        for placed in &mut read.cells {
            let at = placed.cell.at;
            placed.cell.merged = read
                .merged
                .iter()
                .find(|range| range.contains(at.row, at.column))
                .copied();
        }
    }
    Ok(read)
}

/// Walks the content of one cell element, to after its close.
fn cell_parts(xml: &mut Xml<'_>) -> Result<CellParts, Failure> {
    let mut parts = CellParts::default();
    loop {
        match xml.next()? {
            Some(Event::Open {
                name: "f", closed, ..
            }) => {
                parts.formula = true;
                if !closed {
                    xml.skip()?;
                }
            }
            Some(Event::Open {
                name: "v",
                closed: false,
                ..
            }) => loop {
                match xml.next()? {
                    Some(Event::Text { text, start, end }) => {
                        if parts.value.is_some() {
                            return Err(malformed("a cell's value is written in pieces"));
                        }
                        parts.value = Some((text.into_owned(), ByteSpan::new(start, end)));
                    }
                    Some(Event::Close { name: "v" }) => break,
                    _ => return Err(malformed("a cell's value holds something that is not text")),
                }
            },
            Some(Event::Open {
                name: "is",
                closed: false,
                ..
            }) => parts.inline = Some(string_item(xml, "is")?),
            Some(Event::Open { closed: false, .. }) => xml.skip()?,
            Some(Event::Close { name: "c" }) => return Ok(parts),
            Some(_) => {}
            None => return Err(malformed("a cell is not closed")),
        }
    }
}

type Valued = (Option<RawCell>, Option<ByteSpan>, Option<u32>);

/// What a cell holds, by the type the cell states. Nothing when it states no
/// value.
fn cell_value(
    kind: Option<&str>,
    parts: &CellParts,
    style: Option<&Style>,
    shared: &[String],
) -> Result<Valued, Failure> {
    let text_cell = |text: &str| {
        if text.is_empty() {
            RawCell::Empty
        } else {
            RawCell::Text(text.to_owned())
        }
    };
    if kind == Some("inlineStr") {
        return Ok(match &parts.inline {
            Some((text, span)) => (Some(text_cell(text)), *span, None),
            None => (None, None, None),
        });
    }
    let Some((value, span)) = &parts.value else {
        return Ok((None, None, None));
    };
    let span = Some(*span);
    Ok(match kind {
        Some("s") => {
            let index: u32 = value
                .parse()
                .map_err(|_| malformed("a cell's shared string is not a number"))?;
            let text = shared
                .get(index as usize)
                .ok_or_else(|| malformed("a cell names a shared string the workbook lacks"))?;
            (Some(text_cell(text)), span, Some(index))
        }
        Some("str" | "d") => (Some(text_cell(&unescape_text(value))), span, None),
        Some("b") => match value.as_str() {
            "1" => (Some(RawCell::Bool(true)), span, None),
            "0" => (Some(RawCell::Bool(false)), span, None),
            _ => return Err(malformed("a true-or-false cell holds something else")),
        },
        Some("e") => (Some(RawCell::Error(value.clone())), span, None),
        Some("n") | None => {
            let raw = if style.is_some_and(|style| style.date) {
                RawCell::DateSerial(value.clone())
            } else {
                RawCell::Number(value.clone())
            };
            (Some(raw), span, None)
        }
        Some(_) => return Err(malformed("a cell states a type the format does not define")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_format_is_a_date_by_a_fixed_rule() {
        assert!(is_date_format(14, None));
        assert!(is_date_format(22, None));
        assert!(!is_date_format(0, None));
        assert!(!is_date_format(2, None));
        assert!(is_date_format(164, Some("yyyy-mm-dd")));
        assert!(is_date_format(165, Some("[h]:mm")));
        assert!(!is_date_format(166, Some("0.00 \"days\"")));
        assert!(!is_date_format(167, Some("#,##0.00\\ [$EUR]")));
        assert!(!is_date_format(168, Some("0.0E+00")));
        assert!(!is_date_format(169, Some("[Red]0.00")));
    }

    #[test]
    fn a_part_named_from_another_stays_inside_the_package() {
        assert_eq!(
            resolve("xl", "worksheets/sheet1.xml").as_deref(),
            Some("xl/worksheets/sheet1.xml")
        );
        assert_eq!(
            resolve("xl/worksheets", "../tables/table1.xml").as_deref(),
            Some("xl/tables/table1.xml")
        );
        assert_eq!(
            resolve("xl", "/xl/styles.xml").as_deref(),
            Some("xl/styles.xml")
        );
        assert_eq!(resolve("xl", "../../outside.xml"), None);
    }

    #[test]
    fn a_workbooks_escaped_characters_are_read() {
        assert_eq!(unescape_text("a_x000D_b"), "a\rb");
        assert_eq!(unescape_text("a_xb"), "a_xb");
        assert_eq!(unescape_text("plain"), "plain");
    }
}
