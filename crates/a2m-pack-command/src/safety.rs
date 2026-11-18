//! What is checked about a program's file before it is ever started: that
//! it is there, is a program and not a script or a shell, and that nobody
//! but the engine's user, or the system's administrator, can replace it.
//!
//! "Replace" covers the whole way from the declared path to the file. The
//! path is walked as the system would walk it when the program is started:
//! every folder on the way, every link, the folder each link is in, and
//! every folder on the way to where a link leads. Whoever can write one of
//! them chooses what the path names, whatever the file at its end is.
//!
//! On Unix the mode, the owner, and the group are read; access lists beyond
//! the mode are not. A group's write counts as the administrator's only for
//! the groups [`Trust`] holds. On Windows the owner and the discretionary
//! access list of each folder and of the file are read, and a link, or any
//! other reparse point, on the way is refused rather than followed.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use a2m_core::Digest;
use sha2::{Digest as _, Sha256};

use crate::kind::is_shell_or_interpreter;

/// Why a program's file may not be started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
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
pub enum Role {
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
pub struct Trust {
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
    pub fn with_groups(named: Vec<u32>) -> Self {
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
pub fn check(path: &Path, trust: &Trust, role: Role) -> Result<(), Refused> {
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

#[cfg(windows)]
#[path = "safety/windows.rs"]
mod os;

#[cfg(not(any(unix, windows)))]
mod os {
    //! A system that is neither: nothing is known of who may write a file
    //! there, so every program is refused (rule 7).

    use std::path::{Path, PathBuf};

    use super::{Refused, Trust};

    pub(super) fn walk(declared: &Path, _trust: &Trust) -> Result<PathBuf, Refused> {
        Err(Refused::Unreadable(declared.to_path_buf()))
    }

    pub(super) fn owned_by_us(
        real: &Path,
        _metadata: &std::fs::Metadata,
        _trust: &Trust,
    ) -> Result<(), Refused> {
        Err(Refused::Unreadable(real.to_path_buf()))
    }
}

/// The SHA-256 of a file, read in pieces.
///
/// # Errors
///
/// When the file cannot be read.
pub fn digest_of(path: &Path) -> std::io::Result<Digest> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(Digest::from_bytes(hasher.finalize().into()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Refused, Role, Trust};

    /// The check of a program, with no group named.
    fn check(path: &Path) -> Result<(), Refused> {
        super::check(path, &Trust::default(), Role::Program)
    }

    #[test]
    fn a_relative_path_a_missing_file_and_a_shell_are_refused() {
        assert_eq!(check(Path::new("example-tool")), Err(Refused::NotAbsolute));
        let missing = std::env::temp_dir().join("a2m-pack-command-no-such-program");
        assert_eq!(check(&missing), Err(Refused::Missing));
        assert_eq!(check(&std::env::temp_dir()), Err(Refused::Missing));
        #[cfg(unix)]
        assert_eq!(check(Path::new("/bin/sh")), Err(Refused::Script));
    }

    #[cfg(unix)]
    #[test]
    fn a_script_and_a_file_others_can_write_are_refused() {
        use std::os::unix::fs::PermissionsExt as _;

        let folder =
            std::env::temp_dir().join(format!("a2m-pack-command-safety-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let script = folder.join("example-tool");
        std::fs::write(&script, "#!/bin/sh\necho hi\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(check(&script), Err(Refused::Script));

        let open = folder.join("example-open");
        std::fs::write(&open, [0x7f, b'E', b'L', b'F']).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(
            matches!(check(&open), Err(Refused::Writable(path)) if path.ends_with("example-open"))
        );

        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(check(&open), Err(Refused::Missing), "it cannot be run");
        std::fs::remove_dir_all(&folder).unwrap();
    }

    /// A folder of the test's own with a program in `real/`, and a way to
    /// make more folders with a mode.
    #[cfg(unix)]
    struct Place(std::path::PathBuf);

    #[cfg(unix)]
    impl Place {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "a2m-pack-command-safety-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            // As the system names it, so that a refusal's path can be
            // compared: the temporary folder is behind a link on macOS.
            let place = Self(std::fs::canonicalize(&root).unwrap());
            place.folder("", 0o755);
            place.folder("real", 0o755);
            let program = place.0.join("real").join("example-tool");
            std::fs::write(&program, [0x7f, b'E', b'L', b'F']).unwrap();
            Self::mode(&program, 0o755);
            place
        }

        fn mode(path: &Path, mode: u32) {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        }

        fn folder(&self, name: &str, mode: u32) -> std::path::PathBuf {
            let folder = self.0.join(name);
            std::fs::create_dir_all(&folder).unwrap();
            Self::mode(&folder, mode);
            folder
        }

        fn link(target: &Path, at: &Path) {
            std::os::unix::fs::symlink(target, at).unwrap();
        }
    }

    #[cfg(unix)]
    impl Drop for Place {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_a_folder_others_can_write_is_refused_and_a_closed_chain_is_followed() {
        let place = Place::new("links");
        let program = place.0.join("real").join("example-tool");
        assert_eq!(check(&program), Ok(()));

        // The declared path is a link in a folder anyone may write: whoever
        // can write the folder chooses where the link leads.
        let open = place.folder("open", 0o777);
        Place::link(&program, &open.join("example-tool"));
        assert_eq!(
            check(&open.join("example-tool")),
            Err(Refused::Writable(open.clone()))
        );
        // The same for an open folder that the path only passes through.
        Place::link(&place.0.join("real"), &open.join("through"));
        assert_eq!(
            check(&open.join("through").join("example-tool")),
            Err(Refused::Writable(open.clone()))
        );

        // A chain that nobody else can write is followed to the file: a
        // link to a link, one of them relative and through `..`.
        let closed = place.folder("closed", 0o755);
        Place::link(
            Path::new("../real/example-tool"),
            &closed.join("example-first"),
        );
        Place::link(&closed.join("example-first"), &closed.join("example-tool"));
        assert_eq!(check(&closed.join("example-tool")), Ok(()));
        // The folder the first link is in is opened afterwards: refused.
        Place::link(&closed.join("example-tool"), &open.join("example-far"));
        assert!(matches!(
            check(&open.join("example-far")),
            Err(Refused::Writable(_))
        ));

        // Where only an entry's owner may replace it, a link of ours holds.
        let shared = place.folder("shared", 0o1777);
        Place::link(&program, &shared.join("example-tool"));
        assert_eq!(check(&shared.join("example-tool")), Ok(()));

        // Links that lead in a circle lead nowhere, and a link to a shell
        // is a shell.
        Place::link(&closed.join("example-b"), &closed.join("example-a"));
        Place::link(&closed.join("example-a"), &closed.join("example-b"));
        assert_eq!(check(&closed.join("example-a")), Err(Refused::Missing));
        Place::link(Path::new("/bin/sh"), &closed.join("example-shell"));
        assert_eq!(check(&closed.join("example-shell")), Err(Refused::Script));
        // A file is the end of a path.
        assert_eq!(check(&program.join("more")), Err(Refused::Missing));
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_or_a_file_a_group_can_write_is_refused_unless_the_group_counts() {
        let place = Place::new("group");
        let folder = place.folder("grouped", 0o775);
        let program = folder.join("example-tool");
        std::fs::copy(place.0.join("real").join("example-tool"), &program).unwrap();
        Place::mode(&program, 0o755);
        assert_eq!(check(&program), Err(Refused::Writable(folder.clone())));
        Place::mode(&folder, 0o755);
        assert_eq!(check(&program), Ok(()));
        Place::mode(&program, 0o775);
        assert_eq!(check(&program), Err(Refused::Writable(program.clone())));
    }

    /// A group-writable folder with a program in it, and the folder's group.
    #[cfg(unix)]
    fn grouped(place: &Place) -> (std::path::PathBuf, std::path::PathBuf, u32) {
        use std::os::unix::fs::MetadataExt as _;
        let folder = place.folder("grouped", 0o775);
        let program = folder.join("example-tool");
        std::fs::copy(place.0.join("real").join("example-tool"), &program).unwrap();
        Place::mode(&program, 0o775);
        let group = std::fs::metadata(&folder).unwrap().gid();
        (folder, program, group)
    }

    #[cfg(unix)]
    #[test]
    fn a_group_the_person_names_counts_as_the_administrators_and_no_other_does() {
        let place = Place::new("named");
        let (folder, program, group) = grouped(&place);
        let named =
            |groups: Vec<u32>| super::check(&program, &Trust::with_groups(groups), Role::Program);
        // Named: the folder and the file that group may write are started.
        assert_eq!(named(vec![group]), Ok(()));
        // Not named, or another group named: refused, as before.
        assert_eq!(named(vec![]), Err(Refused::Writable(folder.clone())));
        assert_eq!(
            named(vec![group.wrapping_add(1)]),
            Err(Refused::Writable(folder.clone()))
        );
        // A group's name never stands for it: only the system's own list
        // and the setting's numbers do. Others' write is refused whatever
        // group is named.
        Place::mode(&folder, 0o777);
        assert_eq!(named(vec![group]), Err(Refused::Writable(folder.clone())));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn on_linux_no_group_counts_by_default() {
        let place = Place::new("linux");
        let (folder, program, _) = grouped(&place);
        assert_eq!(check(&program), Err(Refused::Writable(folder)));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn on_macos_the_administrators_group_counts_and_staff_does_not() {
        let place = Place::new("macos");
        let (folder, program, _) = grouped(&place);
        // Giving a file to a group needs no privilege when the test's user
        // is in it; the runner's user is an administrator, but a machine
        // where it is not cannot prove this.
        for (group, path) in [(80, &folder), (80, &program)] {
            if let Err(error) = std::os::unix::fs::chown(path, None, Some(group)) {
                println!("skipped: this user cannot give a file to the group {group}: {error}");
                return;
            }
        }
        assert_eq!(check(&program), Ok(()));
        if let Err(error) = std::os::unix::fs::chown(&folder, None, Some(20)) {
            println!("skipped: this user cannot give a file to the group 20: {error}");
            return;
        }
        assert_eq!(check(&program), Err(Refused::Writable(folder)));
    }
}
