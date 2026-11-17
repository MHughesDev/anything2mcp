//! File access for packs: named roots, and the one check that makes a
//! [`CheckedPath`].
//!
//! A pack never resolves a path. It names a root and a relative path, and
//! [`Roots::check`] either gives a `CheckedPath` or refuses. The rules are
//! the same on every operating system: a form that would escape on one
//! system is refused on all of them, so a target file that works on one
//! machine cannot reach outside its root on another.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use a2m_core::Failure;

use crate::codes::{self, shown};
use crate::name::RootName;

/// What a path is wanted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Access {
    /// Reading.
    Read,
    /// Reading and writing.
    Write,
}

/// The longest relative path, in bytes.
const LONGEST_RELATIVE: usize = 1024;

/// Names that open a device on Windows, whatever follows the first dot.
const DEVICE_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9", "conin$",
    "conout$",
];

#[derive(Debug, Clone)]
struct Root {
    /// The root, resolved once when it was added.
    path: PathBuf,
    write: bool,
}

/// The named roots: folders from settings and the project, each readable or
/// writable. Made by the product crate; a pack receives the subset its
/// manifest declares.
#[derive(Debug, Clone, Default)]
pub struct Roots {
    roots: BTreeMap<RootName, Root>,
}

fn refuse(relative: &str, why: &str) -> Failure {
    codes::input(
        codes::PATH_NOT_ALLOWED,
        format!("The path `{}` is refused: {why}.", shown(relative)),
        "Name a file inside the root by a relative path with forward slashes and no `..`.",
    )
}

/// Checks the text of a relative path and gives its parts. The check is
/// lexical and the same on every operating system.
fn parts(relative: &str) -> Result<Vec<&str>, Failure> {
    if relative.is_empty() {
        return Err(refuse(relative, "it is empty"));
    }
    if relative.len() > LONGEST_RELATIVE {
        return Err(refuse(relative, "it is too long"));
    }
    if relative.chars().any(char::is_control) {
        return Err(refuse(relative, "it holds a control character"));
    }
    if relative.contains('\\') {
        return Err(refuse(
            relative,
            "it holds a backslash, which separates folders, or starts a network path, on one operating system",
        ));
    }
    if relative.contains(':') {
        return Err(refuse(
            relative,
            "it holds a colon, which names a drive or a stream on one operating system",
        ));
    }
    if relative.starts_with('/') {
        return Err(refuse(relative, "it is absolute"));
    }
    if relative.starts_with('~') {
        return Err(refuse(relative, "it starts with a home folder mark"));
    }
    let mut found = Vec::new();
    for part in relative.split('/') {
        if part.is_empty() {
            return Err(refuse(relative, "it has an empty part"));
        }
        if part == "." || part == ".." {
            return Err(refuse(relative, "it steps through `.` or `..`"));
        }
        if part.ends_with('.') || part.ends_with(' ') {
            return Err(refuse(
                relative,
                "a part ends with a dot or a space, which one operating system drops",
            ));
        }
        let stem = part.split('.').next().unwrap_or(part).to_ascii_lowercase();
        if DEVICE_NAMES.contains(&stem.as_str()) {
            return Err(refuse(
                relative,
                "a part names a device on one operating system",
            ));
        }
        found.push(part);
    }
    Ok(found)
}

/// Resolves a path that exists, following every link in it.
fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    fs::canonicalize(path).map(|real| plain(&real))
}

/// On Windows a resolved path comes back in the verbatim form, which many
/// programs do not take as an argument or as a working folder. This gives
/// the ordinary form of the same path where there is one. Elsewhere a path
/// has no prefix and comes back unchanged.
fn plain(path: &Path) -> PathBuf {
    use std::path::{Component, Prefix};
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
    out.extend(components.filter(|part| !matches!(part, Component::RootDir)));
    out
}

impl Roots {
    /// No root at all: every check is refused.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Adds a root. `folder` must be an absolute path of a folder that
    /// exists; it is resolved once, here, and never again.
    ///
    /// # Errors
    ///
    /// `input`/`path_not_allowed` when the folder is not absolute, does not
    /// exist, or is not a folder, or the name is taken.
    pub fn add(&mut self, name: RootName, folder: &Path, access: Access) -> Result<(), Failure> {
        let unusable = |why: &str| {
            codes::input(
                codes::PATH_NOT_ALLOWED,
                format!("The root `{name}` cannot be used: {why}."),
                "Name an existing folder by its absolute path in the settings.",
            )
        };
        if !folder.is_absolute() {
            return Err(unusable("its folder is not an absolute path"));
        }
        if self.roots.contains_key(&name) {
            return Err(unusable("a root of that name exists"));
        }
        let path = canonical(folder).map_err(|_| unusable("its folder does not exist"))?;
        if !path.is_dir() {
            return Err(unusable("it is not a folder"));
        }
        self.roots.insert(
            name,
            Root {
                path,
                write: access == Access::Write,
            },
        );
        Ok(())
    }

    /// The roots' names, in order.
    pub fn names(&self) -> impl Iterator<Item = &RootName> {
        self.roots.keys()
    }

    /// The subset a provider declared: `read` roots readable, `write` roots
    /// writable where this set allows writing.
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared` when a declared write root is one
    /// this set only reads. A declared root that does not exist here is left
    /// out: the provider learns it when it checks a path.
    pub fn narrowed(&self, read: &[RootName], write: &[RootName]) -> Result<Self, Failure> {
        let mut roots = BTreeMap::new();
        for name in read {
            if let Some(root) = self.roots.get(name) {
                roots.insert(
                    name.clone(),
                    Root {
                        path: root.path.clone(),
                        write: false,
                    },
                );
            }
        }
        for name in write {
            if let Some(root) = self.roots.get(name) {
                if !root.write {
                    return Err(codes::policy(
                        codes::EFFECT_NOT_DECLARED,
                        format!(
                            "The root `{name}` is read only here and a provider declares it writes there."
                        ),
                        "Make the root writable in the settings, or disable the provider.",
                    ));
                }
                roots.insert(name.clone(), root.clone());
            }
        }
        Ok(Self { roots })
    }

    fn root(&self, name: &RootName, access: Access) -> Result<&Root, Failure> {
        let root = self.roots.get(name).ok_or_else(|| {
            codes::policy(
                codes::EFFECT_NOT_DECLARED,
                format!("The root `{name}` is not one this caller may use."),
                "Declare the root in the provider's manifest, and name its folder in the settings.",
            )
        })?;
        if access == Access::Write && !root.write {
            return Err(codes::policy(
                codes::EFFECT_NOT_DECLARED,
                format!("The root `{name}` may be read and not written."),
                "Declare the root under `write_roots` in the provider's manifest.",
            ));
        }
        Ok(root)
    }

    /// The root folder itself.
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared` when the root is not in this set, or
    /// is read only and `access` is `Write`.
    pub fn folder(&self, root: &RootName, access: Access) -> Result<CheckedPath, Failure> {
        let found = self.root(root, access)?;
        Ok(CheckedPath {
            root: root.clone(),
            base: found.path.clone(),
            relative: String::new(),
            full: found.path.clone(),
            access,
        })
    }

    /// The one way a path is made: a named root and a relative path with
    /// forward slashes. Refused: an empty path, an absolute path, `..` or
    /// `.`, a backslash, a colon (so every drive, stream, and network form),
    /// a control character, a part that ends with a dot or a space, a device
    /// name, and a link that leads outside the root. The path is resolved
    /// once, here.
    ///
    /// # Errors
    ///
    /// `input`/`path_not_allowed`; `policy`/`effect_not_declared` for a root
    /// this set does not hold or may not write.
    pub fn check(
        &self,
        root: &RootName,
        relative: &str,
        access: Access,
    ) -> Result<CheckedPath, Failure> {
        let found = self.root(root, access)?;
        resolve(root, &found.path, relative, access)
    }
}

/// Resolves `relative` under `base`, following links only while they stay
/// inside `base`.
fn resolve(
    root: &RootName,
    base: &Path,
    relative: &str,
    access: Access,
) -> Result<CheckedPath, Failure> {
    let parts = parts(relative)?;
    let mut full = base.to_path_buf();
    let mut missing = false;
    for part in &parts {
        if missing {
            full.push(part);
            continue;
        }
        let next = full.join(part);
        match fs::symlink_metadata(&next) {
            Ok(_) => {
                // It exists: resolve it, link or not, and stay inside.
                let real =
                    canonical(&next).map_err(|_| refuse(relative, "a link in it leads nowhere"))?;
                if !real.starts_with(base) {
                    return Err(refuse(relative, "a link in it leads outside the root"));
                }
                full = real;
            }
            Err(_) => {
                missing = true;
                full.push(part);
            }
        }
    }
    Ok(CheckedPath {
        root: root.clone(),
        base: base.to_path_buf(),
        relative: parts.join("/"),
        full,
        access,
    })
}

/// A path inside a named root, proven to be inside it when it was made.
///
/// There is no public constructor: only [`Roots::check`], [`Roots::folder`],
/// and [`CheckedPath::join`] make one. Every method that opens the file
/// looks once more that the path still resolves inside the root, so a link
/// planted after the check is refused too.
#[derive(Clone, PartialEq, Eq)]
pub struct CheckedPath {
    root: RootName,
    base: PathBuf,
    relative: String,
    full: PathBuf,
    access: Access,
}

impl fmt::Debug for CheckedPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CheckedPath({}:{})", self.root, self.relative)
    }
}

impl fmt::Display for CheckedPath {
    /// The root's name and the relative path. Never the folder on disk.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.root, self.relative)
    }
}

impl CheckedPath {
    /// The root it is in.
    #[must_use]
    pub fn root(&self) -> &RootName {
        &self.root
    }

    /// The path below the root, with forward slashes; empty for the root.
    #[must_use]
    pub fn relative(&self) -> &str {
        &self.relative
    }

    /// What it may be used for.
    #[must_use]
    pub fn access(&self) -> Access {
        self.access
    }

    /// The resolved path, to hand to a program the supervisor starts or to
    /// an operating-system call. Never build another path from it: use
    /// [`CheckedPath::join`].
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.full
    }

    /// A path below this one, under the same check.
    ///
    /// # Errors
    ///
    /// As [`Roots::check`].
    pub fn join(&self, relative: &str) -> Result<Self, Failure> {
        let whole = if self.relative.is_empty() {
            relative.to_owned()
        } else {
            format!("{}/{relative}", self.relative)
        };
        parts(relative)?;
        resolve(&self.root, &self.base, &whole, self.access)
    }

    fn unavailable(&self, what: &str) -> Failure {
        codes::input(
            codes::FILE_UNAVAILABLE,
            format!("The file `{self}` {what}."),
            "Check that the file exists and that the engine may use it.",
        )
    }

    /// Looks again, at the moment of use, that the path is inside its root.
    fn still_inside(&self) -> Result<(), Failure> {
        let deepest = self
            .full
            .ancestors()
            .find(|ancestor| fs::symlink_metadata(ancestor).is_ok())
            .unwrap_or(&self.base);
        let real = canonical(deepest).map_err(|_| self.unavailable("cannot be resolved"))?;
        if real.starts_with(&self.base) && (deepest != self.full || real == self.full) {
            Ok(())
        } else {
            Err(codes::input(
                codes::PATH_NOT_ALLOWED,
                format!("The path `{self}` no longer resolves inside its root."),
                "Remove the link that was placed in the folder, then try again.",
            ))
        }
    }

    fn writable(&self) -> Result<(), Failure> {
        if self.access == Access::Write {
            Ok(())
        } else {
            Err(codes::policy(
                codes::EFFECT_NOT_DECLARED,
                format!("The path `{self}` was checked for reading and not for writing."),
                "Check the path with write access, in a root the manifest declares it writes.",
            ))
        }
    }

    /// Whether something exists at the path.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.still_inside().is_ok() && self.full.exists()
    }

    /// Whether the path is a folder.
    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.still_inside().is_ok() && self.full.is_dir()
    }

    /// The file's length in bytes.
    ///
    /// # Errors
    ///
    /// `input`/`file_unavailable`; `input`/`path_not_allowed`.
    pub fn size(&self) -> Result<u64, Failure> {
        self.still_inside()?;
        fs::metadata(&self.full)
            .map(|meta| meta.len())
            .map_err(|_| self.unavailable("cannot be read"))
    }

    /// Opens the file for reading.
    ///
    /// # Errors
    ///
    /// `input`/`file_unavailable`; `input`/`path_not_allowed`.
    pub fn open(&self) -> Result<fs::File, Failure> {
        self.still_inside()?;
        fs::File::open(&self.full).map_err(|_| self.unavailable("cannot be opened"))
    }

    /// The whole file, when it has at most `max_bytes` bytes.
    ///
    /// # Errors
    ///
    /// `input`/`file_unavailable`, also when the file is larger;
    /// `input`/`path_not_allowed`.
    pub fn read(&self, max_bytes: u64) -> Result<Vec<u8>, Failure> {
        use std::io::Read as _;
        let file = self.open()?;
        let mut bytes = Vec::new();
        file.take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| self.unavailable("cannot be read"))?;
        if bytes.len() as u64 > max_bytes {
            return Err(self.unavailable("is larger than the limit for reading it"));
        }
        Ok(bytes)
    }

    /// Writes the whole file, replacing what was there: the bytes go to a
    /// new file beside it, which then takes its name.
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared` for a path checked for reading;
    /// `input`/`file_unavailable`; `input`/`path_not_allowed`.
    pub fn write(&self, bytes: &[u8]) -> Result<(), Failure> {
        self.writable()?;
        self.still_inside()?;
        let name = self
            .full
            .file_name()
            .ok_or_else(|| self.unavailable("is a root and not a file"))?;
        let mut beside = name.to_os_string();
        beside.push(".a2m-new");
        let temporary = self.full.with_file_name(beside);
        fs::write(&temporary, bytes).map_err(|_| self.unavailable("cannot be written"))?;
        fs::rename(&temporary, &self.full).map_err(|_| {
            // Nothing is left behind when the new file cannot take the name.
            let _ = fs::remove_file(&temporary);
            self.unavailable("cannot be replaced")
        })
    }

    /// Makes the folder and every folder above it inside the root.
    ///
    /// # Errors
    ///
    /// As [`CheckedPath::write`].
    pub fn create_dir_all(&self) -> Result<(), Failure> {
        self.writable()?;
        self.still_inside()?;
        fs::create_dir_all(&self.full).map_err(|_| self.unavailable("cannot be made"))?;
        self.still_inside()
    }

    /// Removes the file.
    ///
    /// # Errors
    ///
    /// As [`CheckedPath::write`].
    pub fn remove_file(&self) -> Result<(), Failure> {
        self.writable()?;
        self.still_inside()?;
        fs::remove_file(&self.full).map_err(|_| self.unavailable("cannot be removed"))
    }

    /// The entries of the folder, in order of name, each checked. An entry
    /// whose name is not a form a path may take, or that links outside the
    /// root, is left out and counted.
    ///
    /// # Errors
    ///
    /// `input`/`file_unavailable`; `input`/`path_not_allowed`.
    pub fn list(&self) -> Result<(Vec<Self>, usize), Failure> {
        self.still_inside()?;
        let entries = fs::read_dir(&self.full).map_err(|_| self.unavailable("cannot be listed"))?;
        let mut names = Vec::new();
        let mut left_out = 0;
        for entry in entries {
            let entry = entry.map_err(|_| self.unavailable("cannot be listed"))?;
            match entry.file_name().to_str() {
                Some(name) => names.push(name.to_owned()),
                None => left_out += 1,
            }
        }
        names.sort();
        let mut found = Vec::new();
        for name in names {
            match self.join(&name) {
                Ok(path) => found.push(path),
                Err(_) => left_out += 1,
            }
        }
        Ok((found, left_out))
    }
}

/// Every file a run may touch, each already checked, by the name the plan
/// gives it. A driver resolves none itself.
#[derive(Debug, Clone, Default)]
pub struct CheckedPaths {
    paths: BTreeMap<String, CheckedPath>,
}

impl CheckedPaths {
    /// No path.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Adds a path under a name.
    pub fn insert(&mut self, name: impl Into<String>, path: CheckedPath) {
        self.paths.insert(name.into(), path);
    }

    /// The path of that name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&CheckedPath> {
        self.paths.get(name)
    }

    /// Every name and path, in order of name.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &CheckedPath)> {
        self.paths.iter().map(|(name, path)| (name.as_str(), path))
    }
}
