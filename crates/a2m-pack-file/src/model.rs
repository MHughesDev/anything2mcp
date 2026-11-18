//! What the pack compiles its files to, and what its steps carry. These are
//! the pack's own payloads: the kernel stores and hashes them and never
//! reads them. No field here can hold a path that leaves a folder: a file is
//! one name, and a place is a list of typed parts.

use std::collections::BTreeMap;

use a2m_core::{FileKindName, StepKind, SurfaceKind, Verb};
use serde::{Deserialize, Serialize};

/// The pack's name, as its manifest states it.
pub const PACK: &str = "file";
/// The mode the pack drives.
pub const MODE: &str = "file";
/// The version of the section payload's shape. Version 2 says whether a
/// document states `may_be_open`, and whether a field's text may begin as a
/// formula does (decision 0149).
pub const SECTION_FORMAT: u16 = 2;
/// The root a file surface is in when it names none.
pub const DEFAULT_ROOT: &str = "files";

/// The surface kind the pack serves.
pub const SURFACE_KIND: SurfaceKind = SurfaceKind::fixed("file");
/// The file kind the pack owns: the files under `documents/`.
pub const DOCUMENT_KIND: FileKindName = FileKindName::fixed("document");

/// Reads the file from disk, as a new read.
pub const OPEN: StepKind = StepKind::fixed("open");
/// Finds a named place in the parsed file.
pub const LOCATE: StepKind = StepKind::fixed("locate");
/// Builds the new bytes by patching the places named.
pub const SET: StepKind = StepKind::fixed("set");
/// Reads the values at the place found.
pub const READ: StepKind = StepKind::fixed("read");
/// Replaces the file with the new bytes, in one step.
pub const SAVE: StepKind = StepKind::fixed("save");

/// Reads one record by its key.
pub const VERB_READ: Verb = Verb::fixed("read");
/// Reads the records of a kind, up to a limit.
pub const VERB_LIST: Verb = Verb::fixed("list");
/// Sets fields of one record.
pub const VERB_UPDATE: Verb = Verb::fixed("update");
/// Adds one record.
pub const VERB_CREATE: Verb = Verb::fixed("create");
/// Removes one record.
pub const VERB_DELETE: Verb = Verb::fixed("delete");

/// The format of a file, as its document declares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// Delimited text, as in RFC 4180.
    Delimited,
    /// JSON, as in RFC 8259.
    Json,
    /// The ECMA-376 spreadsheet format.
    Workbook,
}

impl Format {
    /// The formats a document may name. The last four are not built.
    pub const NAMES: &'static [&'static str] = &[
        "delimited",
        "json",
        "workbook",
        "yaml",
        "toml",
        "xml",
        "text_document",
    ];

    /// The format of that name, when the pack reads it.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "delimited" => Some(Self::Delimited),
            "json" => Some(Self::Json),
            "workbook" => Some(Self::Workbook),
            _ => None,
        }
    }

    /// The name, as a document writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Delimited => "delimited",
            Self::Json => "json",
            Self::Workbook => "workbook",
        }
    }

    /// Whether the pack has a patch editor for the format.
    #[must_use]
    pub const fn writable(self) -> bool {
        matches!(self, Self::Delimited | Self::Json)
    }

    /// The name of the reader in `a2m-sources` that parses it.
    #[must_use]
    pub const fn reader(self) -> &'static str {
        match self {
            Self::Delimited => "csv",
            Self::Json => "json",
            Self::Workbook => "xlsx",
        }
    }
}

/// What identifies a file's layout. A list that is absent is not compared.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    /// A workbook's sheets, in order.
    pub sheets: Option<Vec<String>>,
    /// A workbook's tables, in order of sheet and then of declaration.
    pub tables: Option<Vec<String>>,
    /// A workbook's defined names, in order.
    pub names: Option<Vec<String>>,
    /// The header row of delimited text, in order.
    pub headers: Option<Vec<String>>,
    /// The keys of a JSON document's root record, in order.
    pub keys: Option<Vec<String>>,
}

/// Where a kind of record lives in its file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceSpec {
    /// Every row of delimited text under its header row.
    Rows,
    /// A table a workbook defines on a sheet.
    Table {
        /// The sheet.
        sheet: String,
        /// The table's name.
        name: String,
    },
    /// A sheet whose header row is found by its text.
    Sheet {
        /// The sheet.
        sheet: String,
    },
    /// One cell, by a defined name.
    Cell {
        /// The defined name.
        name: String,
    },
    /// A key path in a JSON document: to a record, or, with a key, to a
    /// list whose elements are the records.
    Path {
        /// The keys, outermost first. Each is one key, never split.
        keys: Vec<String>,
    },
}

/// One field of a record kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSpec {
    /// The field's name.
    pub name: String,
    /// The header text of its column, in a table, a sheet, or rows.
    pub header: Option<String>,
    /// Its key path inside the record, in a JSON document.
    pub path: Vec<String>,
    /// The position of its field type in the plan.
    pub field_type: u32,
    /// Whether a binding may set it.
    pub writable: bool,
    /// Whether the place holds a formula, read as its kept result.
    pub formula: bool,
    /// Whether text written here may begin with a character that makes a
    /// spreadsheet program run the text as a formula. Delimited text only.
    pub may_begin_formula: bool,
}

/// One kind of record in a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordSpec {
    /// Where it lives.
    pub place: PlaceSpec,
    /// The fields that identify one record. Empty for a single place.
    pub key: Vec<String>,
    /// Its fields, in declared order.
    pub fields: Vec<FieldSpec>,
}

impl RecordSpec {
    /// The field of that name.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&FieldSpec> {
        self.fields.iter().find(|field| field.name == name)
    }
}

/// One document: one file on a file surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentSpec {
    /// The surface the file is on.
    pub surface: String,
    /// The file's name inside the surface's folder.
    pub file: String,
    /// Its format.
    pub format: Format,
    /// The delimiter of delimited text.
    pub delimiter: Option<String>,
    /// The line ending a new row gets when the file shows none: `lf` or
    /// `crlf`.
    pub line_ending: Option<String>,
    /// What identifies its layout.
    pub layout: Layout,
    /// Whether an application may hold the file open. Nothing when the
    /// document does not say: then no write to the file compiles or runs.
    pub may_be_open: Option<bool>,
    /// The sibling file whose presence shows that one does.
    pub lock_sibling: Option<String>,
    /// Its record kinds, by name.
    pub records: BTreeMap<String, RecordSpec>,
}

/// What a file kind's section holds: the documents, by name.
pub type Section = BTreeMap<String, DocumentSpec>;

/// What a `locate` step expects to find.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Expect {
    /// The record with the key. A lookup may find none.
    Present,
    /// No record with the key: the check before a create.
    Absent,
    /// The whole record kind, for a list.
    All,
}

/// What a `set` step changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    /// Fields of the record found.
    Update,
    /// A new record.
    Create,
    /// The record found, removed.
    Delete,
}

/// What one step does. The step's `refers_to` names the document; its
/// arguments are `key.<field>` and `field.<field>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "do")]
pub enum StepSpec {
    /// Reads the file from disk.
    Open {
        /// Whether the binding goes on to write, so the lock signal counts.
        for_write: bool,
    },
    /// Finds a place.
    Locate {
        /// The record kind.
        record: String,
        /// What must be there.
        expect: Expect,
    },
    /// Builds the new bytes.
    Set {
        /// The record kind.
        record: String,
        /// What changes.
        change: Change,
    },
    /// Reads the record found.
    One {
        /// The record kind.
        record: String,
    },
    /// Reads the records of a kind, up to a limit.
    List {
        /// The record kind.
        record: String,
        /// The most rows returned.
        limit: u32,
    },
    /// Sees that the file at the path is the one saved.
    Confirm,
    /// Replaces the file.
    Save,
}

/// The name of the argument that carries a key field's value.
#[must_use]
pub fn key_arg(field: &str) -> String {
    format!("key.{field}")
}

/// The name of the argument that carries a field's new value.
#[must_use]
pub fn field_arg(field: &str) -> String {
    format!("field.{field}")
}

/// The digest of the file as `open` read it, among a step's values for
/// reconcile.
pub const FILE_DIGEST: &str = "file_digest";
/// The digest the file had before the write, among `set`'s values for
/// reconcile.
pub const BEFORE_DIGEST: &str = "before_digest";
/// The digest the file has once the write lands, among `set`'s values for
/// reconcile.
pub const INTENDED_DIGEST: &str = "intended_digest";
/// The name under which a list's rows are returned.
pub const ROWS: &str = "rows";
/// The name under which a list says that more rows exist than its limit.
pub const TRUNCATED: &str = "truncated";
