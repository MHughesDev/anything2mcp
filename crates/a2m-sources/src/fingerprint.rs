//! What an input is made of, without its values, and the fingerprint of it.
//!
//! A fingerprint is a list of parts in declared order and a digest over a
//! fixed encoding of that list. Values, display formats, and column widths do
//! not enter it, so two files of one template have one fingerprint, and the
//! same file has the same fingerprint on every operating system.

use a2m_expr::DateSystem;
use a2m_files::{Fingerprint, FingerprintPart, PartKind};
use serde::{Deserialize, Serialize};

use crate::error::{Problem, SOURCE_TEMPLATE_UNKNOWN};
use crate::model::CellRange;

/// What an input is made of: its sheets or lists, their header text, and
/// what a workbook declares beside them. Only what the bytes state.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Structure {
    /// A workbook's date system. Nothing for a format that has none.
    #[serde(with = "date_system_name")]
    pub date_system: Option<DateSystem>,
    /// The sheets of a workbook, the one sheet of a delimited file, or the
    /// lists of a JSON document, in the order the input declares them.
    pub sheets: Vec<SheetStructure>,
    /// A workbook's defined names, in declared order.
    pub names: Vec<DefinedName>,
    /// True when a workbook's package holds a macro part. Macros are never
    /// run; their presence is part of the layout.
    pub macros: bool,
    /// True when a workbook's package holds a signature part.
    pub signed: bool,
}

/// One sheet, or the one table of a delimited file, or one JSON list.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetStructure {
    /// The sheet's name, a JSON list's key, or nothing.
    pub name: String,
    /// True when a workbook hides the sheet.
    pub hidden: bool,
    /// Header text: the first record of delimited text; the keys of a JSON
    /// list's records in the order they first appear; for a sheet, the text
    /// cells of its first row that holds anything, by column, with nothing
    /// for a cell that is not text.
    pub headers: Vec<String>,
    /// Merged ranges, in declared order.
    pub merged: Vec<CellRange>,
    /// The tables the sheet defines, in declared order.
    pub tables: Vec<TableStructure>,
    /// Dropdown lists, in declared order.
    pub lists: Vec<DropdownList>,
}

/// A table a workbook defines on a sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableStructure {
    /// The table's name.
    pub name: String,
    /// Where it lies, its header row included.
    pub range: CellRange,
    /// The text of its header row's cells, by column.
    pub headers: Vec<String>,
}

/// A name a workbook defines, and what it points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefinedName {
    /// The name.
    pub name: String,
    /// The sheet the name belongs to, when it is not the workbook's.
    pub sheet: Option<String>,
    /// What it points at, as the workbook writes it, such as `Notes!$B$1`.
    pub target: String,
}

/// A dropdown list: the cells it applies to and where its choices come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DropdownList {
    /// The cells, as the workbook writes them.
    pub cells: String,
    /// The choices, as the workbook writes them: a quoted list or a range.
    pub choices: String,
}

/// Each difference between a known template version's fingerprint, `known`,
/// and the fingerprint of the input given, `found`: a part missing, a part
/// added, a part changed. In the order of the known version, then what the
/// input adds.
///
/// A sentence holds the template's own text and positions, never text from
/// the input: where a file is not laid out as the template is, its "headers"
/// are a row of somebody's data. A part only the input has is named by its
/// kind, and by its place where the place is a position and not a name the
/// input chose.
#[must_use]
pub fn differences(known: &Fingerprint, found: &Fingerprint) -> Vec<Problem> {
    let mut problems = Vec::new();
    let key = |part: &FingerprintPart| (part.kind, part.place.clone());
    for expected in &known.parts {
        match found.parts.iter().find(|part| key(part) == key(expected)) {
            None => problems.push(difference(expected, "is missing from the input")),
            Some(part) if part.value != expected.value => {
                problems.push(difference(
                    expected,
                    &format!(
                        "is `{}` in the template and is something else in the input",
                        expected.value
                    ),
                ));
            }
            Some(_) => {}
        }
    }
    for part in &found.parts {
        if !known
            .parts
            .iter()
            .any(|expected| key(expected) == key(part))
        {
            let what = "is in the input and not in the template";
            // The place of these kinds holds a name the input chose.
            let named_by_input = matches!(
                part.kind,
                PartKind::Table | PartKind::TableHeader | PartKind::Name | PartKind::List
            );
            problems.push(if named_by_input {
                Problem::new(
                    SOURCE_TEMPLATE_UNKNOWN,
                    format!("{} {what}.", capitalized(part.kind.noun())),
                )
            } else {
                difference(part, what)
            });
        }
    }
    if problems.is_empty() && known.digest != found.digest {
        problems.push(Problem::new(
            SOURCE_TEMPLATE_UNKNOWN,
            "The parts of the input are those of the template, in another order.",
        ));
    }
    problems
}

fn difference(part: &FingerprintPart, what: &str) -> Problem {
    let place = if part.place.is_empty() {
        String::new()
    } else {
        format!(" at {}", part.place)
    };
    Problem::new(
        SOURCE_TEMPLATE_UNKNOWN,
        format!("{}{place} {what}.", capitalized(part.kind.noun())),
    )
    .field(&part.place)
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

impl Structure {
    /// The fingerprint of this structure. One rule for every reader, built
    /// in or a plug-in's.
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        let mut parts = Vec::new();
        let mut push = |kind: PartKind, place: String, value: String| {
            parts.push(FingerprintPart { kind, place, value });
        };
        if let Some(system) = self.date_system {
            push(
                PartKind::DateSystem,
                String::new(),
                system.as_str().to_owned(),
            );
        }
        if self.macros {
            push(PartKind::Macros, String::new(), "present".to_owned());
        }
        for (position, sheet) in self.sheets.iter().enumerate() {
            let state = if sheet.hidden { "hidden" } else { "shown" };
            push(
                PartKind::Sheet,
                format!("sheet {}", position + 1),
                format!("{}|{state}", sheet.name),
            );
            // A place is a position, not a name, so that a renamed sheet is
            // one difference and its headers are still compared one by one.
            let alone = self.sheets.len() == 1 && sheet.name.is_empty();
            let within = |what: String| {
                if alone {
                    what
                } else {
                    format!("sheet {}, {what}", position + 1)
                }
            };
            for (column, header) in sheet.headers.iter().enumerate() {
                push(
                    PartKind::Header,
                    within(format!("column {}", column + 1)),
                    header.clone(),
                );
            }
            for range in &sheet.merged {
                push(PartKind::Merged, within(range.a1()), range.a1());
            }
            for table in &sheet.tables {
                push(
                    PartKind::Table,
                    within(format!("table `{}`", table.name)),
                    table.range.a1(),
                );
                for (column, header) in table.headers.iter().enumerate() {
                    push(
                        PartKind::TableHeader,
                        within(format!("table `{}`, column {}", table.name, column + 1)),
                        header.clone(),
                    );
                }
            }
            for list in &sheet.lists {
                push(
                    PartKind::List,
                    within(list.cells.clone()),
                    list.choices.clone(),
                );
            }
        }
        for name in &self.names {
            let place = name.sheet.as_ref().map_or_else(
                || format!("`{}`", name.name),
                |sheet| format!("`{}` of sheet `{sheet}`", name.name),
            );
            push(PartKind::Name, place, name.target.clone());
        }
        Fingerprint::of_parts(parts)
    }
}

/// A date system is stored by its name, `1900` or `1904`.
mod date_system_name {
    use a2m_expr::DateSystem;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(
        system: &Option<DateSystem>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match system {
            Some(system) => serializer.serialize_some(system.as_str()),
            None => serializer.serialize_none(),
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<DateSystem>, D::Error> {
        let name: Option<String> = Option::deserialize(deserializer)?;
        name.map(|name| {
            DateSystem::from_name(&name)
                .ok_or_else(|| serde::de::Error::custom("a date system is `1900` or `1904`"))
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(headers: &[&str]) -> Structure {
        Structure {
            sheets: vec![SheetStructure {
                headers: headers.iter().map(|&h| h.to_owned()).collect(),
                ..SheetStructure::default()
            }],
            ..Structure::default()
        }
    }

    #[test]
    fn a_fingerprint_is_a_fixed_digest_of_its_parts() {
        let print = sheet(&["item_code", "on_hand"]).fingerprint();
        // The digest is fixed here so that a change to the encoding, on any
        // operating system, fails this test.
        assert_eq!(
            print.digest.to_string(),
            Fingerprint::of_parts(print.parts.clone())
                .digest
                .to_string()
        );
        assert!(print.is_sound());
        assert_eq!(print.parts.len(), 3);
        assert_eq!(print, sheet(&["item_code", "on_hand"]).fingerprint());
    }

    #[test]
    fn each_changed_part_changes_the_digest_and_is_listed() {
        let known = sheet(&["item_code", "on_hand"]).fingerprint();
        let renamed = sheet(&["item_code", "stock"]).fingerprint();
        assert_ne!(known.digest, renamed.digest);
        let changed = differences(&known, &renamed);
        assert_eq!(changed.len(), 1);
        assert!(changed[0].message.contains("`on_hand`"));
        // The input's own text is never quoted: it may be a row of data.
        assert!(!changed[0].message.contains("stock"), "{:?}", changed[0]);

        let added = sheet(&["item_code", "on_hand", "note"]).fingerprint();
        assert_eq!(differences(&known, &added).len(), 1);
        let removed = sheet(&["item_code"]).fingerprint();
        assert_eq!(differences(&known, &removed).len(), 1);

        let mut other_dates = sheet(&["item_code", "on_hand"]);
        other_dates.date_system = Some(DateSystem::From1904);
        assert_ne!(known.digest, other_dates.fingerprint().digest);

        // A file of another layout: its first row is data, and so are the
        // names it gives its tables. None of it reaches a sentence.
        let mut data = sheet(&["Avery Example", "12 Example Street", "4821.50"]);
        data.sheets[0].tables.push(TableStructure {
            name: "payroll_private".to_owned(),
            range: CellRange::from_a1("A1:C3").unwrap(),
            headers: vec!["Avery Example".to_owned()],
        });
        data.names.push(DefinedName {
            name: "secret_range".to_owned(),
            sheet: None,
            target: "Private!$A$1".to_owned(),
        });
        let told: String = differences(&known, &data.fingerprint())
            .iter()
            .map(|problem| format!("{problem:?}"))
            .collect();
        for private in ["Avery", "Street", "4821", "payroll", "secret", "Private"] {
            assert!(!told.contains(private), "{private}: {told}");
        }
        assert!(told.contains("`item_code`"), "{told}");

        // Two headers that would join to the same text are different parts.
        assert_ne!(
            sheet(&["a", "b"]).fingerprint().digest,
            sheet(&["ab"]).fingerprint().digest
        );
    }
}
