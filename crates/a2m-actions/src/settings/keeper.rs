//! Who may change the installation's settings file (decision 0241, carrying
//! out point 4 of decision 0237; made stricter by decision 0273).
//!
//! The installation layer may loosen a protected setting. That is safe only
//! when no account but an administrator can change the file the layer is
//! read from: otherwise any program that runs as the person, an agent's
//! shell among them, could write the file, or point `A2M_HOME` at a folder
//! it prepared, and loosen what the design protects. So the file is read as
//! the administrator's only when the file, and every folder on the way to
//! it from the root of its drive, can be changed by no account but an
//! administrator. A file that fails the check is still read, and held to
//! the rule of the person's own layers: it may tighten a protected setting
//! and never loosen it.
//!
//! On Unix the owner and the mode are read: each folder and the file belong
//! to the superuser, and neither a group nor any other account may write
//! them, except a folder with the sticky mark, in which only an entry's
//! owner may replace it. No group counts as the administrator's, the
//! administrators' group of macOS included: the person's own account is
//! usually a member of it, and a member writes what the group may write
//! without any prompt. Loosening a protected setting asks more than
//! starting a program, where the provider's check of a program does count
//! that group (decision 0273). A link on the way is followed only when the
//! superuser owns it, and where it leads is walked the same way. Access
//! lists beyond the mode are read by the provider's check of a program
//! (`a2m_provider::check_program`, decision 0271): on macOS an entry that
//! allows anything, on the file or a folder on the way, is a refusal.
//!
//! The file is checked and read as one object: the walk ends at the file's
//! own entry; the file is then opened there without following a link, the
//! opened handle's own owner and mode are checked and its device and number
//! held to the entry the walk found, and the text is read from that handle.
//! A file replaced between the walk and the open is not the
//! administrator's.
//!
//! On Windows, and on any other system, this build does not yet read the
//! owner and the access list of a file: the check is not made, and the
//! file is held to the person's rule, with a refusal that says so.

use std::path::Path;

/// Who may change the installation's settings file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallationKeeper {
    /// No account but an administrator can change the file or a folder on
    /// the way to it: it may loosen a protected setting.
    Administrator,
    /// Another account can change `path`, which is the file or a folder on
    /// the way to it, for the reason `why`.
    Open {
        /// The file, or the folder, that another account can change.
        path: std::path::PathBuf,
        /// Why, as the end of a sentence.
        why: &'static str,
    },
    /// This build cannot tell on this system, for the reason given.
    NotChecked(&'static str),
}

impl Default for InstallationKeeper {
    /// Not checked: a file of which nothing is known is held to the
    /// person's rule.
    fn default() -> Self {
        Self::NotChecked("who may change the file was not checked")
    }
}

impl InstallationKeeper {
    /// Whether the file may loosen a protected setting.
    #[must_use]
    pub const fn may_loosen(&self) -> bool {
        matches!(self, Self::Administrator)
    }

    /// Why the file may not loosen, as the end of a sentence; `None` when
    /// it may.
    #[must_use]
    pub fn why_not(&self) -> Option<String> {
        match self {
            Self::Administrator => None,
            Self::Open { path, why } => Some(format!("`{}` {why}", path.display())),
            Self::NotChecked(why) => Some((*why).to_owned()),
        }
    }
}

/// Reads the installation's settings file at `file` and tells who may
/// change it, checking and reading the same file. Touches the disk.
///
/// # Errors
///
/// What reading the file answers: `NotFound` when nothing is there, and
/// another kind for a file that cannot be read as text.
pub(crate) fn read_installation_file(file: &Path) -> std::io::Result<(String, InstallationKeeper)> {
    os::read(file)
}

/// Where an administrator keeps the installation's settings on this
/// system, for a message.
#[must_use]
pub fn system_installation_folder() -> &'static str {
    if cfg!(windows) {
        "the `anything2mcp` folder in the system's program-data folder"
    } else if cfg!(target_os = "macos") {
        "`/Library/Application Support/anything2mcp`"
    } else {
        "`/etc/anything2mcp`"
    }
}

#[cfg(unix)]
mod os {
    use std::ffi::OsString;
    use std::io::Read as _;
    use std::os::unix::fs::MetadataExt as _;
    use std::path::{Component, Path, PathBuf};

    use a2m_provider::{ProgramRefused, ProgramRole, ProgramTrust, check_program};

    use super::InstallationKeeper;

    const OTHERS_WRITE: u32 = 0o002;
    const GROUP_WRITE: u32 = 0o020;
    const STICKY: u32 = 0o1000;
    /// How many links one path may lead through, as the system itself
    /// allows; more is a circle.
    const MOST_LINKS: u32 = 40;
    /// The superuser: in the product, the only account that counts as the
    /// administrator. A test adds its own account to stand for it.
    pub(super) const ROOT: u32 = 0;

    /// Why another account than an administrator may change it, or `None`.
    /// `admins` are the accounts that count as the administrator.
    fn open(metadata: &std::fs::Metadata, folder: bool, admins: &[u32]) -> Option<&'static str> {
        if !admins.contains(&metadata.uid()) {
            return Some("belongs to an account that is not the administrator's");
        }
        let mode = metadata.mode();
        // In a folder with the sticky mark only an entry's owner may
        // replace it, and every entry on the way is checked in its turn.
        if folder && mode & STICKY != 0 {
            return None;
        }
        if mode & OTHERS_WRITE != 0 {
            return Some("may be changed by every account");
        }
        // No group is the administrator's here, the administrators' group
        // of macOS included: the person's own account is usually in it.
        if mode & GROUP_WRITE != 0 {
            return Some(
                "may be changed by the members of a group, and the person's own account may be one",
            );
        }
        None
    }

    /// The names of a path, last first, for a stack.
    fn names(path: &Path) -> impl Iterator<Item = OsString> {
        path.components().rev().filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_owned()),
            Component::ParentDir => Some(OsString::from("..")),
            Component::Prefix(_) | Component::RootDir | Component::CurDir => None,
        })
    }

    /// Walks every folder and link from the root to the file, and gives the
    /// file's own entry, with no link left in its path, and its metadata as
    /// the walk found it; or who else may change it.
    fn walk(
        file: &Path,
        admins: &[u32],
    ) -> Result<(PathBuf, std::fs::Metadata), InstallationKeeper> {
        let open_at = |path: PathBuf, why| InstallationKeeper::Open { path, why };
        if !file.is_absolute() {
            return Err(InstallationKeeper::NotChecked(
                "the file's path is not absolute",
            ));
        }
        let root = PathBuf::from("/");
        let Ok(top) = std::fs::metadata(&root) else {
            return Err(InstallationKeeper::NotChecked(
                "the root folder cannot be read",
            ));
        };
        if let Some(why) = open(&top, true, admins) {
            return Err(open_at(root, why));
        }
        let mut at = root.clone();
        let mut ahead: Vec<OsString> = names(file).collect();
        let mut links = 0;
        while let Some(name) = ahead.pop() {
            if name == ".." {
                // `at` holds no link, so its parent was entered, and
                // checked, on the way down.
                at.pop();
                continue;
            }
            let next = at.join(&name);
            let Ok(metadata) = std::fs::symlink_metadata(&next) else {
                return Err(InstallationKeeper::NotChecked(
                    "a folder on the way cannot be read",
                ));
            };
            if metadata.file_type().is_symlink() {
                links += 1;
                if links > MOST_LINKS {
                    return Err(InstallationKeeper::NotChecked(
                        "its path leads through a circle of links",
                    ));
                }
                if !admins.contains(&metadata.uid()) {
                    return Err(open_at(
                        next,
                        "is a link that belongs to an account that is not the administrator's",
                    ));
                }
                let Ok(target) = std::fs::read_link(&next) else {
                    return Err(InstallationKeeper::NotChecked(
                        "a link on the way cannot be read",
                    ));
                };
                if target.is_absolute() {
                    at.clone_from(&root);
                }
                ahead.extend(names(&target));
            } else if metadata.is_dir() {
                if let Some(why) = open(&metadata, true, admins) {
                    return Err(open_at(next, why));
                }
                at = next;
            } else {
                if !ahead.is_empty() || !metadata.is_file() {
                    return Err(InstallationKeeper::NotChecked(
                        "the path does not lead to a file",
                    ));
                }
                if let Some(why) = open(&metadata, false, admins) {
                    return Err(open_at(next, why));
                }
                return Ok((next, metadata));
            }
        }
        Err(InstallationKeeper::NotChecked(
            "the path does not lead to a file",
        ))
    }

    /// Opens the file at `real` for reading, never through a link at its
    /// end, and without waiting on a pipe put there in place of a file.
    fn open_without_link(real: &Path) -> std::io::Result<std::fs::File> {
        use rustix::fs::{Mode, OFlags};
        let handle = rustix::fs::open(
            real,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        Ok(std::fs::File::from(handle))
    }

    /// Who may change the file the handle holds: the walk found `seen` at
    /// `real`, and `held` is the handle's own metadata.
    fn of_handle(
        real: &Path,
        seen: &std::fs::Metadata,
        held: &std::fs::Metadata,
        admins: &[u32],
    ) -> InstallationKeeper {
        if !held.is_file() || (held.dev(), held.ino()) != (seen.dev(), seen.ino()) {
            return InstallationKeeper::NotChecked("the file was replaced while it was checked");
        }
        if let Some(why) = open(held, false, admins) {
            return InstallationKeeper::Open {
                path: real.to_path_buf(),
                why,
            };
        }
        beyond_the_mode(real)
    }

    /// What the access lists of the file and of every folder on the way
    /// grant beyond the mode, as the provider's check of a program reads
    /// them (decision 0271). Its own rules of owner and mode are looser than
    /// this check's, which has already passed; what it adds is the lists.
    fn beyond_the_mode(real: &Path) -> InstallationKeeper {
        match check_program(real, &ProgramTrust::default(), ProgramRole::Script) {
            Ok(()) => InstallationKeeper::Administrator,
            Err(ProgramRefused::Unreadable(path)) => InstallationKeeper::Open {
                path,
                why: "has an access list beyond its mode with an entry that allows, or one that cannot be read",
            },
            Err(ProgramRefused::Writable(path)) => InstallationKeeper::Open {
                path,
                why: "may be changed by another account",
            },
            Err(_) => InstallationKeeper::NotChecked(
                "the check of the access lists on the way to the file did not pass",
            ),
        }
    }

    pub(super) fn read(file: &Path) -> std::io::Result<(String, InstallationKeeper)> {
        read_with(file, &[ROOT], &mut || {})
    }

    /// [`read`], with the accounts that count as the administrator, and a
    /// step taken between the walk and the open, for a test that replaces
    /// the file there.
    pub(super) fn read_with(
        file: &Path,
        admins: &[u32],
        between: &mut dyn FnMut(),
    ) -> std::io::Result<(String, InstallationKeeper)> {
        let walked = walk(file, admins);
        between();
        let (real, seen) = match walked {
            Ok(found) => found,
            // Not the administrator's: read as the person's, by its path.
            Err(keeper) => return Ok((std::fs::read_to_string(file)?, keeper)),
        };
        let handle = match open_without_link(&real) {
            Ok(handle) => handle,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err(error),
            // A link or something else was put there after the walk: what
            // is there now is read as the person's.
            Err(_) => {
                return Ok((
                    std::fs::read_to_string(file)?,
                    InstallationKeeper::NotChecked("the file was replaced while it was checked"),
                ));
            }
        };
        let held = handle.metadata()?;
        let keeper = of_handle(&real, &seen, &held, admins);
        let mut text = String::new();
        (&handle).read_to_string(&mut text)?;
        Ok((text, keeper))
    }
}

#[cfg(not(unix))]
mod os {
    use std::path::Path;

    use super::InstallationKeeper;

    pub(super) fn read(file: &Path) -> std::io::Result<(String, InstallationKeeper)> {
        Ok((
            std::fs::read_to_string(file)?,
            InstallationKeeper::NotChecked(
                "this build does not read who may change a file on this operating system yet, so it cannot tell that only an administrator can change this one",
            ),
        ))
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};

    use super::InstallationKeeper;
    use super::os::{ROOT, read_with};

    fn is_root() -> bool {
        rustix::process::geteuid().is_root()
    }

    /// The accounts that count as the administrator in these tests: the
    /// superuser, and the test's own account standing for it, so that the
    /// walk's rules run where the suite runs as an ordinary account too.
    fn admins() -> [u32; 2] {
        [ROOT, rustix::process::geteuid().as_raw()]
    }

    fn keeper(file: &Path) -> InstallationKeeper {
        read_with(file, &admins(), &mut || {}).map_or_else(
            |_| InstallationKeeper::NotChecked("not read"),
            |(_, keeper)| keeper,
        )
    }

    fn mode(path: &Path, mode: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    /// A folder of the test's own, as the system names it, with a settings
    /// file in it.
    struct Place(PathBuf);

    impl Place {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("a2m-actions-keeper-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            let place = Self(std::fs::canonicalize(&root).unwrap());
            mode(&place.0, 0o755);
            std::fs::write(place.file(), "kept = \"administrator\"\n").unwrap();
            mode(&place.file(), 0o644);
            place
        }

        fn file(&self) -> PathBuf {
            self.0.join("settings.toml")
        }
    }

    impl Drop for Place {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_file_or_folder_another_account_can_change_is_not_the_administrators() {
        let place = Place::new("open");
        // The product counts the superuser alone: as any other account the
        // test's own folder is never the administrator's.
        if !is_root() {
            assert!(matches!(
                read_with(&place.file(), &[ROOT], &mut || {}).unwrap().1,
                InstallationKeeper::Open { .. }
            ));
        }
        // With the test's own account standing for the administrator, the
        // test's folders are the administrator's.
        assert_eq!(keeper(&place.file()), InstallationKeeper::Administrator);
        // Every account may write the folder: refused, by the folder.
        mode(&place.0, 0o777);
        assert_eq!(
            keeper(&place.file()),
            InstallationKeeper::Open {
                path: place.0.clone(),
                why: "may be changed by every account"
            }
        );
        // A group may write the file.
        mode(&place.0, 0o755);
        mode(&place.file(), 0o664);
        assert!(matches!(
            keeper(&place.file()),
            InstallationKeeper::Open { path, .. } if path == place.file()
        ));
        mode(&place.file(), 0o644);
        if !is_root() {
            return;
        }
        // Another account owns the folder.
        std::os::unix::fs::chown(&place.0, Some(65_534), None).unwrap();
        assert!(matches!(
            keeper(&place.file()),
            InstallationKeeper::Open { path, why } if path == place.0 && why.contains("not the administrator's")
        ));
        std::os::unix::fs::chown(&place.0, Some(0), None).unwrap();
        // A link of another account's on the way is not followed.
        let link = place.0.join("through");
        std::os::unix::fs::symlink(&place.0, &link).unwrap();
        std::os::unix::fs::lchown(&link, Some(65_534), None).unwrap();
        assert!(matches!(
            keeper(&link.join("settings.toml")),
            InstallationKeeper::Open { path, .. } if path == link
        ));
    }

    /// Finding B1 of the sixth audit, decision 0273: on macOS the person's
    /// own account is usually in the administrators' group (80), so a file
    /// that group may write is the person's to change, and never loosens.
    /// Where the test's account cannot give the file to that group (an
    /// ordinary account on Linux), its own primary group stands for it.
    #[test]
    fn a_file_the_administrators_group_may_write_is_not_the_administrators() {
        let place = Place::new("group");
        mode(&place.file(), 0o664);
        for group in [80, rustix::process::getegid().as_raw()] {
            if std::os::unix::fs::chown(place.file(), None, Some(group)).is_err() {
                continue;
            }
            assert!(
                matches!(
                    keeper(&place.file()),
                    InstallationKeeper::Open { ref path, .. } if *path == place.file()
                ),
                "group {group}: {:?}",
                keeper(&place.file())
            );
        }
        // A folder on the way that group may write, likewise.
        mode(&place.file(), 0o644);
        mode(&place.0, 0o775);
        assert!(matches!(
            keeper(&place.file()),
            InstallationKeeper::Open { path, .. } if path == place.0
        ));
    }

    /// Finding B1, the second half: the file is checked and read as one
    /// object. A file put in place of the checked one between the walk and
    /// the read is never read as the administrator's, whichever way round
    /// the two are swapped.
    #[test]
    fn a_file_replaced_between_the_check_and_the_read_is_not_the_administrators() {
        let place = Place::new("swap");
        let other = place.0.join("other.toml");
        let wide = "policy = \"loosened by another account\"\n";
        let not_loosened = |(text, keeper): (String, InstallationKeeper)| {
            assert!(
                !(text == wide && keeper.may_loosen()),
                "read {text:?} as {keeper:?}"
            );
        };

        // Another account's file is there first, and the administrator's
        // is put back after the first look.
        let kept = place.0.join("kept.toml");
        std::fs::rename(place.file(), &kept).unwrap();
        std::fs::write(place.file(), wide).unwrap();
        mode(&place.file(), 0o666);
        not_loosened(
            read_with(&place.file(), &admins(), &mut || {
                std::fs::rename(&kept, place.file()).unwrap();
            })
            .unwrap(),
        );

        // The administrator's file is there first, and a file of the same
        // owner and mode, other bytes, is put in its place after the walk:
        // only the handle's device and number tell them apart.
        std::fs::write(&other, wide).unwrap();
        mode(&other, 0o644);
        let read = read_with(&place.file(), &admins(), &mut || {
            std::fs::rename(&other, place.file()).unwrap();
        })
        .unwrap();
        assert_eq!(read.0, wide);
        assert_eq!(
            read.1,
            InstallationKeeper::NotChecked("the file was replaced while it was checked")
        );
        not_loosened(read);

        // A link put in place of the file after the walk is not followed.
        std::fs::write(&other, wide).unwrap();
        mode(&other, 0o644);
        let read = read_with(&place.file(), &admins(), &mut || {
            std::fs::remove_file(place.file()).unwrap();
            std::os::unix::fs::symlink(&other, place.file()).unwrap();
        })
        .unwrap();
        assert!(!read.1.may_loosen(), "{:?}", read.1);
    }

    /// Finding B2 of the sixth audit, decision 0273: on macOS an access
    /// list entry that allows, on the file or on a folder on the way, leaves
    /// the mode as it was and is read, through the provider's check of a
    /// program. Runs on macOS alone.
    #[cfg(target_os = "macos")]
    #[test]
    fn an_access_list_entry_that_allows_is_not_the_administrators() {
        let add = |path: &Path, entry: &str| {
            let made = std::process::Command::new("/bin/chmod")
                .arg("+a")
                .arg(entry)
                .arg(path)
                .output()
                .unwrap();
            assert!(made.status.success(), "{made:?}");
        };
        let place = Place::new("acl");
        assert_eq!(keeper(&place.file()), InstallationKeeper::Administrator);
        add(&place.file(), "everyone allow write");
        assert!(matches!(
            keeper(&place.file()),
            InstallationKeeper::Open { path, .. } if path == place.file()
        ));
        let place = Place::new("acl-folder");
        add(&place.0, "everyone allow add_file");
        assert!(matches!(
            keeper(&place.file()),
            InstallationKeeper::Open { path, .. } if path == place.0
        ));
    }

    #[test]
    fn a_path_that_is_not_a_file_is_not_checked() {
        let relative = read_with(Path::new("relative.toml"), &admins(), &mut || {});
        assert!(relative.is_err() || !relative.unwrap().1.may_loosen());
        let place = Place::new("missing");
        // Not there: the read says so, and nothing is the administrator's.
        let missing = read_with(&place.0.join("no-such.toml"), &admins(), &mut || {});
        assert_eq!(missing.unwrap_err().kind(), std::io::ErrorKind::NotFound);
        // A folder where the file should be.
        std::fs::create_dir(place.0.join("folder.toml")).unwrap();
        let folder = read_with(&place.0.join("folder.toml"), &admins(), &mut || {});
        assert!(folder.is_err() || !folder.unwrap().1.may_loosen());
        assert!(!InstallationKeeper::default().may_loosen());
    }
}
