//! Which folder may be a project folder, and which of its files may be
//! read as a source.
//!
//! A project folder is stated by the client end of the local channel, which
//! runs as the person. It is a statement, and whoever can run that client
//! can state any folder, so the engine bounds what it will take: a folder
//! that holds a project file, inside what `files.allowed_roots` allows,
//! and never a folder in which another program keeps its own data. A source
//! file is then read only from the project's `sources` folder. The engine
//! never reads another program's cookie store or keys through a project
//! (rule 5; spec section 502).

use std::path::{Component, Path, PathBuf};

use crate::paths::{Env, OsFamily, Paths};
use crate::settings::{PROJECT_FILE_NAME, Settings, SourceFile, Sources};

/// The setting that bounds which folder may be a project folder.
pub(crate) const ALLOWED_ROOTS: &str = "files.allowed_roots";

/// The word in `files.allowed_roots` that stands for "any folder that holds
/// a project file".
pub(crate) const PROJECT_ROOT_WORD: &str = "<project>";

/// The folder of a project a source file is read from.
pub(crate) const SOURCES_FOLDER: &str = "sources";

/// The largest project file that is read.
const PROJECT_FILE_LIMIT: u64 = 1024 * 1024;

/// Where programs keep their own data on this machine: what a project
/// folder may neither hold nor lie inside.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Places {
    /// The person's home folder.
    home: Option<PathBuf>,
    /// The folder for temporary files, which on one operating system lies
    /// inside a program-data folder and is still nobody's own.
    temp: Option<PathBuf>,
    /// The folders the operating system gives programs for their data.
    program: Vec<PathBuf>,
}

impl Places {
    /// The places of this process's machine.
    pub(crate) fn of_process() -> Self {
        Self::of(&Env::of_process(), Some(std::env::temp_dir()))
    }

    /// The places an environment names.
    pub(crate) fn of(env: &Env, temp: Option<PathBuf>) -> Self {
        let dir = |name: &str| {
            env.vars
                .get(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };
        let (home, names): (Option<PathBuf>, &[&str]) = match env.os {
            OsFamily::Windows => (
                dir("USERPROFILE"),
                &["APPDATA", "LOCALAPPDATA", "PROGRAMDATA"],
            ),
            OsFamily::MacOs => (dir("HOME"), &[]),
            OsFamily::Linux => (
                dir("HOME"),
                &[
                    "XDG_CONFIG_HOME",
                    "XDG_DATA_HOME",
                    "XDG_STATE_HOME",
                    "XDG_CACHE_HOME",
                    "XDG_RUNTIME_DIR",
                ],
            ),
        };
        let mut program: Vec<PathBuf> = names.iter().filter_map(|name| dir(name)).collect();
        if let (OsFamily::MacOs, Some(home)) = (env.os, &home) {
            program.push(home.join("Library"));
        }
        Self {
            home,
            temp,
            program,
        }
    }
}

/// A path with every link followed, in its ordinary form; as it is written
/// when nothing is there.
fn resolved(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).map_or_else(|_| path.to_path_buf(), |real| plain(&real))
}

/// On Windows a resolved path comes back in the verbatim form. This gives
/// the ordinary form of the same path, so that two paths are compared in
/// one form. Elsewhere a path has no prefix and comes back unchanged.
fn plain(path: &Path) -> PathBuf {
    use std::path::Prefix;
    let mut components = path.components();
    let start = match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::VerbatimDisk(letter) => format!("{}:\\", char::from(letter)),
            Prefix::VerbatimUNC(server, share) => format!(
                "\\\\{}\\{}\\",
                server.to_string_lossy(),
                share.to_string_lossy()
            ),
            _ => return path.to_path_buf(),
        },
        _ => return path.to_path_buf(),
    };
    let mut out = PathBuf::from(start);
    for component in components {
        if let Component::Normal(part) = component {
            out.push(part);
        }
    }
    out
}

/// Whether a part of a path is hidden: its name starts with a dot.
fn hidden(part: &std::ffi::OsStr) -> bool {
    part.to_string_lossy().starts_with('.')
}

/// Checks a project folder a front door stated, and gives it with every
/// link followed. Touches the disk: call it off the async threads.
///
/// # Errors
///
/// The reason the folder is not used, as the end of a sentence.
pub(crate) fn check(
    stated: &Path,
    paths: &Paths,
    settings: &Settings,
    places: &Places,
) -> Result<PathBuf, &'static str> {
    let folder = std::fs::canonicalize(stated)
        .ok()
        .filter(|folder| folder.is_dir())
        .map(|folder| plain(&folder))
        .ok_or("there is no such folder")?;

    // The engine's own folders: the store, the vault's files, the settings.
    for own in [
        &paths.installation_config,
        &paths.config,
        &paths.data,
        &paths.runtime,
        &paths.cache,
    ] {
        let own = resolved(own);
        if folder.starts_with(&own) || own.starts_with(&folder) {
            return Err("it holds, or lies inside, one of the engine's own folders");
        }
    }

    // Other programs' folders: a browser's profile, a key folder, a
    // program's settings. A folder that holds them, the home folder or one
    // above it, is refused as well: a path inside it could name them.
    let temporary = places
        .temp
        .as_ref()
        .is_some_and(|temp| folder.starts_with(resolved(temp)));
    if let Some(home) = places.home.as_ref().map(|home| resolved(home)) {
        if home.starts_with(&folder) {
            return Err(
                "it is the home folder or a folder above it, which holds the folders other programs keep their data in",
            );
        }
        if !temporary
            && let Ok(below) = folder.strip_prefix(&home)
            && below
                .components()
                .next()
                .is_some_and(|first| matches!(first, Component::Normal(part) if hidden(part)))
        {
            return Err(
                "it lies inside a hidden folder of the home folder, where other programs keep their data",
            );
        }
    }
    for program in &places.program {
        let program = resolved(program);
        if program.starts_with(&folder) {
            return Err("it holds a folder other programs keep their data in");
        }
        if !temporary && folder.starts_with(&program) {
            return Err("it lies inside a folder other programs keep their data in");
        }
    }

    // A project folder is one a person made a project: it holds a project
    // file, itself a file and not a link, that reads as a settings file.
    let file = folder.join(PROJECT_FILE_NAME);
    let is_file = std::fs::symlink_metadata(&file)
        .is_ok_and(|found| found.is_file() && found.len() <= PROJECT_FILE_LIMIT);
    if !is_file {
        return Err("it holds no project file, `anything2mcp.toml`");
    }
    // Nobody but the person, or the administrator, may change the folder or
    // its project file: another account that can write either can make the
    // folder a project, or change what it says. Inside the folder for
    // temporary files, which everyone may write, every folder between it
    // and the project is held to the same rule: that is what keeps the
    // folder excused from the rules above.
    let mut kept = vec![folder.clone(), file.clone()];
    if temporary && let Some(temp) = places.temp.as_ref().map(|temp| resolved(temp)) {
        kept.extend(
            folder
                .ancestors()
                .skip(1)
                .take_while(|above| above.starts_with(&temp) && *above != temp)
                .map(Path::to_path_buf),
        );
    }
    for path in &kept {
        if let Some(why) = others_may_change(path) {
            return Err(why);
        }
    }
    let text = std::fs::read_to_string(&file)
        .map_err(|_| "its project file, `anything2mcp.toml`, cannot be read")?;
    let alone = Sources {
        project: Some(SourceFile::new(PROJECT_FILE_NAME, text)),
        packs: settings.packs().to_vec(),
        ..Sources::none()
    };
    if !Settings::load_all(&alone).1.is_empty() {
        return Err(
            "its project file, `anything2mcp.toml`, is not a settings file this engine can read",
        );
    }

    // What a person allowed: any folder that holds a project file, or only
    // those inside the folders the setting lists.
    let roots = settings.list(ALLOWED_ROOTS);
    let allowed = roots.iter().any(|root| {
        root == PROJECT_ROOT_WORD || {
            let root = Path::new(root);
            root.is_absolute() && folder.starts_with(resolved(root))
        }
    });
    if !allowed {
        return Err("it is not inside a folder the setting `files.allowed_roots` allows");
    }
    Ok(folder)
}

/// Why an account other than this process's user may change the file or
/// folder at `path`, or `None` when only that user or the administrator
/// may. The rule is the client's own (`kept_by` of `a2m-local`), kept here
/// because this crate is below that one: the owner of the entry, and of
/// what a link at the path leads to, is this user or the administrator;
/// every account may not write it; and a group may write it only when the
/// group is this user's primary group. On Windows the owner and the access
/// list are not read yet, and nothing is refused by this rule.
fn others_may_change(path: &Path) -> Option<&'static str> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let me = rustix::process::getuid().as_raw();
        let mine = rustix::process::getgid().as_raw();
        let (Ok(link), Ok(found)) = (std::fs::symlink_metadata(path), std::fs::metadata(path))
        else {
            return Some("who may change it cannot be read");
        };
        if [&link, &found]
            .iter()
            .any(|meta| meta.uid() != me && meta.uid() != 0)
        {
            return Some("it, or its project file, belongs to another account");
        }
        let mode = found.mode();
        if mode & 0o002 != 0 {
            return Some("every account may change it, or its project file");
        }
        if mode & 0o020 != 0 && found.gid() != mine {
            return Some("a group of other accounts may change it, or its project file");
        }
        None
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// Checks the path of a source file, as a call wrote it: a path inside the
/// project's `sources` folder, with no hidden part.
///
/// # Errors
///
/// The reason the path is not used, as the end of a sentence.
pub(crate) fn source_path(path: &str) -> Result<(), &'static str> {
    let mut parts = path.split('/');
    if parts.next() != Some(SOURCES_FOLDER) || path.len() <= SOURCES_FOLDER.len() + 1 {
        return Err("a source file is read only from the project's `sources` folder");
    }
    if path.split('/').any(|part| part.starts_with('.')) {
        return Err("a part of it is hidden: its name starts with a dot");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Places, check, source_path};
    use crate::paths::{Env, OsFamily, Paths};
    use crate::settings::{Settings, SourceFile, Sources};

    /// A folder under the system's temporary folder, deleted when dropped.
    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("a2m-actions-project-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(std::fs::canonicalize(&path).unwrap())
        }

        /// A folder below this one that holds a project file.
        fn project(&self, below: &[&str]) -> PathBuf {
            let folder = below
                .iter()
                .fold(self.0.clone(), |path, part| path.join(part));
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("anything2mcp.toml"), "").unwrap();
            folder
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn places(home: &Path, vars: &[(&str, &Path)]) -> Places {
        let mut all = vec![("HOME", home.to_str().unwrap())];
        all.extend(
            vars.iter()
                .map(|(name, path)| (*name, path.to_str().unwrap())),
        );
        // No temporary folder is named: every folder of the test is under
        // the made-up home, and none is excused.
        Places::of(&Env::with(OsFamily::Linux, &all), None)
    }

    #[test]
    fn a_project_folder_holds_a_project_file_and_is_no_programs_own_folder() {
        let root = Folder::new("bounds");
        let home = root.0.join("home").join("pat");
        std::fs::create_dir_all(&home).unwrap();
        let paths = Paths::under(&root.0.join("engine"));
        let settings = Settings::defaults();
        let data = root.0.join("moved-data");
        std::fs::create_dir_all(&data).unwrap();
        let places = places(&home, &[("XDG_DATA_HOME", &data)]);
        let look = |folder: &Path| check(folder, &paths, &settings, &places);
        let home_of = |below: &[&str]| {
            let mut parts = vec!["home", "pat"];
            parts.extend_from_slice(below);
            root.project(&parts)
        };

        // A folder of the person's own that holds a project file is taken,
        // and comes back resolved.
        let work = home_of(&["work", "orders"]);
        assert_eq!(look(&work).unwrap(), work);

        // The same folder without its project file is not a project.
        let bare = home.join("work").join("notes");
        std::fs::create_dir_all(&bare).unwrap();
        assert!(look(&bare).unwrap_err().contains("no project file"));
        // A project file that is not a settings file does not make one.
        let odd = home_of(&["work", "odd"]);
        std::fs::write(odd.join("anything2mcp.toml"), "[no_such_group]\nkey = 1\n").unwrap();
        assert!(look(&odd).unwrap_err().contains("not a settings file"));
        std::fs::write(odd.join("anything2mcp.toml"), "not toml [").unwrap();
        assert!(look(&odd).unwrap_err().contains("not a settings file"));

        // A key folder, a browser's profile, a program's settings: hidden
        // folders of the home folder, and the folder the environment gives
        // programs for their data. A project file put there changes
        // nothing.
        for (below, why) in [
            (&[".keys"][..], "hidden folder"),
            (&[".config", "browser", "Default"][..], "hidden folder"),
            (&[".local", "share", "program"][..], "hidden folder"),
        ] {
            let folder = home_of(below);
            assert!(look(&folder).unwrap_err().contains(why), "{folder:?}");
        }
        let program = root.project(&["moved-data", "program"]);
        assert!(look(&program).unwrap_err().contains("other programs"));
        // The home folder, and a folder above it, hold all of those.
        std::fs::write(home.join("anything2mcp.toml"), "").unwrap();
        assert!(look(&home).unwrap_err().contains("home folder"));
        std::fs::write(root.0.join("anything2mcp.toml"), "").unwrap();
        assert!(look(&root.0).unwrap_err().contains("engine's own"));
        let above = root.0.join("home");
        std::fs::write(above.join("anything2mcp.toml"), "").unwrap();
        assert!(look(&above).unwrap_err().contains("home folder"));
        // A hidden folder deeper down is the person's own business.
        let deeper = home_of(&["work", ".drafts"]);
        assert_eq!(look(&deeper).unwrap(), deeper);

        // A link to a key folder is the key folder.
        #[cfg(unix)]
        {
            let link = home.join("work").join("innocent");
            std::os::unix::fs::symlink(home.join(".keys"), &link).unwrap();
            assert!(look(&link).unwrap_err().contains("hidden folder"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_project_folder_or_file_that_others_may_change_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let root = Folder::new("kept");
        let home = root.0.join("home").join("pat");
        std::fs::create_dir_all(&home).unwrap();
        let paths = Paths::under(&root.0.join("engine"));
        let settings = Settings::defaults();
        let places = places(&home, &[]);
        let look = |folder: &Path| check(folder, &paths, &settings, &places);
        let mode = |path: &Path, mode: u32| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        let work = root.project(&["home", "pat", "work"]);
        mode(&work, 0o755);
        mode(&work.join("anything2mcp.toml"), 0o644);
        assert_eq!(look(&work).unwrap(), work);
        // A project file every account may write.
        mode(&work.join("anything2mcp.toml"), 0o666);
        assert!(look(&work).unwrap_err().contains("every account"));
        mode(&work.join("anything2mcp.toml"), 0o644);
        // A folder every account may write.
        mode(&work, 0o777);
        assert!(look(&work).unwrap_err().contains("every account"));
        mode(&work, 0o755);
        assert_eq!(look(&work).unwrap(), work);

        // Inside the folder for temporary files, the folders between it and
        // the project are held to the rule as well.
        let excused = Places::of(
            &Env::with(OsFamily::Linux, &[("HOME", home.to_str().unwrap())]),
            Some(root.0.clone()),
        );
        let shared = root.project(&["shared", "work"]);
        mode(&root.0.join("shared"), 0o777);
        let refused = check(&shared, &paths, &settings, &excused).unwrap_err();
        assert!(refused.contains("every account"), "{refused}");
        mode(&root.0.join("shared"), 0o755);
        assert_eq!(check(&shared, &paths, &settings, &excused).unwrap(), shared);
    }

    #[test]
    fn the_allowed_roots_bound_which_folder_may_be_a_project() {
        let root = Folder::new("roots");
        let home = root.0.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let paths = Paths::under(&root.0.join("engine"));
        let places = places(&home, &[]);
        let inside = root.project(&["home", "work", "orders"]);
        let outside = root.project(&["home", "other", "orders"]);
        let with = |roots: &str| {
            Settings::load(&Sources {
                user: Some(SourceFile::new(
                    "settings.toml",
                    format!("[files]\nallowed_roots = {roots}\n"),
                )),
                ..Sources::none()
            })
            .unwrap()
        };

        // The default: any folder that holds a project file.
        let default = Settings::defaults();
        assert!(check(&inside, &paths, &default, &places).is_ok());
        assert!(check(&outside, &paths, &default, &places).is_ok());

        // The setting tightened to nothing: no folder is a project.
        let none = with("[]");
        for folder in [&inside, &outside] {
            let refused = check(folder, &paths, &none, &places).unwrap_err();
            assert!(refused.contains("files.allowed_roots"), "{refused}");
        }
        // A name that is neither the word nor a full path allows nothing.
        let named = Settings::load(&Sources {
            installation: Some(SourceFile::new(
                "settings.toml",
                "[files]\nallowed_roots = [\"work\"]\n",
            )),
            ..Sources::none()
        })
        .unwrap();
        assert!(check(&inside, &paths, &named, &places).is_err());
        // One full path, set where a loosening may be set: only folders
        // inside it.
        let work = home.join("work");
        let one = Settings::load(&Sources {
            installation: Some(SourceFile::new(
                "settings.toml",
                format!(
                    "[files]\nallowed_roots = [{}]\n",
                    serde_json::Value::String(work.to_string_lossy().into_owned())
                ),
            )),
            ..Sources::none()
        })
        .unwrap();
        assert_eq!(check(&inside, &paths, &one, &places).unwrap(), inside);
        assert!(check(&outside, &paths, &one, &places).is_err());
    }

    #[test]
    fn a_source_file_is_named_inside_the_sources_folder_with_no_hidden_part() {
        assert!(source_path("sources/items.csv").is_ok());
        assert!(source_path("sources/2026/items.csv").is_ok());
        for refused in [
            "items.csv",
            "sheets/items.csv",
            "sources",
            "sources/",
            "sourcesx/items.csv",
            "targets/example/target.yaml",
            "anything2mcp.toml",
            "sources/.env",
            "sources/.git/config",
            ".anything2mcp/out/x",
        ] {
            assert!(source_path(refused).is_err(), "{refused}");
        }
    }
}
