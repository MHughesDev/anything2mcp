//! The harness: gives a test a fresh folder of seed files with one call, and
//! answers, from the bytes on disk, what is stored there now.
//!
//! A test never proves a write using Anything2MCP's own read path alone. It asks here.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, Metadata, Permissions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::control::{
    EntryKind, FileTruth, Ledger, Observation, Seen, WriteCount, identity_and_links, sha256_hex,
};
use crate::faults::{Change, Fault, SaveStyle};
use crate::package::{self, PackagePart};
use crate::reader::{self, JsonStep, Unreadable};
use crate::seed::{self, Edit, Known};

/// Why the stand could not do what a test asked.
#[derive(Debug)]
pub enum StandError {
    /// The operating system refused something the stand itself tried.
    Io {
        /// What the stand was doing.
        doing: &'static str,
        /// The path it was doing it to.
        path: PathBuf,
        /// What the operating system said.
        source: io::Error,
    },
    /// The name is not a path below the root: it is empty, absolute, or has a
    /// part that is empty, `.`, `..`, or holds `\` or `:`.
    BadName(String),
    /// The file the question or the fault is about is not there.
    Absent(String),
    /// The stand's own reader could not read the file.
    Unreadable {
        /// The file's name below the root.
        name: String,
        /// What is wrong with it.
        why: Unreadable,
    },
    /// The header row has no column with this text, or a matching row is too short.
    NoSuchColumn {
        /// The file's name below the root.
        name: String,
        /// The header that was asked for.
        header: String,
    },
    /// The stand knows no change of this kind for this file.
    NoKnownChange(String),
    /// The bytes another writer was to change do not occur in the file.
    BytesNotFound(String),
    /// The fault cannot be shown here: by this operating system, or to this user.
    Unsupported(&'static str),
}

impl StandError {
    /// Whether a fault was refused because it cannot be shown here. A test of
    /// such a fault ends early instead of failing.
    #[must_use]
    pub fn is_unsupported(&self) -> bool {
        matches!(self, Self::Unsupported(_))
    }
}

impl fmt::Display for StandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                doing,
                path,
                source,
            } => write!(
                f,
                "the stand could not {doing} at {}: {source}",
                path.display()
            ),
            Self::BadName(name) => write!(f, "{name:?} is not a name below the stand's root"),
            Self::Absent(name) => write!(f, "{name} is not there"),
            Self::Unreadable { name, why } => {
                write!(f, "the stand's reader cannot read {name}: {why}")
            }
            Self::NoSuchColumn { name, header } => {
                write!(f, "{name} has no column {header:?} in the row asked for")
            }
            Self::NoKnownChange(name) => {
                write!(f, "the stand knows no change of that kind for {name}")
            }
            Self::BytesNotFound(name) => {
                write!(f, "the bytes to be changed do not occur in {name}")
            }
            Self::Unsupported(why) => write!(f, "this fault cannot be shown here: {why}"),
        }
    }
}

impl std::error::Error for StandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn io_at<T>(doing: &'static str, path: &Path, result: io::Result<T>) -> Result<T, StandError> {
    result.map_err(|source| StandError::Io {
        doing,
        path: path.to_path_buf(),
        source,
    })
}

/// A path text that tries to leave the root. The stand resolves none of them; a
/// test hands each to whatever takes a file's name and expects a refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EscapeAttempt {
    /// How the text tries to leave.
    pub how: &'static str,
    /// The text.
    pub text: String,
}

/// What the stand holds besides the files.
#[derive(Debug, Default)]
struct State {
    /// The faults set since the last clear or reset, oldest first.
    faults: Vec<Fault>,
    /// Names below the root that a fault made, so they are not taken for strays.
    planted: BTreeSet<String>,
    /// Files held open without sharing, by name.
    held: Vec<(String, File)>,
    /// What to put back when the standing faults end, oldest first.
    restore: Vec<(PathBuf, Permissions)>,
    ledger: Ledger,
}

/// One folder of seed files in the system's temporary folder, removed when dropped.
///
/// Two stands share nothing. The stand is plain blocking code with no runtime of
/// its own; its files are small, and a test may call it from anywhere.
#[derive(Debug)]
pub struct FileStand {
    home: PathBuf,
    root: PathBuf,
    outside: PathBuf,
    state: Mutex<State>,
}

/// Counts the stands one process starts, so each gets its own folder.
static NEXT_STAND: AtomicU64 = AtomicU64::new(0);

impl FileStand {
    /// Makes a new folder, writes every seed file into it, and starts the ledger.
    ///
    /// # Errors
    ///
    /// Returns an error if the folder or a seed file could not be made.
    pub fn start() -> Result<Self, StandError> {
        let parent = std::env::temp_dir();
        let process = std::process::id();
        let home = loop {
            let number = NEXT_STAND.fetch_add(1, Ordering::Relaxed);
            let candidate = parent.join(format!("example-files-{process}-{number}"));
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                // Left behind by an earlier process with the same number.
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists && number < 10_000 => {}
                Err(e) => return io_at("make its folder", &candidate, Err(e)),
            }
        };
        // Links in the path are resolved once, here, so that the root a test is
        // given is the path the operating system itself would report.
        let home = match fs::canonicalize(&home) {
            Ok(resolved) => resolved,
            Err(e) => {
                let _ = fs::remove_dir_all(&home);
                return io_at("resolve its folder", &home, Err(e));
            }
        };
        let stand = Self {
            root: home.join("root"),
            outside: home.join("outside"),
            home,
            state: Mutex::new(State::default()),
        };
        stand.reset()?;
        Ok(stand)
    }

    /// The folder the seed files are in, with every link in its path resolved.
    /// This is the allowed root a test gives to what it tests. On Windows it is
    /// in the long form that begins `\\?\`.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A folder beside the root, outside it. It holds one file, and nothing under
    /// test may read or change anything in it.
    #[must_use]
    pub fn outside_folder(&self) -> &Path {
        &self.outside
    }

    /// The real path of a name below the root. The name uses `/` between its
    /// parts on every operating system.
    ///
    /// # Errors
    ///
    /// Returns [`StandError::BadName`] for a name that is not below the root.
    pub fn path(&self, name: &str) -> Result<PathBuf, StandError> {
        let bad = || StandError::BadName(name.to_owned());
        if name.is_empty() {
            return Err(bad());
        }
        let mut path = self.root.clone();
        for part in name.split('/') {
            if part.is_empty() || part == "." || part == ".." || part.contains(['\\', ':', '\0']) {
                return Err(bad());
            }
            path.push(part);
        }
        Ok(path)
    }

    /// Restores the seed files, ends every fault, and starts the ledger again.
    ///
    /// # Errors
    ///
    /// Returns an error if a file could not be removed or written.
    pub fn reset(&self) -> Result<(), StandError> {
        let mut state = self.lock();
        *state = State::default();
        make_removable(&self.home);
        for folder in [&self.root, &self.outside] {
            io_at("make a folder", folder, fs::create_dir_all(folder))?;
            empty_folder(folder)?;
        }
        for file in seed::files() {
            let path = self.path(file.name)?;
            if let Some(parent) = path.parent() {
                io_at("make a folder", parent, fs::create_dir_all(parent))?;
            }
            io_at("write a seed file", &path, fs::write(&path, &file.bytes))?;
        }
        let outside = self.outside.join(seed::OUTSIDE_FILE);
        io_at(
            "write the outside file",
            &outside,
            fs::write(&outside, seed::outside_file()),
        )?;
        state.ledger = Ledger::starting_from(self.look(&state)?);
        Ok(())
    }

    // ---- Inspect -----------------------------------------------------------

    /// The truth about one name below the root, read from disk now.
    ///
    /// # Errors
    ///
    /// Returns an error if the name is bad or the entry could not be read.
    pub fn inspect(&self, name: &str) -> Result<FileTruth, StandError> {
        let state = self.lock();
        self.truth(&state, name)
    }

    /// The bytes of a regular file, or `None` if no regular file has that name.
    /// A link is not followed.
    ///
    /// # Errors
    ///
    /// Returns an error if the name is bad or the file could not be read.
    pub fn bytes(&self, name: &str) -> Result<Option<Vec<u8>>, StandError> {
        let state = self.lock();
        self.read(&state, name)
    }

    /// The SHA-256 digest of a regular file, as 64 lowercase hexadecimal
    /// characters, or `None` if no regular file has that name.
    ///
    /// # Errors
    ///
    /// Returns an error if the name is bad or the file could not be read.
    pub fn sha256(&self, name: &str) -> Result<Option<String>, StandError> {
        Ok(self.bytes(name)?.map(|bytes| sha256_hex(&bytes)))
    }

    /// Whether anything at all sits at the name.
    ///
    /// # Errors
    ///
    /// Returns an error if the name is bad or the entry could not be read.
    pub fn exists(&self, name: &str) -> Result<bool, StandError> {
        Ok(self.inspect(name)?.exists())
    }

    /// Every file and link below the root, by name, in order. Folders are not listed.
    ///
    /// # Errors
    ///
    /// Returns an error if a folder could not be listed.
    pub fn listing(&self) -> Result<Vec<String>, StandError> {
        let mut entries = Vec::new();
        walk(&self.root, "", &mut entries)?;
        let mut names: Vec<String> = entries
            .into_iter()
            .filter(|(_, kind)| *kind != EntryKind::Folder)
            .map(|(name, _)| name)
            .collect();
        names.sort();
        Ok(names)
    }

    /// Every file and link below the root that is neither a seed file nor made by
    /// a fault: what the thing under test left behind, such as a temporary file.
    ///
    /// # Errors
    ///
    /// Returns an error if a folder could not be listed.
    pub fn strays(&self) -> Result<Vec<String>, StandError> {
        let seeded: BTreeSet<&str> = seed::files().iter().map(|f| f.name).collect();
        let listing = self.listing()?;
        let state = self.lock();
        Ok(listing
            .into_iter()
            .filter(|name| !seeded.contains(name.as_str()) && !state.planted.contains(name))
            .collect())
    }

    /// Whether the folder outside the root still holds exactly its one file with
    /// exactly its seed bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the folder could not be read.
    pub fn outside_untouched(&self) -> Result<bool, StandError> {
        let mut entries = Vec::new();
        walk(&self.outside, "", &mut entries)?;
        if entries != [(seed::OUTSIDE_FILE.to_owned(), EntryKind::File)] {
            return Ok(false);
        }
        let path = self.outside.join(seed::OUTSIDE_FILE);
        let bytes = io_at("read the outside file", &path, fs::read(&path))?;
        Ok(bytes == seed::outside_file())
    }

    /// Texts that try to name the file outside the root. Each must be refused.
    #[must_use]
    pub fn escape_attempts(&self) -> Vec<EscapeAttempt> {
        let outside = self.outside.join(seed::OUTSIDE_FILE);
        let attempt = |how, text: String| EscapeAttempt { how, text };
        vec![
            attempt(
                "a parent step from the root",
                format!("../outside/{}", seed::OUTSIDE_FILE),
            ),
            attempt(
                "parent steps from inside a folder",
                format!("prices/../../outside/{}", seed::OUTSIDE_FILE),
            ),
            attempt(
                "parent steps written with the other separator",
                format!("prices\\..\\..\\outside\\{}", seed::OUTSIDE_FILE),
            ),
            attempt("an absolute path", outside.to_string_lossy().into_owned()),
        ]
    }

    // ---- Values, through the stand's own readers ---------------------------

    /// The rows of a delimited file, read by the stand's own reader.
    ///
    /// # Errors
    ///
    /// Returns an error if the file is absent or is not valid delimited text.
    pub fn stored_rows(&self, name: &str, delimiter: char) -> Result<Vec<Vec<String>>, StandError> {
        let bytes = self.present(name)?;
        reader::delimited_rows(&bytes, delimiter).map_err(|why| StandError::Unreadable {
            name: name.to_owned(),
            why,
        })
    }

    /// The text under `header` in every row whose `key_header` holds `key`, in
    /// file order. One row gives one text; no row gives none; two rows that share
    /// the key give two.
    ///
    /// # Errors
    ///
    /// Returns an error if the file is absent, is not valid delimited text, or
    /// has no such column.
    pub fn stored_cells(
        &self,
        name: &str,
        delimiter: char,
        key_header: &str,
        key: &str,
        header: &str,
    ) -> Result<Vec<String>, StandError> {
        let rows = self.stored_rows(name, delimiter)?;
        let missing = |header: &str| StandError::NoSuchColumn {
            name: name.to_owned(),
            header: header.to_owned(),
        };
        let mut rows = rows.into_iter();
        let head = rows.next().unwrap_or_default();
        let column = |text: &str| head.iter().position(|h| h == text).ok_or(missing(text));
        let key_column = column(key_header)?;
        let value_column = column(header)?;
        rows.filter(|row| row.get(key_column).is_some_and(|k| k == key))
            .map(|row| row.get(value_column).cloned().ok_or(missing(header)))
            .collect()
    }

    /// Every value a path reaches in a JSON file, as text, read by the stand's
    /// own reader. See [`reader::json_values`] for what the text is.
    ///
    /// # Errors
    ///
    /// Returns an error if the file is absent or is not valid JSON.
    pub fn stored_json_values(
        &self,
        name: &str,
        path: &[JsonStep],
    ) -> Result<Vec<String>, StandError> {
        let bytes = self.present(name)?;
        reader::json_values(&bytes, path).map_err(|why| StandError::Unreadable {
            name: name.to_owned(),
            why,
        })
    }

    /// The parts of a workbook's package, each with the digest of its bytes as
    /// they sit in the file. A part that a save left untouched has the same
    /// digest before and after.
    ///
    /// # Errors
    ///
    /// Returns an error if the file is absent or is not a package.
    pub fn package_parts(&self, name: &str) -> Result<Vec<PackagePart>, StandError> {
        let bytes = self.present(name)?;
        package::parts(&bytes).ok_or(StandError::Unreadable {
            name: name.to_owned(),
            why: Unreadable {
                reason: "the bytes are not a package the stand can list",
                at: 0,
            },
        })
    }

    // ---- Count writes ------------------------------------------------------

    /// Looks at every file below the root, counts each difference from the last
    /// look, and returns the differences in order of name.
    ///
    /// # Errors
    ///
    /// Returns an error if a file could not be read.
    pub fn observe(&self) -> Result<Vec<Observation>, StandError> {
        let mut state = self.lock();
        let now = self.look(&state)?;
        Ok(state.ledger.look(now))
    }

    /// Looks, then says how many times one file was seen to change since the
    /// last reset. Changes the stand's own faults made are not counted.
    ///
    /// # Errors
    ///
    /// Returns an error if a file could not be read.
    pub fn write_count(&self, name: &str) -> Result<WriteCount, StandError> {
        self.observe()?;
        Ok(self.lock().ledger.count(name))
    }

    /// Looks, then gives the count for every file that was seen to change.
    ///
    /// # Errors
    ///
    /// Returns an error if a file could not be read.
    pub fn write_counts(&self) -> Result<BTreeMap<String, WriteCount>, StandError> {
        self.observe()?;
        Ok(self.lock().ledger.counts())
    }

    // ---- Faults ------------------------------------------------------------

    /// Switches a fault on. An act of another writer happens now; a standing
    /// fault holds until [`Self::clear_faults`] or [`Self::reset`].
    ///
    /// # Errors
    ///
    /// Returns [`StandError::Unsupported`] if the fault cannot be shown on this
    /// operating system or to this user, and another error if it could not be set.
    pub fn set_fault(&self, fault: Fault) -> Result<(), StandError> {
        let mut state = self.lock();
        // What the thing under test did so far is counted before the stand acts,
        // and what the stand does is then taken as the new starting point.
        let before = self.look(&state)?;
        state.ledger.look(before);
        let outcome = self.apply(&mut state, &fault);
        let after = self.look(&state)?;
        state.ledger.accept(after);
        outcome?;
        state.faults.push(fault);
        Ok(())
    }

    /// The faults set since the last clear or reset, oldest first.
    #[must_use]
    pub fn faults(&self) -> Vec<Fault> {
        self.lock().faults.clone()
    }

    /// Ends every standing fault: a file held open is let go, read-only marks are
    /// taken off, lock signals are removed. What another writer did to the files
    /// stays; only a reset undoes that.
    ///
    /// # Errors
    ///
    /// Returns an error if a mark could not be taken off or a signal removed.
    pub fn clear_faults(&self) -> Result<(), StandError> {
        let mut state = self.lock();
        let before = self.look(&state)?;
        state.ledger.look(before);
        let outcome = self.end_standing_faults(&mut state);
        let after = self.look(&state)?;
        state.ledger.accept(after);
        outcome
    }

    // ---- Inside ------------------------------------------------------------

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn truth(&self, state: &State, name: &str) -> Result<FileTruth, StandError> {
        let path = self.path(name)?;
        let absent = FileTruth {
            name: name.to_owned(),
            kind: EntryKind::Absent,
            size: 0,
            sha256: None,
            read_only: false,
            identity: None,
            links: None,
        };
        let Some(metadata) = metadata_of(&path)? else {
            return Ok(absent);
        };
        let kind = kind_of(&metadata);
        let (identity, links) = identity_and_links(&metadata);
        let sha256 = match kind {
            EntryKind::File => self.read(state, name)?.map(|bytes| sha256_hex(&bytes)),
            _ => None,
        };
        Ok(FileTruth {
            kind,
            size: if kind == EntryKind::File {
                metadata.len()
            } else {
                0
            },
            sha256,
            read_only: metadata.permissions().readonly(),
            identity,
            links,
            ..absent
        })
    }

    /// The bytes of a regular file. A file the stand itself holds without sharing
    /// is read through the handle that holds it.
    fn read(&self, state: &State, name: &str) -> Result<Option<Vec<u8>>, StandError> {
        let path = self.path(name)?;
        match metadata_of(&path)? {
            Some(metadata) if kind_of(&metadata) == EntryKind::File => {}
            _ => return Ok(None),
        }
        if let Some((_, held)) = state.held.iter().find(|(held, _)| held == name) {
            let mut handle: &File = held;
            let mut bytes = Vec::new();
            io_at("read a held file", &path, handle.seek(SeekFrom::Start(0)))?;
            io_at("read a held file", &path, handle.read_to_end(&mut bytes))?;
            return Ok(Some(bytes));
        }
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => io_at("read a file", &path, Err(e)),
        }
    }

    fn present(&self, name: &str) -> Result<Vec<u8>, StandError> {
        self.bytes(name)?
            .ok_or_else(|| StandError::Absent(name.to_owned()))
    }

    /// One look at every regular file below the root.
    fn look(&self, state: &State) -> Result<BTreeMap<String, Seen>, StandError> {
        let mut entries = Vec::new();
        walk(&self.root, "", &mut entries)?;
        let mut seen = BTreeMap::new();
        for (name, kind) in entries {
            if kind != EntryKind::File {
                continue;
            }
            let truth = self.truth(state, &name)?;
            if let Some(sha256) = truth.sha256 {
                seen.insert(
                    name,
                    Seen {
                        sha256,
                        identity: truth.identity,
                    },
                );
            }
        }
        Ok(seen)
    }

    fn apply(&self, state: &mut State, fault: &Fault) -> Result<(), StandError> {
        match fault {
            Fault::ChangedByAnother { file, change, how } => {
                let path = self.path(file)?;
                let old = self
                    .read(state, file)?
                    .ok_or_else(|| StandError::Absent(file.clone()))?;
                let new = changed_bytes(file, &old, change)?;
                save_as_another_writer(&path, &new, *how)
            }
            Fault::FileRemoved { file } => {
                let path = self.path(file)?;
                io_at("remove a file", &path, fs::remove_file(&path))
            }
            Fault::FolderRemoved { folder } => {
                let path = self.path(folder)?;
                make_removable(&path);
                io_at("remove a folder", &path, fs::remove_dir_all(&path))
            }
            Fault::PartialWrite { beside, temp_name } => {
                let target = self.path(beside)?;
                let whole = self
                    .read(state, beside)?
                    .ok_or_else(|| StandError::Absent(beside.clone()))?;
                let planted = sibling_name(beside, temp_name)?;
                let path = self.path(&planted)?;
                debug_assert_eq!(path.parent(), target.parent());
                let half = &whole[..whole.len() / 2];
                io_at("leave a partial write", &path, fs::write(&path, half))?;
                state.planted.insert(planted);
                Ok(())
            }
            Fault::LockSignal { sibling } => {
                let path = self.path(sibling)?;
                io_at("make a lock signal", &path, fs::write(&path, b""))?;
                state.planted.insert(sibling.clone());
                Ok(())
            }
            Fault::HeldWithoutSharing { file } => {
                let path = self.path(file)?;
                let held = hold_without_sharing(&path)?;
                state.held.push((file.clone(), held));
                Ok(())
            }
            Fault::ReadOnlyFile { file } => {
                let path = self.path(file)?;
                let saved = mark_read_only(&path)?;
                state.restore.push((path, saved));
                Ok(())
            }
            Fault::ReadOnlyFolder { folder } => {
                let path = self.path(folder)?;
                let saved = deny_writes_in_folder(&path)?;
                state.restore.push((path, saved));
                Ok(())
            }
            Fault::LinkOut { at } => {
                let path = self.path(at)?;
                make_link(&self.outside.join(seed::OUTSIDE_FILE), &path, false)?;
                state.planted.insert(at.clone());
                Ok(())
            }
            Fault::FolderLinkOut { at } => {
                let path = self.path(at)?;
                make_link(&self.outside, &path, true)?;
                state.planted.insert(at.clone());
                Ok(())
            }
            Fault::SecondLink { file, at } => {
                let original = self.path(file)?;
                let path = self.path(at)?;
                io_at(
                    "give a file a second name",
                    &path,
                    fs::hard_link(&original, &path),
                )?;
                state.planted.insert(at.clone());
                Ok(())
            }
        }
    }

    fn end_standing_faults(&self, state: &mut State) -> Result<(), StandError> {
        state.held.clear();
        let mut first_failure = Ok(());
        while let Some((path, saved)) = state.restore.pop() {
            match fs::set_permissions(&path, saved) {
                Ok(()) => {}
                // The entry was removed while it was marked; nothing to put back.
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => first_failure = first_failure.and(io_at("end a fault", &path, Err(e))),
            }
        }
        for fault in std::mem::take(&mut state.faults) {
            if let Fault::LockSignal { sibling } = fault {
                let path = self.path(&sibling)?;
                match fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => {
                        first_failure =
                            first_failure.and(io_at("remove a lock signal", &path, Err(e)));
                    }
                }
                state.planted.remove(&sibling);
            }
        }
        first_failure
    }
}

impl Drop for FileStand {
    fn drop(&mut self) {
        // Let go of held files and put permissions back before removing anything.
        // A folder that cannot be removed is left for the system to clear.
        let mut state = self.lock();
        *state = State::default();
        make_removable(&self.home);
        let _ = fs::remove_dir_all(&self.home);
    }
}

/// The metadata of what sits at a path itself, a link not followed, or `None`
/// if nothing does.
fn metadata_of(path: &Path) -> Result<Option<Metadata>, StandError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(None)
        }
        Err(e) => io_at("look at an entry", path, Err(e)),
    }
}

fn kind_of(metadata: &Metadata) -> EntryKind {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        EntryKind::Link
    } else if file_type.is_dir() {
        EntryKind::Folder
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

/// Lists everything below a folder, by name with `/` between the parts. A link
/// is listed and never followed.
fn walk(
    folder: &Path,
    prefix: &str,
    found: &mut Vec<(String, EntryKind)>,
) -> Result<(), StandError> {
    for entry in io_at("list a folder", folder, fs::read_dir(folder))? {
        let entry = io_at("list a folder", folder, entry)?;
        let path = entry.path();
        let own_name = entry.file_name().to_string_lossy().into_owned();
        let name = if prefix.is_empty() {
            own_name
        } else {
            format!("{prefix}/{own_name}")
        };
        let Some(metadata) = metadata_of(&path)? else {
            continue;
        };
        let kind = kind_of(&metadata);
        found.push((name.clone(), kind));
        if kind == EntryKind::Folder {
            walk(&path, &name, found)?;
        }
    }
    Ok(())
}

/// Removes everything in a folder and keeps the folder.
fn empty_folder(folder: &Path) -> Result<(), StandError> {
    for entry in io_at("list a folder", folder, fs::read_dir(folder))? {
        let path = io_at("list a folder", folder, entry)?.path();
        let Some(metadata) = metadata_of(&path)? else {
            continue;
        };
        let removed = match kind_of(&metadata) {
            EntryKind::Folder => fs::remove_dir_all(&path),
            // A link to a folder is removed as a folder on Windows and as a
            // file everywhere else.
            EntryKind::Link => fs::remove_file(&path).or_else(|_| fs::remove_dir(&path)),
            _ => fs::remove_file(&path),
        };
        io_at("remove a seed entry", &path, removed)?;
    }
    Ok(())
}

/// Takes off whatever would stop the stand removing its own files. Failures are
/// ignored: the removal that follows reports what still cannot be removed.
fn make_removable(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    allow_removal(path, &metadata);
    if metadata.is_dir()
        && let Ok(entries) = fs::read_dir(path)
    {
        for entry in entries.flatten() {
            make_removable(&entry.path());
        }
    }
}

/// On Unix a folder's own permissions decide whether its entries can be removed.
#[cfg(unix)]
fn allow_removal(path: &Path, metadata: &Metadata) {
    use std::os::unix::fs::PermissionsExt;
    if metadata.is_dir() {
        let mut permissions = metadata.permissions();
        permissions.set_mode(permissions.mode() | 0o700);
        let _ = fs::set_permissions(path, permissions);
    }
}

/// Elsewhere a read-only mark on the entry itself stops its removal.
#[cfg(not(unix))]
// On these systems the call only takes the read-only mark off; the lint warns
// about what it does on Unix, where this function is not built.
#[allow(clippy::permissions_set_readonly_false)]
fn allow_removal(path: &Path, metadata: &Metadata) {
    let mut permissions = metadata.permissions();
    if permissions.readonly() {
        permissions.set_readonly(false);
        let _ = fs::set_permissions(path, permissions);
    }
}

/// The name of a file in the same folder as `beside`.
fn sibling_name(beside: &str, own_name: &str) -> Result<String, StandError> {
    if own_name.is_empty() || own_name.contains('/') {
        return Err(StandError::BadName(own_name.to_owned()));
    }
    Ok(match beside.rsplit_once('/') {
        Some((folder, _)) => format!("{folder}/{own_name}"),
        None => own_name.to_owned(),
    })
}

/// The bytes a file holds after another writer made this change to it.
fn changed_bytes(name: &str, old: &[u8], change: &Change) -> Result<Vec<u8>, StandError> {
    let known = |which| {
        seed::known_edit(name, which).ok_or_else(|| StandError::NoKnownChange(name.to_owned()))
    };
    let edit = match change {
        Change::ThePlace => known(Known::ThePlace)?,
        Change::Elsewhere => known(Known::Elsewhere)?,
        Change::Layout => known(Known::Layout)?,
        Change::Bytes { find, replace } => {
            return replaced(old, find, replace, false)
                .ok_or_else(|| StandError::BytesNotFound(name.to_owned()));
        }
        Change::Whole(bytes) => return Ok(bytes.clone()),
    };
    match edit {
        Edit::Whole(bytes) => Ok(bytes),
        Edit::Replace {
            find,
            replace,
            every,
        } => replaced(old, find, replace, every)
            .ok_or_else(|| StandError::BytesNotFound(name.to_owned())),
    }
}

/// `old` with `find` replaced, once or every time. `None` if it never occurs.
fn replaced(old: &[u8], find: &[u8], replace: &[u8], every: bool) -> Option<Vec<u8>> {
    if find.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(old.len());
    let mut rest = old;
    let mut count = 0;
    while let Some(at) = rest.windows(find.len()).position(|window| window == find) {
        out.extend_from_slice(&rest[..at]);
        out.extend_from_slice(replace);
        rest = &rest[at + find.len()..];
        count += 1;
        if !every {
            break;
        }
    }
    out.extend_from_slice(rest);
    (count > 0).then_some(out)
}

fn save_as_another_writer(path: &Path, bytes: &[u8], how: SaveStyle) -> Result<(), StandError> {
    match how {
        SaveStyle::InPlace => io_at("write over a file", path, fs::write(path, bytes)),
        SaveStyle::Replace => {
            let mut temp_name = std::ffi::OsString::from(".");
            temp_name.push(path.file_name().unwrap_or_default());
            temp_name.push(".other-writer");
            let temp = path.with_file_name(temp_name);
            io_at("write a new file", &temp, fs::write(&temp, bytes))?;
            io_at("replace a file", path, fs::rename(&temp, path))
        }
    }
}

/// Marks an entry read-only and returns what its permissions were.
fn mark_read_only(path: &Path) -> Result<Permissions, StandError> {
    let saved = io_at("look at an entry", path, fs::metadata(path))?.permissions();
    let mut read_only = saved.clone();
    read_only.set_readonly(true);
    io_at(
        "mark an entry read-only",
        path,
        fs::set_permissions(path, read_only),
    )?;
    Ok(saved)
}

/// Stops anything being made, replaced, or removed in a folder, and returns what
/// its permissions were. The stand then tries to make a file there itself, and
/// calls the fault unsupported if it can: the superuser is not held by the mark.
#[cfg(unix)]
fn deny_writes_in_folder(path: &Path) -> Result<Permissions, StandError> {
    let saved = mark_read_only(path)?;
    let probe = path.join(".example-files-probe");
    if fs::write(&probe, b"").is_ok() {
        let _ = fs::remove_file(&probe);
        io_at(
            "end a fault",
            path,
            fs::set_permissions(path, saved.clone()),
        )?;
        return Err(StandError::Unsupported(
            "this user may write in a folder that is marked read-only",
        ));
    }
    Ok(saved)
}

#[cfg(not(unix))]
fn deny_writes_in_folder(_path: &Path) -> Result<Permissions, StandError> {
    Err(StandError::Unsupported(
        "only on Unix does a mark on a folder stop files being made in it",
    ))
}

#[cfg(windows)]
fn hold_without_sharing(path: &Path) -> Result<File, StandError> {
    use std::os::windows::fs::OpenOptionsExt;
    let opened = fs::OpenOptions::new().read(true).share_mode(0).open(path);
    io_at("hold a file open", path, opened)
}

#[cfg(not(windows))]
fn hold_without_sharing(_path: &Path) -> Result<File, StandError> {
    Err(StandError::Unsupported(
        "only Windows lets a program hold a file open and share it with nobody",
    ))
}

/// Makes a link at `at` that points at `target`.
fn make_link(target: &Path, at: &Path, to_folder: bool) -> Result<(), StandError> {
    match link(target, at, to_folder) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::Unsupported => Err(StandError::Unsupported(
            "this operating system has no links the stand can make",
        )),
        // Windows: the user does not hold the privilege to make links.
        Err(e) if cfg!(windows) && e.raw_os_error() == Some(1314) => {
            Err(StandError::Unsupported("this user may not make links"))
        }
        Err(e) => io_at("make a link", at, Err(e)),
    }
}

#[cfg(unix)]
fn link(target: &Path, at: &Path, _to_folder: bool) -> io::Result<()> {
    std::os::unix::fs::symlink(target, at)
}

#[cfg(windows)]
fn link(target: &Path, at: &Path, to_folder: bool) -> io::Result<()> {
    if to_folder {
        std::os::windows::fs::symlink_dir(target, at)
    } else {
        std::os::windows::fs::symlink_file(target, at)
    }
}

#[cfg(not(any(unix, windows)))]
fn link(_target: &Path, _at: &Path, _to_folder: bool) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}
