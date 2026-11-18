//! A folder as a collection of entries (decision 0246).
//!
//! The folder is the surface's, fixed by the target; an entry is named by
//! one name inside it, which a binding may take from an input, as the key
//! of a row is. A name is one name: no separator, no `..`, no drive, no
//! device, no part that one operating system would drop, and no more than
//! 255 bytes. It names an entry only when the folder holds that very name:
//! a name the folder holds only in another case is a named stop, because on
//! a system that folds case the two are one entry and elsewhere they are
//! two. Nothing is followed through a link, and a link, a device, a pipe,
//! or a socket is listed by its kind and never opened.
//!
//! A list reads the folder's entries, and folders below it to a declared
//! depth, in the order of their names' bytes, within a bound. Writes are of
//! entries directly in the folder, each proven from disk:
//!
//! - create: the bytes go to a temporary file of a random name, which is
//!   then given the entry's name by a second name for the same file, a call
//!   that fails where anything has the name, so a file that appeared
//!   meanwhile is never replaced; then the temporary name is removed;
//! - replace: the pack's verified save, held to the file that was read, by
//!   its identity and its digest;
//! - rename: the same second name, given in the same folder, and then the
//!   old name removed, only while it still holds the file that was read;
//! - delete: a file that is still the one that was read, or a folder that
//!   is empty.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use a2m_core::{Date, Digest, Value, Values};
use a2m_provider::StepFailure;

use crate::codes::{self, stop};
use crate::open::{missing, not_allowed, unavailable};
use crate::place::{Folder, Identity, Named, is_one_name};

/// The longest entry name, in bytes: the most most file systems hold.
const LONGEST_NAME: usize = 255;

/// What an entry is, at its own name, with no link followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    File,
    Folder,
    Link,
    Other,
}

impl Kind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Folder => "folder",
            Self::Link => "link",
            Self::Other => "other",
        }
    }

    fn of(file_type: fs::FileType) -> Self {
        if file_type.is_symlink() {
            Self::Link
        } else if file_type.is_dir() {
            Self::Folder
        } else if file_type.is_file() {
            Self::File
        } else {
            Self::Other
        }
    }
}

/// One entry of a folder, as found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    /// Its name below the surface's folder, with `/` between folders.
    pub name: String,
    pub kind: Kind,
    /// A file's length in bytes.
    pub size: Option<u64>,
    /// When a file or a folder was last changed, in UTC, to the second.
    pub modified: Option<String>,
}

impl Entry {
    /// The entry's values, as a read gives them.
    pub(crate) fn values(&self) -> Values {
        let mut values = Values::new()
            .with("name", self.name.clone())
            .with("kind", self.kind.as_str());
        values.insert(
            "size",
            self.size
                .and_then(|size| i64::try_from(size).ok())
                .map_or(Value::Null, Value::Int),
        );
        values.insert(
            "modified",
            self.modified.clone().map_or(Value::Null, Value::Text),
        );
        values
    }
}

/// A folder's entries, as one list gives them.
#[derive(Debug, Clone)]
pub(crate) struct Listing {
    pub entries: Vec<Entry>,
    /// Entries left out because their name is not a name the pack can use:
    /// not UTF-8, or a form some system would read otherwise.
    pub skipped: u64,
}

fn too_many() -> StepFailure {
    stop(
        codes::FILE_TOO_LARGE,
        "The folder holds more entries than the file pack reads.",
        "Raise `packs.file.max_rows`, or name a folder that holds fewer entries.",
    )
}

/// The moment, in UTC, to the second, as `YYYY-MM-DDTHH:MM:SS`.
fn moment(time: std::time::SystemTime) -> Option<String> {
    let seconds = i64::try_from(time.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs()).ok()?;
    let days = i32::try_from(seconds.div_euclid(86_400)).ok()?;
    let rest = seconds.rem_euclid(86_400);
    let date = Date::from_days(days).ok()?;
    Some(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        date.year(),
        date.month(),
        date.day(),
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    ))
}

/// Whether a name is one the pack names an entry by: one name, at most 255
/// bytes. The roots' check holds it to the rest.
pub(crate) fn is_entry_name(name: &str) -> bool {
    is_one_name(name) && name.len() <= LONGEST_NAME
}

fn look(path: &Path, name: String) -> Result<Entry, StepFailure> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            missing()
        } else {
            StepFailure::not_sent(unavailable("has an entry that cannot be looked at"))
        }
    })?;
    let kind = Kind::of(metadata.file_type());
    Ok(Entry {
        name,
        kind,
        size: (kind == Kind::File).then_some(metadata.len()),
        modified: matches!(kind, Kind::File | Kind::Folder)
            .then(|| metadata.modified().ok().and_then(moment))
            .flatten(),
    })
}

/// The names a folder holds, as written: the ones the pack can use, and how
/// many it cannot.
fn names(path: &Path, ceiling: usize) -> Result<(Vec<String>, u64), StepFailure> {
    let read = fs::read_dir(path).map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            missing()
        } else {
            StepFailure::not_sent(unavailable("cannot be listed"))
        }
    })?;
    let mut found = Vec::new();
    let mut skipped = 0;
    for entry in read {
        let entry = entry.map_err(|_| StepFailure::not_sent(unavailable("cannot be listed")))?;
        if found.len() + usize::try_from(skipped).unwrap_or(usize::MAX) >= ceiling {
            return Err(too_many());
        }
        match entry.file_name().to_str() {
            Some(name) if is_entry_name(name) => found.push(name.to_owned()),
            _ => skipped += 1,
        }
    }
    found.sort();
    Ok((found, skipped))
}

/// The folder's entries, and those of the folders below it to `depth`,
/// in the order of their names' bytes. No link is followed.
pub(crate) fn list(folder: &Folder, depth: u8, ceiling: usize) -> Result<Listing, StepFailure> {
    let mut listing = Listing {
        entries: Vec::new(),
        skipped: 0,
    };
    let mut pending: Vec<(PathBuf, String, u8)> = vec![(folder.listed_path()?, String::new(), 0)];
    while let Some((path, prefix, level)) = pending.pop() {
        let (found, skipped) = names(&path, ceiling)?;
        listing.skipped += skipped;
        for name in found {
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let entry = look(&path.join(&name), relative.clone())?;
            if entry.kind == Kind::Folder && level < depth {
                pending.push((path.join(&name), relative, level + 1));
            }
            listing.entries.push(entry);
            if listing.entries.len() > ceiling {
                return Err(too_many());
            }
        }
    }
    listing.entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(listing)
}

/// The folder an entry is in, and its own last name. A name with folders
/// in it is read only a declared depth down; a write names an entry
/// directly in the folder.
fn split<'a>(folder: &Folder, name: &'a str, depth: u8) -> Result<(Folder, &'a str), StepFailure> {
    let parts = name.split('/').count();
    if name.is_empty() || parts > usize::from(depth) + 1 {
        return Err(StepFailure::not_sent(not_allowed(
            "its name holds more folders than the folder's declared depth",
        )));
    }
    match name.rsplit_once('/') {
        None => Ok((folder.clone(), name)),
        Some((above, last)) => Ok((folder.below(above)?, last)),
    }
}

/// How a name stands in its folder.
enum Held {
    /// The folder holds the very name.
    Exactly,
    /// It holds the name only in another case.
    OtherCase,
    /// It holds no such name.
    No,
}

fn held(folder: &Folder, name: &str, ceiling: usize) -> Result<Held, StepFailure> {
    let (found, _) = names(&folder.listed_path()?, ceiling)?;
    if found.iter().any(|held| held == name) {
        return Ok(Held::Exactly);
    }
    let folded = name.to_lowercase();
    if found.iter().any(|held| held.to_lowercase() == folded) {
        return Ok(Held::OtherCase);
    }
    Ok(Held::No)
}

fn other_case() -> StepFailure {
    stop(
        codes::NAME_CASE_CONFLICT,
        "The folder holds an entry whose name differs from this one only in case. On a system that folds case they are one entry; on others they are two, so the pack does not take one for the other.",
        "Give the name exactly as the folder holds it, or choose a name that differs in more than case.",
    )
}

/// One entry by name, when the folder holds that very name.
pub(crate) fn find(
    folder: &Folder,
    name: &str,
    depth: u8,
    ceiling: usize,
) -> Result<Option<Entry>, StepFailure> {
    let (inner, last) = match split(folder, name, depth) {
        Ok(found) => found,
        Err(failure) if failure.failure.code() == &codes::FILE_MISSING => return Ok(None),
        Err(failure) => return Err(failure),
    };
    // The roots' check sees the name: a device, a form no path may take,
    // and a link that leads out of the root are refused here by name.
    inner.file(last)?;
    match held(&inner, last, ceiling)? {
        Held::Exactly => Ok(Some(look(&inner.beside(last)?, name.to_owned())?)),
        Held::OtherCase => Err(other_case()),
        Held::No => Ok(None),
    }
}

/// The identity of the regular file at an entry's name, with no link
/// followed; nothing for anything else.
pub(crate) fn identity(
    folder: &Folder,
    name: &str,
    depth: u8,
) -> Result<Option<Identity>, StepFailure> {
    let (inner, last) = split(folder, name, depth)?;
    Ok(identity_at(&inner.beside(last)?))
}

/// The file an entry is, by its own name, for reading its bytes.
pub(crate) fn named(folder: &Folder, name: &str, depth: u8) -> Result<Named, StepFailure> {
    let (inner, last) = split(folder, name, depth)?;
    inner.file(last)
}

/// A regular file's bytes, within `max_bytes`, read through a handle that
/// is proven to be the file at the name. A link at the name is refused.
/// Nothing when the file is larger.
pub(crate) fn read(file: &Named, max_bytes: u64) -> Result<Option<Vec<u8>>, StepFailure> {
    let checked = file.checked();
    match checked.entry_kind() {
        Ok(a2m_provider::EntryKind::File) => {}
        Ok(a2m_provider::EntryKind::Absent) => return Err(missing()),
        Ok(_) => {
            return Err(StepFailure::not_sent(not_allowed(
                "it is a link, a folder, or a special file, and only a regular file's bytes are read",
            )));
        }
        Err(failure) => return Err(StepFailure::not_sent(failure)),
    }
    let handle = checked.open().map_err(StepFailure::not_sent)?;
    let opened = a2m_provider::FileIdentity::of_open(&handle).map_err(StepFailure::not_sent)?;
    if checked.identity().ok() != Some(opened) {
        return Err(StepFailure::not_sent(unavailable(
            "was put under its name while it was being opened, so it was not used",
        )));
    }
    let mut bytes = Vec::new();
    handle
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| StepFailure::not_sent(unavailable("cannot be read")))?;
    Ok((bytes.len() as u64 <= max_bytes).then_some(bytes))
}

/// A file's content as text: UTF-8, every byte kept, a byte-order mark
/// included.
pub(crate) fn text(bytes: Vec<u8>) -> Result<String, StepFailure> {
    String::from_utf8(bytes).map_err(|_| {
        stop(
            codes::VALUE_UNREADABLE,
            "The file's content is not UTF-8 text, and the file pack reads an entry's content only as text.",
            "Read the entry's name, size, and digest instead.",
        )
    })
}

/// The digest of a regular file, when it has at most `max_bytes`.
pub(crate) fn digest(file: &Named, max_bytes: u64) -> Result<Option<Digest>, StepFailure> {
    Ok(read(file, max_bytes)?.map(|bytes| Digest::of(&bytes)))
}

fn changed() -> StepFailure {
    stop(
        codes::CHANGED_SINCE_PREVIEW,
        "The folder changed after it was read: an entry has the name, or the entry is no longer the one that was read. Nothing was written.",
        "Read the folder again, look at the new preview, and approve again.",
    )
}

fn no_second_name() -> StepFailure {
    stop(
        codes::SHAPE_UNSUPPORTED,
        "The folder's file system gives a file no second name, so a file cannot be put at a name without the risk of replacing one that appeared there. Nothing was written.",
        "Keep the folder on a file system that has hard links, or do not write entries of it.",
    )
}

/// What a creation or a rename found when it gave the second name.
fn linked(result: std::io::Result<()>) -> Result<(), StepFailure> {
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Err(changed()),
        Err(error) if error.kind() == ErrorKind::NotFound => Err(changed()),
        Err(error) if error.kind() == ErrorKind::Unsupported || refuses_links(&error) => {
            Err(no_second_name())
        }
        Err(error) if error.kind() == ErrorKind::StorageFull => Err(stop(
            codes::STORAGE_FULL,
            "The disk or the folder is full. Nothing was written.",
            "Free space in the folder, then try again.",
        )),
        Err(_) => Err(StepFailure::not_sent(not_allowed(
            "its folder cannot be written to",
        ))),
    }
}

/// Whether the system said it gives this file no second name: on Linux and
/// macOS a file system without hard links answers "operation not
/// permitted", which a folder the user may not write does not.
#[cfg(unix)]
fn refuses_links(error: &std::io::Error) -> bool {
    const NOT_PERMITTED: i32 = 1;
    error.raw_os_error() == Some(NOT_PERMITTED)
}

/// Whether the system said it gives this file no second name: on Windows a
/// file system without hard links answers "invalid function".
#[cfg(not(unix))]
fn refuses_links(error: &std::io::Error) -> bool {
    const INVALID_FUNCTION: i32 = 1;
    error.raw_os_error() == Some(INVALID_FUNCTION)
}

/// The identity at a name, with no link followed.
fn identity_at(path: &Path) -> Option<Identity> {
    fs::symlink_metadata(path)
        .ok()
        .filter(|metadata| metadata.file_type().is_file())
        .and_then(|metadata| Identity::of(&metadata))
}

/// A temporary file the pack made, removed when dropped, only while its
/// name still holds the file it made.
struct Made {
    path: PathBuf,
    identity: Option<Identity>,
}

impl Drop for Made {
    fn drop(&mut self) {
        if self.identity.is_some() && identity_at(&self.path) == self.identity {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn random_name() -> Result<String, StepFailure> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|_| {
        StepFailure::not_sent(unavailable(
            "cannot be made: the operating system gave no random name for its temporary file",
        ))
    })?;
    let mut name = String::from(".a2m-new-");
    for byte in random {
        name.push_str(&format!("{byte:02x}"));
    }
    Ok(name)
}

#[cfg(unix)]
fn private(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt as _;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn private(_options: &mut OpenOptions) {}

/// Creates a file at `name`, directly in the folder, holding `bytes`, only
/// where no entry has the name. Blocking; never retried.
pub(crate) fn create(
    folder: &Folder,
    name: &str,
    bytes: &[u8],
    ceiling: usize,
) -> Result<(), StepFailure> {
    folder.reached_by_no_link()?;
    let target = folder.file(name)?;
    match held(folder, name, ceiling)? {
        Held::Exactly => return Err(changed()),
        Held::OtherCase => return Err(other_case()),
        Held::No => {}
    }
    let temporary = folder.beside(&random_name()?)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    private(&mut options);
    let mut file: File = options.open(&temporary).map_err(|error| {
        linked(Err(error)).err().unwrap_or_else(|| {
            StepFailure::not_sent(unavailable("cannot have its temporary file made"))
        })
    })?;
    let made_identity = file
        .metadata()
        .ok()
        .and_then(|metadata| Identity::of(&metadata));
    let made = Made {
        path: temporary,
        identity: made_identity,
    };
    if made.identity.is_none() {
        return Err(StepFailure::not_sent(unavailable(
            "has a temporary file whose identity is not known",
        )));
    }
    let written = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| file.seek(SeekFrom::Start(0)).map(|_| ()));
    if written.is_err() {
        return Err(StepFailure::not_sent(unavailable(
            "could not be written beside the folder's entries",
        )));
    }
    let mut back = Vec::new();
    let read_back = (&file)
        .take(bytes.len() as u64 + 1)
        .read_to_end(&mut back)
        .is_ok();
    drop(file);
    if !read_back || back != bytes {
        return Err(StepFailure::not_sent(unavailable(
            "was not written as it was built; nothing was put in place",
        )));
    }
    // The commit point: a second name for the file, which the system gives
    // only where nothing has the name.
    let target_path = folder.beside(name)?;
    linked(fs::hard_link(&made.path, &target_path))?;
    if identity_at(&target_path) != made.identity {
        return Err(StepFailure::unknown(unavailable(
            "was given its name, and the name does not hold the file that was written",
        )));
    }
    let removed = fs::remove_file(&made.path).is_ok();
    drop(made);
    if !removed {
        return Err(StepFailure::unknown(unavailable(
            "was created, and its temporary name could not be removed",
        )));
    }
    target.checked().flush_folder().map_err(|_| {
        StepFailure::unknown(unavailable(
            "was created, and its folder could not be written to the disk",
        ))
    })
}

/// Gives the file at `from` the name `to`, both directly in the folder, only
/// while `from` holds the file that was read and no entry has `to`.
pub(crate) fn rename(
    folder: &Folder,
    from: &str,
    to: &str,
    read: Identity,
    ceiling: usize,
) -> Result<(), StepFailure> {
    folder.reached_by_no_link()?;
    let source = folder.file(from)?;
    let metadata = source.regular().map_err(|failure| {
        if failure.failure.code() == &codes::FILE_MISSING {
            changed()
        } else {
            failure
        }
    })?;
    if Identity::of(&metadata) != Some(read) {
        return Err(changed());
    }
    let target = folder.file(to)?;
    match held(folder, to, ceiling)? {
        Held::Exactly => return Err(changed()),
        Held::OtherCase => return Err(other_case()),
        Held::No => {}
    }
    let from_path = folder.beside(from)?;
    let to_path = folder.beside(to)?;
    linked(fs::hard_link(&from_path, &to_path))?;
    if identity_at(&to_path) != Some(read) {
        return Err(StepFailure::unknown(unavailable(
            "was given its new name, and the name does not hold the file that was read",
        )));
    }
    if identity_at(&from_path) != Some(read) || fs::remove_file(&from_path).is_err() {
        return Err(StepFailure::unknown(unavailable(
            "has its new name, and its old name could not be removed",
        )));
    }
    target.checked().flush_folder().map_err(|_| {
        StepFailure::unknown(unavailable(
            "was renamed, and its folder could not be written to the disk",
        ))
    })
}

/// Removes the entry `name`, directly in the folder: a file that is still
/// the one that was read, or a folder that is empty.
pub(crate) fn delete(
    folder: &Folder,
    name: &str,
    kind: Kind,
    read: Option<Identity>,
) -> Result<(), StepFailure> {
    folder.reached_by_no_link()?;
    let entry = folder.file(name)?;
    match kind {
        Kind::File => {
            let metadata = entry.regular().map_err(|failure| {
                if failure.failure.code() == &codes::FILE_MISSING {
                    changed()
                } else {
                    failure
                }
            })?;
            if read.is_none() || Identity::of(&metadata) != read {
                return Err(changed());
            }
            entry
                .checked()
                .remove_file()
                .map_err(StepFailure::not_sent)?;
        }
        Kind::Folder => {
            let path = folder.beside(name)?;
            match fs::symlink_metadata(&path) {
                Ok(found) if found.file_type().is_dir() => {}
                Ok(_) | Err(_) => return Err(changed()),
            }
            let empty = fs::read_dir(&path)
                .map_err(|_| StepFailure::not_sent(unavailable("cannot be listed")))?
                .next()
                .is_none();
            if !empty {
                return Err(not_empty());
            }
            // The system removes only an empty folder: one that gained an
            // entry since the look is refused by the call itself.
            fs::remove_dir(&path).map_err(|_| not_empty())?;
        }
        Kind::Link | Kind::Other => {
            return Err(StepFailure::not_sent(not_allowed(
                "it is a link or a special file, and the file pack removes only a file or an empty folder",
            )));
        }
    }
    entry.checked().flush_folder().map_err(|_| {
        StepFailure::unknown(unavailable(
            "was removed, and its folder could not be written to the disk",
        ))
    })
}

fn not_empty() -> StepFailure {
    stop(
        codes::FOLDER_NOT_EMPTY,
        "The folder is not empty, and the file pack removes only an empty folder. Nothing was removed.",
        "Remove its entries first, each as its own write.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_is_written_in_utc_to_the_second() {
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000);
        assert_eq!(moment(at).as_deref(), Some("2027-01-15T08:00:00"));
    }

    #[test]
    fn an_entry_name_is_one_name_of_at_most_255_bytes() {
        assert!(is_entry_name("note.txt"));
        assert!(is_entry_name(&"a".repeat(255)));
        for name in ["", ".", "..", "a/b", "a\\b", "c:x", &"a".repeat(256)] {
            assert!(!is_entry_name(name), "{name:?}");
        }
    }
}
