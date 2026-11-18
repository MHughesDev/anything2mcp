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
/// formula does (decision 0149). Version 3 adds settings files, their
/// dialect, and the sections of their fingerprint (decision 0246).
pub const SECTION_FORMAT: u16 = 3;
/// The root a file surface is in when it names none.
pub const DEFAULT_ROOT: &str = "files";

/// The surface kind the pack serves.
pub const SURFACE_KIND: SurfaceKind = SurfaceKind::fixed("file");
/// The file kind the pack owns: the files under `documents/`.
pub const DOCUMENT_KIND: FileKindName = FileKindName::fixed("document");
/// The file kind of a folder read and written as a collection of entries:
/// the files under `folders/` (decision 0246).
pub const FOLDER_KIND: FileKindName = FileKindName::fixed("folder");
/// The file kind of a growing file read from where the last read stopped:
/// the files under `logs/` (decision 0246).
pub const LOG_KIND: FileKindName = FileKindName::fixed("log");

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
/// Gives an entry of a folder another name in the same folder.
pub const VERB_RENAME: Verb = Verb::fixed("rename");

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
    /// A settings file of sections and keys (decision 0246).
    Ini,
    /// A property list in its XML form (decision 0246).
    PropertyList,
}

impl Format {
    /// The formats a document may name. The last four are not built.
    pub const NAMES: &'static [&'static str] = &[
        "delimited",
        "json",
        "workbook",
        "ini",
        "property_list",
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
            "ini" => Some(Self::Ini),
            "property_list" => Some(Self::PropertyList),
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
            Self::Ini => "ini",
            Self::PropertyList => "property_list",
        }
    }

    /// Whether the pack has a patch editor for the format: one that sets a
    /// value in place.
    #[must_use]
    pub const fn writable(self) -> bool {
        matches!(
            self,
            Self::Delimited | Self::Json | Self::Ini | Self::PropertyList
        )
    }

    /// Whether the editor also adds and removes records. A settings file's
    /// editor sets one value and adds or removes nothing (decision 0246).
    #[must_use]
    pub const fn adds_records(self) -> bool {
        matches!(self, Self::Delimited | Self::Json)
    }

    /// Whether the format is a settings file: named places of one value
    /// each, by a path of keys.
    #[must_use]
    pub const fn is_settings(self) -> bool {
        matches!(self, Self::Ini | Self::PropertyList)
    }

    /// The name of the reader in `a2m-sources` that parses it.
    #[must_use]
    pub const fn reader(self) -> &'static str {
        match self {
            Self::Delimited => "csv",
            Self::Json => "json",
            Self::Workbook => "xlsx",
            // Read by the pack's own readers in `settings`, never through
            // `a2m-sources`.
            Self::Ini => "ini",
            Self::PropertyList => "property_list",
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
    /// The keys of a JSON document's root record, or of a property list's
    /// root dictionary, in order.
    pub keys: Option<Vec<String>>,
    /// The section headers of an INI file, in order, each as often as it
    /// is written.
    pub sections: Option<Vec<String>>,
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
    /// list whose elements are the records. In a settings file, the keys
    /// to one record: for an INI file, no key (the keys before the first
    /// section) or one section; for a property list, the keys of nested
    /// dictionaries.
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
    /// The character between a key and its value in an INI file: `=` or
    /// `:`. Nothing means `=`.
    pub separator: Option<String>,
    /// What quotes around an INI value are: `literal`, part of the value;
    /// `strip`, around it. Nothing means not said, and a quoted value is
    /// then not read (decision 0246).
    pub quotes: Option<String>,
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
    /// Looks at a folder's entries, for a list or a look by name.
    FolderOpen {
        /// Whether the binding goes on to write.
        for_write: bool,
    },
    /// Finds one entry by its name, the argument `key.name`.
    FolderLocate {
        /// What must be there.
        expect: Expect,
    },
    /// Gives the entries, up to a limit.
    FolderList {
        /// The most entries returned.
        limit: u32,
        /// Whether each file's digest is given, under the folder's bound.
        digest: bool,
    },
    /// Gives the entry found.
    FolderRead {
        /// Whether its digest is given.
        digest: bool,
        /// Whether its content is given, as text.
        content: bool,
        /// Each value of the entry also given under the name of the input
        /// it was set from: `(value, input)`, for the read-back of a
        /// rename, whose new name is an input of another name.
        also: Vec<(String, String)>,
    },
    /// Builds the change to one entry.
    FolderSet {
        /// What changes.
        change: FolderChange,
    },
    /// Carries the change out: the commit point.
    FolderSave,
    /// Sees that the change landed as built.
    FolderConfirm,
    /// Reads the complete lines of a growing file after a position.
    LogRead,
}

/// What a `set` step of a folder changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderChange {
    /// A new file, where no entry has the name.
    Create,
    /// The content of the file found.
    Replace,
    /// The file found, under a new name in the same folder.
    Rename,
    /// The entry found, removed.
    Delete,
}

impl FolderChange {
    /// The verb that makes this change.
    #[must_use]
    pub const fn verb(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Replace => "update",
            Self::Rename => "rename",
            Self::Delete => "delete",
        }
    }
}

/// One folder of a surface, read and written as a collection of entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FolderSpec {
    /// The surface whose folder it is.
    pub surface: String,
    /// How many folders down a list reaches: 0 is the folder's own entries.
    pub depth: u8,
    /// The most entries one list gives.
    pub max_entries: u32,
    /// The largest file whose digest is given.
    pub digest_max_bytes: u64,
    /// The largest file whose content is read or written as text.
    pub content_max_bytes: u64,
    /// The verbs that may write: `create`, `update`, `rename`, `delete`.
    pub writes: Vec<String>,
    /// Whether an application may hold an entry open. Nothing when not said.
    pub may_be_open: Option<bool>,
}

/// What a file kind's section of folders holds: the folders, by name.
pub type FolderSection = BTreeMap<String, FolderSpec>;

/// One growing file of a surface, read from where the last read stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogSpec {
    /// The surface it is on.
    pub surface: String,
    /// Its name in the surface's folder.
    pub file: String,
    /// The most lines one read gives.
    pub max_lines: u32,
    /// The most bytes one read takes from the file.
    pub max_bytes: u32,
}

/// What a file kind's section of growing files holds: the files, by name.
pub type LogSection = BTreeMap<String, LogSpec>;

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

/// The argument of a folder's steps that carries the entry's name.
pub const ENTRY_NAME: &str = "name";
/// The argument of a folder's `set` that carries a file's new content.
pub const ENTRY_CONTENT: &str = "content";
/// The argument of a log's read that carries where the last read stopped.
pub const LOG_FROM: &str = "from";
/// The names under which a log's read gives its lines, the position to hand
/// back next time, what it found, and whether complete lines remain.
pub const LOG_LINES: &str = "lines";
/// See [`LOG_LINES`].
pub const LOG_POSITION: &str = "position";
/// See [`LOG_LINES`].
pub const LOG_OUTCOME: &str = "outcome";
/// See [`LOG_LINES`].
pub const LOG_MORE: &str = "more";
