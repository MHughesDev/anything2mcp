//! The compiled source definition: what `sources/*.yaml` compiles to, beside
//! the compiled target's own `CompiledSource`, and what the source readers
//! load.
//!
//! The compiler writes a [`SourceSpec`] into the payload of
//! `a2m_core::CompiledSource`; `a2m-sources` reads it back. The types live here,
//! with the file formats, so the two crates share one definition and not
//! only a shape (decision 0087, and decision 0079 for where they live).

use a2m_core::{Digest, FieldTypeIndex};
use serde::{Deserialize, Serialize};

/// The reader name of a source whose rows are another operation's result:
/// no reader is registered under it.
pub const OPERATION_READER: &str = "operation";

/// Where the data lies in an input. Always a name or header text, never a
/// coordinate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Locate {
    /// The whole document: a delimited file, or the list at the root of a
    /// JSON document. A workbook's data is never found this way.
    Document,
    /// A table or a defined name of a workbook, whose first row is the
    /// header row; or the list under that key of a JSON document.
    Named(String),
    /// The sheet with that name. The header row is the first row that holds
    /// every header the definition names.
    Sheet(String),
}

/// What is done with a hidden row or column inside the located data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HiddenRule {
    /// A hidden cell in the data is a problem: nobody saw what it holds.
    #[default]
    Reject,
    /// A hidden cell is read like any other.
    Include,
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
    /// The name of every kind, as a source file and a payload write it.
    pub const NAMES: &'static [&'static str] = &[
        "date_system",
        "sheet",
        "header",
        "merged",
        "table",
        "table_header",
        "list",
        "name",
        "macros",
    ];

    /// The kind's name, as a source file and a payload write it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
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

    /// The kind, as a sentence names it.
    #[must_use]
    pub const fn noun(self) -> &'static str {
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
    /// its parts.
    #[must_use]
    pub fn is_sound(&self) -> bool {
        Self::of_parts(self.parts.clone()).digest == self.digest
    }
}

/// One version of a template: its name, and the fingerprint a file of that
/// version has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateVersion {
    /// The version, as the source file states it.
    pub version: String,
    /// The fingerprint, with its parts, so that a file that matches no
    /// version can be told how it differs.
    pub fingerprint: Fingerprint,
}

/// One row field: its name, its field type, and where its value comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowField {
    /// The row field's name.
    pub name: String,
    /// Its field type, by which its value is typed. Every row field has
    /// one, so every row field has a type before any file is read.
    pub field_type: FieldTypeIndex,
    /// True when a row without a value for it is a problem.
    pub required: bool,
    /// The header of the column it is read from. When absent, the value is
    /// the result of the expression of the same name in the compiled target's
    /// mapping.
    pub column: Option<String>,
}

/// What a source definition tells its reader about the format's dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderOptions {
    /// For delimited text: the character between fields. A comma when absent.
    /// One ASCII character that is not a quote or a line break.
    pub delimiter: Option<char>,
}

/// What a source definition says beyond the compiled target's own fields: the
/// payload of `a2m_core::CompiledSource`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpec {
    /// The template versions the definition knows. Empty only for a source
    /// whose reader is `operation`.
    pub versions: Vec<TemplateVersion>,
    /// Where the data lies.
    pub locate: Locate,
    /// The header text of every column the definition reads, each once. A
    /// column in the data that is not listed here is a problem.
    pub columns: Vec<String>,
    /// The row fields, in declared order.
    pub fields: Vec<RowField>,
    /// The row fields whose values, together, identify a row. Two rows with
    /// the same values are a problem. Empty when rows have no key.
    pub key: Vec<String>,
    /// The texts that count as no value. Empty unless the source file lists
    /// some: text such as `NA` is text.
    pub missing: Vec<String>,
    /// What is done with hidden rows and columns.
    pub hidden: HiddenRule,
    /// The dialect the reader is told.
    pub options: ReaderOptions,
}

#[cfg(test)]
mod tests {
    use super::{Fingerprint, FingerprintPart, PartKind};

    #[test]
    fn every_part_kind_is_named_as_it_is_written_and_a_fingerprint_is_the_digest_of_its_parts() {
        let all = [
            PartKind::DateSystem,
            PartKind::Sheet,
            PartKind::Header,
            PartKind::Merged,
            PartKind::Table,
            PartKind::TableHeader,
            PartKind::List,
            PartKind::Name,
            PartKind::Macros,
        ];
        let names: Vec<&str> = all.iter().map(|kind| kind.as_str()).collect();
        assert_eq!(names, PartKind::NAMES);
        for kind in all {
            let written = serde_json::to_value(kind).unwrap();
            assert_eq!(written, kind.as_str());
        }
        let part = |kind, place: &str, value: &str| FingerprintPart {
            kind,
            place: place.to_owned(),
            value: value.to_owned(),
        };
        let fingerprint = Fingerprint::of_parts(vec![
            part(PartKind::Sheet, "sheet 1", "|shown"),
            part(PartKind::Header, "column 1", "Code"),
            part(PartKind::Header, "column 2", "Length"),
        ]);
        // The encoding is fixed: a change here is a change of every stored template.
        assert_eq!(
            fingerprint.digest.to_string(),
            "7c00941f8d452ede3c7f4908015699942a87aecdd932649718d72bf6851999c7"
        );
        assert!(fingerprint.is_sound());
        let mut other = fingerprint.clone();
        other.parts[1].value = "Item".to_owned();
        assert!(!other.is_sound());
    }
}
