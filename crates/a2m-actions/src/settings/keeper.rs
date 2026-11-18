//! Who may change the installation's settings file (decision 0241, carrying
//! out point 4 of decision 0237).
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
//! On Unix the owner and the mode are read (the model is the check the
//! command pack makes of a program's file, with the engine's own user no
//! longer counted): each folder and the file belong to the superuser, no
//! group and no other account may write them, except a group the system
//! gives only to its administrators, and a folder with the sticky mark,
//! in which only an entry's owner may replace it. A link on the way is
//! followed only when the superuser owns it, and where it leads is walked
//! the same way. Access lists beyond the mode are not read.
//!
//! On Windows, and on any other system, this build does not yet read the
//! owner and the access list of a file: the check is not made, and the
//! file is held to the person's rule, with a refusal that says so.

use std::path::{Path, PathBuf};

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
        path: PathBuf,
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

    /// Who may change the file at `file`, and every folder on the way to
    /// it. Touches the disk.
    #[must_use]
    pub fn of_file(file: &Path) -> Self {
        os::keeper(file)
    }
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
    use std::os::unix::fs::MetadataExt as _;
    use std::path::{Component, Path, PathBuf};

    use super::InstallationKeeper;

    const OTHERS_WRITE: u32 = 0o002;
    const GROUP_WRITE: u32 = 0o020;
    const STICKY: u32 = 0o1000;
    /// How many links one path may lead through, as the system itself
    /// allows; more is a circle.
    const MOST_LINKS: u32 = 40;
    /// The superuser.
    const ROOT: u32 = 0;

    /// The groups the system gives only to its administrators: on macOS
    /// the group with id 80, whose members become the superuser through the
    /// system's own prompt. On Linux no group is one: a group's id says
    /// nothing about who is in it.
    #[cfg(target_os = "macos")]
    const ADMIN_GROUPS: &[u32] = &[80];
    #[cfg(not(target_os = "macos"))]
    const ADMIN_GROUPS: &[u32] = &[];

    /// Why another account than an administrator may change it, or `None`.
    fn open(metadata: &std::fs::Metadata, folder: bool) -> Option<&'static str> {
        if metadata.uid() != ROOT {
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
        if mode & GROUP_WRITE != 0 && !ADMIN_GROUPS.contains(&metadata.gid()) {
            return Some("may be changed by a group that is not the administrators'");
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

    pub(super) fn keeper(file: &Path) -> InstallationKeeper {
        let open_at = |path: PathBuf, why| InstallationKeeper::Open { path, why };
        if !file.is_absolute() {
            return InstallationKeeper::NotChecked("the file's path is not absolute");
        }
        let root = PathBuf::from("/");
        let Ok(top) = std::fs::metadata(&root) else {
            return InstallationKeeper::NotChecked("the root folder cannot be read");
        };
        if let Some(why) = open(&top, true) {
            return open_at(root, why);
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
                return InstallationKeeper::NotChecked("a folder on the way cannot be read");
            };
            if metadata.file_type().is_symlink() {
                links += 1;
                if links > MOST_LINKS {
                    return InstallationKeeper::NotChecked(
                        "its path leads through a circle of links",
                    );
                }
                if metadata.uid() != ROOT {
                    return open_at(
                        next,
                        "is a link that belongs to an account that is not the administrator's",
                    );
                }
                let Ok(target) = std::fs::read_link(&next) else {
                    return InstallationKeeper::NotChecked("a link on the way cannot be read");
                };
                if target.is_absolute() {
                    at.clone_from(&root);
                }
                ahead.extend(names(&target));
            } else if metadata.is_dir() {
                if let Some(why) = open(&metadata, true) {
                    return open_at(next, why);
                }
                at = next;
            } else {
                if !ahead.is_empty() || !metadata.is_file() {
                    return InstallationKeeper::NotChecked("the path does not lead to a file");
                }
                if let Some(why) = open(&metadata, false) {
                    return open_at(next, why);
                }
                at = next;
            }
        }
        InstallationKeeper::Administrator
    }
}

#[cfg(not(unix))]
mod os {
    use std::path::Path;

    use super::InstallationKeeper;

    pub(super) fn keeper(_file: &Path) -> InstallationKeeper {
        InstallationKeeper::NotChecked(
            "this build does not read who may change a file on this operating system yet, so it cannot tell that only an administrator can change this one",
        )
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};

    use super::InstallationKeeper;

    fn is_root() -> bool {
        rustix::process::geteuid().is_root()
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
            std::fs::write(place.file(), "").unwrap();
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
        if !is_root() {
            // The test's own folder is the person's, never the
            // administrator's.
            assert!(matches!(
                InstallationKeeper::of_file(&place.file()),
                InstallationKeeper::Open { .. }
            ));
            return;
        }
        // As the superuser the test's folders are the administrator's.
        assert_eq!(
            InstallationKeeper::of_file(&place.file()),
            InstallationKeeper::Administrator
        );
        // Every account may write the folder: refused, by the folder.
        mode(&place.0, 0o777);
        assert_eq!(
            InstallationKeeper::of_file(&place.file()),
            InstallationKeeper::Open {
                path: place.0.clone(),
                why: "may be changed by every account"
            }
        );
        // A group may write the file.
        mode(&place.0, 0o755);
        mode(&place.file(), 0o664);
        assert!(matches!(
            InstallationKeeper::of_file(&place.file()),
            InstallationKeeper::Open { path, .. } if path == place.file()
        ));
        // Another account owns the folder.
        mode(&place.file(), 0o644);
        std::os::unix::fs::chown(&place.0, Some(65_534), None).unwrap();
        assert!(matches!(
            InstallationKeeper::of_file(&place.file()),
            InstallationKeeper::Open { path, why } if path == place.0 && why.contains("not the administrator's")
        ));
        std::os::unix::fs::chown(&place.0, Some(0), None).unwrap();
        // A link of another account's on the way is not followed.
        let link = place.0.join("through");
        std::os::unix::fs::symlink(&place.0, &link).unwrap();
        std::os::unix::fs::lchown(&link, Some(65_534), None).unwrap();
        assert!(matches!(
            InstallationKeeper::of_file(&link.join("settings.toml")),
            InstallationKeeper::Open { path, .. } if path == link
        ));
    }

    #[test]
    fn a_path_that_is_not_a_file_is_not_checked() {
        assert!(!InstallationKeeper::of_file(Path::new("relative.toml")).may_loosen());
        let place = Place::new("missing");
        assert!(matches!(
            InstallationKeeper::of_file(&place.0.join("no-such.toml")),
            InstallationKeeper::NotChecked(_)
        ));
        assert!(!InstallationKeeper::default().may_loosen());
    }
}
