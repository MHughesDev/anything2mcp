//! The on-disk layout, as one pure function, so that the local channel, the
//! runner journal, and the store agree on paths.
//!
//! Each operating system's own conventions are followed. `A2M_HOME`
//! overrides all of them with one directory. The layout is resolved from
//! an environment and touches no disk; only [`Env::on_this_machine`] reads
//! one folder's entry, the session manager's folder of the user on Linux.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use a2m_core::Failure;

use crate::codes;

/// The folder name the engine uses inside each of the system's directories.
pub const APP_FOLDER: &str = "anything2mcp";
/// The variable that puts everything under one directory.
pub const HOME_VARIABLE: &str = "A2M_HOME";

/// The environment variable a person sets the vault's passphrase in, for a
/// machine whose vault takes its root key from one. Only the command that
/// starts the engine reads it, and hands it to the engine on a pipe.
pub const PASSPHRASE_VARIABLE: &str = "A2M_VAULT_PASSPHRASE";

/// The family of operating system, which decides the conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OsFamily {
    /// Windows.
    Windows,
    /// macOS.
    MacOs,
    /// Linux and the other Unix systems that follow the same conventions.
    Linux,
}

impl OsFamily {
    /// The family this program was built for.
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

/// What the layout is resolved from: the operating system's family and the
/// process environment. A test builds one; a process reads its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Env {
    /// The family of operating system.
    pub os: OsFamily,
    /// The environment variables.
    pub vars: BTreeMap<String, String>,
    /// The folder the system's session manager makes for this user's
    /// runtime files, `/run/user/<uid>` on Linux, when it is a folder of
    /// this user's that no other account may enter: the folder
    /// `XDG_RUNTIME_DIR` names in a desktop session. It is read from the
    /// machine by [`Env::on_this_machine`], and is `None` on every other
    /// system; a test gives its own (decision 0432).
    pub session_folder: Option<PathBuf>,
}

impl Env {
    /// The environment of this process. A variable that is not valid text
    /// is left out.
    #[must_use]
    pub fn of_process() -> Self {
        Self::on_this_machine(
            std::env::vars_os()
                .filter_map(|(name, value)| {
                    Some((name.into_string().ok()?, value.into_string().ok()?))
                })
                .collect(),
        )
    }

    /// The environment of a process of this machine with these variables:
    /// this program's family of operating system, and, on Linux, the
    /// session manager's folder of this user when it is the user's and
    /// closed. It reads that folder's entry on the disk, and nothing else.
    #[must_use]
    pub fn on_this_machine(vars: BTreeMap<String, String>) -> Self {
        let os = OsFamily::current();
        Self {
            os,
            vars,
            session_folder: if os == OsFamily::Linux {
                session_folder_of_this_user()
            } else {
                None
            },
        }
    }

    /// An environment with these variables.
    #[must_use]
    pub fn with(os: OsFamily, vars: &[(&str, &str)]) -> Self {
        Self {
            os,
            vars: vars
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
            session_folder: None,
        }
    }

    fn dir(&self, name: &str) -> Option<PathBuf> {
        self.vars
            .get(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    }
}

/// The session manager's folder of the user this process runs as,
/// `/run/user/<uid>`, when it is a folder of this user's that no other
/// account may enter.
fn session_folder_of_this_user() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        let me = rustix::process::getuid().as_raw();
        session_folder_at(&PathBuf::from(format!("/run/user/{me}")), me)
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// `folder`, when it is a folder, not a link, that the account `me` owns
/// and no other account may enter or list. A test names a stand-in folder.
#[cfg(unix)]
fn session_folder_at(folder: &Path, me: u32) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt as _;
    let entry = std::fs::symlink_metadata(folder).ok()?;
    (entry.is_dir() && entry.uid() == me && entry.mode() & 0o077 == 0).then(|| folder.to_path_buf())
}

/// Where everything is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The installation's configuration directory: its settings file and a
    /// site's presets.
    pub installation_config: PathBuf,
    /// The user's configuration directory: the user's settings file.
    pub config: PathBuf,
    /// The data directory, which holds the five folders below.
    pub data: PathBuf,
    /// The store: the database, and the folder of evidence blobs inside it.
    pub store: PathBuf,
    /// The evidence blobs.
    pub blobs: PathBuf,
    /// One engine-owned profile directory per identity.
    pub profiles: PathBuf,
    /// The runner's local journal.
    pub journal: PathBuf,
    /// The logs.
    pub logs: PathBuf,
    /// The local channel and its lock file.
    pub runtime: PathBuf,
    /// Pinned programs and caches.
    pub cache: PathBuf,
}

impl Paths {
    /// Everything under one directory, as `A2M_HOME` gives it.
    #[must_use]
    pub fn under(home: &Path) -> Self {
        let data = home.join("data");
        Self::of(
            home.join("installation"),
            home.join("config"),
            data,
            home.join("run"),
            home.join("cache"),
        )
    }

    fn of(
        installation_config: PathBuf,
        config: PathBuf,
        data: PathBuf,
        runtime: PathBuf,
        cache: PathBuf,
    ) -> Self {
        let store = data.join("store");
        Self {
            installation_config,
            config,
            blobs: store.join(a2m_ledger::BLOB_FOLDER_NAME),
            store,
            profiles: data.join("profiles"),
            journal: data.join("journal"),
            logs: data.join("logs"),
            data,
            runtime,
            cache,
        }
    }

    /// The layout for an environment. `A2M_HOME` wins; otherwise each
    /// operating system's own directories are used.
    ///
    /// # Errors
    ///
    /// `input`/`path_not_allowed` when `A2M_HOME` is not an absolute path,
    /// or the environment does not say where the user's own directories
    /// are.
    pub fn resolve(env: &Env) -> Result<Self, Failure> {
        if let Some(home) = env
            .vars
            .get(HOME_VARIABLE)
            .filter(|value| !value.is_empty())
        {
            let home = PathBuf::from(home);
            if !home.is_absolute() {
                return Err(codes::input(
                    codes::PATH_NOT_ALLOWED,
                    format!("`{HOME_VARIABLE}` is not an absolute path."),
                    format!("Set `{HOME_VARIABLE}` to the full path of a directory."),
                ));
            }
            return Ok(Self::under(&home));
        }
        let missing = |what: &str| {
            codes::input(
                codes::PATH_NOT_ALLOWED,
                format!("The environment does not say where the user's {what} directory is."),
                format!("Set `{HOME_VARIABLE}` to the full path of a directory for the engine."),
            )
        };
        match env.os {
            OsFamily::Windows => {
                let roaming = env
                    .dir("APPDATA")
                    .ok_or_else(|| missing("application data"))?;
                let local = env
                    .dir("LOCALAPPDATA")
                    .ok_or_else(|| missing("local application data"))?;
                let machine = env.dir("PROGRAMDATA").unwrap_or_else(|| local.clone());
                let data = local.join(APP_FOLDER);
                Ok(Self::of(
                    machine.join(APP_FOLDER),
                    roaming.join(APP_FOLDER),
                    data.clone(),
                    data.join("run"),
                    data.join("cache"),
                ))
            }
            OsFamily::MacOs => {
                let home = env.dir("HOME").ok_or_else(|| missing("home"))?;
                let support = home
                    .join("Library")
                    .join("Application Support")
                    .join(APP_FOLDER);
                // Not `TMPDIR`: it differs between one user's processes,
                // and the system cleans it (decision 0432).
                let runtime = support.join("run");
                Ok(Self::of(
                    PathBuf::from("/Library/Application Support").join(APP_FOLDER),
                    support.clone(),
                    support,
                    runtime,
                    home.join("Library").join("Caches").join(APP_FOLDER),
                ))
            }
            OsFamily::Linux => {
                let home = env.dir("HOME");
                let pick = |variable: &str, below_home: &[&str]| {
                    env.dir(variable).or_else(|| {
                        home.as_ref().map(|home| {
                            below_home
                                .iter()
                                .fold(home.clone(), |path, part| path.join(part))
                        })
                    })
                };
                let config = pick("XDG_CONFIG_HOME", &[".config"])
                    .ok_or_else(|| missing("configuration"))?;
                let data = pick("XDG_DATA_HOME", &[".local", "share"])
                    .ok_or_else(|| missing("data"))?
                    .join(APP_FOLDER);
                let cache = pick("XDG_CACHE_HOME", &[".cache"]).ok_or_else(|| missing("cache"))?;
                // Without the variable, the folder it would have named,
                // when the session manager made it (decision 0432).
                let runtime = env
                    .dir("XDG_RUNTIME_DIR")
                    .or_else(|| env.session_folder.clone())
                    .map_or_else(|| data.join("run"), |run| run.join(APP_FOLDER));
                Ok(Self::of(
                    PathBuf::from("/etc").join(APP_FOLDER),
                    config.join(APP_FOLDER),
                    data,
                    runtime,
                    cache.join(APP_FOLDER),
                ))
            }
        }
    }
}

/// Which of the engine's own folders other accounts may enter, and which the
/// engine could not close.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct OpenFolders {
    /// These may be entered, or listed, by another account.
    pub(crate) open: Vec<PathBuf>,
    /// These are not a folder of this user's: another account owns them,
    /// or a link or a file stands where the folder should be. The engine
    /// closes only a folder of its own user's, so it could not close these
    /// when it started (finding B4 of the sixth audit, decision 0273).
    pub(crate) not_ours: Vec<PathBuf>,
    /// This build does not read who may on this operating system.
    pub(crate) not_checked: bool,
}

impl OpenFolders {
    /// Whether every folder that exists is this user's and closed.
    pub(crate) fn closed(&self) -> bool {
        !self.not_checked && self.open.is_empty() && self.not_ours.is_empty()
    }
}

/// Which of the data, configuration, cache, and runtime folders another
/// account may enter or list, and which are not a folder of this user's: on
/// Unix, the entry itself, a link not followed, its owner, and any of the
/// group's or others' bits. Touches the disk. On Windows the access list is
/// not read here: the engine sets it when it starts.
pub(crate) fn open_to_others(paths: &Paths) -> OpenFolders {
    #[cfg(unix)]
    {
        open_to_others_as(paths, rustix::process::getuid().as_raw())
    }
    #[cfg(not(unix))]
    {
        let _ = paths;
        OpenFolders {
            not_checked: true,
            ..OpenFolders::default()
        }
    }
}

/// [`open_to_others`], for the account `me`; a test names another account
/// to stand for its own.
#[cfg(unix)]
fn open_to_others_as(paths: &Paths, me: u32) -> OpenFolders {
    use std::os::unix::fs::MetadataExt as _;
    let mut found = OpenFolders::default();
    for folder in [&paths.data, &paths.config, &paths.cache, &paths.runtime] {
        let Ok(entry) = std::fs::symlink_metadata(folder) else {
            // Not there: nothing in it to reach.
            continue;
        };
        if !entry.is_dir() || entry.uid() != me {
            found.not_ours.push(folder.clone());
        } else if entry.mode() & 0o077 != 0 {
            found.open.push(folder.clone());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Env, OsFamily, Paths};

    fn absolute(parts: &[&str]) -> PathBuf {
        let root = if cfg!(windows) { "C:\\" } else { "/" };
        parts
            .iter()
            .fold(PathBuf::from(root), |path, part| path.join(part))
    }

    #[test]
    fn one_home_directory_overrides_every_convention() {
        let home = absolute(&["a2m"]);
        let env = Env::with(
            OsFamily::Linux,
            &[("A2M_HOME", home.to_str().unwrap()), ("HOME", "ignored")],
        );
        let paths = Paths::resolve(&env).unwrap();
        assert_eq!(paths, Paths::under(&home));
        assert_eq!(paths.store, home.join("data").join("store"));
        assert_eq!(paths.blobs, paths.store.join("blobs"));
        for inside in [&paths.profiles, &paths.journal, &paths.logs, &paths.store] {
            assert!(inside.starts_with(&paths.data));
        }
        assert!(Paths::resolve(&Env::with(OsFamily::Linux, &[("A2M_HOME", "relative")])).is_err());
    }

    #[test]
    fn each_operating_system_keeps_settings_and_data_where_it_expects_them() {
        let home = absolute(&["home", "pat"]);
        let home_text = home.to_str().unwrap();
        let linux = Paths::resolve(&Env::with(OsFamily::Linux, &[("HOME", home_text)])).unwrap();
        assert_eq!(linux.config, home.join(".config").join("anything2mcp"));
        assert_eq!(
            linux.data,
            home.join(".local").join("share").join("anything2mcp")
        );
        let mac = Paths::resolve(&Env::with(OsFamily::MacOs, &[("HOME", home_text)])).unwrap();
        assert!(
            mac.config
                .ends_with(Path::new("Application Support").join("anything2mcp"))
        );
        assert!(
            mac.cache
                .ends_with(Path::new("Caches").join("anything2mcp"))
        );
        let roaming = absolute(&["Users", "pat", "Roaming"]);
        let local = absolute(&["Users", "pat", "Local"]);
        let windows = Paths::resolve(&Env::with(
            OsFamily::Windows,
            &[
                ("APPDATA", roaming.to_str().unwrap()),
                ("LOCALAPPDATA", local.to_str().unwrap()),
            ],
        ))
        .unwrap();
        assert_eq!(windows.config, roaming.join("anything2mcp"));
        assert_eq!(windows.data, local.join("anything2mcp"));
        assert!(Paths::resolve(&Env::with(OsFamily::Linux, &[])).is_err());
    }

    /// Decision 0432: on macOS the runtime folder is `run` in the data
    /// folder, whatever `TMPDIR` says, so two processes of one user find
    /// one socket, in a folder the system does not clean.
    #[test]
    fn on_macos_the_runtime_folder_is_in_the_data_folder_whatever_tmpdir_says() {
        let home = absolute(&["Users", "pat"]);
        let tmp = absolute(&["var", "folders", "xy", "T"]);
        let without = Paths::resolve(&Env::with(
            OsFamily::MacOs,
            &[("HOME", home.to_str().unwrap())],
        ))
        .unwrap();
        let with = Paths::resolve(&Env::with(
            OsFamily::MacOs,
            &[
                ("HOME", home.to_str().unwrap()),
                ("TMPDIR", tmp.to_str().unwrap()),
            ],
        ))
        .unwrap();
        assert_eq!(without.runtime, without.data.join("run"));
        assert_eq!(with.runtime, without.runtime);
        assert!(!with.runtime.starts_with(&tmp));
    }

    /// Decision 0432: on Linux without `XDG_RUNTIME_DIR`, the session
    /// manager's folder of the user is the runtime folder's parent when it
    /// is the user's and closed, as the variable would have named it; when
    /// it is open to others, or missing, the runtime folder is `run` in the
    /// data folder. The variable, where it is set, still wins.
    #[cfg(unix)]
    #[test]
    fn on_linux_without_the_variable_the_session_folder_is_used_only_when_it_is_the_users_and_closed()
     {
        use std::os::unix::fs::PermissionsExt as _;
        let root = std::env::temp_dir().join(format!("a2m-actions-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let session = root.join("run-user");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::set_permissions(&session, std::fs::Permissions::from_mode(0o700)).unwrap();
        let home = root.join("home");
        let home_text = home.to_str().unwrap();
        let me = rustix::process::getuid().as_raw();
        let resolved = |session_folder: Option<PathBuf>, vars: &[(&str, &str)]| {
            let mut env = Env::with(OsFamily::Linux, vars);
            env.session_folder = session_folder;
            Paths::resolve(&env).unwrap()
        };

        let closed = super::session_folder_at(&session, me);
        assert_eq!(closed.as_deref(), Some(session.as_path()));
        let paths = resolved(closed, &[("HOME", home_text)]);
        assert_eq!(paths.runtime, session.join("anything2mcp"));
        // A process with the variable naming the same folder finds the
        // same runtime folder.
        let with_variable = resolved(
            None,
            &[
                ("HOME", home_text),
                ("XDG_RUNTIME_DIR", session.to_str().unwrap()),
            ],
        );
        assert_eq!(with_variable.runtime, paths.runtime);
        // The variable wins over the session folder.
        let elsewhere = root.join("elsewhere");
        let other = resolved(
            Some(session.clone()),
            &[
                ("HOME", home_text),
                ("XDG_RUNTIME_DIR", elsewhere.to_str().unwrap()),
            ],
        );
        assert_eq!(other.runtime, elsewhere.join("anything2mcp"));

        // Another account's folder is not the user's.
        assert_eq!(super::session_folder_at(&session, me.wrapping_add(1)), None);
        // Open to others: not used.
        std::fs::set_permissions(&session, std::fs::Permissions::from_mode(0o755)).unwrap();
        let open = super::session_folder_at(&session, me);
        assert_eq!(open, None);
        let paths = resolved(open, &[("HOME", home_text)]);
        assert_eq!(paths.runtime, paths.data.join("run"));
        // A link in its place, to a closed folder of the user's: not used.
        std::fs::set_permissions(&session, std::fs::Permissions::from_mode(0o700)).unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&session, &link).unwrap();
        assert_eq!(super::session_folder_at(&link, me), None);
        // Missing: not used.
        assert_eq!(super::session_folder_at(&root.join("missing"), me), None);
        // `Env::with` reads nothing from the machine.
        assert_eq!(Env::with(OsFamily::Linux, &[]).session_folder, None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Finding B4 of the sixth audit, decision 0273: a folder the engine
    /// could not close, because it is not a folder of this user's, is
    /// named, as well as one open to others. Another account id stands for
    /// the test's own, so that the test's folders are another account's
    /// whoever runs it.
    #[cfg(unix)]
    #[test]
    fn a_folder_of_another_account_or_a_link_in_its_place_is_named() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = std::env::temp_dir().join(format!("a2m-actions-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths::under(&root);
        for folder in [&paths.data, &paths.config, &paths.cache, &paths.runtime] {
            std::fs::create_dir_all(folder).unwrap();
            std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let me = rustix::process::getuid().as_raw();
        assert!(super::open_to_others_as(&paths, me).closed());
        // Every folder is another account's.
        let other = super::open_to_others_as(&paths, me.wrapping_add(1));
        assert_eq!(other.not_ours.len(), 4, "{other:?}");
        assert!(!other.closed());
        // A link in the place of a folder, to a folder of this user's, is
        // not followed; a folder open to others is named apart.
        let real = root.join("real-cache");
        std::fs::create_dir(&real).unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::remove_dir(&paths.cache).unwrap();
        std::os::unix::fs::symlink(&real, &paths.cache).unwrap();
        std::fs::set_permissions(&paths.config, std::fs::Permissions::from_mode(0o750)).unwrap();
        let found = super::open_to_others_as(&paths, me);
        assert_eq!(found.not_ours, vec![paths.cache.clone()]);
        assert_eq!(found.open, vec![paths.config.clone()]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
