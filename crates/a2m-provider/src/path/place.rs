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

    use rustix::fs::{
        AtFlags, CWD, FileType, Mode, OFlags, mkdirat, openat, renameat, statat, unlinkat,
    };
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

        /// Makes a new file of that name for reading and writing. It fails
        /// when anything stands at the name, a link included.
        pub(in super::super) fn create_new(&self, name: &OsStr) -> io::Result<File> {
            let handle = openat(
                &self.0,
                name,
                OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                // Private to the engine's user: the new bytes are read by
                // nobody else before the file is put in place.
                Mode::from_raw_mode(0o600),
            )?;
            Ok(File::from(handle))
        }

        /// Gives `file` the owner, the group, and the permission bits of
        /// what stands at `name`, which is looked at without following a
        /// link, through the file's own handle, and then checks that it has
        /// them. `false` where any of the three cannot be kept: the target
        /// is not there or is no regular file, it has a set-user, set-group,
        /// or sticky bit, or the system refuses the owner or the group to
        /// this user.
        pub(in super::super) fn keep_owner_and_mode(
            &self,
            name: &OsStr,
            file: &File,
        ) -> io::Result<bool> {
            let target = match statat(&self.0, name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(target) => target,
                Err(Errno::NOENT) => return Ok(false),
                Err(error) => return Err(error.into()),
            };
            if FileType::from_raw_mode(target.st_mode) != FileType::RegularFile {
                return Ok(false);
            }
            #[allow(
                clippy::useless_conversion,
                reason = "the width of the mode differs between operating systems"
            )]
            let bits = u32::from(target.st_mode) & 0o7777;
            if bits & 0o7000 != 0 {
                return Ok(false);
            }
            let own = rustix::fs::fstat(file)?;
            if own.st_uid != target.st_uid || own.st_gid != target.st_gid {
                match rustix::fs::fchown(
                    file,
                    Some(rustix::fs::Uid::from_raw(target.st_uid)),
                    Some(rustix::fs::Gid::from_raw(target.st_gid)),
                ) {
                    Ok(()) => {}
                    Err(Errno::PERM | Errno::ACCESS) => return Ok(false),
                    Err(error) => return Err(error.into()),
                }
            }
            // After the owner: a change of owner may clear bits.
            rustix::fs::fchmod(file, Mode::from_raw_mode(bits as _))?;
            let now = rustix::fs::fstat(file)?;
            #[allow(
                clippy::useless_conversion,
                reason = "the width of the mode differs between operating systems"
            )]
            let kept = u32::from(now.st_mode) & 0o7777 == bits
                && now.st_uid == target.st_uid
                && now.st_gid == target.st_gid;
            Ok(kept)
        }

        /// Opens the file of that name for writing, and changes nothing in
        /// it: no new file, no truncation. A link there is not followed.
        pub(in super::super) fn open_for_write(&self, name: &OsStr) -> io::Result<File> {
            let handle = openat(
                &self.0,
                name,
                OFlags::WRONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )?;
            Ok(File::from(handle))
        }

        /// The device and the file number of what stands at the name. A link
        /// is not followed.
        pub(in super::super) fn identity(&self, name: &OsStr) -> io::Result<(u64, u128)> {
            let found = statat(&self.0, name, AtFlags::SYMLINK_NOFOLLOW)?;
            #[allow(
                clippy::useless_conversion,
                reason = "the width of these fields differs between operating systems"
            )]
            Ok((u64::from(found.st_dev), u128::from(found.st_ino)))
        }

        /// Writes the folder's list of names to the disk, so that a name
        /// just given survives a power cut.
        pub(in super::super) fn sync(&self) -> io::Result<()> {
            Ok(rustix::fs::fsync(&self.0)?)
        }
    }

    /// The device and the file number of an open file.
    pub(in super::super) fn handle_identity(file: &File) -> io::Result<(u64, u128)> {
        let found = rustix::fs::fstat(file)?;
        #[allow(
            clippy::useless_conversion,
            reason = "the width of these fields differs between operating systems"
        )]
        Ok((u64::from(found.st_dev), u128::from(found.st_ino)))
    }

    impl Folder {

        /// Makes a new folder of that name. It fails when anything stands
        /// at the name, a link included.
        pub(in super::super) fn create_dir(&self, name: &OsStr) -> io::Result<()> {
            Ok(mkdirat(&self.0, name, Mode::from_raw_mode(0o777))?)
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

#[cfg(windows)]
mod windows {
    use std::fs::File;
    use std::io;
    use std::os::windows::io::AsRawHandle as _;

    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    /// The volume's serial number and the file's index, from an open
    /// handle.
    #[allow(unsafe_code)] // one call into the operating system, explained below
    pub(super) fn identity(file: &File) -> io::Result<(u64, u128)> {
        let mut info = BY_HANDLE_FILE_INFORMATION {
            dwFileAttributes: 0,
            ftCreationTime: windows_sys::Win32::Foundation::FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            },
            ftLastAccessTime: windows_sys::Win32::Foundation::FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            },
            ftLastWriteTime: windows_sys::Win32::Foundation::FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            },
            dwVolumeSerialNumber: 0,
            nFileSizeHigh: 0,
            nFileSizeLow: 0,
            nNumberOfLinks: 0,
            nFileIndexHigh: 0,
            nFileIndexLow: 0,
        };
        // SAFETY: the handle is the open file's own and stays open for the
        // call, which only writes the structure it is given, a structure
        // of plain numbers owned by this frame.
        let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let index = (u128::from(info.nFileIndexHigh) << 32) | u128::from(info.nFileIndexLow);
        Ok((u64::from(info.dwVolumeSerialNumber), index))
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

    /// The volume and the file identifier of an open file.
    #[cfg(windows)]
    pub(in super::super) fn handle_identity(file: &File) -> io::Result<(u64, u128)> {
        super::windows::identity(file)
    }

    /// No identity can be had of a file on this system.
    #[cfg(not(windows))]
    pub(in super::super) fn handle_identity(_file: &File) -> io::Result<(u64, u128)> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

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

        /// Makes a new file of that name for reading and writing. It fails
        /// when anything stands at the name, a link included.
        pub(in super::super) fn create_new(&self, name: &OsStr) -> io::Result<File> {
            let mut options = fs::OpenOptions::new();
            options.read(true).write(true).create_new(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt as _;
                options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
            }
            options.open(self.0.join(name))
        }

        /// Makes a new folder of that name. It fails when anything stands
        /// at the name, a link included.
        pub(in super::super) fn create_dir(&self, name: &OsStr) -> io::Result<()> {
            fs::create_dir(self.0.join(name))
        }

        /// Opens the file of that name for writing, and changes nothing in
        /// it: no new file, no truncation. A link there is opened itself, and
        /// the caller has refused one before.
        pub(in super::super) fn open_for_write(&self, name: &OsStr) -> io::Result<File> {
            let mut options = fs::OpenOptions::new();
            options.write(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt as _;
                options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
            }
            options.open(self.0.join(name))
        }

        /// The volume and the file identifier of what stands at the name.
        #[cfg(windows)]
        pub(in super::super) fn identity(&self, name: &OsStr) -> io::Result<(u64, u128)> {
            use std::os::windows::fs::OpenOptionsExt as _;
            let mut options = fs::OpenOptions::new();
            // Neither read nor write: the file's information alone, of the
            // link itself where one stands.
            options
                .access_mode(0)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
            let file = options.open(self.0.join(name))?;
            super::windows::identity(&file)
        }

        /// No identity can be had of a file on this system.
        #[cfg(not(windows))]
        pub(in super::super) fn identity(&self, _name: &OsStr) -> io::Result<(u64, u128)> {
            Err(io::Error::from(io::ErrorKind::Unsupported))
        }

        /// Windows keeps no owner, group, or permission bits a program can
        /// copy through the standard library: a file's access is its
        /// security descriptor, and a new file takes the one its folder
        /// passes down. The read-only flag is kept through the file's own
        /// handle; the owner and the access list are not, so this answers
        /// `false` (decision 0176).
        pub(in super::super) fn keep_owner_and_mode(
            &self,
            name: &OsStr,
            file: &File,
        ) -> io::Result<bool> {
            match fs::symlink_metadata(self.0.join(name)) {
                Ok(target) if target.file_type().is_file() => {
                    file.set_permissions(target.permissions())?;
                    Ok(false)
                }
                Ok(_) => Ok(false),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
                Err(error) => Err(error),
            }
        }

        /// Writes the folder's list of names to the disk. Windows keeps a
        /// folder's names in its file system's journal and gives no handle
        /// to a folder that could be flushed, so there is nothing more to do.
        #[allow(
            clippy::unnecessary_wraps,
            clippy::unused_self,
            reason = "the same call as on the other systems, where it can fail"
        )]
        pub(in super::super) fn sync(&self) -> io::Result<()> {
            Ok(())
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

pub(super) use os::{Folder, handle_identity};
