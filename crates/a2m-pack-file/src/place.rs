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
use std::path::{Path, PathBuf};

use a2m_core::Digest;
use a2m_provider::{Access, CheckedPath, StepFailure};

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

/// What tells one file from another that took its name. On Unix it is the
/// device and the file's number, which no other file has. Elsewhere safe
/// code is given neither, and the moment the file was made, with its
/// length, stands in for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Identity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    file: u64,
    #[cfg(not(unix))]
    created: std::time::SystemTime,
    #[cfg(not(unix))]
    len: u64,
}

impl Identity {
    /// The identity the metadata shows, where the system gives one.
    #[cfg(unix)]
    pub(crate) fn of(metadata: &Metadata) -> Option<Self> {
        use std::os::unix::fs::MetadataExt as _;
        Some(Self {
            device: metadata.dev(),
            file: metadata.ino(),
        })
    }

    /// The identity the metadata shows, where the system gives one.
    #[cfg(not(unix))]
    pub(crate) fn of(metadata: &Metadata) -> Option<Self> {
        metadata.created().ok().map(|created| Self {
            created,
            len: metadata.len(),
        })
    }
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
}

impl Folder {
    /// The folder `checked`, inside the root whose own folder is `root`.
    pub(crate) fn new(checked: CheckedPath, root: &CheckedPath) -> Self {
        Self {
            checked,
            root: root.as_path().to_path_buf(),
        }
    }

    /// What the folder was checked for.
    pub(crate) fn access(&self) -> Access {
        self.checked.access()
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
        self.checked.join(name).map_err(StepFailure::not_sent)?;
        Ok(Named {
            folder: self.clone(),
            path,
        })
    }

    /// Whether anything at all is at a sibling's name: a file, a folder, or
    /// a link, wherever the link points and whether or not it points at
    /// anything. A lock signal counts by being there.
    pub(crate) fn holds(&self, name: &str) -> Result<bool, StepFailure> {
        match fs::symlink_metadata(self.beside(name)?) {
            Ok(_) => Ok(true),
            Err(error) if gone(&error) => Ok(false),
            // It cannot be told whether the signal is there.
            Err(_) => Err(StepFailure::not_sent(unavailable(
                "has a lock signal that cannot be looked at",
            ))),
        }
    }

    /// Flushes the folder, so that a new name in it survives a loss of
    /// power. Only where the system allows a folder to be opened and flushed.
    #[cfg(unix)]
    pub(crate) fn flush(&self) {
        if let Ok(folder) = File::open(self.checked.as_path()) {
            let _ = folder.sync_all();
        }
    }

    /// Flushes the folder, where the system allows it. Here it does not.
    #[cfg(not(unix))]
    pub(crate) fn flush(&self) {}
}

fn gone(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory)
}

/// What a file is opened for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Open {
    /// Reading its bytes.
    Read,
    /// Nothing is written through the handle: the open itself asks the
    /// operating system whether the engine's user may write the file.
    WriteProbe,
}

/// One file of a surface's folder, by its own name.
#[derive(Debug, Clone)]
pub(crate) struct Named {
    folder: Folder,
    path: PathBuf,
}

impl Named {
    /// The folder the file is in.
    pub(crate) fn folder(&self) -> &Folder {
        &self.folder
    }

    /// The path of the name itself, for a call that follows no link at the
    /// last part: a look at the name, and a rename onto it.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// What is at the name itself, with no link followed: a regular file
    /// with one name, or a refusal. A link at the name is refused wherever
    /// it points: a document's file is the file under its own name.
    pub(crate) fn regular(&self) -> Result<Metadata, StepFailure> {
        self.folder.present()?;
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if gone(&error) => return Err(missing()),
            Err(_) => {
                return Err(StepFailure::not_sent(unavailable("cannot be looked at")));
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(StepFailure::not_sent(not_allowed(
                "it is a link, and a document's file is a regular file under its own name",
            )));
        }
        if !metadata.file_type().is_file() {
            return Err(StepFailure::not_sent(not_allowed(
                "it is not a regular file",
            )));
        }
        if link_count(&metadata).is_some_and(|links| links != 1) {
            return Err(StepFailure::not_sent(not_allowed(
                "it has more than one name, so replacing it would leave the other name on the old file",
            )));
        }
        Ok(metadata)
    }

    /// Opens the file, and proves that what was opened is the regular file
    /// at the name: the open handle and the name show one identity. A link
    /// put at the name between the look and the open is caught here, and
    /// nothing is read or written through it.
    pub(crate) fn open(&self, how: Open) -> Result<(File, Metadata), StepFailure> {
        self.regular()?;
        let mut options = OpenOptions::new();
        match how {
            Open::Read => options.read(true),
            Open::WriteProbe => options.write(true),
        };
        let file = options.open(&self.path).map_err(|error| {
            if held_by_another(&error) {
                StepFailure::not_sent(locked())
            } else if gone(&error) {
                missing()
            } else if how == Open::WriteProbe
                && matches!(
                    error.kind(),
                    ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem
                )
            {
                not_writable()
            } else if how == Open::WriteProbe {
                StepFailure::not_sent(unavailable("cannot be opened for writing"))
            } else {
                StepFailure::not_sent(unavailable("cannot be opened for reading"))
            }
        })?;
        let opened = file
            .metadata()
            .map_err(|_| StepFailure::not_sent(unavailable("cannot be looked at once open")))?;
        let named = self.regular()?;
        if !opened.file_type().is_file() || Identity::of(&opened) != Identity::of(&named) {
            return Err(StepFailure::not_sent(unavailable(
                "was put under its name while it was being opened, so it was not used",
            )));
        }
        Ok((file, named))
    }

    /// The whole file, read once, within `max_bytes`. Nothing when it has
    /// more.
    pub(crate) fn read(&self, max_bytes: u64) -> Result<Option<Vec<u8>>, StepFailure> {
        let (file, metadata) = self.open(Open::Read)?;
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
