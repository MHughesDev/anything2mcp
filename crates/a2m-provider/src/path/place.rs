//! A folder inside a root, reached at the moment of use, and what is done
//! to one name in it.
//!
//! On Linux and macOS the folder is a handle: it is opened from the root
//! down, one folder at a time, and no step follows a link. Everything is
//! then done relative to that handle, so nothing a person or a program does
//! to the path afterwards can lead the work elsewhere. The resolved path a
//! [`CheckedPath`](super::CheckedPath) holds has no link in it, so a link
//! met on the way down was put there after the check.
//!
//! On Windows the standard library has no way to open a name relative to a
//! folder's handle. The folder is resolved again and must lie inside the
//! root, and is then used by name: a link swapped in between those two
//! moments is followed. Making such a link there needs the right to write
//! in a folder of the root.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::path::Path;

use super::EntryKind;

/// Why a folder was not reached.
pub(super) enum Unreached {
    /// It, or a folder above it, is not there.
    Absent,
    /// A link, or something that is no folder, stands on the way; or the
    /// folder is not inside the root.
    Outside,
    /// The operating system refused for another reason.
    Other,
}

#[cfg(unix)]
mod os {
    use std::os::fd::OwnedFd;
    use std::path::Component;

    use rustix::fs::{AtFlags, CWD, FileType, Mode, OFlags, openat, renameat, statat, unlinkat};
    use rustix::io::Errno;

    use super::{EntryKind, File, OsStr, Path, Unreached, io};

    /// A folder inside a root, held open.
    pub(in super::super) struct Folder(OwnedFd);

    impl Folder {
        /// Opens `folder`, which lies under `base`, from `base` down. No
        /// step follows a link.
        pub(in super::super) fn reach(base: &Path, folder: &Path) -> Result<Self, Unreached> {
            let below = folder.strip_prefix(base).map_err(|_| Unreached::Outside)?;
            let unreached = |error: Errno| match error {
                Errno::NOENT => Unreached::Absent,
                // A link where a folder was asked for without following is
                // answered as one or the other of these.
                Errno::LOOP | Errno::NOTDIR => Unreached::Outside,
                _ => Unreached::Other,
            };
            // The root was resolved when it was added, and is opened as it
            // is named.
            let folders = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
            let mut handle = openat(CWD, base, folders, Mode::empty()).map_err(unreached)?;
            for part in below.components() {
                let Component::Normal(part) = part else {
                    return Err(Unreached::Outside);
                };
                handle = openat(&handle, part, folders | OFlags::NOFOLLOW, Mode::empty())
                    .map_err(unreached)?;
            }
            Ok(Self(handle))
        }

        /// What stands at `name` in the folder. A link is not followed.
        pub(in super::super) fn kind(&self, name: &OsStr) -> io::Result<EntryKind> {
            match statat(&self.0, name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(found) => Ok(match FileType::from_raw_mode(found.st_mode) {
                    FileType::RegularFile => EntryKind::File,
                    FileType::Directory => EntryKind::Folder,
                    FileType::Symlink => EntryKind::Link,
                    _ => EntryKind::Other,
                }),
                Err(Errno::NOENT) => Ok(EntryKind::Absent),
                Err(error) => Err(error.into()),
            }
        }

        /// Makes a new file of that name for writing. It fails when
        /// anything stands at the name, a link included.
        pub(in super::super) fn create_new(&self, name: &OsStr) -> io::Result<File> {
            let handle = openat(
                &self.0,
                name,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o666),
            )?;
            Ok(File::from(handle))
        }

        /// Opens the file of that name for reading. A link there is not
        /// followed.
        pub(in super::super) fn open(&self, name: &OsStr) -> io::Result<File> {
            let handle = openat(
                &self.0,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?;
            Ok(File::from(handle))
        }

        /// Gives the file `from` the name `to`, both in this folder. What
        /// stood at `to` is replaced, and a link there is not followed.
        pub(in super::super) fn rename(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
            Ok(renameat(&self.0, from, &self.0, to)?)
        }

        /// Removes the name from the folder. A link there is not followed.
        pub(in super::super) fn remove(&self, name: &OsStr) -> io::Result<()> {
            Ok(unlinkat(&self.0, name, AtFlags::empty())?)
        }
    }
}

#[cfg(not(unix))]
mod os {
    use std::fs;
    use std::path::PathBuf;

    use super::{EntryKind, File, OsStr, Path, Unreached, io};

    /// Opens a link itself and not what it leads to. With it, making a new
    /// file fails where a link stands at the name, whatever the link leads
    /// to: without it the system would make the file the link names.
    #[cfg(windows)]
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

    /// A folder inside a root, resolved at this moment.
    pub(in super::super) struct Folder(PathBuf);

    impl Folder {
        /// Resolves `folder` now. It must lie inside `base`.
        pub(in super::super) fn reach(base: &Path, folder: &Path) -> Result<Self, Unreached> {
            let real = match super::super::canonical(folder) {
                Ok(real) => real,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Err(Unreached::Absent);
                }
                Err(_) => return Err(Unreached::Other),
            };
            if real.starts_with(base) && real.is_dir() {
                Ok(Self(real))
            } else {
                Err(Unreached::Outside)
            }
        }

        /// What stands at `name` in the folder. A link is not followed.
        pub(in super::super) fn kind(&self, name: &OsStr) -> io::Result<EntryKind> {
            match fs::symlink_metadata(self.0.join(name)) {
                Ok(found) => {
                    let kind = found.file_type();
                    Ok(if kind.is_symlink() {
                        EntryKind::Link
                    } else if kind.is_dir() {
                        EntryKind::Folder
                    } else if kind.is_file() {
                        EntryKind::File
                    } else {
                        EntryKind::Other
                    })
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(EntryKind::Absent),
                Err(error) => Err(error),
            }
        }

        /// Makes a new file of that name for writing. It fails when
        /// anything stands at the name, a link included.
        pub(in super::super) fn create_new(&self, name: &OsStr) -> io::Result<File> {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt as _;
                options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
            }
            options.open(self.0.join(name))
        }

        /// Gives the file `from` the name `to`, both in this folder. What
        /// stood at `to` is replaced, and a link there is not followed.
        pub(in super::super) fn rename(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
            fs::rename(self.0.join(from), self.0.join(to))
        }

        /// Removes the name from the folder. A link there is not followed.
        pub(in super::super) fn remove(&self, name: &OsStr) -> io::Result<()> {
            fs::remove_file(self.0.join(name))
        }
    }
}

pub(super) use os::Folder;
