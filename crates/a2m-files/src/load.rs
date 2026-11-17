//! The loader: a target folder, and only that folder, read into located
//! documents.

use std::fs;
use std::path::{Path, PathBuf};

use a2m_core::{Digest, FileKindName};

use crate::codes::{self, kernel};
use crate::diag::{Diagnostic, Diagnostics, Stage};
use crate::doc::{Doc, Located, Place, RelPath};
use crate::kernel::{FIXTURE, FIXTURE_MARKER, KERNEL_FILES, KERNEL_FOLDER_KINDS};
use crate::register::CompilerSet;
use crate::yaml;

/// How deep folders may be nested inside a target folder.
const DEEPEST_FOLDER: u32 = 16;

/// The folder that holds one target's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetRoot(PathBuf);

impl TargetRoot {
    /// The target folder at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// The folder's path on this machine. It never reaches a plan.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }
}

/// One file of a target, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// Its path inside the target folder.
    pub path: RelPath,
    /// Its file kind.
    pub kind: FileKindName,
    /// The digest of its content: for YAML, of the text with every line
    /// ending a line feed and no byte-order mark, so the digest is the same
    /// on every machine; for a fixture's other files, of the bytes.
    pub digest: Digest,
    /// Its documents. A fixture's recorded files have none.
    pub docs: Vec<Located<Doc>>,
}

/// A target folder, read. Every value keeps its place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sources {
    /// The folder the files came from.
    pub root: TargetRoot,
    /// The files, sorted by path.
    pub files: Vec<SourceFile>,
    /// The digest of the sorted list of paths and file digests.
    pub digest: Digest,
}

impl Sources {
    /// Sources from files already read, sorted and digested.
    #[must_use]
    pub fn from_files(root: TargetRoot, mut files: Vec<SourceFile>) -> Self {
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let mut listing = Vec::new();
        for file in &files {
            listing.extend_from_slice(file.path.as_str().as_bytes());
            listing.push(0);
            listing.extend_from_slice(file.digest.to_string().as_bytes());
            listing.push(b'\n');
        }
        Self {
            root,
            files,
            digest: Digest::of(&listing),
        }
    }

    /// The file at `path`.
    #[must_use]
    pub fn file(&self, path: &str) -> Option<&SourceFile> {
        self.files.iter().find(|file| file.path.as_str() == path)
    }

    /// The files of one kind, in order of path.
    pub fn of_kind<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a SourceFile> {
        self.files.iter().filter(move |file| file.kind.as_str() == kind)
    }

    /// The documents of one kind, in order of path and then of place.
    pub fn docs_of<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Located<Doc>> {
        self.of_kind(kind).flat_map(|file| file.docs.iter())
    }
}

/// The file kind a path belongs to, by the kernel's names and the folders
/// packs registered.
fn kind_of(path: &RelPath, set: &CompilerSet) -> Option<FileKindName> {
    let name = match path.folder() {
        None => KERNEL_FILES
            .iter()
            .find(|(file, _)| *file == path.as_str())
            .map(|(_, kind)| (*kind).to_owned()),
        Some(folder) => KERNEL_FOLDER_KINDS
            .iter()
            .find(|(known, _)| *known == folder)
            .map(|(_, kind)| (*kind).to_owned())
            .or_else(|| set.folders.get(folder).map(|kind| kind.as_str().to_owned())),
    }?;
    FileKindName::new(&name).ok()
}

fn unknown_file(path: &RelPath, set: &CompilerSet) -> Diagnostic {
    let mut known: Vec<String> = KERNEL_FILES.iter().map(|(file, _)| (*file).to_owned()).collect();
    known.extend(KERNEL_FOLDER_KINDS.iter().map(|(folder, _)| format!("{folder}/")));
    known.extend(set.folders.keys().map(|folder| format!("{folder}/")));
    Diagnostic::error(
        kernel(codes::UNKNOWN_FILE),
        Stage::Parse,
        &Place::start_of(path),
        format!("No file kind claims `{path}`: the kernel does not own it and no enabled pack registered its folder."),
        format!(
            "Move it to one of: {}; name it with the extension `.yaml`; or enable the pack that owns it.",
            known.join(", ")
        ),
    )
}

impl SourceFile {
    /// Reads one file from its bytes: finds its kind and parses it.
    ///
    /// # Errors
    ///
    /// A diagnostic when no kind claims the path, the bytes are not UTF-8,
    /// or the text is not YAML this compiler reads.
    pub fn read(path: RelPath, bytes: &[u8], set: &CompilerSet) -> Result<Self, Diagnostic> {
        let Some(kind) = kind_of(&path, set) else {
            return Err(unknown_file(&path, set));
        };
        let recorded = kind.as_str() == FIXTURE && path.file_name() != FIXTURE_MARKER;
        if recorded {
            return Ok(Self {
                path,
                kind,
                digest: Digest::of(bytes),
                docs: Vec::new(),
            });
        }
        if !path.file_name().ends_with(".yaml") {
            return Err(unknown_file(&path, set));
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Err(Diagnostic::error(
                kernel(codes::YAML_MALFORMED),
                Stage::Parse,
                &Place::start_of(&path),
                "The file is not UTF-8 text.",
                "Save the file as UTF-8.",
            ));
        };
        let docs = yaml::parse(&path, text)?;
        Ok(Self {
            digest: Digest::of(yaml::normalize_text(text).as_bytes()),
            path,
            kind,
            docs,
        })
    }
}

/// Reads files held in memory, such as the sources stored beside a
/// published plan.
///
/// # Errors
///
/// Every file that cannot be read, each with its place and a fix.
pub fn load_files(
    root: &TargetRoot,
    files: &[(RelPath, Vec<u8>)],
    set: &CompilerSet,
) -> Result<Sources, Diagnostics> {
    let mut read = Vec::new();
    let mut problems = Diagnostics::new();
    for (path, bytes) in files {
        match SourceFile::read(path.clone(), bytes, set) {
            Ok(file) => read.push(file),
            Err(problem) => problems.push(problem),
        }
    }
    if problems.is_empty() {
        Ok(Sources::from_files(root.clone(), read))
    } else {
        Err(problems)
    }
}

fn at_root(code: &'static str, message: String, fix: &str) -> Diagnostic {
    // The folder itself has no line; the place is the start of `target.yaml`.
    let place = Place::start_of(&RelPath::target_file());
    Diagnostic::error(kernel(code), Stage::Parse, &place, message, fix)
}

/// Reads the target folder at `root`: every file a kind claims, sorted by
/// path. Names that start with a dot are passed over.
///
/// # Errors
///
/// Diagnostics for everything that stops the folder being read: a link that
/// leaves the folder (`kernel.path_outside_target`), a file no kind claims,
/// a file that is not YAML, a missing `target.yaml`.
pub fn load(root: &TargetRoot, set: &CompilerSet) -> Result<Sources, Diagnostics> {
    let unreadable = |what: &str| {
        Diagnostics::from(at_root(
            codes::FILE_MISSING,
            format!("The target folder cannot be read: {what}."),
            "Point at a folder that holds a `target.yaml`.",
        ))
    };
    let Ok(real_root) = fs::canonicalize(root.path()) else {
        return Err(unreadable("it does not exist or cannot be opened"));
    };
    let mut found: Vec<(RelPath, PathBuf)> = Vec::new();
    let mut problems = Diagnostics::new();
    walk(&real_root, &real_root, None, 0, &mut found, &mut problems);
    found.sort_by(|a, b| a.0.cmp(&b.0));

    let mut files = Vec::new();
    for (path, on_disk) in found {
        match fs::read(&on_disk) {
            Ok(bytes) => match SourceFile::read(path, &bytes, set) {
                Ok(file) => files.push(file),
                Err(problem) => problems.push(problem),
            },
            Err(_) => problems.push(Diagnostic::error(
                kernel(codes::FILE_MISSING),
                Stage::Parse,
                &Place::start_of(&path),
                "The file cannot be read.",
                "Check that the file exists and may be read by this user.",
            )),
        }
    }
    if !files.iter().any(|file| file.path.as_str() == "target.yaml") && problems.is_empty() {
        return Err(unreadable("it has no `target.yaml`"));
    }
    if problems.is_empty() {
        Ok(Sources::from_files(root.clone(), files))
    } else {
        Err(problems)
    }
}

fn walk(
    real_root: &Path,
    folder: &Path,
    prefix: Option<&RelPath>,
    depth: u32,
    found: &mut Vec<(RelPath, PathBuf)>,
    problems: &mut Diagnostics,
) {
    let Ok(entries) = fs::read_dir(folder) else {
        problems.push(at_root(
            codes::FILE_MISSING,
            "A folder inside the target folder cannot be read.".to_owned(),
            "Check that the folder may be read by this user.",
        ));
        return;
    };
    let mut names: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|entry| (entry.file_name().to_string_lossy().into_owned(), entry.path()))
        .collect();
    names.sort();
    for (name, on_disk) in names {
        if name.starts_with('.') {
            continue;
        }
        let path = match prefix {
            Some(prefix) => prefix.join(&name),
            None => RelPath::new(&name),
        };
        let Ok(path) = path else {
            problems.push(at_root(
                codes::UNKNOWN_FILE,
                "A file in the target folder has a name that cannot be written as a path.".to_owned(),
                "Rename the file with letters, digits, dots, and underscores.",
            ));
            continue;
        };
        // A link may point anywhere; what it points at must be inside.
        let inside = fs::canonicalize(&on_disk).is_ok_and(|real| real.starts_with(real_root));
        if !inside {
            problems.push(Diagnostic::error(
                kernel(codes::PATH_OUTSIDE_TARGET),
                Stage::Parse,
                &Place::start_of(&path),
                format!("`{path}` is a link that leaves the target folder, or leads nowhere."),
                "Copy the file into the target folder, or remove the link.",
            ));
            continue;
        }
        if on_disk.is_dir() {
            if depth >= DEEPEST_FOLDER {
                problems.push(Diagnostic::error(
                    kernel(codes::UNKNOWN_FILE),
                    Stage::Parse,
                    &Place::start_of(&path),
                    format!("`{path}` is nested more than {DEEPEST_FOLDER} folders deep."),
                    "Move the files nearer the top of the target folder.",
                ));
                continue;
            }
            walk(real_root, &on_disk, Some(&path), depth + 1, found, problems);
        } else {
            found.push((path, on_disk));
        }
    }
}
