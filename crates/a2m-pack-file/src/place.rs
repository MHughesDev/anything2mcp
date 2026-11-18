//! A file by its own name: what the pack does to the file system, and how
//! each call is kept to the file the document names.
//!
//! The roots' check resolves a name that exists, and follows a link while
//! the link stays inside the root. That is right for a path a caller only
//! reads through. It is wrong for the three things this pack does by name:
//! a document's file, which a write replaces; a temporary file, which the
//! pack makes and removes; and a lock signal, which counts by being there.
//! For those the pack asks about the name itself, and never about what a
//! link at the name points to.
//!
//! No path is built from text here. A path is the surface's folder, as the
//! roots' check gave it, with one name added, and the name is one name:
//! [`is_one_name`] holds at compile time and again here.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{ErrorKind, Read as _};
use std::path::PathBuf;

use a2m_core::Digest;
use a2m_provider::{Access, CheckedPath, EntryKind, HeldFolder, StepFailure};

use crate::codes::{self, stop};
use crate::open::{held_by_another, locked, missing, not_allowed, unavailable};

/// Whether the text is one file name: no folder, no step out, no drive, no
/// placeholder.
pub(crate) fn is_one_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '{', '}', '*', '?'])
        && !name.chars().any(char::is_control)
}

/// What tells one file from another that took its name: the provider's
/// identity, the device and the file's number on Linux and macOS, and the
/// volume's serial number and the file's identifier on Windows, read from
/// a handle there (decision 0272). No other file has it while this one
/// exists, whatever is written in it.
pub(crate) type Identity = a2m_provider::FileIdentity;

/// What tells one file from another for as long as it grows, from its
/// open handle and what it showed: the device and the file's number on
/// Unix, and elsewhere the volume and the file's identifier. Text, to be
/// digested into a position and never shown as it is.
#[cfg(unix)]
pub(crate) fn lasting_identity(_file: &File, metadata: &Metadata) -> Option<String> {
    use std::os::unix::fs::MetadataExt as _;
    Some(format!("unix:{}:{}", metadata.dev(), metadata.ino()))
}

/// See the Unix form.
#[cfg(not(unix))]
pub(crate) fn lasting_identity(file: &File, _metadata: &Metadata) -> Option<String> {
    let identity = a2m_provider::FileIdentity::of_open(file).ok()?;
    let mut text = String::from("id:");
    for byte in identity.to_bytes() {
        text.push_str(&format!("{byte:02x}"));
    }
    Some(text)
}

/// How many names a file has. Only Unix says; elsewhere it is not known.
#[cfg(unix)]
fn link_count(metadata: &Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt as _;
    Some(metadata.nlink())
}

#[cfg(not(unix))]
fn link_count(_metadata: &Metadata) -> Option<u64> {
    None
}

/// The surface's folder: the path the roots' check gave, and the root's own
/// folder, from which the way down is looked at part by part.
#[derive(Debug, Clone)]
pub(crate) struct Folder {
    checked: CheckedPath,
    root: PathBuf,
    /// What the pack may do in the folder: what it was checked for, or
    /// less where the manifest declares less.
    access: Access,
}

impl Folder {
    /// The folder `checked`, inside the root whose own folder is `root`.
    pub(crate) fn new(checked: CheckedPath, root: &CheckedPath) -> Self {
        Self {
            access: checked.access(),
            checked,
            root: root.as_path().to_path_buf(),
        }
    }

    /// The same, for reading alone, whatever it was checked for: a folder
    /// of the caller's project, which the manifest declares the pack reads
    /// and never writes.
    pub(crate) fn reading(checked: CheckedPath, root: &CheckedPath) -> Self {
        Self {
            access: Access::Read,
            ..Self::new(checked, root)
        }
    }

    /// What the pack may do in the folder.
    pub(crate) fn access(&self) -> Access {
        self.access
    }

    /// The folder as the roots' check resolved it, for listing its entries.
    /// Never a base to build a path from text: a name is added only through
    /// [`Folder::beside`], [`Folder::file`], or [`Folder::below`].
    pub(crate) fn listed_path(&self) -> Result<PathBuf, StepFailure> {
        self.present()?;
        Ok(self.checked.as_path().to_path_buf())
    }

    /// A folder below this one, by a relative path whose parts are each one
    /// name, for reading a folder's entries a declared depth down. The
    /// roots' check sees the path, and refuses a form no path may take and a
    /// link that leads out of the root; a link at any part is refused here,
    /// whatever it leads to.
    pub(crate) fn below(&self, relative: &str) -> Result<Self, StepFailure> {
        let mut at = self.checked.as_path().to_path_buf();
        for part in relative.split('/') {
            if !is_one_name(part) {
                return Err(StepFailure::not_sent(not_allowed(
                    "its name is not a name inside the surface's folder",
                )));
            }
            at.push(part);
            match fs::symlink_metadata(&at) {
                Ok(found) if found.file_type().is_dir() => {}
                Ok(found) if found.file_type().is_symlink() => {
                    return Err(StepFailure::not_sent(not_allowed(
                        "a link stands in the way to it, and a folder's entries are reached by no link",
                    )));
                }
                Ok(_) => {
                    return Err(StepFailure::not_sent(not_allowed(
                        "a part of its name is not a folder",
                    )));
                }
                Err(error) if gone(&error) => return Err(missing()),
                Err(_) => {
                    return Err(StepFailure::not_sent(unavailable(
                        "is in a folder that cannot be looked at",
                    )));
                }
            }
        }
        let checked = self.checked.join(relative).map_err(StepFailure::not_sent)?;
        Ok(Self {
            checked,
            root: self.root.clone(),
            access: self.access,
        })
    }

    /// Looks, at the moment of use, that the folder is still the folder the
    /// roots' check resolved: there, a folder, and reached by no link that
    /// was placed since.
    fn present(&self) -> Result<(), StepFailure> {
        if self.checked.is_dir() {
            return Ok(());
        }
        Err(match fs::symlink_metadata(self.checked.as_path()) {
            Err(error) if gone(&error) => missing(),
            _ => StepFailure::not_sent(not_allowed(
                "its folder is no longer the folder that was checked",
            )),
        })
    }

    /// For a write: no part of the way from the root down to the folder is
    /// a link. The roots' check follows such a link while it stays inside
    /// the root, which a read may use; a write through it would land in a
    /// folder the surface does not name.
    pub(crate) fn reached_by_no_link(&self) -> Result<(), StepFailure> {
        let mut at = self.root.clone();
        for part in self.checked.relative().split('/') {
            if part.is_empty() {
                continue;
            }
            at.push(part);
            match fs::symlink_metadata(&at) {
                Ok(found) if found.file_type().is_symlink() => {
                    return Err(StepFailure::not_sent(not_allowed(
                        "a link stands in the way to its folder, so a write would land in a folder the surface does not name",
                    )));
                }
                Ok(_) => {}
                Err(error) if gone(&error) => return Err(missing()),
                Err(_) => {
                    return Err(StepFailure::not_sent(unavailable(
                        "is in a folder that cannot be looked at",
                    )));
                }
            }
        }
        self.present()
    }

    /// For a write of a folder's entries: the folder, held for the calls
    /// that follow, so each acts in this one folder whatever is put at its
    /// path meanwhile (decision 0272). No part of the way from the root is
    /// a link as the surface names it, and the folder is reached again from
    /// the root down with no link followed.
    pub(crate) fn held(&self) -> Result<HeldFolder, StepFailure> {
        self.reached_by_no_link()?;
        self.checked.hold_folder().map_err(|failure| {
            if failure.code() == &a2m_provider::codes::PATH_MISSING {
                missing()
            } else if failure.code() == &a2m_provider::codes::PATH_NOT_ALLOWED {
                StepFailure::not_sent(not_allowed(
                    "a link stands in the way to its folder, so a write would land in a folder the surface does not name",
                ))
            } else {
                StepFailure::not_sent(unavailable("is in a folder that cannot be held"))
            }
        })
    }

    /// The path of one name in the folder: the name itself, whatever is
    /// there, with no link at it followed.
    pub(crate) fn beside(&self, name: &str) -> Result<PathBuf, StepFailure> {
        if !is_one_name(name) {
            return Err(StepFailure::not_sent(not_allowed(
                "its name is not one name inside the surface's folder",
            )));
        }
        self.present()?;
        Ok(self.checked.as_path().join(name))
    }

    /// A file of the folder, by one name. The roots' check sees the name
    /// too, and refuses a form no path may take and a link that leads out
    /// of the root.
    pub(crate) fn file(&self, name: &str) -> Result<Named, StepFailure> {
        let path = self.beside(name)?;
        let checked = self.checked.join(name).map_err(StepFailure::not_sent)?;
        Ok(Named {
            folder: self.clone(),
            path,
            checked,
        })
    }

    /// Whether anything at all is at a sibling's name: a file, a folder, or
    /// a link, wherever the link points and whether or not it points at
    /// anything. A lock signal counts by being there.
    pub(crate) fn holds(&self, name: &str) -> Result<bool, StepFailure> {
        // The pack's own look, not the provider's: the roots' check cannot
        // join a name that is a link to nothing, and that name is a signal.
        match fs::symlink_metadata(self.beside(name)?) {
            Ok(_) => Ok(true),
            Err(error) if gone(&error) => Ok(false),
            // It cannot be told whether the signal is there.
            Err(_) => Err(StepFailure::not_sent(unavailable(
                "has a lock signal that cannot be looked at",
            ))),
        }
    }
}

fn gone(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory)
}

/// One file of a surface's folder, by its own name.
#[derive(Debug, Clone)]
pub(crate) struct Named {
    folder: Folder,
    path: PathBuf,
    /// The same name, as the roots' check gave it, for the provider's own
    /// look at the name itself.
    checked: CheckedPath,
}

impl Named {
    /// The folder the file is in.
    pub(crate) fn folder(&self) -> &Folder {
        &self.folder
    }

    /// The same name, as the roots' check gave it, for the provider
    /// contract's own calls on it: whether the engine's user may write it,
    /// and the flush of its folder.
    pub(crate) fn checked(&self) -> &CheckedPath {
        &self.checked
    }

    /// What is at the name itself, with no link followed: a regular file
    /// with one name, or a refusal. A link at the name is refused wherever
    /// it points: a document's file is the file under its own name.
    pub(crate) fn regular(&self) -> Result<Metadata, StepFailure> {
        self.folder.present()?;
        // The provider's look at the name itself, which never follows a link.
        match self.checked.entry_kind() {
            Ok(EntryKind::File) => {}
            Ok(EntryKind::Absent) => return Err(missing()),
            Ok(EntryKind::Link) => {
                return Err(StepFailure::not_sent(not_allowed(
                    "it is a link, and a document's file is a regular file under its own name",
                )));
            }
            Ok(EntryKind::Folder | EntryKind::Other) => {
                return Err(StepFailure::not_sent(not_allowed(
                    "it is not a regular file",
                )));
            }
            Err(_) => {
                return Err(StepFailure::not_sent(unavailable("cannot be looked at")));
            }
        }
        // Its length, identity, and number of names: what the kind does not say.
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) if metadata.file_type().is_file() => metadata,
            Ok(_) => {
                return Err(StepFailure::not_sent(not_allowed(
                    "it changed while it was looked at",
                )));
            }
            Err(error) if gone(&error) => return Err(missing()),
            Err(_) => {
                return Err(StepFailure::not_sent(unavailable("cannot be looked at")));
            }
        };
        if link_count(&metadata).is_some_and(|links| links != 1) {
            return Err(StepFailure::not_sent(not_allowed(
                "it has more than one name, so replacing it would leave the other name on the old file",
            )));
        }
        Ok(metadata)
    }

    /// Opens the file for reading, and proves that what was opened is the
    /// regular file at the name: the open handle and the name show one
    /// identity. A link put at the name between the look and the open is
    /// caught here, and nothing is read through it. Whether the engine's
    /// user may write the file is the contract's question
    /// (`CheckedPath::may_write`), asked at the save.
    pub(crate) fn open(&self) -> Result<(File, Metadata), StepFailure> {
        self.regular()?;
        let file = OpenOptions::new()
            .read(true)
            .open(&self.path)
            .map_err(|error| {
                if held_by_another(&error) {
                    StepFailure::not_sent(locked())
                } else if gone(&error) {
                    missing()
                } else {
                    StepFailure::not_sent(unavailable("cannot be opened for reading"))
                }
            })?;
        let opened = file
            .metadata()
            .map_err(|_| StepFailure::not_sent(unavailable("cannot be looked at once open")))?;
        let named = self.regular()?;
        // The handle's own identity, and the name's, each from the system:
        // on Windows from a handle, so a file put at the name since, which
        // may carry the old one's creation time, is told apart.
        let by_handle = a2m_provider::FileIdentity::of_open(&file).ok();
        let by_name = self.checked.identity().ok();
        if !opened.file_type().is_file() || by_handle.is_none() || by_handle != by_name {
            return Err(StepFailure::not_sent(unavailable(
                "was put under its name while it was being opened, so it was not used",
            )));
        }
        Ok((file, named))
    }

    /// The whole file, read once, within `max_bytes`. Nothing when it has
    /// more.
    pub(crate) fn read(&self, max_bytes: u64) -> Result<Option<Vec<u8>>, StepFailure> {
        let (file, metadata) = self.open()?;
        if metadata.len() > max_bytes {
            return Ok(None);
        }
        let mut bytes = Vec::new();
        file.take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| {
                StepFailure::not_sent(if held_by_another(&error) {
                    locked()
                } else {
                    unavailable("cannot be read")
                })
            })?;
        Ok((bytes.len() as u64 <= max_bytes).then_some(bytes))
    }

    /// The digest of the regular file at the name now, when it can be read.
    pub(crate) fn digest(&self, max_bytes: u64) -> Option<Digest> {
        self.read(max_bytes)
            .ok()
            .flatten()
            .map(|bytes| Digest::of(&bytes))
    }
}

/// The engine's user may not write the file itself.
pub(crate) fn not_writable() -> StepFailure {
    stop(
        codes::FILE_PROTECTED,
        "The engine's user may not write the file, and the file pack replaces no file it could not write.",
        "Give the engine's user the right to write the file if it is meant to be changed.",
    )
}
