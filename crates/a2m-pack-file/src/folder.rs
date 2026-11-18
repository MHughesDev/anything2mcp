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
//!
//! Every write acts through the surface's folder held for it (decision
//! 0272): reached from the root down with no link followed, and then each
//! call names one entry relative to that folder, so a folder swapped for a
//! link between the look and the act cannot lead the write elsewhere. A
//! folder moved away whole meanwhile is told: before the act, nothing is
//! done; after it, the outcome is unknown. A file is removed, and an old
//! name taken away, only while the name still holds the file that was read,
//! by the identity the system gives.

use std::fs;
use std::io::{ErrorKind, Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use a2m_core::{Date, Digest, Value, Values};
use a2m_provider::{HeldFolder, StepFailure};

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

/// What discovery sees of a folder: its entries to a depth, the folders at
/// that depth whose entries were not read, and whether a bound cut it.
#[derive(Debug, Clone)]
pub(crate) struct Survey {
    /// At most the bound's number of entries, in the order of their names'
    /// bytes.
    pub entries: Vec<Entry>,
    /// The folders at the depth looked to, whose own entries were not read.
    pub cut: Vec<String>,
    /// Names the pack cannot use, as in a list.
    pub skipped: u64,
    /// Whether more entries were there than the bound.
    pub truncated: bool,
}

/// The folder's entries, and those of the folders below it to `depth`, one
/// level after another, at most `most` of them. No link is followed and no
/// content is read: a link is an entry of the kind `link`, whatever it
/// points at. A folder with more names than `ceiling` is `file_too_large`,
/// as in a list.
pub(crate) fn survey(
    folder: &Folder,
    depth: u8,
    most: usize,
    ceiling: usize,
) -> Result<Survey, StepFailure> {
    let mut survey = Survey {
        entries: Vec::new(),
        cut: Vec::new(),
        skipped: 0,
        truncated: false,
    };
    let mut level: Vec<(PathBuf, String)> = vec![(folder.listed_path()?, String::new())];
    for at in 0..=depth {
        let mut next = Vec::new();
        for (path, prefix) in level {
            let (found, skipped) = names(&path, ceiling)?;
            survey.skipped += skipped;
            for name in found {
                if survey.entries.len() >= most {
                    survey.truncated = true;
                    break;
                }
                let relative = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                let entry = look(&path.join(&name), relative.clone())?;
                if entry.kind == Kind::Folder {
                    if at < depth {
                        next.push((path.join(&name), relative));
                    } else {
                        survey.cut.push(relative);
                    }
                }
                survey.entries.push(entry);
            }
        }
        level = next;
        if survey.truncated {
            break;
        }
    }
    survey.entries.sort_by(|a, b| a.name.cmp(&b.name));
    survey.cut.sort();
    Ok(survey)
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

/// How `name` stands among the names a folder lists.
fn standing(found: &[String], name: &str) -> Held {
    if found.iter().any(|held| held == name) {
        return Held::Exactly;
    }
    let folded = name.to_lowercase();
    if found.iter().any(|held| held.to_lowercase() == folded) {
        return Held::OtherCase;
    }
    Held::No
}

fn held(folder: &Folder, name: &str, ceiling: usize) -> Result<Held, StepFailure> {
    let (found, _) = names(&folder.listed_path()?, ceiling)?;
    Ok(standing(&found, name))
}

fn other_case() -> StepFailure {
    stop(
        codes::NAME_CASE_CONFLICT,
        "The folder holds an entry whose name differs from this one only in case. On a system that folds case they are one entry; on others they are two, so the pack does not take one for the other.",
        "Give the name exactly as the folder holds it, or choose a name that differs in more than case.",
    )
}

/// The folder lists no name the pack takes for this one, and its file
/// system finds an entry by this name all the same: it folds names by its
/// own rules, which the pack does not repeat, such as one table of case or
/// one form of letters written in two ways (decision 0272).
fn other_form() -> StepFailure {
    stop(
        codes::NAME_CASE_CONFLICT,
        "The folder's file system finds an entry by this name, and the folder lists it under another spelling: one that differs in case or in how its letters are composed. The pack does not take one for the other. Nothing was written.",
        "Give the name exactly as the folder lists it, or choose a name that differs in more than its spelling.",
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
        // Listed under no spelling the pack knows, and found by the file
        // system's own folding all the same.
        Held::No if fs::symlink_metadata(inner.beside(last)?).is_ok() => Err(other_form()),
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
    let regular = fs::symlink_metadata(inner.beside(last)?)
        .is_ok_and(|metadata| metadata.file_type().is_file());
    if !regular {
        return Ok(None);
    }
    Ok(inner.file(last)?.checked().identity().ok())
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

pub(crate) fn changed() -> StepFailure {
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

/// The names a held folder holds that the pack can use.
fn held_names(held: &HeldFolder, ceiling: usize) -> Result<Vec<String>, StepFailure> {
    let all = held
        .names()
        .map_err(|_| StepFailure::not_sent(unavailable("cannot be listed")))?;
    if all.len() > ceiling {
        return Err(too_many());
    }
    Ok(all
        .into_iter()
        .filter_map(|name| name.into_string().ok())
        .filter(|name| is_entry_name(name))
        .collect())
}

/// How a name stands in a held folder.
/// That no entry of the held folder has `name`, in any spelling the pack or
/// the file system takes for it, for a name about to be given.
fn name_free(held: &HeldFolder, name: &str, ceiling: usize) -> Result<(), StepFailure> {
    match standing(&held_names(held, ceiling)?, name) {
        Held::Exactly => Err(changed()),
        Held::OtherCase => Err(other_case()),
        Held::No => match held.kind(name) {
            Ok(a2m_provider::EntryKind::Absent) => Ok(()),
            Ok(_) => Err(other_form()),
            Err(_) => Err(StepFailure::not_sent(unavailable(
                "has an entry that cannot be looked at",
            ))),
        },
    }
}

/// The commit point of a create and a rename: `to` given to what stands at
/// `from`, which the system does only where nothing has the name. Where it
/// says something has it and the folder lists none the pack takes for it,
/// the file system folded the name.
fn link_new(held: &HeldFolder, from: &str, to: &str, ceiling: usize) -> Result<(), StepFailure> {
    match held.link(from, to) {
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            match held_names(held, ceiling).map(|found| standing(&found, to)) {
                Ok(Held::No) => Err(other_form()),
                Ok(Held::OtherCase) => Err(other_case()),
                Ok(Held::Exactly) | Err(_) => Err(changed()),
            }
        }
        other => linked(other),
    }
}

/// The folder was moved away, or another put at its path, after it was
/// held and before anything was done. Nothing was written.
fn moved_before() -> StepFailure {
    stop(
        codes::CHANGED_SINCE_PREVIEW,
        "The surface's folder was moved, or another put in its place, while the write was being made ready. Nothing was written.",
        "Put the folder back where the surface names it, read it again, and approve again.",
    )
}

/// The folder was moved away, or another put at its path, while the write
/// was made: it was made in the folder that was held, which is no longer
/// the one the surface names.
fn moved_during(what: &str) -> StepFailure {
    StepFailure::unknown(unavailable(&format!(
        "was {what} in the surface's folder, and the folder was moved, or another put in its place, meanwhile: the change is in the folder that was held, wherever it is now"
    )))
}

/// A temporary file the pack made, removed when dropped, only while its
/// name still holds the file it made.
struct Made<'a> {
    held: &'a HeldFolder,
    name: String,
    identity: Option<Identity>,
}

impl Drop for Made<'_> {
    fn drop(&mut self) {
        if let Some(identity) = self.identity {
            let _ = self.held.remove_file_if(&self.name, identity, |_| true);
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

/// Creates a file at `name`, directly in the folder, holding `bytes`, only
/// where no entry has the name. Blocking; never retried.
pub(crate) fn create(
    folder: &Folder,
    name: &str,
    bytes: &[u8],
    ceiling: usize,
) -> Result<(), StepFailure> {
    create_with(folder, name, bytes, ceiling, || {})
}

/// The same, with `before_act` called after the last look and just before
/// the file is given its name: where a test puts what another program may
/// do in that moment.
fn create_with(
    folder: &Folder,
    name: &str,
    bytes: &[u8],
    ceiling: usize,
    before_act: impl FnOnce(),
) -> Result<(), StepFailure> {
    let held = folder.held()?;
    // The roots' check sees the name: a device, a form no path may take.
    folder.file(name)?;
    name_free(&held, name, ceiling)?;
    let temporary = random_name()?;
    let mut file = held.create_new(&temporary).map_err(|error| {
        linked(Err(error)).err().unwrap_or_else(|| {
            StepFailure::not_sent(unavailable("cannot have its temporary file made"))
        })
    })?;
    // Its identity from its own handle, before anything else can stand at
    // its name: it is removed only while its name still holds it.
    let mut made = Made {
        held: &held,
        name: temporary.clone(),
        identity: Identity::of_open(&file).ok(),
    };
    let Some(identity) = made.identity else {
        drop(file);
        return Err(StepFailure::not_sent(unavailable(
            "has a temporary file whose identity is not known",
        )));
    };
    let written = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| file.seek(SeekFrom::Start(0)).map(|_| ()));
    if written.is_err() {
        // Closed first: on Windows it is shared with nobody while open, so
        // the removal by `made` could not open it.
        drop(file);
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
    if !held.still_at_its_path() {
        return Err(moved_before());
    }
    before_act();
    // The commit point: a second name for the file, which the system gives
    // only where nothing has the name, in the folder that was held.
    link_new(&held, &temporary, name, ceiling)?;
    if held.identity(name).ok() != Some(identity) {
        return Err(StepFailure::unknown(unavailable(
            "was given its name, and the name does not hold the file that was written",
        )));
    }
    let removed = matches!(
        held.remove_file_if(&temporary, identity, |_| true),
        Ok(a2m_provider::Removal::Removed)
    );
    made.identity = None;
    drop(made);
    if !removed {
        return Err(StepFailure::unknown(unavailable(
            "was created, and its temporary name could not be removed",
        )));
    }
    finish(&held, "created")
}

/// After the act: the folder's names written to the disk, and the folder
/// still the one at the surface's path.
fn finish(held: &HeldFolder, what: &str) -> Result<(), StepFailure> {
    held.sync().map_err(|_| {
        StepFailure::unknown(unavailable(&format!(
            "was {what}, and its folder could not be written to the disk"
        )))
    })?;
    if held.still_at_its_path() {
        Ok(())
    } else {
        Err(moved_during(what))
    }
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
    rename_with(folder, from, to, read, ceiling, || {})
}

/// The same, with `before_act` called after the last look and just before
/// the new name is given.
fn rename_with(
    folder: &Folder,
    from: &str,
    to: &str,
    read: Identity,
    ceiling: usize,
    before_act: impl FnOnce(),
) -> Result<(), StepFailure> {
    let held = folder.held()?;
    folder.file(from)?.regular().map_err(|failure| {
        if failure.failure.code() == &codes::FILE_MISSING {
            changed()
        } else {
            failure
        }
    })?;
    if held.identity(from).ok() != Some(read) {
        return Err(changed());
    }
    folder.file(to)?;
    name_free(&held, to, ceiling)?;
    if !held.still_at_its_path() {
        return Err(moved_before());
    }
    before_act();
    link_new(&held, from, to, ceiling)?;
    if held.identity(to).ok() != Some(read) {
        return Err(StepFailure::unknown(unavailable(
            "was given its new name, and the name does not hold the file that was read",
        )));
    }
    // The old name goes only while it holds the file that was read.
    if !matches!(
        held.remove_file_if(from, read, |_| true),
        Ok(a2m_provider::Removal::Removed)
    ) {
        return Err(StepFailure::unknown(unavailable(
            "has its new name, and its old name could not be removed",
        )));
    }
    finish(&held, "renamed")
}

/// What a delete is held to: for a file, the identity and the digest of
/// the file that was read, and the most bytes read to digest it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WasRead {
    pub identity: Option<Identity>,
    pub digest: Option<Digest>,
    pub max_bytes: u64,
}

/// Whether the file, read from its start, holds the bytes of that digest.
fn holds_bytes(file: &mut fs::File, digest: Digest, max_bytes: u64) -> bool {
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .is_ok()
        && bytes.len() as u64 <= max_bytes
        && Digest::of(&bytes) == digest
}

/// Removes the entry `name`, directly in the folder: a file that is still
/// the one that was read, by its identity and its bytes, or a folder that
/// is empty.
pub(crate) fn delete(
    folder: &Folder,
    name: &str,
    kind: Kind,
    read: WasRead,
) -> Result<(), StepFailure> {
    delete_with(folder, name, kind, read, || {})
}

/// The same, with `before_act` called after the last look and just before
/// the entry is removed.
fn delete_with(
    folder: &Folder,
    name: &str,
    kind: Kind,
    read: WasRead,
    before_act: impl FnOnce(),
) -> Result<(), StepFailure> {
    let held = folder.held()?;
    let entry = folder.file(name)?;
    match kind {
        Kind::File => {
            entry.regular().map_err(|failure| {
                if failure.failure.code() == &codes::FILE_MISSING {
                    changed()
                } else {
                    failure
                }
            })?;
            let (Some(identity), Some(digest)) = (read.identity, read.digest) else {
                return Err(changed());
            };
            if held.identity(name).ok() != Some(identity)
                || self::digest(&entry, read.max_bytes)? != Some(digest)
            {
                return Err(changed());
            }
            if !held.still_at_its_path() {
                return Err(moved_before());
            }
            before_act();
            // Removed only while the name holds the file that was read, by
            // its identity and its bytes: a file put there after the look is
            // never the one removed, even one given the old one's number.
            match held.remove_file_if(name, identity, |file| {
                holds_bytes(file, digest, read.max_bytes)
            }) {
                Ok(a2m_provider::Removal::Removed) => {}
                Ok(a2m_provider::Removal::NotThatFile { put_back: true }) => {
                    return Err(changed());
                }
                Ok(a2m_provider::Removal::NotThatFile { put_back: false }) => {
                    return Err(StepFailure::unknown(unavailable(
                        "was not removed: another file had taken its name, and it was moved aside to be looked at and could not be put back, so it keeps a name that begins `.a2m-old-`",
                    )));
                }
                Err(error) if error.kind() == ErrorKind::NotFound => return Err(changed()),
                Err(_) => {
                    return Err(StepFailure::not_sent(not_allowed(
                        "its folder cannot be written to",
                    )));
                }
            }
        }
        Kind::Folder => {
            if !matches!(held.kind(name), Ok(a2m_provider::EntryKind::Folder)) {
                return Err(changed());
            }
            // The look is through the folder reached with no link; the
            // system removes only an empty folder, so one that gained an
            // entry since is refused by the call itself.
            let inner = folder.below(name)?.held()?;
            let empty = inner
                .names()
                .map_err(|_| StepFailure::not_sent(unavailable("cannot be listed")))?
                .is_empty();
            drop(inner);
            if !empty {
                return Err(not_empty());
            }
            if !held.still_at_its_path() {
                return Err(moved_before());
            }
            before_act();
            held.remove_empty_folder(name)
                .map_err(|error| match error.kind() {
                    ErrorKind::DirectoryNotEmpty | ErrorKind::AlreadyExists => not_empty(),
                    ErrorKind::NotFound | ErrorKind::NotADirectory => changed(),
                    _ => StepFailure::not_sent(not_allowed("its folder cannot be written to")),
                })?;
        }
        Kind::Link | Kind::Other => {
            return Err(StepFailure::not_sent(not_allowed(
                "it is a link or a special file, and the file pack removes only a file or an empty folder",
            )));
        }
    }
    finish(&held, "removed")
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

    /// A root with the surface's folder `inbox` in it, and a folder outside
    /// the root, removed when dropped.
    #[cfg(unix)]
    struct Swap {
        root: PathBuf,
        outside: PathBuf,
        roots: a2m_provider::Roots,
    }

    #[cfg(unix)]
    impl Swap {
        fn new(tag: &str) -> Self {
            let base = std::env::temp_dir()
                .join(format!("a2m-pack-file-swap-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&base);
            let root = base.join("root");
            let outside = base.join("outside");
            fs::create_dir_all(root.join("inbox")).unwrap();
            fs::create_dir_all(&outside).unwrap();
            let mut roots = a2m_provider::Roots::none();
            roots
                .add(
                    a2m_provider::RootName::new("files").unwrap(),
                    &root,
                    a2m_provider::Access::Write,
                )
                .unwrap();
            Self {
                root,
                outside,
                roots,
            }
        }

        fn folder(&self) -> Folder {
            let name = a2m_provider::RootName::new("files").unwrap();
            let root = self
                .roots
                .folder(&name, a2m_provider::Access::Write)
                .unwrap();
            Folder::new(
                self.roots
                    .check(&name, "inbox", a2m_provider::Access::Write)
                    .unwrap(),
                &root,
            )
        }

        /// A moment long past, given to the folder outside the root: a name
        /// made or removed there, even for a moment, replaces it.
        fn mark_outside(&self) -> std::time::SystemTime {
            let past = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
            fs::File::open(&self.outside)
                .unwrap()
                .set_modified(past)
                .unwrap();
            past
        }

        /// What another program does between the pack's look and its act:
        /// the folder moved away inside the root, and a link to the folder
        /// outside the root put at its name.
        fn swap(&self) {
            fs::rename(self.root.join("inbox"), self.root.join("inbox-moved")).unwrap();
            std::os::unix::fs::symlink(&self.outside, self.root.join("inbox")).unwrap();
        }

        /// The names outside the root, and whether its folder was changed
        /// at all since it was marked.
        fn outside(&self, marked: std::time::SystemTime) -> (Vec<String>, bool) {
            let mut names: Vec<String> = fs::read_dir(&self.outside)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            let touched = fs::metadata(&self.outside).unwrap().modified().unwrap() != marked;
            (names, touched)
        }
    }

    #[cfg(unix)]
    impl Drop for Swap {
        fn drop(&mut self) {
            if let Some(base) = self.root.parent() {
                let _ = fs::remove_dir_all(base);
            }
        }
    }

    // Each of the three is a test-only pause point between the pack's last
    // look and its act: no other way puts a change in that moment every time.

    #[cfg(unix)]
    #[test]
    fn a_folder_swapped_for_a_link_after_the_look_gets_no_created_file_outside_the_root() {
        let swap = Swap::new("create");
        let marked = swap.mark_outside();
        let done = create_with(&swap.folder(), "new.txt", b"new bytes\n", 100, || {
            swap.swap()
        });
        assert_eq!(swap.outside(marked), (Vec::new(), false));
        let failure = done.unwrap_err();
        assert_eq!(
            failure.sent,
            a2m_provider::Sent::Unknown,
            "the act was made, in the held folder"
        );
        let moved = swap.root.join("inbox-moved");
        assert_eq!(fs::read(moved.join("new.txt")).unwrap(), b"new bytes\n");
        assert_eq!(
            fs::read_dir(&moved).unwrap().count(),
            1,
            "no temporary file is left"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_swapped_for_a_link_after_the_look_gets_no_renamed_name_outside_the_root() {
        let swap = Swap::new("rename");
        fs::write(swap.root.join("inbox").join("a.txt"), b"inside\n").unwrap();
        fs::write(swap.outside.join("a.txt"), b"outside\n").unwrap();
        let marked = swap.mark_outside();
        let folder = swap.folder();
        let read = identity(&folder, "a.txt", 0).unwrap().unwrap();
        let done = rename_with(&folder, "a.txt", "b.txt", read, 100, || swap.swap());
        assert_eq!(swap.outside(marked), (vec!["a.txt".to_owned()], false));
        assert!(done.is_err());
        assert_eq!(fs::read(swap.outside.join("a.txt")).unwrap(), b"outside\n");
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_swapped_for_a_link_after_the_look_gets_no_folder_removed_outside_the_root() {
        let swap = Swap::new("delete");
        fs::create_dir_all(swap.root.join("inbox").join("old")).unwrap();
        fs::create_dir_all(swap.outside.join("old")).unwrap();
        let marked = swap.mark_outside();
        let read = WasRead {
            identity: None,
            digest: None,
            max_bytes: 100,
        };
        let done = delete_with(&swap.folder(), "old", Kind::Folder, read, || swap.swap());
        assert_eq!(swap.outside(marked), (vec!["old".to_owned()], false));
        assert!(done.is_err());
    }

    /// A file put at the entry's name after the look is never the one
    /// removed: the delete is stale and the file stays.
    #[cfg(unix)]
    #[test]
    fn a_file_put_at_the_name_after_the_look_is_not_the_one_deleted() {
        let swap = Swap::new("replaced");
        let inbox = swap.root.join("inbox");
        fs::write(inbox.join("a.txt"), b"read\n").unwrap();
        let folder = swap.folder();
        let read = WasRead {
            identity: identity(&folder, "a.txt", 0).unwrap(),
            digest: Some(Digest::of(b"read\n")),
            max_bytes: 100,
        };
        // The file is removed and another made at its name, which the file
        // system may give the removed one's number.
        let done = delete_with(&folder, "a.txt", Kind::File, read, || {
            fs::remove_file(inbox.join("a.txt")).unwrap();
            fs::write(inbox.join("a.txt"), b"another\n").unwrap();
        });
        assert_eq!(
            done.unwrap_err().failure.code(),
            &codes::CHANGED_SINCE_PREVIEW
        );
        assert_eq!(fs::read(inbox.join("a.txt")).unwrap(), b"another\n");
        assert_eq!(fs::read_dir(&inbox).unwrap().count(), 1);
    }

    /// A name the file system takes for an entry it lists under another
    /// form of the same letters is a named stop, for a read and for a
    /// write, and not "missing" or "changed". Only macOS folds the two
    /// forms of an accented letter into one name; Linux keeps them apart.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_name_the_file_system_folds_into_a_listed_entry_is_a_named_stop() {
        let swap = Swap::new("form");
        let inbox = swap.root.join("inbox");
        fs::write(inbox.join("caf\u{e9}.txt"), b"one\n").unwrap();
        let listed = fs::read_dir(&inbox)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .file_name()
            .into_string()
            .unwrap();
        // The other form of the name the folder lists.
        let asked = if listed.contains('\u{e9}') {
            "cafe\u{301}.txt"
        } else {
            "caf\u{e9}.txt"
        };
        let folder = swap.folder();
        let conflict = |done: Result<(), StepFailure>| done.unwrap_err().failure.code().clone();
        assert_eq!(
            conflict(find(&folder, asked, 0, 100).map(|_| ())),
            codes::NAME_CASE_CONFLICT
        );
        assert_eq!(
            conflict(create(&folder, asked, b"two\n", 100)),
            codes::NAME_CASE_CONFLICT
        );
        assert_eq!(fs::read(inbox.join(&listed)).unwrap(), b"one\n");
        assert_eq!(fs::read_dir(&inbox).unwrap().count(), 1);
    }

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
