//! Finding the data in a parsed input, by name or by header text, never by
//! a coordinate: which part, which row is the header, which column each
//! header names, and which rows are data.

use crate::definition::{HiddenRule, Locate};
use crate::error::{Problem, SCHEMA_INVALID};
use crate::model::{CellRange, CellRef, ContentKind, Parsed, ParsedPart, PlacedCell, RawCell};

/// One located column: its header text and its column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableColumn {
    /// The header's text.
    pub header: String,
    /// The column, from one.
    pub column: u32,
}

/// The located data: a view of the parsed form. It holds no cell; cells are
/// read from the part it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellTable {
    /// The part's position in the parsed form.
    pub part: u32,
    /// The header row. Zero for a JSON list, whose headers are its keys.
    pub header_row: u32,
    /// The columns, by column.
    pub columns: Vec<TableColumn>,
    /// The data rows, in order.
    pub rows: Vec<u32>,
}

impl CellTable {
    /// The column under that header.
    #[must_use]
    pub fn column(&self, header: &str) -> Option<u32> {
        self.columns
            .iter()
            .find(|column| column.header == header)
            .map(|column| column.column)
    }
}

fn problem(message: impl Into<String>) -> Problem {
    Problem::new(SCHEMA_INVALID, message)
}

fn text_of(placed: &PlacedCell) -> Option<&str> {
    match placed.cell.raw.settled() {
        Some(RawCell::Text(text)) => Some(text),
        _ => None,
    }
}

fn holds_something(placed: &PlacedCell) -> bool {
    !matches!(placed.cell.raw, RawCell::Empty)
}

/// The sheet and the range a defined name points at, when it points at one
/// rectangle of one sheet: `Sheet!$A$1:$E$4` or `'Sheet name'!$A$1`.
fn name_target(target: &str) -> Option<(String, CellRange)> {
    let (sheet, range) = target.rsplit_once('!')?;
    let sheet = match sheet.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        Some(quoted) => quoted.replace("''", "'"),
        None => sheet.to_owned(),
    };
    Some((sheet, CellRange::from_a1(range)?))
}

/// Finds the data. Every problem found is returned, not only the first.
///
/// # Errors
///
/// The problems, each `schema_invalid`: the place is not in the input, a
/// header the definition names is missing or appears twice, a column has a
/// header the definition does not name, a row has another number of fields
/// than the header, an empty row or a merged range lies inside the data, or
/// a hidden row or column does and the rule refuses it.
pub fn locate(
    parsed: &Parsed,
    place: &Locate,
    wanted: &[String],
    hidden: HiddenRule,
) -> Result<CellTable, Vec<Problem>> {
    let workbook = parsed.kind == ContentKind::Xlsx;
    let mut table = match (workbook, place) {
        (true, Locate::Document) => {
            return Err(vec![problem(
                "A workbook's data is found by a table, a defined name, or a sheet, not as the whole document.",
            )]);
        }
        (true, Locate::Named(name)) => named_range(parsed, name)?,
        (true, Locate::Sheet(name)) => by_headers(parsed, name, wanted)?,
        (false, Locate::Sheet(_)) => {
            return Err(vec![problem(
                "Only a workbook has sheets; this input is found as the whole document or by a key.",
            )]);
        }
        (false, Locate::Document) => flat(parsed, "")?,
        (false, Locate::Named(name)) => flat(parsed, name)?,
    };
    let Some(part) = parsed.part(table.part) else {
        return Err(vec![problem("The located part is not in the input.")]);
    };
    let mut problems = Vec::new();
    let header_at = |column: u32| CellRef::new(table.part, table.header_row.max(1), column);

    // Headers: each once, each named by the definition, none missing.
    for (position, column) in table.columns.iter().enumerate() {
        if column.header.is_empty() {
            problems
                .push(problem("A column in the data has no header.").at(header_at(column.column)));
        } else if table.columns[..position]
            .iter()
            .any(|earlier| earlier.header == column.header)
        {
            problems.push(
                problem("A header appears twice, so its column cannot be told apart.")
                    .at(header_at(column.column))
                    .field(&column.header),
            );
        } else if !wanted.contains(&column.header) {
            problems.push(
                problem("A column has a header the source definition does not name.")
                    .at(header_at(column.column))
                    .field(&column.header),
            );
        }
    }
    for header in wanted {
        if table.column(header).is_none() {
            problems.push(
                problem("A header the source definition names is not in the data.").field(header),
            );
        }
    }

    if workbook {
        check_sheet(parsed, part, &mut table, hidden, &mut problems);
    } else if parsed.kind == ContentKind::Csv {
        let width = u32::try_from(table.columns.len()).unwrap_or(u32::MAX);
        for &row in &table.rows {
            let found = part
                .widths
                .get(row as usize - 1)
                .copied()
                .unwrap_or_default();
            if found != width {
                problems.push(
                    problem(format!(
                        "The row has {found} field(s) where the header has {width}."
                    ))
                    .at(CellRef::new(table.part, row, 1)),
                );
            }
        }
    }
    if problems.is_empty() {
        Ok(table)
    } else {
        Err(problems)
    }
}

/// A delimited file, or one list of a JSON document.
fn flat(parsed: &Parsed, name: &str) -> Result<CellTable, Vec<Problem>> {
    let Some(index) = parsed.part_named(name) else {
        return Err(vec![problem(if name.is_empty() {
            "The document's root is not a list of records.".to_owned()
        } else {
            format!("The document has no list of records under the key `{name}`.")
        })]);
    };
    let part = parsed
        .part(index)
        .ok_or_else(|| vec![problem("The located part is not in the input.")])?;
    let headers = parsed
        .structure
        .sheets
        .get(index as usize)
        .map(|sheet| sheet.headers.clone())
        .unwrap_or_default();
    let columns = headers
        .into_iter()
        .zip(1_u32..)
        .map(|(header, column)| TableColumn { header, column })
        .collect();
    if parsed.kind == ContentKind::Csv {
        if part.rows == 0 {
            return Err(vec![problem("The input has no header row.")]);
        }
        Ok(CellTable {
            part: index,
            header_row: 1,
            columns,
            rows: (2..=part.rows).collect(),
        })
    } else {
        Ok(CellTable {
            part: index,
            header_row: 0,
            columns,
            rows: (1..=part.rows).collect(),
        })
    }
}

/// A table or a defined name of a workbook.
fn named_range(parsed: &Parsed, name: &str) -> Result<CellTable, Vec<Problem>> {
    let mut found: Vec<(u32, CellRange)> = Vec::new();
    for (index, sheet) in parsed.structure.sheets.iter().enumerate() {
        for table in &sheet.tables {
            if table.name == name {
                found.push((u32::try_from(index).unwrap_or(u32::MAX), table.range));
            }
        }
    }
    for defined in &parsed.structure.names {
        if defined.name != name {
            continue;
        }
        let Some((sheet, range)) = name_target(&defined.target) else {
            return Err(vec![problem(format!(
                "The defined name `{name}` does not point at one rectangle of one sheet."
            ))]);
        };
        let Some(index) = parsed.part_named(&sheet) else {
            return Err(vec![problem(format!(
                "The defined name `{name}` points at a sheet the workbook does not have."
            ))]);
        };
        found.push((index, range));
    }
    let (index, range) = match found.as_slice() {
        [] => {
            return Err(vec![problem(format!(
                "The workbook has no table and no defined name called `{name}`."
            ))]);
        }
        [only] => *only,
        _ => {
            return Err(vec![problem(format!(
                "The workbook has more than one table or defined name called `{name}`."
            ))]);
        }
    };
    let part = parsed
        .part(index)
        .ok_or_else(|| vec![problem("The located part is not in the input.")])?;
    let columns = (range.left..=range.right)
        .map(|column| TableColumn {
            header: part
                .cell(range.top, column)
                .and_then(text_of)
                .unwrap_or_default()
                .to_owned(),
            column,
        })
        .collect();
    Ok(CellTable {
        part: index,
        header_row: range.top,
        columns,
        rows: (range.top + 1..=range.bottom).collect(),
    })
}

/// A sheet, whose header row is the first row holding every wanted header.
fn by_headers(parsed: &Parsed, sheet: &str, wanted: &[String]) -> Result<CellTable, Vec<Problem>> {
    let Some(index) = parsed.part_named(sheet) else {
        return Err(vec![problem(format!(
            "The workbook has no sheet called `{sheet}`."
        ))]);
    };
    let part = parsed
        .part(index)
        .ok_or_else(|| vec![problem("The located part is not in the input.")])?;
    if wanted.is_empty() {
        return Err(vec![problem(
            "The source definition names no header, so no header row can be found.",
        )]);
    }
    let mut header_row = None;
    let mut at = 0;
    while let Some(first) = part.cells.get(at) {
        let row = first.cell.at.row;
        let cells = part.row(row);
        at += cells.len().max(1);
        let has = |header: &String| cells.iter().any(|placed| text_of(placed) == Some(header));
        if wanted.iter().all(has) {
            header_row = Some(row);
            break;
        }
    }
    let Some(header_row) = header_row else {
        return Err(vec![problem(format!(
            "No row of the sheet `{sheet}` holds every header the source definition names."
        ))]);
    };
    let columns = part
        .row(header_row)
        .iter()
        .filter_map(|placed| {
            text_of(placed).map(|text| TableColumn {
                header: text.to_owned(),
                column: placed.cell.at.column,
            })
        })
        .collect();
    Ok(CellTable {
        part: index,
        header_row,
        columns,
        rows: (header_row + 1..=part.rows).collect(),
    })
}

/// The checks only a sheet needs: rows that hold nothing after the data are
/// not rows; a row that holds nothing inside the data, a merged range in the
/// data, and, when refused, a hidden row or column, are problems.
fn check_sheet(
    parsed: &Parsed,
    part: &ParsedPart,
    table: &mut CellTable,
    hidden: HiddenRule,
    problems: &mut Vec<Problem>,
) {
    let located = |placed: &&PlacedCell| {
        table
            .columns
            .iter()
            .any(|column| column.column == placed.cell.at.column)
    };
    let row_holds = |row: u32| part.row(row).iter().filter(located).any(holds_something);
    while table.rows.last().is_some_and(|&row| !row_holds(row)) {
        table.rows.pop();
    }
    for &row in &table.rows {
        if !row_holds(row) {
            problems.push(
                problem("A row that holds nothing lies inside the data.")
                    .at(CellRef::new(table.part, row, 1)),
            );
        }
    }
    let (Some(&first), Some(&last)) = (table.rows.first(), table.rows.last()) else {
        return;
    };
    if let Some(sheet) = parsed.structure.sheets.get(table.part as usize) {
        for range in &sheet.merged {
            let rows_meet = range.top <= last && range.bottom >= first;
            let columns_meet = table
                .columns
                .iter()
                .any(|column| column.column >= range.left && column.column <= range.right);
            if rows_meet && columns_meet {
                problems.push(
                    problem(format!(
                        "The merged range {} lies inside the data, so its rows cannot be told apart.",
                        range.a1()
                    ))
                    .at(CellRef::new(table.part, range.top, range.left)),
                );
            }
        }
    }
    if hidden == HiddenRule::Reject {
        let mut hidden_columns: Vec<u32> = Vec::new();
        for placed in part.row(table.header_row).iter().filter(located) {
            if placed.cell.hidden {
                hidden_columns.push(placed.cell.at.column);
                problems.push(
                    problem("A column of the data is hidden.")
                        .at(placed.cell.at)
                        .field(text_of(placed).unwrap_or_default()),
                );
            }
        }
        for &row in &table.rows {
            let hidden_cell = part.row(row).iter().filter(located).find(|placed| {
                placed.cell.hidden && !hidden_columns.contains(&placed.cell.at.column)
            });
            if let Some(placed) = hidden_cell {
                problems.push(problem("A row of the data is hidden.").at(placed.cell.at));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_defined_name_points_at_one_rectangle_of_one_sheet() {
        let (sheet, range) = name_target("Notes!$B$1").unwrap();
        assert_eq!((sheet.as_str(), range.a1().as_str()), ("Notes", "B1"));
        let (sheet, range) = name_target("'Price list'!$A$1:$E$4").unwrap();
        assert_eq!(
            (sheet.as_str(), range.a1().as_str()),
            ("Price list", "A1:E4")
        );
        assert!(name_target("Notes!$A:$A").is_none());
        assert!(name_target("A1,B2").is_none());
        assert!(name_target("OFFSET(Notes!$A$1,0,0)").is_none());
    }
}
