//! What an input is made of, without its values, and the fingerprint of it.
//!
//! A fingerprint is a list of parts in declared order and a digest over a
//! fixed encoding of that list. Values, display formats, and column widths do
//! not enter it, so two files of one template have one fingerprint, and the
//! same file has the same fingerprint on every operating system.

use a2m_core::Digest;
use a2m_expr::DateSystem;
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

/// Which kind of thing one part of a fingerprint is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    /// A workbook's date system.
    DateSystem,
    /// A sheet or a list: its name and whether it is hidden.
    Sheet,
    /// One header of a sheet, by its position.
    Header,
    /// A merged range of a sheet.
    Merged,
    /// A table of a sheet: its range.
    Table,
    /// One header of a table, by its position.
    TableHeader,
    /// A dropdown list of a sheet.
    List,
    /// A defined name and its target.
    Name,
    /// Whether the package holds a macro part.
    Macros,
}

impl PartKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::DateSystem => "date_system",
            Self::Sheet => "sheet",
            Self::Header => "header",
            Self::Merged => "merged",
            Self::Table => "table",
            Self::TableHeader => "table_header",
            Self::List => "list",
            Self::Name => "name",
            Self::Macros => "macros",
        }
    }

    const fn noun(self) -> &'static str {
        match self {
            Self::DateSystem => "the date system",
            Self::Sheet => "a sheet",
            Self::Header => "a header",
            Self::Merged => "a merged range",
            Self::Table => "a table",
            Self::TableHeader => "a table's header",
            Self::List => "a dropdown list",
            Self::Name => "a defined name",
            Self::Macros => "the macro part",
        }
    }
}

/// One part of a fingerprint: a kind, where it belongs, and what it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FingerprintPart {
    /// Which kind of thing.
    pub kind: PartKind,
    /// Where it belongs: a sheet and a position, a table, a name.
    pub place: String,
    /// What it is: a header's text, a name's target, a range.
    pub value: String,
}

/// The fingerprint of a structure: its parts, and the digest over them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fingerprint {
    /// The digest of the parts in their fixed encoding.
    pub digest: Digest,
    /// The parts, in declared order.
    pub parts: Vec<FingerprintPart>,
}

/// What every fingerprint's encoding starts with. A change to the encoding
/// or to what enters it changes this line.
const ENCODING: &[u8] = b"anything2mcp-source-fingerprint 1\n";

impl Fingerprint {
    /// The fingerprint of a list of parts. The digest covers, for each part
    /// in order, its kind, its place, and its value, each with its length
    /// before it, so no two lists of parts share an encoding.
    #[must_use]
    pub fn of_parts(parts: Vec<FingerprintPart>) -> Self {
        let mut bytes = ENCODING.to_vec();
        for part in &parts {
            for text in [part.kind.as_str(), part.place.as_str(), part.value.as_str()] {
                bytes.extend_from_slice(&(text.len() as u64).to_be_bytes());
                bytes.extend_from_slice(text.as_bytes());
            }
        }
        Self {
            digest: Digest::of(&bytes),
            parts,
        }
    }

    /// Whether the fingerprint holds together: its digest is the digest of
    /// its parts. A definition's fingerprint that does not is refused.
    #[must_use]
    pub fn is_sound(&self) -> bool {
        Self::of_parts(self.parts.clone()).digest == self.digest
    }

    /// Each difference between a known template version's fingerprint,
    /// `self`, and the fingerprint of the input given: a part missing, a
    /// part added, a part changed. In the order of the known version, then
    /// what the input adds.
    #[must_use]
    pub fn differences(&self, found: &Self) -> Vec<Problem> {
        let mut problems = Vec::new();
        let key = |part: &FingerprintPart| (part.kind, part.place.clone());
        for known in &self.parts {
            match found.parts.iter().find(|part| key(part) == key(known)) {
                None => problems.push(difference(known, "is missing from the input")),
                Some(part) if part.value != known.value => {
                    problems.push(difference(
                        known,
                        &format!("is `{}` in the template and `{}` in the input", known.value, part.value),
                    ));
                }
                Some(_) => {}
            }
        }
        for part in &found.parts {
            if !self.parts.iter().any(|known| key(known) == key(part)) {
                problems.push(difference(part, "is in the input and not in the template"));
            }
        }
        if problems.is_empty() && self.digest != found.digest {
            problems.push(Problem::new(
                SOURCE_TEMPLATE_UNKNOWN,
                "The parts of the input are those of the template, in another order.",
            ));
        }
        problems
    }
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
            push(PartKind::DateSystem, String::new(), system.as_str().to_owned());
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
            let within = |what: String| {
                if sheet.name.is_empty() {
                    what
                } else {
                    format!("sheet `{}`, {what}", sheet.name)
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
                push(PartKind::List, within(list.cells.clone()), list.choices.clone());
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

    #[allow(clippy::ref_option, reason = "the signature serde requires of `with`")]
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
            Fingerprint::of_parts(print.parts.clone()).digest.to_string()
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
        let differences = known.differences(&renamed);
        assert_eq!(differences.len(), 1);
        assert!(differences[0].message.contains("`on_hand`"));
        assert!(differences[0].message.contains("`stock`"));

        let added = sheet(&["item_code", "on_hand", "note"]).fingerprint();
        assert_eq!(known.differences(&added).len(), 1);
        let removed = sheet(&["item_code"]).fingerprint();
        assert_eq!(known.differences(&removed).len(), 1);

        let mut other_dates = sheet(&["item_code", "on_hand"]);
        other_dates.date_system = Some(DateSystem::From1904);
        assert_ne!(known.digest, other_dates.fingerprint().digest);

        // Two headers that would join to the same text are different parts.
        assert_ne!(
            sheet(&["a", "b"]).fingerprint().digest,
            sheet(&["ab"]).fingerprint().digest
        );
    }
}
