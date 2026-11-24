//! Provider packages found through a Python entry point (package F3).
//!
//! A provider package is a pip package that declares, in the group
//! [`ENTRY_POINT_GROUP`], an entry whose name is the provider's name and
//! whose value is the dotted name of a package of its own that holds the
//! provider's manifest. The search is open and small:
//!
//! - It looks only in the site folders it is given, which the caller names
//!   and states to the person; it never searches a path of its own.
//! - It reads text, `entry_points.txt` of each installed distribution and
//!   the manifest the entry names, and nothing else. No Python is started,
//!   no module is imported, and no entry is loaded: an entry's value is
//!   read as a folder name, never as code to run.
//! - A package that is found is listed as present and not enabled
//!   ([`RegistryBuilder::add_plugin`]); nothing starts until a person
//!   enables it, and then only under the supervisor.
//!
//! [`RegistryBuilder::add_plugin`]: crate::RegistryBuilder::add_plugin

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use a2m_core::Failure;

use crate::codes::{self, shown};
use crate::manifest::{MANIFEST_FILE_NAME, Manifest};
use crate::name::{RootName, SubsystemKind};
use crate::path::{Access, Roots};
use crate::registry::FoundManifest;
use crate::supervisor::{EnvAllowlist, LaunchSpec, ProgramRef};

/// The entry-point group a provider package declares its entry in.
pub const ENTRY_POINT_GROUP: &str = "anything2mcp.providers";

/// The file of a distribution that lists its entry points.
const ENTRY_POINTS_FILE: &str = "entry_points.txt";
/// The most bytes read of an entry-point list.
const MOST_ENTRY_POINT_BYTES: u64 = 64 * 1024;
/// The most bytes read of a manifest.
const MOST_MANIFEST_BYTES: u64 = 1024 * 1024;
/// The most dotted parts in a package name.
const MOST_PARTS: usize = 8;

/// One provider package that was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonProvider {
    /// The pip distribution that declared it, as its folder names it.
    pub distribution: String,
    /// The entry's name; it is the provider's name too.
    pub entry: String,
    /// The package the entry names, as written.
    pub package: String,
    /// The manifest's text and the folder that holds it.
    pub found: FoundManifest,
}

/// What a search found, and what it looked at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryPointSearch {
    /// The site folders it looked in, in order, so that the person can be
    /// told where it looked.
    pub searched: Vec<PathBuf>,
    /// The packages found.
    pub found: Vec<PythonProvider>,
    /// What was wrong with an entry or its manifest, each as a failure
    /// with a next step. A package with a problem is not in `found`.
    pub problems: Vec<Failure>,
}

impl EntryPointSearch {
    /// What the search did, in words, to show a person with the list: where
    /// it looked, and that it ran nothing.
    #[must_use]
    pub fn statement(&self) -> String {
        let places = if self.searched.is_empty() {
            "no folder".to_owned()
        } else {
            self.searched
                .iter()
                .map(|folder| format!("`{}`", folder.display()))
                .collect::<Vec<_>>()
                .join(", ")
        };
        format!(
            "Looked for provider packages in {places}, by reading the `{ENTRY_POINT_GROUP}` entry points of installed packages and the manifest each names. No package code was run or imported; {} found, each listed as present and not enabled.",
            self.found.len()
        )
    }
}

fn is_identifier(part: &str) -> bool {
    let mut chars = part.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && part.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The entries of the group in the text of an `entry_points.txt`.
fn group_entries(text: &str) -> Vec<(String, String)> {
    let mut in_group = false;
    let mut entries = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_group = section.trim() == ENTRY_POINT_GROUP;
        } else if in_group && let Some((name, value)) = line.split_once('=') {
            entries.push((name.trim().to_owned(), value.trim().to_owned()));
        }
    }
    entries
}

fn problem(message: String, next: &str) -> Failure {
    codes::failure(&codes::row::MANIFEST_INVALID, message, next)
}

/// Looks for provider packages in the installed distributions of each site
/// folder, reading text only. See the module's own words for what it does
/// not do.
#[must_use]
pub fn find_python_providers(site_folders: &[PathBuf]) -> EntryPointSearch {
    let mut search = EntryPointSearch {
        searched: site_folders.to_vec(),
        ..EntryPointSearch::default()
    };
    for site in site_folders {
        search_site(site, &mut search);
    }
    search
}

fn search_site(site: &Path, search: &mut EntryPointSearch) {
    let mut roots = Roots::none();
    let Ok(root) = RootName::new("site") else {
        return;
    };
    if roots.add(root.clone(), site, Access::Read).is_err() {
        return; // not a folder: nothing installed there
    }
    let Ok(listing) = fs::read_dir(site) else {
        return;
    };
    let mut distributions: Vec<String> = listing
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.ends_with(".dist-info"))
        .collect();
    distributions.sort();
    for distribution in distributions {
        let list = format!("{distribution}/{ENTRY_POINTS_FILE}");
        let Ok(list_path) = roots.check(&root, &list, Access::Read) else {
            continue;
        };
        if !list_path.exists() {
            continue;
        }
        let text = match list_path
            .read(MOST_ENTRY_POINT_BYTES)
            .map(|bytes| String::from_utf8(bytes).map_err(|_| ()))
        {
            Ok(Ok(text)) => text,
            _ => {
                search.problems.push(problem(
                    format!(
                        "The entry points of `{}` could not be read as text.",
                        shown(&distribution)
                    ),
                    "Reinstall the package, or remove it.",
                ));
                continue;
            }
        };
        for (entry, value) in group_entries(&text) {
            match one_entry(&roots, &root, &distribution, &entry, &value) {
                Ok(found) => search.found.push(found),
                Err(failure) => search.problems.push(failure),
            }
        }
    }
}

fn one_entry(
    roots: &Roots,
    root: &RootName,
    distribution: &str,
    entry: &str,
    value: &str,
) -> Result<PythonProvider, Failure> {
    let parts: Vec<&str> = value.split('.').collect();
    if parts.len() > MOST_PARTS || !parts.iter().all(|part| is_identifier(part)) {
        return Err(problem(
            format!(
                "The entry `{}` of `{}` is not the dotted name of a package. An entry is read as a folder and never loaded as code.",
                shown(entry),
                shown(distribution)
            ),
            "Make the entry's value the name of the package that holds the manifest, such as `example_sheet_provider`.",
        ));
    }
    let relative = format!("{}/{MANIFEST_FILE_NAME}", parts.join("/"));
    let path = roots.check(root, &relative, Access::Read)?;
    let bytes = path.read(MOST_MANIFEST_BYTES)?;
    let text = String::from_utf8(bytes).map_err(|_| {
        problem(
            format!("The manifest of `{}` is not text.", shown(distribution)),
            "Install a version of the package whose manifest is TOML text.",
        )
    })?;
    let manifest = Manifest::parse(&text)?;
    if manifest.provider.name.as_str() != entry {
        return Err(problem(
            format!(
                "The entry `{}` of `{}` names a manifest whose provider is `{}`.",
                shown(entry),
                shown(distribution),
                manifest.provider.name
            ),
            "Name the entry as the provider is named in its manifest.",
        ));
    }
    let folder = path
        .as_path()
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            problem(
                "The manifest has no folder.".to_owned(),
                "Install the package again.",
            )
        })?;
    Ok(PythonProvider {
        distribution: distribution.to_owned(),
        entry: entry.to_owned(),
        package: value.to_owned(),
        found: FoundManifest { text, folder },
    })
}

impl PythonProvider {
    /// What the supervisor is given to start the provider's program, once a
    /// person has enabled it: the program of the manifest's `run` section,
    /// relative to the package folder and resolved inside it, with the
    /// manifest's digest, its fixed arguments, and an environment of only
    /// the names it lists. Nothing from a caller is added. The supervisor's
    /// own checks of a program (a real program, not a script, owned by the
    /// engine's user or the system) are made when it starts.
    ///
    /// # Errors
    ///
    /// `input`/`manifest_invalid` when the manifest has no `run` section;
    /// `input`/`path_not_allowed` when the program leaves the package
    /// folder.
    pub fn launch_spec(&self) -> Result<LaunchSpec, Failure> {
        let manifest = Manifest::parse(&self.found.text)?;
        let Some(run) = &manifest.run else {
            return Err(problem(
                format!("`{}` has no `[run]` section.", manifest.provider.name),
                "Add the `[run]` section that says how the provider starts.",
            ));
        };
        let mut roots = Roots::none();
        let root = RootName::new("package")?;
        roots.add(root.clone(), &self.found.folder, Access::Read)?;
        let program = roots.check(&root, &run.program, Access::Read)?;
        Ok(LaunchSpec {
            program: ProgramRef {
                path: program.as_path().to_path_buf(),
                digest: manifest.provider.digest,
            },
            args: run.args.iter().map(OsString::from).collect(),
            env: EnvAllowlist {
                pass: run.environment.clone(),
                set: Vec::new(),
            },
            cwd: None,
            kind: SubsystemKind::new("plugin")?,
            place: None,
        })
    }
}
