//! Drafting a surface's folder and a growing file, by rule (decision 0255).
//!
//! From a `file_folder` finding: a `folders/` file named after the surface,
//! at the depth where discovery found entries, and beside it a `list` of
//! the folder and a `read` of one entry by its name. From a `file_log`
//! finding: a `logs/` file, and beside it a `read` from the position the
//! last read gave. Each compiles as drafted and reads only.
//!
//! Nothing is drafted for a write. A folder's file states no `writes` and
//! no `may_be_open`, so no create, update, rename, or delete compiles over
//! it until a person states them; a growing file is never written. An
//! entry's content is not drafted into the read: a person adds `content:
//! true` and its output where they mean the read to give it. A name an
//! entry has, and a line a growing file holds, are the system's words, and
//! are read as `untrusted_text`. A description holds the pack's own words
//! and its own spelling of a surface's or a file's name, nothing else.

use std::collections::BTreeSet;

use a2m_core::{Failure, FileKindName, Finding, FindingKind, OperationName};
use a2m_files::{DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath, UNTRUSTED_TEXT};

use crate::discover::invalid;
use crate::draft::name_from;
use crate::evidence::{FILE_FOLDER, FILE_LOG, FolderFound, LineForm, LogFound};
use crate::model::{FOLDER_KIND, LOG_KIND, MODE, TRUNCATED};

/// The kernel's file kind for operations.
const OPERATION: FileKindName = FileKindName::fixed("operation");

fn kind_of(name: &str) -> Result<FindingKind, Failure> {
    FindingKind::new(name).map_err(|_| invalid("its kind is not a name"))
}

fn folder_of(finding: &Finding) -> Result<FolderFound, Failure> {
    serde_json::from_str(finding.found())
        .map_err(|_| invalid("a `file_folder` finding does not hold a folder"))
}

fn log_of(finding: &Finding) -> Result<LogFound, Failure> {
    serde_json::from_str(finding.found())
        .map_err(|_| invalid("a `file_log` finding does not hold a growing file"))
}

/// An output of a type, a list of them where `list`.
fn output(ty: &str, list: bool) -> DraftValue {
    let mut output = vec![("type", DraftValue::text(ty))];
    if list {
        output.push(("list", DraftValue::Bool(true)));
    }
    DraftValue::map(output)
}

/// The name a folder's file is drafted under: its surface's, in the pack's
/// spelling. Nothing for a finding that names no surface.
fn folder_name(found: &FolderFound) -> Option<String> {
    found
        .surface
        .as_deref()
        .map(|surface| name_from(surface, &mut BTreeSet::new()))
}

/// The name a growing file's file is drafted under: the file's name without
/// its extension, in the pack's spelling.
fn log_name(found: &LogFound) -> String {
    let stem = found
        .file
        .rsplit_once('.')
        .map_or(found.file.as_str(), |(stem, _)| stem);
    name_from(stem, &mut BTreeSet::new())
}

/// From a `file_folder` finding to a `folders/` file.
pub struct FolderFromFinding {
    finding_kind: FindingKind,
}

impl FolderFromFinding {
    /// The rule.
    ///
    /// # Errors
    ///
    /// Never for the name this crate fixes; a failure of the name check.
    pub fn new() -> Result<Self, Failure> {
        Ok(Self {
            finding_kind: kind_of(FILE_FOLDER)?,
        })
    }
}

impl DraftingRule for FolderFromFinding {
    fn finding_kind(&self) -> FindingKind {
        self.finding_kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        FOLDER_KIND
    }

    /// The surface and the depth discovery found entries at. No `writes`
    /// and no `may_be_open`: nothing is drafted for a write.
    fn draft(&self, finding: &Finding, _cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let found = folder_of(finding)?;
        let (Some(name), Some(surface)) = (folder_name(&found), found.surface.as_deref()) else {
            return Ok(Vec::new());
        };
        let mut body = vec![("surface", DraftValue::text(surface))];
        let depth = found.deepest();
        if depth > 0 {
            body.push(("depth", DraftValue::Int(i64::from(depth))));
        }
        Ok(vec![DraftDoc {
            path: RelPath::new(&format!("folders/{name}.yaml"))?,
            body: DraftValue::map(body),
        }])
    }
}

/// From a `file_folder` finding to a `list` of the folder and a `read` of
/// one entry by its name.
pub struct FolderReads {
    finding_kind: FindingKind,
}

impl FolderReads {
    /// The rule.
    ///
    /// # Errors
    ///
    /// Never for the name this crate fixes; a failure of the name check.
    pub fn new() -> Result<Self, Failure> {
        Ok(Self {
            finding_kind: kind_of(FILE_FOLDER)?,
        })
    }
}

impl DraftingRule for FolderReads {
    fn finding_kind(&self) -> FindingKind {
        self.finding_kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        OPERATION
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let found = folder_of(finding)?;
        // A tool is named after its target; with none there is no name.
        let (Some(target), Some(name)) = (cx.target(), folder_name(&found)) else {
            return Ok(Vec::new());
        };
        let list_name = format!("{}_{name}_list", target.as_str());
        let get_name = format!("{}_{name}_get", target.as_str());
        if OperationName::new(&list_name).is_err() || OperationName::new(&get_name).is_err() {
            return Ok(Vec::new());
        }
        let below = if found.deepest() > 0 {
            " and of the folders below it to the depth its file declares"
        } else {
            ""
        };
        let cut = if found.cut.is_empty() {
            ""
        } else {
            " Discovery did not look below the depth it was bounded to; raise `depth` in the folder's file where the entries below are meant to be listed."
        };
        let fields = |list: bool| {
            vec![
                ("name".to_owned(), output(UNTRUSTED_TEXT, list)),
                ("kind".to_owned(), output("identifier", list)),
                ("size".to_owned(), output("integer", list)),
                ("modified".to_owned(), output("datetime", list)),
            ]
        };
        let mut list_outputs = fields(true);
        list_outputs.push((
            TRUNCATED.to_owned(),
            DraftValue::map(vec![("type", DraftValue::text("boolean"))]),
        ));
        let binding = |verb: &str| {
            vec![
                ("mode", DraftValue::text(MODE)),
                ("verb", DraftValue::text(verb)),
                ("folder", DraftValue::text(&name)),
            ]
        };
        let mut read_binding = binding("read");
        read_binding.push((
            "key",
            DraftValue::map(vec![("name", DraftValue::text("input.name"))]),
        ));
        Ok(vec![
            DraftDoc {
                path: RelPath::new(&format!("operations/{name}_list.yaml"))?,
                body: DraftValue::map(vec![
                    ("operation", DraftValue::text(&list_name)),
                    ("effect", DraftValue::text("read")),
                    ("inputs", DraftValue::Map(Vec::new())),
                    ("outputs", DraftValue::Map(list_outputs)),
                    (
                        "bindings",
                        DraftValue::List(vec![DraftValue::map(binding("list"))]),
                    ),
                    (
                        "description",
                        DraftValue::text(&format!(
                            "Lists the entries of the folder of the surface `{name}`{below}, in the order of their names, as many as the list limit allows: each entry's name as text marked untrusted, its kind, a file's size in bytes, and when it last changed, in UTC. No content is read, and no link is followed.{cut} Drafted by rule from the folder's entries; its name is provisional."
                        )),
                    ),
                ]),
            },
            DraftDoc {
                path: RelPath::new(&format!("operations/{name}_get.yaml"))?,
                body: DraftValue::map(vec![
                    ("operation", DraftValue::text(&get_name)),
                    ("effect", DraftValue::text("read")),
                    (
                        "inputs",
                        DraftValue::map(vec![(
                            "name",
                            DraftValue::map(vec![
                                ("type", DraftValue::text("file")),
                                (
                                    "description",
                                    DraftValue::text(
                                        "The entry's name inside the folder, as the list gives it",
                                    ),
                                ),
                            ]),
                        )]),
                    ),
                    ("outputs", DraftValue::Map(fields(false))),
                    (
                        "bindings",
                        DraftValue::List(vec![DraftValue::map(read_binding)]),
                    ),
                    (
                        "description",
                        DraftValue::text(&format!(
                            "Reads one entry of the folder of the surface `{name}` by its name: its kind, a file's size in bytes, and when it last changed, in UTC. Its content is not read until `content: true` is added to the binding. Drafted by rule from the folder's entries; its name is provisional."
                        )),
                    ),
                ]),
            },
        ])
    }
}

/// From a `file_log` finding to a `logs/` file.
pub struct LogFromFinding {
    finding_kind: FindingKind,
}

impl LogFromFinding {
    /// The rule.
    ///
    /// # Errors
    ///
    /// Never for the name this crate fixes; a failure of the name check.
    pub fn new() -> Result<Self, Failure> {
        Ok(Self {
            finding_kind: kind_of(FILE_LOG)?,
        })
    }
}

impl DraftingRule for LogFromFinding {
    fn finding_kind(&self) -> FindingKind {
        self.finding_kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        LOG_KIND
    }

    /// The surface and the file, with the default bounds of one read.
    fn draft(&self, finding: &Finding, _cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let found = log_of(finding)?;
        let Some(surface) = found.surface.as_deref() else {
            return Ok(Vec::new());
        };
        Ok(vec![DraftDoc {
            path: RelPath::new(&format!("logs/{}.yaml", log_name(&found)))?,
            body: DraftValue::map(vec![
                ("surface", DraftValue::text(surface)),
                ("file", DraftValue::text(&found.file)),
            ]),
        }])
    }
}

/// From a `file_log` finding to a `read` from the position the last read
/// gave.
pub struct LogReads {
    finding_kind: FindingKind,
}

impl LogReads {
    /// The rule.
    ///
    /// # Errors
    ///
    /// Never for the name this crate fixes; a failure of the name check.
    pub fn new() -> Result<Self, Failure> {
        Ok(Self {
            finding_kind: kind_of(FILE_LOG)?,
        })
    }
}

impl DraftingRule for LogReads {
    fn finding_kind(&self) -> FindingKind {
        self.finding_kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        OPERATION
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let found = log_of(finding)?;
        let Some(target) = cx.target() else {
            return Ok(Vec::new());
        };
        if found.surface.is_none() {
            return Ok(Vec::new());
        }
        let name = log_name(&found);
        let operation = format!("{}_{name}_read", target.as_str());
        if OperationName::new(&operation).is_err() {
            return Ok(Vec::new());
        }
        let form = match found.line_form {
            Some(LineForm::Delimited) => {
                " Its lines were seen to be fields apart by one delimiter; each is given whole."
            }
            Some(LineForm::KeyValue) => {
                " Its lines were seen to be words of the form key=value; each is given whole."
            }
            Some(LineForm::JsonPerLine) => {
                " Its lines were seen to be one JSON record each; each is given whole, as text."
            }
            None => "",
        };
        Ok(vec![DraftDoc {
            path: RelPath::new(&format!("operations/{name}_read.yaml"))?,
            body: DraftValue::map(vec![
                ("operation", DraftValue::text(&operation)),
                ("effect", DraftValue::text("read")),
                (
                    "inputs",
                    DraftValue::map(vec![(
                        "position",
                        DraftValue::map(vec![
                            ("type", DraftValue::text("identifier")),
                            ("required", DraftValue::Bool(false)),
                            (
                                "description",
                                DraftValue::text(
                                    "The position the last read gave; none to read from the start",
                                ),
                            ),
                        ]),
                    )]),
                ),
                (
                    "outputs",
                    DraftValue::map(vec![
                        ("lines", output(UNTRUSTED_TEXT, true)),
                        ("position", output("identifier", false)),
                        ("outcome", output("identifier", false)),
                        ("more", output("boolean", false)),
                    ]),
                ),
                (
                    "bindings",
                    DraftValue::List(vec![DraftValue::map(vec![
                        ("mode", DraftValue::text(MODE)),
                        ("verb", DraftValue::text("read")),
                        ("log", DraftValue::text(&name)),
                        ("from", DraftValue::text("input.position")),
                    ])]),
                ),
                (
                    "description",
                    DraftValue::text(&format!(
                        "Reads the complete lines added to the growing file `{name}` since the position given, within the bounds its file declares, each as text marked untrusted, and the position to hand to the next read. A file replaced or cut short is told by its outcome, and nothing is read from it.{form} Drafted by rule from the file's name and its first lines; its name is provisional."
                    )),
                ),
            ]),
        }])
    }
}
