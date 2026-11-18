//! File access for packs: named roots, and the one check that makes a
//! [`CheckedPath`].
//!
//! A pack never resolves a path. It names a root and a relative path, and
//! [`Roots::check`] either gives a `CheckedPath` or refuses. The rules are
//! the same on every operating system: a form that would escape on one
//! system is refused on all of them, so a target file that works on one
//! machine cannot reach outside its root on another.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use a2m_core::Failure;

use crate::codes::{self, shown};
use crate::name::RootName;

mod place;

/// What a path is wanted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Access {
    /// Reading.
    Read,
    /// Reading and writing.
    Write,
}

/// What stands at a path's final name, as it is now. A link there is
/// reported and never followed, so a caller can tell a file from a link to
/// one, which [`CheckedPath::as_path`] and the reading methods cannot: they
/// hold the path with every link already resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntryKind {
    /// Nothing is there, or a folder on the way to it is not there.
    Absent,
    /// A regular file.
    File,
    /// A folder.
    Folder,
    /// A link, whatever it leads to and whether or not it leads anywhere.
    Link,
    /// Something else: a device, a pipe, a socket.
    Other,
}

/// The longest relative path, in bytes.
const LONGEST_RELATIVE: usize = 1024;

/// Whether a part of a path names a device on Windows, wherever it stands
/// in a path and whatever follows its first dot: `con`, `prn`, `aux`,
/// `nul`, `conin$`, `conout$`, and `com` or `lpt` with one digit, `0` to
/// `9` or a superscript one, two, or three, with spaces before the dot
/// ignored, as Windows ignores them.
fn names_a_device(part: &str) -> bool {
    let stem = part
        .split('.')
        .next()
        .unwrap_or(part)
        .trim_end_matches(' ')
        .to_lowercase();
    let numbered = stem
        .strip_prefix("com")
        .or_else(|| stem.strip_prefix("lpt"))
        .is_some_and(|number| {
            let mut chars = number.chars();
            matches!(
                (chars.next(), chars.next()),
                (Some('0'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}'), None)
            )
        });
    numbered
        || matches!(
            stem.as_str(),
            "con" | "prn" | "aux" | "nul" | "conin$" | "conout$"
        )
}

#[derive(Debug, Clone)]
struct Root {
    /// The root, resolved once when it was added; `None` for a root that
    /// stands for the project folder of each request ([`Roots::add_project`]).
    path: Option<PathBuf>,
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
        if names_a_device(part) {
            return Err(refuse(
                relative,
                "a part names a device on one operating system",
            ));
        }
        found.push(part);
    }
    Ok(found)
}

/// Makes `path` and the folders above it that are not there, each kept to
/// the engine's user.
fn private_dir_all(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
    }
    #[cfg(not(unix))]
    {
        let mut missing = Vec::new();
        let mut at = Some(path);
        while let Some(folder) = at {
            if folder.exists() {
                break;
            }
            missing.push(folder.to_path_buf());
            at = folder.parent();
        }
        fs::create_dir_all(path)?;
        for folder in missing.iter().rev() {
            crate::supervisor::keep_folder_private(folder)?;
        }
        Ok(())
    }
}

/// A path that exists, with every link in it followed, in the one form in
/// which every root holds its folder and every checked path is made: so a
/// path resolved by this function and the folder of a root compare like
/// with like, and one lies under the other exactly when it is inside it.
///
/// On Windows the system gives a resolved path in the verbatim form,
/// `\\?\C:\...`, which a person does not write and many programs do not
/// take as an argument or as a working folder. This gives the ordinary
/// form, `C:\...` (`\\?\UNC\server\share\...` as `\\server\share\...`),
/// wherever that names the same thing, and the verbatim form where it would
/// not: a part that is `.` or `..`, ends with a dot or a space, holds a
/// `/`, or names a device. Elsewhere a path has no prefix, and this is
/// what [`std::fs::canonicalize`] gives. Touches the disk.
///
/// # Errors
///
/// As [`std::fs::canonicalize`]: the path does not exist, or a folder on
/// the way to it cannot be read.
pub fn resolved_path(path: &Path) -> std::io::Result<PathBuf> {
    fs::canonicalize(path).map(|real| plain(&real))
}

/// The ordinary form of a resolved path, as [`resolved_path`] says. Off
/// Windows a path is never changed.
fn plain(path: &Path) -> PathBuf {
    if !cfg!(windows) {
        return path.to_path_buf();
    }
    path.to_str()
        .and_then(plain_text)
        .map_or_else(|| path.to_path_buf(), PathBuf::from)
}

/// The ordinary form of a path written in Windows' verbatim form, worked
/// out on the text alone so that it is the same on every system:
/// `\\?\C:\x` is `C:\x`, and `\\?\UNC\server\share\x` is
/// `\\server\share\x`. `None` when the text is not in one of those two
/// forms, or when its ordinary form would name something else: a part that
/// is `.` or `..`, ends with a dot or a space (both dropped from an ordinary
/// path), holds a `/` (a separator in an ordinary path), or names a device.
fn plain_text(text: &str) -> Option<String> {
    let rest = text.strip_prefix(r"\\?\")?;
    let (start, parts) = if let Some(unc) = rest.strip_prefix(r"UNC\") {
        let mut split = unc.splitn(3, '\\');
        let server = split.next().filter(|part| !part.is_empty())?;
        let share = split.next().filter(|part| !part.is_empty())?;
        (format!(r"\\{server}\{share}\"), split.next().unwrap_or(""))
    } else {
        let bytes = rest.as_bytes();
        let disk = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'\\';
        if !disk {
            return None;
        }
        (format!("{}\\", &rest[..2]), &rest[3..])
    };
    let names_the_same = parts.is_empty()
        || parts.split('\\').all(|part| {
            !part.is_empty()
                && !part.ends_with('.')
                && !part.ends_with(' ')
                && !part.contains('/')
                && !names_a_device(part)
        });
    names_the_same.then(|| format!("{start}{parts}"))
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
        let path = resolved_path(folder).map_err(|_| unusable("its folder does not exist"))?;
        if !path.is_dir() {
            return Err(unusable("it is not a folder"));
        }
        self.roots.insert(
            name,
            Root {
                path: Some(path),
                write: access == Access::Write,
            },
        );
        Ok(())
    }

    /// Adds a root bound to no folder now: it stands for the project folder
    /// of each request, which a run's [`CheckedPaths::project_folder`]
    /// gives, so that one engine that serves many projects resolves it in
    /// the project of the request at hand and never in the one that started
    /// it. A path under it is made by [`Roots::check_in`] and
    /// [`Roots::folder_in`]; [`Roots::check`] and [`Roots::folder`] refuse it
    /// (decision 0193).
    ///
    /// # Errors
    ///
    /// `input`/`path_not_allowed` when the name is taken.
    pub fn add_project(&mut self, name: RootName, access: Access) -> Result<(), Failure> {
        if self.roots.contains_key(&name) {
            return Err(codes::input(
                codes::PATH_NOT_ALLOWED,
                format!("The root `{name}` cannot be used: a root of that name exists."),
                "Name an existing folder by its absolute path in the settings.",
            ));
        }
        self.roots.insert(
            name,
            Root {
                path: None,
                write: access == Access::Write,
            },
        );
        Ok(())
    }

    /// Whether this set holds the root, for `access`, without resolving
    /// anything: for a driver's health, which a root that stands for each
    /// request's project folder passes with no folder at hand.
    #[must_use]
    pub fn holds(&self, name: &RootName, access: Access) -> bool {
        self.root(name, access).is_ok()
    }

    /// Whether the root stands for the project folder of each request.
    #[must_use]
    pub fn is_project(&self, name: &RootName) -> bool {
        self.roots.get(name).is_some_and(|root| root.path.is_none())
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
        let found = bound(root, self.root(root, access)?)?;
        Ok(folder_path(root, found, access))
    }

    /// The root folder itself, where the root stands for the request's
    /// project folder: that folder, from the run's checked paths. Any other
    /// root as [`Roots::folder`] gives it.
    ///
    /// # Errors
    ///
    /// As [`Roots::folder`]; `input`/`project_folder_missing` for a root of
    /// the request's project when the run has no project folder.
    pub fn folder_in(
        &self,
        root: &RootName,
        access: Access,
        paths: &CheckedPaths,
    ) -> Result<CheckedPath, Failure> {
        let found = self.root(root, access)?;
        match &found.path {
            Some(path) => Ok(folder_path(root, path, access)),
            None => Ok(folder_path(root, paths.project_folder()?.as_path(), access)),
        }
    }

    /// As [`Roots::check`], where a root that stands for the request's
    /// project folder is resolved in the run's checked project folder: the
    /// same refusals, a link that leads out of that folder included, so a
    /// request's path never reaches another project's files.
    ///
    /// # Errors
    ///
    /// As [`Roots::check`]; `input`/`project_folder_missing` for a root of
    /// the request's project when the run has no project folder.
    pub fn check_in(
        &self,
        root: &RootName,
        relative: &str,
        access: Access,
        paths: &CheckedPaths,
    ) -> Result<CheckedPath, Failure> {
        let found = self.root(root, access)?;
        match &found.path {
            Some(path) => resolve(root, path, relative, access),
            None => resolve(root, paths.project_folder()?.as_path(), relative, access),
        }
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
        let found = bound(root, self.root(root, access)?)?;
        resolve(root, found, relative, access)
    }
}

/// The folder a root was bound to; a root of each request's project has
/// none, and is checked with the run's paths.
fn bound<'a>(name: &RootName, root: &'a Root) -> Result<&'a Path, Failure> {
    root.path.as_deref().ok_or_else(|| {
        codes::input(
            codes::PROJECT_FOLDER_MISSING,
            format!("The root `{name}` stands for the project folder of each request, and none was given here."),
            "Check the path with the run's checked paths, which hold the request's project folder.",
        )
    })
}

fn folder_path(root: &RootName, path: &Path, access: Access) -> CheckedPath {
    CheckedPath {
        root: root.clone(),
        base: path.to_path_buf(),
        relative: String::new(),
        full: path.to_path_buf(),
        named: path.to_path_buf(),
        access,
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
    // The same path with the link at its final name, if one stands there,
    // not followed: the folders above it resolved, and the name as written.
    let mut named = base.to_path_buf();
    let mut missing = false;
    for part in &parts {
        named = full.join(part);
        if missing {
            full.push(part);
            continue;
        }
        match fs::symlink_metadata(&named) {
            Ok(_) => {
                // It exists: resolve it, link or not, and stay inside.
                let real = resolved_path(&named)
                    .map_err(|_| refuse(relative, "a link in it leads nowhere"))?;
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
        named,
        access,
    })
}

/// A path inside a named root, proven to be inside it when it was made.
///
/// There is no public constructor: only [`Roots::check`], [`Roots::folder`],
/// and [`CheckedPath::join`] make one.
///
/// What it holds about links. When it is made, every link in the path that
/// exists is followed, a link at the final name too, as long as each leads
/// to a place inside the root; one that leads outside, or nowhere, is
/// refused. [`CheckedPath::as_path`] and the methods that read use that
/// resolved path. A caller that must not act through a link asks
/// [`CheckedPath::entry_kind`], which reports what stands at the final name
/// and follows nothing.
///
/// What it does at the moment of use. [`CheckedPath::write`] and
/// [`CheckedPath::remove_file`] act on the final name in its folder and
/// never through a link: where a link stands at the name, they refuse. On
/// Linux and macOS they, [`CheckedPath::open`], and
/// [`CheckedPath::entry_kind`] reach the folder by opening each folder from
/// the root down without following a link, so a link put on the way after
/// the check is refused and never followed, whenever it is put there. The
/// other methods, and every method on Windows, look once more that the path
/// still resolves inside the root and then use it by name; a link swapped in
/// between those two moments is followed.
#[derive(Clone, PartialEq, Eq)]
pub struct CheckedPath {
    root: RootName,
    base: PathBuf,
    relative: String,
    /// Every link resolved, the one at the final name too.
    full: PathBuf,
    /// The folders above the final name resolved, and that name as written.
    named: PathBuf,
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
        let real = resolved_path(deepest).map_err(|_| self.unavailable("cannot be resolved"))?;
        if real.starts_with(&self.base) && (deepest != self.full || real == self.full) {
            Ok(())
        } else {
            Err(self.outside())
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

    /// The path is no longer inside its root.
    fn outside(&self) -> Failure {
        codes::input(
            codes::PATH_NOT_ALLOWED,
            format!("The path `{self}` no longer resolves inside its root."),
            "Remove the link that was placed in the folder, then try again.",
        )
    }

    /// A link stands where a file is to be written or removed.
    fn through_a_link(&self) -> Failure {
        codes::input(
            codes::PATH_NOT_ALLOWED,
            format!(
                "The path `{self}` is a link, and a file is never written or removed through a link."
            ),
            "Name the file the link leads to, or remove the link, then try again.",
        )
    }

    /// The folder that holds the final name, as written, and that name.
    fn place(&self) -> Result<(&Path, &OsStr), Failure> {
        match (self.named.parent(), self.named.file_name()) {
            (Some(folder), Some(name)) if !self.relative.is_empty() => Ok((folder, name)),
            _ => Err(self.unavailable("is a root and not a file")),
        }
    }

    /// Reaches `folder` now, from the root down. `None` when it is not there.
    fn folder_now(&self, folder: &Path) -> Result<Option<place::Folder>, Failure> {
        match place::Folder::reach(&self.base, folder) {
            Ok(folder) => Ok(Some(folder)),
            Err(place::Unreached::Absent) => Ok(None),
            Err(place::Unreached::Outside) => Err(self.outside()),
            Err(place::Unreached::Other) => Err(self.unavailable("cannot be reached")),
        }
    }

    /// What stands at the final name now. A link there is reported as a
    /// link and is not followed; a link on the way to it that was not there
    /// when the path was checked is refused on Linux and macOS, and on
    /// Windows when it leads outside the root.
    ///
    /// # Errors
    ///
    /// `input`/`path_not_allowed` when the folder that holds the name is no
    /// longer inside the root; `input`/`file_unavailable` when it cannot be
    /// looked at.
    pub fn entry_kind(&self) -> Result<EntryKind, Failure> {
        if self.relative.is_empty() {
            self.still_inside()?;
            return Ok(EntryKind::Folder);
        }
        let (folder, name) = self.place()?;
        match self.folder_now(folder)? {
            None => Ok(EntryKind::Absent),
            Some(folder) => folder
                .kind(name)
                .map_err(|_| self.unavailable("cannot be looked at")),
        }
    }

    /// Opens the file for reading.
    ///
    /// # Errors
    ///
    /// `input`/`file_unavailable`; `input`/`path_not_allowed`.
    pub fn open(&self) -> Result<fs::File, Failure> {
        self.still_inside()?;
        #[cfg(unix)]
        if let (Some(folder), Some(name)) = (self.full.parent(), self.full.file_name())
            && !self.relative.is_empty()
        {
            // The resolved path holds no link, so none is followed on the
            // way down or at the name: one put there since is refused.
            let folder = self
                .folder_now(folder)?
                .ok_or_else(|| self.unavailable("cannot be opened"))?;
            return match folder.kind(name) {
                Ok(EntryKind::Link) => Err(self.outside()),
                _ => folder
                    .open(name)
                    .map_err(|_| self.unavailable("cannot be opened")),
            };
        }
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
    /// new file in the same folder, which then takes the name. The new file
    /// has a name drawn from the operating system's random source, is made
    /// only if nothing stands at that name, and is never opened through a
    /// link; the folder is reached, and proven inside the root, at that
    /// moment. Taking the name replaces only what stands at the name.
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared` for a path checked for reading;
    /// `input`/`file_unavailable`; `input`/`path_not_allowed`, also when a
    /// link stands at the name.
    pub fn write(&self, bytes: &[u8]) -> Result<(), Failure> {
        self.writable()?;
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|_| {
            self.unavailable(
                "cannot be written: the operating system gave no random name for its new file",
            )
        })?;
        let mut temporary = String::from(".a2m-new-");
        for byte in random {
            temporary.push_str(&format!("{byte:02x}"));
        }
        self.write_through(bytes, OsStr::new(&temporary))
    }

    /// Writes the file by way of a new file of the name `temporary` in the
    /// same folder.
    fn write_through(&self, bytes: &[u8], temporary: &OsStr) -> Result<(), Failure> {
        use std::io::Write as _;
        let (folder, name) = self.place()?;
        let folder = self
            .folder_now(folder)?
            .ok_or_else(|| self.unavailable("cannot be written: its folder is not there"))?;
        if matches!(folder.kind(name), Ok(EntryKind::Link)) {
            return Err(self.through_a_link());
        }
        // Made new, or not at all: what already stands at the name, a link
        // or a file, is left as it is and nothing is written.
        let mut file = folder
            .create_new(temporary)
            .map_err(|_| self.unavailable("cannot be written"))?;
        let written = file.write_all(bytes).and_then(|()| file.flush());
        drop(file);
        written
            .and_then(|()| folder.rename(temporary, name))
            .map_err(|_| {
                // Nothing is left behind when the new file cannot take the name.
                let _ = folder.remove(temporary);
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

    /// Makes this one folder, new: the call fails when anything stands at
    /// the name already, a folder, a file, or a link, so a caller that gets
    /// `Ok` holds a folder that was empty and its own at that moment. The
    /// folder above it must be there.
    ///
    /// # Errors
    ///
    /// As [`CheckedPath::write`]; `input`/`file_unavailable` also when
    /// something is already there.
    pub fn create_dir(&self) -> Result<(), Failure> {
        self.writable()?;
        let (folder, name) = self.place()?;
        let folder = self
            .folder_now(folder)?
            .ok_or_else(|| self.unavailable("cannot be made: the folder above it is not there"))?;
        folder.create_dir(name).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                self.unavailable("cannot be made new: something is already there")
            } else {
                self.unavailable("cannot be made")
            }
        })
    }

    /// As [`CheckedPath::create_dir`], and the folder is the engine's
    /// user's alone from the moment it exists: on Linux and macOS its mode
    /// is `0700`; on Windows it is given a protected access list that
    /// grants its owner, the local system account, and the Administrators
    /// group full control and nobody else anything (decision 0250).
    ///
    /// # Errors
    ///
    /// As [`CheckedPath::create_dir`].
    pub fn create_private_dir(&self) -> Result<(), Failure> {
        self.writable()?;
        let (folder, name) = self.place()?;
        let folder = self
            .folder_now(folder)?
            .ok_or_else(|| self.unavailable("cannot be made: the folder above it is not there"))?;
        folder.create_private_dir(name).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                self.unavailable("cannot be made new: something is already there")
            } else {
                self.unavailable("cannot be made")
            }
        })
    }

    /// As [`CheckedPath::create_dir_all`], and each folder this call makes
    /// is made as [`CheckedPath::create_private_dir`] makes one. A folder
    /// that is there already is left as it is; whoever needs it closed
    /// checks it, as [`crate::check_program`] does.
    ///
    /// # Errors
    ///
    /// As [`CheckedPath::create_dir_all`].
    pub fn create_private_dir_all(&self) -> Result<(), Failure> {
        self.writable()?;
        self.still_inside()?;
        let made = private_dir_all(&self.full);
        made.map_err(|_| self.unavailable("cannot be made"))?;
        self.still_inside()
    }

    /// Removes the file. A link at the name is not removed and not
    /// followed: it is refused.
    ///
    /// # Errors
    ///
    /// As [`CheckedPath::write`].
    pub fn remove_file(&self) -> Result<(), Failure> {
        self.writable()?;
        let (folder, name) = self.place()?;
        let folder = self
            .folder_now(folder)?
            .ok_or_else(|| self.unavailable("cannot be removed"))?;
        if matches!(folder.kind(name), Ok(EntryKind::Link)) {
            return Err(self.through_a_link());
        }
        folder
            .remove(name)
            .map_err(|_| self.unavailable("cannot be removed"))
    }

    /// Why a file could not be used, told apart: `busy`/`path_locked`,
    /// `busy`/`disk_full`, `input`/`path_denied`,
    /// `input`/`path_missing`, and `input`/`file_unavailable` for anything
    /// else. `doing` says what was being done, as in "cannot be renamed".
    fn io_failure(&self, error: &std::io::Error, doing: &str) -> Failure {
        use std::io::ErrorKind;
        let raw = error.raw_os_error();
        #[cfg(windows)]
        let (locked, full) = (
            // A sharing or a lock violation.
            matches!(raw, Some(32 | 33)),
            // The disk or the handle's disk is full.
            matches!(raw, Some(39 | 112)),
        );
        #[cfg(not(windows))]
        let (locked, full) = (
            matches!(
                error.kind(),
                ErrorKind::ResourceBusy | ErrorKind::WouldBlock | ErrorKind::ExecutableFileBusy
            ),
            matches!(
                error.kind(),
                ErrorKind::StorageFull | ErrorKind::QuotaExceeded
            ),
        );
        let _ = raw;
        if locked {
            codes::busy(
                codes::PATH_LOCKED,
                format!("The file `{self}` {doing}: another program holds it."),
                "Close the program that holds the file, then call again.",
            )
        } else if full {
            codes::busy(
                codes::DISK_FULL,
                format!("The file `{self}` {doing}: the disk is full."),
                "Free space on the disk, then call again.",
            )
        } else if matches!(
            error.kind(),
            ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem
        ) {
            codes::input(
                codes::PATH_DENIED,
                format!("The file `{self}` {doing}: the engine's user may not write it."),
                "Let the engine's user write the file and its folder, or name another file.",
            )
        } else if error.kind() == ErrorKind::NotFound {
            codes::input(
                codes::PATH_MISSING,
                format!("The file `{self}` {doing}: it, or its folder, is not there."),
                "Check that the file and its folder exist.",
            )
        } else {
            self.unavailable(doing)
        }
    }

    /// Begins a verified save of the file: a new, empty temporary file in
    /// the same folder, under a name of 128 random bits, made only where
    /// nothing stands, never through a link, and handed back open for
    /// reading and writing. The caller writes, flushes, reads back and
    /// checks, and then puts it in place with [`Temporary::replace`], each a
    /// call of its own; dropped before that, it is removed.
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared` for a path checked for reading;
    /// `input`/`path_not_allowed`, also where a link stands at the name;
    /// and the failures of [`CheckedPath::may_write`]'s kinds: locked, full,
    /// denied, missing.
    pub fn create_temporary(&self) -> Result<Temporary, Failure> {
        self.writable()?;
        let (folder, name) = self.place()?;
        let folder = self.folder_now(folder)?.ok_or_else(|| {
            codes::input(
                codes::PATH_MISSING,
                format!("The file `{self}` cannot be saved: its folder is not there."),
                "Check that the file's folder exists.",
            )
        })?;
        if matches!(folder.kind(name), Ok(EntryKind::Link)) {
            return Err(self.through_a_link());
        }
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|_| {
            self.unavailable(
                "cannot be saved: the operating system gave no random name for its new file",
            )
        })?;
        let mut temporary = String::from(".a2m-new-");
        for byte in random {
            temporary.push_str(&format!("{byte:02x}"));
        }
        let temporary = std::ffi::OsString::from(temporary);
        let file = folder
            .create_new(&temporary)
            .map_err(|error| self.io_failure(&error, "cannot be saved"))?;
        // Its own identity, from its handle, before anything else can stand
        // at its name: it is removed only while its name still holds it.
        let own = place::handle_identity(&file)
            .ok()
            .map(|(device, file)| FileIdentity { device, file });
        Ok(Temporary {
            target: self.clone(),
            folder,
            name: temporary,
            file: Some(file),
            own,
        })
    }

    /// Writes the list of names of the folder that holds the file to the
    /// disk, so that a file just put in place keeps its name through a power
    /// cut. On Windows the file system journals the names itself, and this
    /// does nothing more.
    ///
    /// # Errors
    ///
    /// `input`/`path_not_allowed`; the kinds of [`CheckedPath::io_failure`].
    pub fn flush_folder(&self) -> Result<(), Failure> {
        let (folder, _) = self.place()?;
        let folder = self.folder_now(folder)?.ok_or_else(|| {
            codes::input(
                codes::PATH_MISSING,
                format!("The folder of `{self}` is not there."),
                "Check that the file's folder exists.",
            )
        })?;
        folder
            .sync()
            .map_err(|error| self.io_failure(&error, "cannot have its folder flushed"))
    }

    /// The file's identity, the same for as long as the file is the same
    /// file whatever is written in it, and different for a file put in its
    /// place: the device and the file number on Linux and macOS, the volume
    /// and the file identifier on Windows. A link at the name is the link's
    /// own identity, and is not followed.
    ///
    /// # Errors
    ///
    /// `input`/`path_missing`, `input`/`path_denied`,
    /// `input`/`path_not_allowed`, `input`/`file_unavailable`.
    pub fn identity(&self) -> Result<FileIdentity, Failure> {
        let (folder, name) = self.place()?;
        let folder = self.folder_now(folder)?.ok_or_else(|| {
            codes::input(
                codes::PATH_MISSING,
                format!("The file `{self}` is not there."),
                "Check that the file exists.",
            )
        })?;
        let (device, file) = folder
            .identity(name)
            .map_err(|error| self.io_failure(&error, "cannot be looked at"))?;
        Ok(FileIdentity { device, file })
    }

    /// Whether the engine's user may write the file now. Nothing is written,
    /// made, or cut short: the file is opened for writing and closed. `false`
    /// when the user may not, or the file system is read only.
    ///
    /// # Errors
    ///
    /// `input`/`path_missing`; `busy`/`path_locked` where another program
    /// holds it; `input`/`path_not_allowed`, also where a link stands at the
    /// name or the path is no regular file; `policy`/`effect_not_declared`
    /// for a path checked for reading.
    pub fn may_write(&self) -> Result<bool, Failure> {
        self.writable()?;
        let (folder, name) = self.place()?;
        let folder = self.folder_now(folder)?.ok_or_else(|| {
            codes::input(
                codes::PATH_MISSING,
                format!("The file `{self}` is not there."),
                "Check that the file exists.",
            )
        })?;
        match folder.kind(name) {
            Ok(EntryKind::File) => {}
            Ok(EntryKind::Absent) => {
                return Err(codes::input(
                    codes::PATH_MISSING,
                    format!("The file `{self}` is not there."),
                    "Check that the file exists.",
                ));
            }
            Ok(EntryKind::Link) => return Err(self.through_a_link()),
            Ok(_) => return Err(refuse(&self.relative, "it is not a regular file")),
            Err(error) => return Err(self.io_failure(&error, "cannot be looked at")),
        }
        match folder.open_for_write(name) {
            Ok(file) => {
                drop(file);
                Ok(true)
            }
            Err(error) => match self.io_failure(&error, "cannot be written") {
                denied if denied.code() == &codes::PATH_DENIED => Ok(false),
                other => Err(other),
            },
        }
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

/// What makes a file the same file: the same for as long as it is that file,
/// whatever is written in it, and different for a file put in its place.
/// Compared, never shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileIdentity {
    device: u64,
    file: u128,
}

/// A temporary file of a verified save, open for reading and writing, in
/// the folder of the file it will replace. Made by
/// [`CheckedPath::create_temporary`]. Each step of the save is a call of its
/// own, so the caller checks what it wrote before anything is replaced:
/// [`Temporary::write`], [`Temporary::flush`], [`Temporary::read_back`],
/// then [`Temporary::replace`], then [`CheckedPath::flush_folder`]. Dropped
/// before it replaced anything, it is removed.
pub struct Temporary {
    target: CheckedPath,
    folder: place::Folder,
    name: std::ffi::OsString,
    file: Option<fs::File>,
    /// The identity of the file it made, read from its handle.
    own: Option<FileIdentity>,
}

impl FileIdentity {
    /// The identity of an open file, from its handle: comparable with
    /// [`CheckedPath::identity`] of the name it was opened by, to tell
    /// whether that name still holds this file.
    ///
    /// # Errors
    ///
    /// `input`/`file_unavailable` where the system gives none.
    pub fn of_open(file: &fs::File) -> Result<Self, Failure> {
        place::handle_identity(file)
            .map(|(device, file)| Self { device, file })
            .map_err(|_| {
                codes::input(
                    codes::FILE_UNAVAILABLE,
                    "An open file's identity could not be read.",
                    "Try again; if it persists, check the file system.",
                )
            })
    }
}

impl fmt::Debug for Temporary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Temporary")
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

impl Temporary {
    /// The file the save will replace.
    #[must_use]
    pub fn target(&self) -> &CheckedPath {
        &self.target
    }

    /// The temporary file's identity, read from its own handle when it was
    /// made: comparable with [`CheckedPath::identity`].
    ///
    /// # Errors
    ///
    /// `input`/`file_unavailable` where the system gave none.
    pub fn identity(&self) -> Result<FileIdentity, Failure> {
        self.own.ok_or_else(|| {
            self.target
                .unavailable("has a temporary file whose identity is not known")
        })
    }

    /// Gives the temporary file the owner, the group, and the permission
    /// bits of the target, which is looked at without following a link,
    /// through the temporary file's own handle, and then checks that it has
    /// them. Callable before the first write and again after the last.
    /// `Ok(false)` where any of the three cannot be kept: the target is not
    /// there or is no regular file, it carries a set-user, set-group, or
    /// sticky bit, or the system refuses the owner or the group to the
    /// engine's user. On Windows only the read-only flag is kept, and the
    /// answer is always `false`: the owner and the access list are the
    /// security descriptor the folder passes down.
    ///
    /// # Errors
    ///
    /// `input`/`file_unavailable` and the other kinds of a failed look or
    /// change.
    pub fn keep_owner_and_mode(&mut self) -> Result<bool, Failure> {
        let Some(name) = self.target.named.file_name().map(std::ffi::OsStr::to_owned) else {
            return Err(self.target.unavailable("is a root and not a file"));
        };
        let target = self.target.clone();
        let file = self
            .file
            .as_ref()
            .ok_or_else(|| target.unavailable("cannot be saved: its temporary file was closed"))?;
        let kept = self.folder.keep_owner_and_mode(&name, file);
        kept.map_err(|error| target.io_failure(&error, "cannot have its owner and mode kept"))
    }

    /// Whether the temporary file's name still holds the file it made.
    fn still_its_own(&self) -> bool {
        self.own.is_some_and(|own| {
            self.folder
                .identity(&self.name)
                .is_ok_and(|(device, file)| own == FileIdentity { device, file })
        })
    }

    fn open(&mut self) -> Result<&mut fs::File, Failure> {
        let target = &self.target;
        self.file
            .as_mut()
            .ok_or_else(|| target.unavailable("cannot be saved: its temporary file was closed"))
    }

    /// Appends bytes to the temporary file.
    ///
    /// # Errors
    ///
    /// `busy`/`disk_full`, `input`/`path_denied`, and the other
    /// kinds of a failed write.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), Failure> {
        use std::io::Write as _;
        let target = self.target.clone();
        self.open()?
            .write_all(bytes)
            .map_err(|error| target.io_failure(&error, "cannot be saved"))
    }

    /// Writes what the temporary file holds to the disk.
    ///
    /// # Errors
    ///
    /// As [`Temporary::write`].
    pub fn flush(&mut self) -> Result<(), Failure> {
        let target = self.target.clone();
        self.open()?
            .sync_all()
            .map_err(|error| target.io_failure(&error, "cannot be flushed"))
    }

    /// What the temporary file holds, from its start, when it has at most
    /// `max_bytes` bytes: for the caller's check before it replaces the
    /// target.
    ///
    /// # Errors
    ///
    /// `input`/`file_unavailable`, also when it is larger.
    pub fn read_back(&mut self, max_bytes: u64) -> Result<Vec<u8>, Failure> {
        use std::io::{Read as _, Seek as _, SeekFrom};
        let target = self.target.clone();
        let file = self.open()?;
        file.seek(SeekFrom::Start(0))
            .map_err(|error| target.io_failure(&error, "cannot be read back"))?;
        let mut bytes = Vec::new();
        file.take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| target.io_failure(&error, "cannot be read back"))?;
        file.seek(SeekFrom::End(0))
            .map_err(|error| target.io_failure(&error, "cannot be read back"))?;
        if bytes.len() as u64 > max_bytes {
            return Err(target.unavailable("is larger than the limit for reading it back"));
        }
        Ok(bytes)
    }

    /// Gives the temporary file the target's name. What stands at the name
    /// is replaced, and nothing else: a link at the name is refused, and not
    /// followed. Then flush the folder with [`CheckedPath::flush_folder`].
    ///
    /// Only the file this made is put in place. Just before the rename the
    /// temporary file's name must still hold it, by its identity: a link or
    /// another file put at that name since is refused, and neither moved
    /// nor removed. Just after, the target's name must hold it: a swap in
    /// the few calls between the look and the rename is found there, and
    /// answered as a failure, never as a replace (decision 0193).
    ///
    /// # Errors
    ///
    /// `input`/`path_not_allowed` where a link stands at the name;
    /// `input`/`file_unavailable` where the temporary file's name no longer
    /// holds the file this made, or the target's name does not hold it after
    /// the rename; `busy`/`path_locked` where another program holds the
    /// target; and the other kinds of a failed rename. The temporary file is
    /// removed while its name still holds it.
    pub fn replace(mut self) -> Result<(), Failure> {
        drop(self.file.take());
        let Some(name) = self.target.named.file_name().map(std::ffi::OsStr::to_owned) else {
            return Err(self.target.unavailable("is a root and not a file"));
        };
        if matches!(self.folder.kind(&name), Ok(EntryKind::Link)) {
            return Err(self.target.through_a_link());
        }
        let Some(own) = self.own.filter(|_| self.still_its_own()) else {
            return Err(self.target.unavailable(
                "cannot be replaced: its temporary file's name no longer holds the file that was written",
            ));
        };
        self.folder
            .rename(&self.name, &name)
            .map_err(|error| self.target.io_failure(&error, "cannot be replaced"))?;
        // The name now belongs to the target: nothing is left to remove.
        self.name.clear();
        let placed = self
            .folder
            .identity(&name)
            .is_ok_and(|(device, file)| own == FileIdentity { device, file });
        if !placed {
            return Err(self.target.unavailable(
                "was replaced, and its name does not hold the file that was written",
            ));
        }
        Ok(())
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        drop(self.file.take());
        // Nothing is left behind when a save does not finish; and nothing
        // that stands at the name in its place is removed: only while the
        // name holds the file this made.
        if !self.name.is_empty() && self.still_its_own() {
            let _ = self.folder.remove(&self.name);
        }
    }
}

/// Every file a run may touch, each already checked, by the name the plan
/// gives it, and the project folder of the request that started the run. A
/// driver resolves none itself.
#[derive(Debug, Clone, Default)]
pub struct CheckedPaths {
    paths: BTreeMap<String, CheckedPath>,
    project: Option<CheckedPath>,
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

    /// The same, with the project folder of the request that started the
    /// run: the folder itself, checked as a root. A file of the project is a
    /// [`CheckedPath::join`] below it.
    #[must_use]
    pub fn with_project(mut self, project: CheckedPath) -> Self {
        self.project = Some(project);
        self
    }

    /// The project folder of the request that started the run, when it
    /// had one.
    #[must_use]
    pub fn project(&self) -> Option<&CheckedPath> {
        self.project.as_ref()
    }

    /// The project folder, for a pack that resolves a file against the
    /// caller's project.
    ///
    /// # Errors
    ///
    /// `input`/`project_folder_missing` when the run has none: never the
    /// folder the engine was started in.
    pub fn project_folder(&self) -> Result<&CheckedPath, Failure> {
        self.project.as_ref().ok_or_else(|| {
            codes::input(
                codes::PROJECT_FOLDER_MISSING,
                "The run names a file of the project, and the call that started it came with no project folder.",
                "Call the tool from inside the project, through a door that states its folder.",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test: a helper that cannot do its work stops the test"
    )]

    use std::ffi::OsStr;
    use std::path::Path;

    use super::{Access, RootName, Roots, names_a_device, plain, plain_text, resolved_path};
    use crate::codes;

    #[test]
    fn a_verbatim_path_is_given_back_in_its_ordinary_form_where_that_names_the_same() {
        for (verbatim, ordinary) in [
            (r"\\?\C:\Users\pat\work", r"C:\Users\pat\work"),
            (r"\\?\d:\", r"d:\"),
            (r"\\?\C:\Users\pat\.drafts\a.b", r"C:\Users\pat\.drafts\a.b"),
            (r"\\?\C:\Users\Zoë Ann\work", r"C:\Users\Zoë Ann\work"),
            (r"\\?\C:\console\nullish.txt", r"C:\console\nullish.txt"),
            (r"\\?\UNC\server\share\work", r"\\server\share\work"),
            (r"\\?\UNC\server\share", r"\\server\share\"),
        ] {
            assert_eq!(
                plain_text(verbatim).as_deref(),
                Some(ordinary),
                "{verbatim}"
            );
        }
        // Not in either form: left as it is.
        for other in [
            r"C:\Users\pat",
            "/home/pat",
            r"\\server\share\x",
            r"\\?\Volume{0000}\x",
            r"\\?\GLOBALROOT\Device\x",
            r"\\?\C:",
            r"\\?\CC:\x",
            r"\\?\UNC\server",
            r"\\?\UNC\\share\x",
            r"\\.\C:\x",
        ] {
            assert_eq!(plain_text(other), None, "{other}");
        }
        // In the form, but the ordinary form would name something else.
        for other in [
            r"\\?\C:\a\..\b",
            r"\\?\C:\a\.\b",
            r"\\?\C:\a\b.",
            r"\\?\C:\a\b ",
            r"\\?\C:\a\\b",
            r"\\?\C:\a\b/c",
            r"\\?\C:\a\CON",
            r"\\?\C:\a\nul.txt",
            r"\\?\C:\a\Com1 .log",
            "\\\\?\\C:\\a\\lpt\u{b9}",
            r"\\?\UNC\server\share\aux",
        ] {
            assert_eq!(plain_text(other), None, "{other}");
        }
        // On every system but Windows a path is never changed.
        if !cfg!(windows) {
            let odd = Path::new(r"\\?\C:\x");
            assert_eq!(plain(odd), odd);
        }
    }

    #[test]
    fn a_device_name_is_known_wherever_it_stands_and_whatever_follows_it() {
        for device in [
            "con",
            "NUL",
            "aux.txt",
            "Prn.tar.gz",
            "com0",
            "COM9.log",
            "lpt1",
            "com\u{b9}",
            "lpt\u{b3}.x",
            "conin$",
            "CONOUT$",
            "nul .txt",
            "com1  .log",
        ] {
            assert!(names_a_device(device), "{device}");
        }
        for name in [
            "console",
            "nullish.txt",
            "com",
            "com10",
            "lpt",
            "lpt12",
            "comx",
            "auxiliary",
            "a.con",
            "com\u{b4}",
        ] {
            assert!(!names_a_device(name), "{name}");
        }
    }

    /// The one property every comparison with a root rests on: a path
    /// resolved by [`resolved_path`], given in any form the system takes,
    /// lies under the folder of a root added at the same folder, given in
    /// any other form.
    #[test]
    fn a_resolved_path_lies_under_the_folder_of_its_root_whatever_form_each_was_given_in() {
        let folder =
            std::env::temp_dir().join(format!("a2m-provider-forms-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("certs")).unwrap();
        std::fs::write(folder.join("certs").join("a.pem"), b"x").unwrap();
        // The system's own resolved form: on Windows, the verbatim one.
        let system = std::fs::canonicalize(&folder).unwrap();
        let resolved = resolved_path(&folder).unwrap();
        assert_eq!(resolved_path(&system).unwrap(), resolved);
        if cfg!(windows) {
            assert!(
                !resolved.to_string_lossy().starts_with(r"\\?\"),
                "{resolved:?}"
            );
        } else {
            assert_eq!(resolved, system);
        }
        let name = RootName::new("here").unwrap();
        for given in [&folder, &system, &resolved] {
            let mut roots = Roots::none();
            roots.add(name.clone(), given, Access::Read).unwrap();
            let root = roots.folder(&name, Access::Read).unwrap();
            assert_eq!(root.as_path(), resolved);
            for file in [
                folder.join("certs").join("a.pem"),
                system.join("certs").join("a.pem"),
            ] {
                let file = resolved_path(&file).unwrap();
                let inside = file.strip_prefix(root.as_path()).unwrap();
                assert_eq!(inside, Path::new("certs").join("a.pem"));
                let checked = roots.check(&name, "certs/a.pem", Access::Read).unwrap();
                assert_eq!(checked.as_path(), file);
            }
        }
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Makes a link to a file. `None` where this machine does not let the
    /// test make one, as Windows does without the privilege.
    fn link_file(target: &std::path::Path, link: &std::path::Path) -> Option<()> {
        #[cfg(unix)]
        return std::os::unix::fs::symlink(target, link).ok();
        #[cfg(windows)]
        return std::os::windows::fs::symlink_file(target, link).ok();
    }

    /// Decision 0193, finding P-1: what stands at the temporary file's name
    /// in place of the file the save wrote is never put at the target's.
    #[test]
    fn a_file_or_link_swapped_in_at_the_temporary_name_is_never_put_in_place() {
        let folder =
            std::env::temp_dir().join(format!("a2m-provider-swapped-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let project = folder.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let outside = folder.join("outside.txt");
        std::fs::write(&outside, b"secret").unwrap();
        let mut roots = Roots::none();
        let name = RootName::new("project").unwrap();
        roots.add(name.clone(), &project, Access::Write).unwrap();
        let report = roots.check(&name, "report.csv", Access::Write).unwrap();
        std::fs::write(project.join("report.csv"), b"old").unwrap();

        // On Windows the temporary file is shared with nobody while its
        // handle is held, so it cannot be removed to put another in its
        // place; the swap below is for the systems where it can.
        let swappable = |at: &std::path::Path| {
            let removed = std::fs::remove_file(at).is_ok();
            assert_eq!(removed, cfg!(unix), "removing the temporary file");
            removed
        };

        // Another file at the name.
        let mut temporary = report.create_temporary().unwrap();
        temporary.write(b"checked").unwrap();
        temporary.flush().unwrap();
        assert_eq!(temporary.read_back(64).unwrap(), b"checked");
        let at = project.join(&temporary.name);
        if swappable(&at) {
            std::fs::write(&at, b"someone else's").unwrap();
            let refused = temporary.replace().unwrap_err();
            assert_eq!(refused.code(), &codes::FILE_UNAVAILABLE);
            assert_eq!(std::fs::read(project.join("report.csv")).unwrap(), b"old");
            assert_eq!(std::fs::read(&at).unwrap(), b"someone else's");
            std::fs::remove_file(&at).unwrap();
        } else {
            drop(temporary);
        }

        // A link at the name, to a file outside the root.
        let mut temporary = report.create_temporary().unwrap();
        temporary.write(b"checked").unwrap();
        let at = project.join(&temporary.name);
        if swappable(&at) && link_file(&outside, &at).is_some() {
            let refused = temporary.replace().unwrap_err();
            assert_eq!(refused.code(), &codes::FILE_UNAVAILABLE);
            assert_eq!(std::fs::read(project.join("report.csv")).unwrap(), b"old");
            assert!(
                !std::fs::symlink_metadata(project.join("report.csv"))
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert_eq!(std::fs::read(&outside).unwrap(), b"secret");
            std::fs::remove_file(&at).unwrap();
        } else {
            drop(temporary);
        }

        // With nothing swapped the same save puts its file in place.
        let mut temporary = report.create_temporary().unwrap();
        temporary.write(b"new").unwrap();
        temporary.flush().unwrap();
        temporary.replace().unwrap();
        assert_eq!(std::fs::read(project.join("report.csv")).unwrap(), b"new");
        let names: Vec<_> = std::fs::read_dir(&project)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from("report.csv")]);
        std::fs::remove_dir_all(&folder).unwrap();
    }

    /// Decision 0193, finding P-2: while the save writes it, the temporary
    /// file is the engine's alone. On Windows no other open of it, for
    /// reading, writing, or removing, succeeds while its handle is held; on
    /// Linux and macOS only the engine's user may read or write it.
    #[test]
    fn nobody_else_opens_the_temporary_file_while_it_is_written() {
        let folder =
            std::env::temp_dir().join(format!("a2m-provider-alone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let mut roots = Roots::none();
        let name = RootName::new("project").unwrap();
        roots.add(name.clone(), &folder, Access::Write).unwrap();
        let report = roots.check(&name, "report.csv", Access::Write).unwrap();
        let mut temporary = report.create_temporary().unwrap();
        temporary.write(b"not yet checked").unwrap();
        let at = folder.join(&temporary.name);
        #[cfg(windows)]
        {
            // ERROR_SHARING_VIOLATION.
            const SHARING_VIOLATION: i32 = 32;
            for opened in [
                std::fs::File::open(&at),
                std::fs::OpenOptions::new().write(true).open(&at),
            ] {
                assert_eq!(opened.unwrap_err().raw_os_error(), Some(SHARING_VIOLATION));
            }
            assert_eq!(
                std::fs::remove_file(&at).unwrap_err().raw_os_error(),
                Some(SHARING_VIOLATION)
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&at).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        drop(temporary);
        assert!(!at.exists());
        std::fs::remove_dir_all(&folder).unwrap();
    }

    /// Decision 0193, finding P-2: on Windows a caller can tell a target
    /// whose access list a replace would keep from one with a list of its
    /// own, which a replace would drop.
    #[cfg(windows)]
    #[test]
    fn a_target_with_an_access_list_of_its_own_is_told_apart_on_windows() {
        let folder =
            std::env::temp_dir().join(format!("a2m-provider-access-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let mut roots = Roots::none();
        let name = RootName::new("project").unwrap();
        roots.add(name.clone(), &folder, Access::Write).unwrap();
        let report = roots.check(&name, "report.csv", Access::Write).unwrap();
        std::fs::write(folder.join("report.csv"), b"old").unwrap();

        // Made in the same folder by the same user: the same owner and the
        // access list the folder passes down, which a replace keeps.
        let mut temporary = report.create_temporary().unwrap();
        assert!(temporary.keep_owner_and_mode().unwrap());
        drop(temporary);

        // An access list of its own, with nothing passed down.
        let user = std::env::var("USERNAME").unwrap();
        let status = std::process::Command::new("icacls")
            .arg(folder.join("report.csv"))
            .args(["/inheritance:r", "/grant:r", &format!("{user}:F")])
            .output()
            .unwrap()
            .status;
        assert!(status.success());
        let mut temporary = report.create_temporary().unwrap();
        assert!(!temporary.keep_owner_and_mode().unwrap());
        drop(temporary);
        std::fs::remove_dir_all(&folder).unwrap();
    }

    /// The name of a write's temporary file cannot be guessed. This test
    /// stands for the one who guessed it all the same: the name is chosen
    /// here, and a link is planted at it. The file is made new or not at
    /// all, so the write is refused, the link is not followed, and it is
    /// neither moved nor removed.
    #[test]
    fn a_link_planted_at_the_very_name_of_the_temporary_file_is_not_followed() {
        let folder =
            std::env::temp_dir().join(format!("a2m-provider-guessed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let project = folder.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let outside = folder.join("outside.txt");
        std::fs::write(&outside, b"secret").unwrap();
        let guessed = OsStr::new(".a2m-new-guessed");
        let mut roots = Roots::none();
        let name = RootName::new("project").unwrap();
        roots.add(name.clone(), &project, Access::Write).unwrap();
        let report = roots.check(&name, "report.csv", Access::Write).unwrap();

        // A file at the name is left as it is.
        std::fs::write(project.join(guessed), b"someone else's").unwrap();
        assert_eq!(
            report.write_through(b"new", guessed).unwrap_err().code(),
            &codes::FILE_UNAVAILABLE
        );
        assert_eq!(
            std::fs::read(project.join(guessed)).unwrap(),
            b"someone else's"
        );
        assert!(!project.join("report.csv").exists());
        std::fs::remove_file(project.join(guessed)).unwrap();

        // A link at the name, to a file outside the root and to nothing.
        let nowhere = folder.join("not-there.txt");
        for target in [&outside, &nowhere] {
            if link_file(target, &project.join(guessed)).is_none() {
                // Without the right to make a link there is nothing to plant.
                break;
            }
            assert_eq!(
                report.write_through(b"new", guessed).unwrap_err().code(),
                &codes::FILE_UNAVAILABLE
            );
            assert_eq!(std::fs::read(&outside).unwrap(), b"secret");
            assert!(!nowhere.exists(), "the write made the file a link names");
            assert!(!project.join("report.csv").exists());
            assert!(
                std::fs::symlink_metadata(project.join(guessed))
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            std::fs::remove_file(project.join(guessed)).unwrap();
        }

        // With nothing in the way the same call writes the file.
        report.write_through(b"new", guessed).unwrap();
        assert_eq!(std::fs::read(project.join("report.csv")).unwrap(), b"new");
        assert!(!project.join(guessed).exists());
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
