//! The on-disk layout, as one pure function, so that the local channel, the
//! runner journal, and the store agree on paths.
//!
//! Each operating system's own conventions are followed. `A2M_HOME`
//! overrides all of them with one directory. Nothing here touches the disk.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use a2m_core::Failure;

use crate::codes;

/// The folder name the engine uses inside each of the system's directories.
pub const APP_FOLDER: &str = "anything2mcp";
/// The variable that puts everything under one directory.
pub const HOME_VARIABLE: &str = "A2M_HOME";

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
}

impl Env {
    /// The environment of this process. A variable that is not valid text
    /// is left out.
    #[must_use]
    pub fn of_process() -> Self {
        Self {
            os: OsFamily::current(),
            vars: std::env::vars_os()
                .filter_map(|(name, value)| {
                    Some((name.into_string().ok()?, value.into_string().ok()?))
                })
                .collect(),
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
                let runtime = env
                    .dir("TMPDIR")
                    .map_or_else(|| support.join("run"), |tmp| tmp.join(APP_FOLDER));
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
                let runtime = env
                    .dir("XDG_RUNTIME_DIR")
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
}
