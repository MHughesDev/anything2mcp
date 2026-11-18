//! The check of who may change a program as it stood before decision 0250
//! moved it into the provider contract, kept for one test alone: the table
//! test in `safety.rs` gives the old and the new check the same cases and
//! asks for the same verdict. The Linux and macOS half only: the Windows
//! half moved without a change to what it decides. Nothing else uses it.

#![allow(dead_code, reason = "a frozen copy, compared with what replaced it")]

use std::io::Read as _;
use std::path::{Path, PathBuf};

use a2m_provider::is_shell_or_interpreter;

/// Why a program's file may not be started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Refused {
    /// The path is not absolute.
    NotAbsolute,
    /// Nothing is there, or it is not a file, or it cannot be run.
    Missing,
    /// It is a shell, an interpreter, or a script one of them would run.
    Script,
    /// It, or the folder or link named, can be written by another user.
    Writable(PathBuf),
    /// Who may change it, or the folder named, could not be read: an access
    /// list that cannot be read, or holds an entry of a kind this check
    /// does not read, is a refusal, never a pass.
    #[cfg_attr(not(windows), allow(dead_code))]
    Unreadable(PathBuf),
    /// A link, or another kind of reparse point, on the way: on Windows
    /// none is followed.
    #[cfg_attr(not(windows), allow(dead_code))]
    Link(PathBuf),
    /// A path on another machine's share, or a device: its folders are not
    /// this machine's to vouch for.
    #[cfg_attr(not(windows), allow(dead_code))]
    Elsewhere,
}

/// What the file is started as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    /// A program that takes its arguments as data. A shell, an interpreter,
    /// or a program that starts another is refused by name.
    Program,
    /// The interpreter of a fixed pair: it is given a script the target
    /// keeps, and the name list does not refuse it. Every other check holds.
    Interpreter,
}

/// The groups whose write, on Unix, counts as the system's own, as the
/// administrator's does: a member could replace the file as the
/// administrator anyway.
///
/// On macOS the group with id 80, to which the system gives its
/// administrators, is always one: its members become the superuser through
/// the system's own prompt. On Linux no group is one by default: a group's
/// id says nothing about who is in it. Any other group is one only when the
/// person names its numeric id in the protected setting
/// `packs.command.admin_groups`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Trust {
    named: Vec<u32>,
}

/// The groups the system itself gives only to its administrators.
#[cfg(target_os = "macos")]
const SYSTEM_ADMIN_GROUPS: &[u32] = &[80];
#[cfg(not(target_os = "macos"))]
const SYSTEM_ADMIN_GROUPS: &[u32] = &[];

impl Trust {
    /// The system's own groups and those a person named.
    #[must_use]
    pub(super) fn with_groups(named: Vec<u32>) -> Self {
        Self { named }
    }

    /// Whether a write by the group `id` counts as the system's own.
    #[cfg_attr(not(unix), allow(dead_code))]
    fn holds(&self, id: u32) -> bool {
        SYSTEM_ADMIN_GROUPS.contains(&id) || self.named.contains(&id)
    }
}

/// Checks the file at `path`, to be started as `role`.
///
/// # Errors
///
/// The first reason found.
pub(super) fn check(path: &Path, trust: &Trust, role: Role) -> Result<(), Refused> {
    if !path.is_absolute() {
        return Err(Refused::NotAbsolute);
    }
    let by_name = role == Role::Program;
    if by_name && is_shell_or_interpreter(&path.to_string_lossy()) {
        return Err(Refused::Script);
    }
    // Every folder and link from the declared path to the file itself.
    let real = os::walk(path, trust)?;
    let metadata = std::fs::metadata(&real).map_err(|_| Refused::Missing)?;
    if !metadata.is_file() {
        return Err(Refused::Missing);
    }
    if by_name && is_shell_or_interpreter(&real.to_string_lossy()) {
        return Err(Refused::Script);
    }
    let mut start = [0_u8; 2];
    let read = std::fs::File::open(&real)
        .and_then(|mut file| file.read(&mut start))
        .map_err(|_| Refused::Missing)?;
    // A script is never started as a program, nor as an interpreter: an
    // interpreter that is itself a script hands its own text to another.
    if read == 2 && &start == b"#!" {
        return Err(Refused::Script);
    }
    os::owned_by_us(&real, &metadata, trust)
}

#[cfg(unix)]
mod os {
    use std::ffi::OsString;
    use std::os::unix::fs::MetadataExt as _;
    use std::path::{Component, Path, PathBuf};

    use super::{Refused, Trust};

    const OTHERS_WRITE: u32 = 0o002;
    const GROUP_WRITE: u32 = 0o020;
    const STICKY: u32 = 0o1000;
    const ANY_EXECUTE: u32 = 0o111;
    /// How many links one path may lead through, as the system itself
    /// allows; more is a circle.
    const MOST_LINKS: u32 = 40;

    /// Whether the engine's user or the administrator owns it.
    fn ours(metadata: &std::fs::Metadata, us: u32) -> bool {
        metadata.uid() == us || metadata.uid() == 0
    }

    /// Whether only the engine's user or the administrator can change it.
    ///
    /// A file or folder a group may write is refused unless `trust` holds
    /// the group: who a group's members are is not known here, and a member
    /// who is not the engine's user is another user, unless the system, or
    /// the person in a protected setting, says the group is its
    /// administrators'.
    fn closed(metadata: &std::fs::Metadata, us: u32, folder: bool, trust: &Trust) -> bool {
        let mode = metadata.mode();
        if !ours(metadata, us) {
            return false;
        }
        // In a folder with the sticky mark, only an entry's owner may
        // replace it, so what we own there stays ours.
        let sticky = folder && mode & STICKY != 0;
        let group_closed = mode & GROUP_WRITE == 0 || trust.holds(metadata.gid());
        (mode & OTHERS_WRITE == 0 && group_closed) || sticky
    }

    /// The names of a path, last first, for a stack. `..` is kept as a
    /// name; the root and `.` are not names.
    fn names(path: &Path) -> impl Iterator<Item = OsString> {
        path.components().rev().filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_owned()),
            Component::ParentDir => Some(OsString::from("..")),
            Component::Prefix(_) | Component::RootDir | Component::CurDir => None,
        })
    }

    /// Walks an absolute path name by name, as the system does when it
    /// starts a program, and gives the path with no link left in it.
    ///
    /// Every folder entered must be closed to other users, and every link
    /// must be ours or the administrator's; where a link leads is walked
    /// the same way. So when this passes, nobody else can change what the
    /// declared path names between this check and a start.
    pub(super) fn walk(declared: &Path, trust: &Trust) -> Result<PathBuf, Refused> {
        let us = rustix::process::getuid().as_raw();
        let root = PathBuf::from("/");
        let top = std::fs::metadata(&root).map_err(|_| Refused::Missing)?;
        if !closed(&top, us, true, trust) {
            return Err(Refused::Writable(root));
        }
        let mut at = root.clone();
        let mut ahead: Vec<OsString> = names(declared).collect();
        let mut links = 0;
        while let Some(name) = ahead.pop() {
            if name == ".." {
                // `at` holds no link, so its parent is the folder above it,
                // which was entered, and checked, on the way down.
                at.pop();
                continue;
            }
            let next = at.join(&name);
            let metadata = std::fs::symlink_metadata(&next).map_err(|_| Refused::Missing)?;
            if metadata.file_type().is_symlink() {
                links += 1;
                if links > MOST_LINKS {
                    return Err(Refused::Missing);
                }
                // The folder it is in was checked when it was entered. In a
                // folder with the sticky mark that is not enough: the link
                // itself must be ours.
                if !ours(&metadata, us) {
                    return Err(Refused::Writable(next));
                }
                let target = std::fs::read_link(&next).map_err(|_| Refused::Missing)?;
                if target.is_absolute() {
                    at.clone_from(&root);
                }
                ahead.extend(names(&target));
            } else if metadata.is_dir() {
                if !closed(&metadata, us, true, trust) {
                    return Err(Refused::Writable(next));
                }
                at = next;
            } else {
                // A file is the end of a path.
                if !ahead.is_empty() {
                    return Err(Refused::Missing);
                }
                at = next;
            }
        }
        Ok(at)
    }

    /// The file itself: it can be run, and nobody else can write it. Its
    /// folders were checked by [`walk`].
    pub(super) fn owned_by_us(
        real: &Path,
        metadata: &std::fs::Metadata,
        trust: &Trust,
    ) -> Result<(), Refused> {
        if metadata.mode() & ANY_EXECUTE == 0 {
            return Err(Refused::Missing);
        }
        let us = rustix::process::getuid().as_raw();
        if !closed(metadata, us, false, trust) {
            return Err(Refused::Writable(real.to_path_buf()));
        }
        Ok(())
    }
}
