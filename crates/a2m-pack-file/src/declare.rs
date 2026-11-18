//! The file kinds `folders/` and `logs/` (decision 0246): a folder of a
//! surface read and written as a collection of entries, and a growing file
//! read from where the last read stopped. Each compiles into a section of
//! its own in the pack's part of the plan.

use a2m_core::{FileKindName, LintCode, PackPayload, SurfaceKind};
use a2m_files::{
    CompiledSection, DefinedName, Diagnostic, Diagnostics, Doc, ExprSite, FileKind, JsonSchema,
    KindContext, Located, Stage,
};
use serde::Serialize;

use crate::kind::document_name;
use crate::model::{
    FOLDER_KIND, FolderSection, FolderSpec, LOG_KIND, LogSection, LogSpec, SECTION_FORMAT,
    SURFACE_KIND,
};
use crate::place::is_one_name;

/// A folder or a growing file declared in a way the pack cannot carry out.
pub const FOLDER_INVALID: LintCode = LintCode::fixed("file.folder_invalid");

/// The verbs a folder may declare it is written by.
pub(crate) const FOLDER_WRITES: &[&str] = &["create", "update", "rename", "delete"];
/// The deepest a folder's list may reach.
pub(crate) const MOST_DEPTH: i64 = 4;
/// What a folder's declaration states when it says nothing else.
const ENTRIES: i64 = 1_000;
const DIGEST_BYTES: i64 = 1024 * 1024;
const CONTENT_BYTES: i64 = 64 * 1024;
/// What a growing file's declaration states when it says nothing else.
const LINES: i64 = 1_000;
const LOG_BYTES: i64 = 1024 * 1024;

/// The schema of one folder under `folders/`.
#[must_use]
pub fn folder_schema() -> JsonSchema {
    JsonSchema::object()
        .optional("surface", JsonSchema::surface())
        .optional("depth", JsonSchema::int_between(0, MOST_DEPTH))
        .optional("max_entries", JsonSchema::int_between(1, 100_000))
        .optional("digest_max_bytes", JsonSchema::int_between(0, 1 << 30))
        .optional("content_max_bytes", JsonSchema::int_between(1, 1 << 24))
        .optional("writes", JsonSchema::list(JsonSchema::words(FOLDER_WRITES)))
        .optional("may_be_open", JsonSchema::bool())
}

/// The schema of one growing file under `logs/`.
#[must_use]
pub fn log_schema() -> JsonSchema {
    JsonSchema::object()
        .optional("surface", JsonSchema::surface())
        .required("file", JsonSchema::text())
        .optional("encoding", JsonSchema::words(&["utf-8"]))
        .optional("max_lines", JsonSchema::int_between(1, 100_000))
        .optional("max_bytes", JsonSchema::int_between(1, 1 << 24))
}

fn invalid(at: &Located<Doc>, message: impl Into<String>, fix: &str) -> Diagnostic {
    Diagnostic::error(FOLDER_INVALID, Stage::Lower, &at.at, message, fix)
}

fn int(doc: &Located<Doc>, key: &str, default: i64) -> i64 {
    doc.get(key).and_then(Located::as_int).unwrap_or(default)
}

fn surface_name(doc: &Located<Doc>, cx: &KindContext<'_>, problems: &mut Diagnostics) -> String {
    match cx.surface_of(doc) {
        Some(surface) if *surface.kind == SURFACE_KIND => surface.name.as_str().to_owned(),
        _ => {
            problems.push(invalid(
                doc,
                "The file is on no surface of the kind `file`.",
                "Declare a surface of the kind `file` in `target.yaml`, and name it under `surface`.",
            ));
            String::new()
        }
    }
}

fn encode<T: Serialize>(
    section: &T,
    docs: &[Located<Doc>],
) -> Result<CompiledSection, Diagnostics> {
    PackPayload::encode(section)
        .map(|payload| CompiledSection {
            format: SECTION_FORMAT,
            payload,
        })
        .map_err(|failure| {
            let at = docs.first().map_or_else(
                || a2m_files::Place::start_of(&a2m_files::RelPath::target_file()),
                |doc| doc.at.clone(),
            );
            Diagnostics::from(Diagnostic::error(
                FOLDER_INVALID,
                Stage::Lower,
                &at,
                failure.message().to_owned(),
                "Keep the files to names, texts, and whole numbers.",
            ))
        })
}

fn one_name(doc: &Located<Doc>) -> Vec<Located<DefinedName>> {
    vec![Located::new(
        DefinedName {
            name: document_name(doc).to_owned(),
        },
        doc.at.clone(),
    )]
}

/// The `folder` file kind.
pub struct FolderKind {
    schema: JsonSchema,
    kinds: Vec<SurfaceKind>,
}

impl FolderKind {
    /// The file kind, for `file` surfaces.
    #[must_use]
    pub fn new() -> Self {
        Self {
            schema: folder_schema(),
            kinds: vec![SURFACE_KIND],
        }
    }
}

impl Default for FolderKind {
    fn default() -> Self {
        Self::new()
    }
}

fn folder_of(doc: &Located<Doc>, cx: &KindContext<'_>, problems: &mut Diagnostics) -> FolderSpec {
    let surface = surface_name(doc, cx, problems);
    let writes: Vec<String> = doc
        .get("writes")
        .map(Located::items)
        .unwrap_or_default()
        .iter()
        .filter_map(|item| item.as_text().map(str::to_owned))
        .collect();
    let depth = int(doc, "depth", 0);
    let content = int(doc, "content_max_bytes", CONTENT_BYTES);
    if writes.len()
        != writes
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    {
        problems.push(invalid(
            doc,
            "A verb is named twice under `writes`.",
            "Name each verb once.",
        ));
    }
    if !writes.is_empty() && doc.get("may_be_open").and_then(Located::as_bool) != Some(false) {
        problems.push(invalid(
            doc,
            "A folder whose entries are written says that no application holds them open: the pack has no sign per entry that one does.",
            "Add `may_be_open: false` when no application holds the folder's files open, or remove `writes`.",
        ));
    }
    FolderSpec {
        surface,
        depth: u8::try_from(depth).unwrap_or(0),
        max_entries: u32::try_from(int(doc, "max_entries", ENTRIES)).unwrap_or(1),
        digest_max_bytes: u64::try_from(int(doc, "digest_max_bytes", DIGEST_BYTES)).unwrap_or(0),
        content_max_bytes: u64::try_from(content).unwrap_or(1),
        writes,
        may_be_open: doc.get("may_be_open").and_then(Located::as_bool),
    }
}

impl FileKind for FolderKind {
    fn name(&self) -> FileKindName {
        FOLDER_KIND
    }

    fn surface_kinds(&self) -> &[SurfaceKind] {
        &self.kinds
    }

    fn folder(&self) -> &str {
        "folders"
    }

    fn schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn names(&self, doc: &Located<Doc>) -> Vec<Located<DefinedName>> {
        one_name(doc)
    }

    fn expressions(&self, _doc: &Located<Doc>) -> Vec<ExprSite> {
        Vec::new()
    }

    /// A folder describes a write when it names a verb under `writes` or
    /// states `may_be_open`, which only a write needs.
    fn writes(&self, doc: &Located<Doc>) -> bool {
        doc.get("writes")
            .is_some_and(|writes| !writes.items().is_empty())
            || doc.get("may_be_open").is_some()
    }

    fn compile(
        &self,
        docs: &[Located<Doc>],
        cx: &KindContext<'_>,
    ) -> Result<CompiledSection, Diagnostics> {
        let mut problems = Diagnostics::new();
        let mut section = FolderSection::new();
        for doc in docs {
            let spec = folder_of(doc, cx, &mut problems);
            section.insert(document_name(doc).to_owned(), spec);
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        encode(&section, docs)
    }
}

/// The `log` file kind.
pub struct LogKind {
    schema: JsonSchema,
    kinds: Vec<SurfaceKind>,
}

impl LogKind {
    /// The file kind, for `file` surfaces.
    #[must_use]
    pub fn new() -> Self {
        Self {
            schema: log_schema(),
            kinds: vec![SURFACE_KIND],
        }
    }
}

impl Default for LogKind {
    fn default() -> Self {
        Self::new()
    }
}

impl FileKind for LogKind {
    fn name(&self) -> FileKindName {
        LOG_KIND
    }

    fn surface_kinds(&self) -> &[SurfaceKind] {
        &self.kinds
    }

    fn folder(&self) -> &str {
        "logs"
    }

    fn schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn names(&self, doc: &Located<Doc>) -> Vec<Located<DefinedName>> {
        one_name(doc)
    }

    fn expressions(&self, _doc: &Located<Doc>) -> Vec<ExprSite> {
        Vec::new()
    }

    /// A growing file is only read.
    fn writes(&self, _doc: &Located<Doc>) -> bool {
        false
    }

    fn compile(
        &self,
        docs: &[Located<Doc>],
        cx: &KindContext<'_>,
    ) -> Result<CompiledSection, Diagnostics> {
        let mut problems = Diagnostics::new();
        let mut section = LogSection::new();
        for doc in docs {
            let surface = surface_name(doc, cx, &mut problems);
            let file_doc = doc.get("file").unwrap_or(doc);
            let file = file_doc.as_text().unwrap_or_default();
            if !is_one_name(file) {
                problems.push(invalid(
                    file_doc,
                    "A growing file is one name inside its surface's folder.",
                    "Write the file's name with no folder, no `..`, and no placeholder.",
                ));
            }
            section.insert(
                document_name(doc).to_owned(),
                LogSpec {
                    surface,
                    file: file.to_owned(),
                    max_lines: u32::try_from(int(doc, "max_lines", LINES)).unwrap_or(1),
                    max_bytes: u32::try_from(int(doc, "max_bytes", LOG_BYTES)).unwrap_or(1),
                },
            );
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        encode(&section, docs)
    }
}
