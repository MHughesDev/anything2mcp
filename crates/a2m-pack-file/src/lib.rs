//! The file pack: the `file` driver over files under declared roots, read and replaced whole.
//!
//! A file in a known format is the system being operated on (spec section
//! 308). The pack registers one mode, `file`, of the file access kind;
//! the surface kind `file`, a folder inside a named root; the file kinds
//! `documents/`, which names one file and its record kinds, `folders/`, and
//! `logs/`; a driver; a discoverer of file layouts, of a surface's folder,
//! and of growing files; and the rules that draft a document, a folder, a
//! growing file, and the reads over each from what it found.
//!
//! A file is read once, parsed through `a2m-sources` with the position of
//! every value, and written only by patching the bytes of the places
//! named, proving that nothing else changed, and replacing the old file
//! with the new one in one call of the operating system. Delimited text
//! and JSON are written; a workbook is read. No input chooses which file is
//! opened, and the pack resolves no path: a surface's folder is a
//! `CheckedPath` from the roots its manifest declares, and a file is one
//! checked name in it, taken as the name itself and never as what a link at
//! the name points to (decision 0149).
//!
//! The product crate registers the pack with
//! `RegistryBuilder::add_pack(&a2m_pack_file::pack())`.

pub mod codes;
mod declare;
mod discover;
mod draft;
mod draft_kinds;
mod drive_folder;
mod driver;
mod edit;
mod evidence;
mod folder;
mod kind;
mod log;
mod lower;
mod lower_folder;
mod model;
mod open;
mod place;
mod save;
mod settings;

use std::sync::Arc;

use a2m_core::{Failure, Mode};
use a2m_files::{JsonSchema, SurfaceKindSpec};
use a2m_provider::{CompilerRegistrations, Pack, PackContext, Registrar};

pub use declare::{FOLDER_INVALID, FolderKind, LogKind, folder_schema, log_schema};
pub use discover::{
    CellKind, ColumnFound, FILE_KEY_CANDIDATE, FILE_LAYOUT, KeyCandidateFound, LayoutFound,
    TableFound,
};
pub use drive_folder::SKIPPED;
pub use driver::{FileDriver, FileLimits};
pub use evidence::{
    EntryFound, EntryKindFound, FILE_FOLDER, FILE_LOG, FolderFound, LineForm, LogFound,
    SettingFound, ValueShape, Withheld,
};
pub use kind::{DOCUMENT_INVALID, DocumentKind, FORMAT_NOT_BUILT, document_schema};
pub use lower::{
    BENEATH_APPLICATION, BINDING_INVALID, FORMAT_READ_ONLY, OPEN_UNDECLARED, OPEN_WITHOUT_SIGNAL,
    STANDS_UNDECLARED,
};
pub use model::{
    BEFORE_DIGEST, DEFAULT_ROOT, DOCUMENT_KIND, ENTRY_CONTENT, ENTRY_NAME, FILE_DIGEST,
    FOLDER_KIND, Format, INTENDED_DIGEST, LOG_FROM, LOG_KIND, LOG_LINES, LOG_MORE, LOG_OUTCOME,
    LOG_POSITION, MODE, PACK, ROWS, SURFACE_KIND, TRUNCATED,
};

/// The pack's manifest, as the registry reads it.
pub const MANIFEST: &str = include_str!("../anything2mcp-provider.toml");

/// The file pack. It holds nothing: what it registers keeps what it needs.
#[derive(Debug, Clone, Copy, Default)]
pub struct FilePack;

/// The file pack, for `RegistryBuilder::add_pack`.
#[must_use]
pub const fn pack() -> FilePack {
    FilePack
}

impl Pack for FilePack {
    fn manifest(&self) -> &'static str {
        MANIFEST
    }

    fn register(&self, cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        let mode = Mode::new(MODE).map_err(|_| {
            a2m_provider::codes::row::MANIFEST_INVALID.failure(
                "The file pack's mode is not a valid name.",
                "Report this to the provider's author.",
            )
        })?;
        let limits = FileLimits::from_settings(cx.settings());
        to.driver(
            mode.clone(),
            Arc::new(driver::FileDriver::new(cx.roots().clone(), limits)),
        )?;
        let mut compiler = CompilerRegistrations::new()
            .surface_kind(SurfaceKindSpec::new(
                SURFACE_KIND,
                "folder",
                JsonSchema::object()
                    .required("folder", JsonSchema::text())
                    .optional("root", JsonSchema::name()),
            ))
            .file_kind(Arc::new(kind::DocumentKind::new()))
            .file_kind(Arc::new(declare::FolderKind::new()))
            .file_kind(Arc::new(declare::LogKind::new()))
            .lowering(Arc::new(lower::FileLowering::new(mode)))
            .drafting_rule(Arc::new(draft::DocumentFromLayout::new()?))
            .drafting_rule(Arc::new(draft::ReadsFromLayout::new()?))
            .drafting_rule(Arc::new(draft_kinds::FolderFromFinding::new()?))
            .drafting_rule(Arc::new(draft_kinds::FolderReads::new()?))
            .drafting_rule(Arc::new(draft_kinds::LogFromFinding::new()?))
            .drafting_rule(Arc::new(draft_kinds::LogReads::new()?));
        for rule in lower::lint_rules() {
            compiler = compiler.lint_rule(Arc::new(rule));
        }
        to.compiler(compiler)?;
        to.discoverer(
            Arc::new(discover::LayoutDiscoverer::new(
                cx.logs_and_traces().clock(),
                limits,
                cx.roots().clone(),
            )?),
            Arc::new(discover::LayoutDescriber),
        )
    }
}
