//! The loader: a target folder, and only that folder, read into located
//! documents.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use a2m_core::{Digest, FileKindName};

use crate::codes::{self, kernel};
use crate::diag::{Diagnostic, Diagnostics, Stage};
use crate::doc::{Doc, Located, Place, RelPath};
use crate::kernel::{FIXTURE, FIXTURE_MARKER, KERNEL_FILES, KERNEL_FOLDER_KINDS};
use crate::register::CompilerSet;
use crate::yaml;

/// The folder at the top of a target's folder that holds the drafts which
/// wait for an open question to be answered. The loader does not read it
/// (decision 0298).
pub const HELD_FOLDER: &str = "held";

/// How deep folders may be nested inside a target folder.
const DEEPEST_FOLDER: u32 = 16;
/// The most bytes a target file, which is YAML, may hold.
pub const LARGEST_TARGET_FILE: usize = 1024 * 1024;
/// The most bytes a fixture's recorded file may hold.
pub const LARGEST_RECORDED_FILE: usize = 16 * 1024 * 1024;

/// Whether the file at `path` is a fixture's recorded file, which is read
/// as bytes and digested, never parsed.
fn is_recorded(path: &RelPath) -> bool {
    path.folder() == Some("fixtures") && path.file_name() != FIXTURE_MARKER
}

/// The most bytes the file at `path` may hold.
fn largest(path: &RelPath) -> usize {
    if is_recorded(path) {
        LARGEST_RECORDED_FILE
    } else {
        LARGEST_TARGET_FILE
    }
}

fn too_large(path: &RelPath) -> Diagnostic {
    Diagnostic::error(
        kernel(codes::FILE_TOO_LARGE),
        Stage::Parse,
        &Place::start_of(path),
        format!(
            "`{path}` is larger than the {} bytes a file of its kind may hold.",
            largest(path)
        ),
        "Split the file, or keep a smaller recording in the fixture; a target file is at most 1 MiB and a fixture's recorded file at most 16 MiB.",
    )
}

/// The folder that holds one target's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetRoot(PathBuf);

impl TargetRoot {
    /// The target folder at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// The folder's path on this machine. It never reaches a compiled target.
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
        self.files
            .iter()
            .filter(move |file| file.kind.as_str() == kind)
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
    let mut known: Vec<String> = KERNEL_FILES
        .iter()
        .map(|(file, _)| (*file).to_owned())
        .collect();
    known.extend(
        KERNEL_FOLDER_KINDS
            .iter()
            .map(|(folder, _)| format!("{folder}/")),
    );
    known.extend(set.folders.keys().map(|folder| format!("{folder}/")));
    Diagnostic::error(
        kernel(codes::UNKNOWN_FILE),
        Stage::Parse,
        &Place::start_of(path),
        format!(
            "No file kind claims `{path}`: the kernel does not own it and no enabled pack registered its folder."
        ),
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
        if bytes.len() > largest(&path) {
            return Err(too_large(&path));
        }
        let recorded = kind.as_str() == FIXTURE && path.file_name() != FIXTURE_MARKER;
        if recorded {
            // A recording is digested, never parsed; where it is text, it
            // is still looked through for what is plainly a credential.
            if let Ok(text) = std::str::from_utf8(bytes)
                && let Some((line, what)) = crate::compile::recorded_credential(text)
            {
                return Err(Diagnostic::error(
                    kernel(codes::CREDENTIAL_IN_FILE),
                    Stage::Lint,
                    &Place {
                        file: path,
                        line,
                        column: 1,
                    },
                    format!(
                        "A fixture's recorded file holds what looks like a credential: {what}."
                    ),
                    "Scrub the recording: put a made-up value in its place. A fixture holds no secret, whatever its marker says.",
                ));
            }
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
/// published compiled target.
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
    let mut seen = BTreeSet::from([real_root.clone()]);
    walk(
        &real_root,
        &real_root,
        None,
        0,
        &mut seen,
        &mut found,
        &mut problems,
    );
    found.sort_by(|a, b| a.0.cmp(&b.0));

    let mut files = Vec::new();
    for (path, on_disk) in found {
        match read_at_most(&on_disk, largest(&path)) {
            Ok(None) => problems.push(too_large(&path)),
            Ok(Some(bytes)) => match SourceFile::read(path, &bytes, set) {
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

/// The bytes of the file at `path`, or nothing when it holds more than
/// `limit`. No more than one byte past the limit is ever read, so a file of
/// any size costs a bounded amount of memory.
fn read_at_most(path: &Path, limit: usize) -> std::io::Result<Option<Vec<u8>>> {
    let file = fs::File::open(path)?;
    let most = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::new();
    file.take(most).read_to_end(&mut bytes)?;
    Ok((bytes.len() <= limit).then_some(bytes))
}

fn walk(
    real_root: &Path,
    folder: &Path,
    prefix: Option<&RelPath>,
    depth: u32,
    seen: &mut BTreeSet<PathBuf>,
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
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                entry.path(),
            )
        })
        .collect();
    names.sort();
    for (name, on_disk) in names {
        if name.starts_with('.') {
            continue;
        }
        // The holding folder at the top is not part of the target: nothing
        // in it is read, digested, or compiled. A folder of that name
        // deeper down is an ordinary folder.
        if prefix.is_none()
            && name == HELD_FOLDER
            && fs::symlink_metadata(&on_disk).is_ok_and(|found| found.is_dir())
        {
            continue;
        }
        let path = match prefix {
            Some(prefix) => prefix.join(&name),
            None => RelPath::new(&name),
        };
        let Ok(path) = path else {
            problems.push(at_root(
                codes::UNKNOWN_FILE,
                "A file in the target folder has a name that cannot be written as a path."
                    .to_owned(),
                "Rename the file with letters, digits, dots, and underscores.",
            ));
            continue;
        };
        // A link may point anywhere; what it points at must be inside.
        let real = fs::canonicalize(&on_disk)
            .ok()
            .filter(|real| real.starts_with(real_root));
        let Some(real) = real else {
            problems.push(Diagnostic::error(
                kernel(codes::PATH_OUTSIDE_TARGET),
                Stage::Parse,
                &Place::start_of(&path),
                format!("`{path}` is a link that leaves the target folder, or leads nowhere."),
                "Copy the file into the target folder, or remove the link.",
            ));
            continue;
        };
        if on_disk.is_dir() {
            // A folder is read once, where it is. A link to a folder would
            // have it read again under another name, and a link to a folder
            // above itself would have it read without end.
            let linked =
                fs::symlink_metadata(&on_disk).map_or(true, |found| found.file_type().is_symlink());
            if linked || !seen.insert(real) {
                problems.push(Diagnostic::error(
                    kernel(codes::PATH_OUTSIDE_TARGET),
                    Stage::Parse,
                    &Place::start_of(&path),
                    format!("`{path}` is a link to a folder; a target's folders are read where they are, once."),
                    "Remove the link, and keep each file in one folder of the target.",
                ));
                continue;
            }
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
            walk(
                real_root,
                &on_disk,
                Some(&path),
                depth + 1,
                seen,
                found,
                problems,
            );
        } else {
            found.push((path, on_disk));
        }
    }
}

/// One open question a held draft holds, where it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldQuestion {
    /// The question, as the file holds it.
    pub text: String,
    /// Where it stands in the file.
    pub at: Place,
}

/// One draft in the holding folder, read for what it waits on. It is never
/// compiled; it is read only to be listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldFile {
    /// Its path inside the target folder, `held/` first.
    pub path: RelPath,
    /// The open questions it holds, in order of place.
    pub questions: Vec<HeldQuestion>,
    /// What the file names itself by: the text of the first entry of each
    /// of its documents, which a draft writes as `operation: name` or
    /// `endpoint: name`. Looked up to word a message; never a rule.
    pub names: Vec<String>,
    /// The findings it was drafted from, as the drafter's `drafted_from`
    /// lines state them: files of one finding share one.
    pub findings: Vec<String>,
}

fn questions_in(doc: &Located<Doc>, out: &mut Vec<HeldQuestion>) {
    match &doc.value {
        Doc::Map(entries) => {
            if let [only] = entries.as_slice()
                && only.key.value == crate::schema::OPEN_QUESTION
            {
                out.push(HeldQuestion {
                    text: only
                        .value
                        .as_text()
                        .unwrap_or("the system did not state this value")
                        .to_owned(),
                    at: only.key.at.clone(),
                });
                return;
            }
            for entry in entries {
                questions_in(&entry.value, out);
            }
        }
        Doc::List(items) => {
            for item in items {
                questions_in(item, out);
            }
        }
        _ => {}
    }
}

/// Reads one held draft's text, as the file at `path` (which has `held/`
/// first). `None` for text that is not YAML this compiler reads.
#[must_use]
pub fn read_held(path: &RelPath, text: &str) -> Option<HeldFile> {
    let docs = yaml::parse(path, text).ok()?;
    let mut questions = Vec::new();
    let mut names = Vec::new();
    for doc in &docs {
        questions_in(doc, &mut questions);
        if let Some(name) = doc
            .entries()
            .first()
            .and_then(|entry| entry.value.as_text())
        {
            names.push(name.to_owned());
        }
    }
    questions.sort_by_key(|question| (question.at.line, question.at.column));
    let findings = text
        .lines()
        .filter_map(|line| line.strip_prefix("# drafted_from: "))
        .map(|finding| finding.trim().to_owned())
        .collect();
    Some(HeldFile {
        path: path.clone(),
        questions,
        names,
        findings,
    })
}

/// The most held drafts `held_files` reads.
pub const MOST_HELD_FILES: usize = 5000;

/// The drafts in the target's holding folder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Held {
    /// The drafts, ordered by path.
    pub files: Vec<HeldFile>,
    /// Whether the list was cut at [`MOST_HELD_FILES`]: more files wait
    /// there than the list holds.
    pub cut: bool,
}

/// The drafts in the target's holding folder, ordered by path, at most
/// [`MOST_HELD_FILES`] of them. A link is never followed; a name that starts
/// with a dot, a file that is not YAML and one larger than a target file are
/// passed over. A target with no holding folder has none. Looks at the disk.
#[must_use]
pub fn held_files(root: &TargetRoot) -> Held {
    let mut held = Held::default();
    if let Ok(top) = RelPath::new(HELD_FOLDER) {
        walk_held(root.path(), &top, 0, &mut held);
    }
    held.files.sort_by(|a, b| a.path.cmp(&b.path));
    held
}

fn walk_held(root: &Path, at: &RelPath, depth: u32, out: &mut Held) {
    let folder = at.under(root);
    // A link is not a folder of the target.
    if !fs::symlink_metadata(&folder).is_ok_and(|found| found.is_dir()) {
        return;
    }
    let Ok(entries) = fs::read_dir(&folder) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        if out.cut {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(path) = at.join(&name) else { continue };
        let Ok(found) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if found.is_dir() {
            if depth < DEEPEST_FOLDER {
                walk_held(root, &path, depth + 1, out);
            }
        } else if found.is_file()
            && name.ends_with(".yaml")
            && let Ok(Some(bytes)) = read_at_most(&entry.path(), LARGEST_TARGET_FILE)
            && let Ok(text) = std::str::from_utf8(&bytes)
            && let Some(file) = read_held(&path, &yaml::normalize_text(text))
        {
            if out.files.len() >= MOST_HELD_FILES {
                out.cut = true;
                return;
            }
            out.files.push(file);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test: a helper that cannot do its work stops it"
    )]

    use a2m_core::Digest;

    use super::{SourceFile, Sources};
    use crate::doc::RelPath;
    use crate::testing::{call_kind, example_root, stand_set};
    use crate::{load, register::CompilerSet};

    fn file(path: &str) -> SourceFile {
        SourceFile {
            path: RelPath::new(path).unwrap(),
            kind: call_kind(),
            digest: Digest::of(path.as_bytes()),
            docs: Vec::new(),
        }
    }

    /// The listing the digest is of, written out here again from the paths
    /// as text with `/` between names.
    fn listing(files: &[SourceFile]) -> Vec<u8> {
        let mut out = Vec::new();
        for file in files {
            out.extend_from_slice(file.path.as_str().as_bytes());
            out.push(0);
            out.extend_from_slice(file.digest.to_string().as_bytes());
            out.push(b'\n');
        }
        out
    }

    #[test]
    fn the_sort_key_and_the_digest_use_slash_separated_relative_paths() {
        // A name whose first character sorts between `/` and `\`: sorted by
        // paths with `\` between names, `a0.yaml` would come before
        // `a/z.yaml`; with `/` it comes after.
        let sources = Sources::from_files(
            example_root(),
            vec![file("a0.yaml"), file("a/z.yaml"), file("a.yaml")],
        );
        let order: Vec<&str> = sources.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(order, ["a.yaml", "a/z.yaml", "a0.yaml"]);
        assert_eq!(sources.digest, Digest::of(&listing(&sources.files)));
        // A path with the other separator is not a path at all.
        assert!(RelPath::new("a\\z.yaml").is_err());
    }

    #[test]
    fn a_target_read_from_disk_is_listed_by_slash_paths_in_text_order() {
        let set: CompilerSet = stand_set().unwrap();
        let sources = load(&example_root(), &set).unwrap_or_else(|problems| panic!("{problems}"));
        assert!(sources.files.iter().any(|f| f.path.as_str().contains('/')));
        for file in &sources.files {
            assert!(!file.path.as_str().contains('\\'), "{}", file.path);
        }
        let paths: Vec<&str> = sources.files.iter().map(|f| f.path.as_str()).collect();
        let mut sorted = paths.clone();
        sorted.sort_unstable();
        assert_eq!(paths, sorted);
        assert_eq!(sources.digest, Digest::of(&listing(&sources.files)));
    }
}
