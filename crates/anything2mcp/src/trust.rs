//! What the product's HTTP clients trust (decisions 0157 and 0166).
//!
//! When an engine opens, before any pack is registered and before any HTTP
//! client is made, each file named in the installation's setting
//! `network.trust_roots` is read through the folders `files.allowed_roots`
//! allows, by the checked paths of `a2m-provider`, and handed to
//! `a2m_auth::tls::install`. No tool input and no target file reaches the
//! setting. With no file named, nothing is installed and every client
//! trusts the operating system's roots.

use std::path::{Component, Path};
use std::sync::OnceLock;

use a2m_actions::Settings;
use a2m_auth::tls;
use a2m_core::{Digest, Failure, FailureClass};
use a2m_provider::{Access, RootName, Roots};

/// The setting that names further trust roots.
const TRUST_ROOTS: &str = "network.trust_roots";
/// The setting that bounds which folders the engine reads from.
const ALLOWED_ROOTS: &str = "files.allowed_roots";
/// The word in `files.allowed_roots` for the project folder.
const PROJECT_WORD: &str = "<project>";
/// The most one file of trust roots may hold.
const FILE_LIMIT: u64 = 1024 * 1024;

/// What this process installed, as a digest of the files' bytes: a second
/// engine opened in the same process with the same roots installs nothing
/// more, and one with other roots is refused.
static INSTALLED: OnceLock<Digest> = OnceLock::new();

fn unreadable(index: usize, why: &str) -> Failure {
    // The path is the person's own setting and is not repeated; its place
    // in the list is.
    Failure::new(
        FailureClass::Input,
        tls::TRUST_ROOTS_UNREADABLE,
        format!(
            "File {} of the setting `{TRUST_ROOTS}` cannot be used: {why}.",
            index + 1
        ),
        format!(
            "Name a PEM file inside a folder that `{ALLOWED_ROOTS}` allows, by its full path, in the installation's settings file."
        ),
    )
}

/// The folders `files.allowed_roots` allows, as checked roots: each full
/// path, and the project folder where the word `<project>` stands.
fn allowed(settings: &Settings, project: Option<&Path>) -> Vec<(RootName, Roots)> {
    let mut found = Vec::new();
    for (index, entry) in settings.list(ALLOWED_ROOTS).iter().enumerate() {
        let folder = if entry == PROJECT_WORD {
            match project {
                Some(project) => project.to_path_buf(),
                None => continue,
            }
        } else {
            Path::new(entry).to_path_buf()
        };
        let Ok(name) = RootName::new(&format!("allowed_{index}")) else {
            continue;
        };
        let mut roots = Roots::none();
        // A folder that does not exist allows nothing.
        if roots.add(name.clone(), &folder, Access::Read).is_ok() {
            found.push((name, roots));
        }
    }
    found
}

/// The bytes of the file at `path`, read through the first allowed folder
/// it lies inside.
fn read(path: &Path, allowed: &[(RootName, Roots)], index: usize) -> Result<Vec<u8>, Failure> {
    if !path.is_absolute() {
        return Err(unreadable(index, "its path is not a full path"));
    }
    let real = std::fs::canonicalize(path).map_err(|_| unreadable(index, "it cannot be read"))?;
    for (name, roots) in allowed {
        let Ok(folder) = roots.folder(name, Access::Read) else {
            continue;
        };
        let Ok(inside) = real.strip_prefix(folder.as_path()) else {
            continue;
        };
        let mut parts = Vec::new();
        for part in inside.components() {
            match part {
                Component::Normal(part) => match part.to_str() {
                    Some(part) => parts.push(part),
                    None => return Err(unreadable(index, "its path is not text")),
                },
                _ => return Err(unreadable(index, "its path cannot be checked")),
            }
        }
        let checked = roots
            .check(name, &parts.join("/"), Access::Read)
            .map_err(|_| unreadable(index, "its path is not allowed"))?;
        return checked
            .read(FILE_LIMIT)
            .map_err(|_| unreadable(index, "it cannot be read, or is larger than 1 MiB"));
    }
    Err(unreadable(
        index,
        &format!("it is not inside a folder that `{ALLOWED_ROOTS}` allows"),
    ))
}

/// Reads the trust roots the settings name and installs them for every
/// HTTP client of this process. Called when an engine opens, before
/// anything makes a client.
///
/// # Errors
///
/// `input`/`trust_roots_unreadable` for a file that cannot be read through
/// the allowed folders or holds no certificate; `input`/`tls_not_enabled`
/// for roots named in a build without TLS; `input`/`tls_setup_failed` when
/// this process installed other roots already, or a client was made
/// before.
pub(crate) fn install(settings: &Settings, project: Option<&Path>) -> Result<(), Failure> {
    let named = settings.list(TRUST_ROOTS);
    if named.is_empty() && INSTALLED.get().is_none() {
        return Ok(());
    }
    let allowed = allowed(settings, project);
    let mut files = Vec::with_capacity(named.len());
    for (index, path) in named.iter().enumerate() {
        files.push(read(Path::new(path), &allowed, index)?);
    }
    let mut all = Vec::new();
    for file in &files {
        all.extend_from_slice(&(file.len() as u64).to_be_bytes());
        all.extend_from_slice(file);
    }
    let digest = Digest::of(&all);
    if let Some(installed) = INSTALLED.get() {
        return if *installed == digest {
            Ok(())
        } else {
            Err(Failure::new(
                FailureClass::Input,
                tls::TLS_SETUP_FAILED,
                format!(
                    "This process trusts other roots already than `{TRUST_ROOTS}` now names."
                ),
                "Start the engine again, so that it reads the setting once.",
            ))
        };
    }
    let pem: Vec<&[u8]> = files.iter().map(Vec::as_slice).collect();
    tls::install(tls::Trust::with_named_roots(&pem)?)?;
    let _ = INSTALLED.set(digest);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use a2m_actions::Settings;

    use super::{allowed, read};

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("anything2mcp-trust-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(std::fs::canonicalize(&path).unwrap())
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_trust_root_is_read_only_through_a_folder_the_settings_allow() {
        let project = Folder::new("project");
        let outside = Folder::new("outside");
        std::fs::create_dir_all(project.0.join("certs")).unwrap();
        let inside = project.0.join("certs").join("authority.pem");
        std::fs::write(&inside, "made-up certificate text").unwrap();
        let elsewhere = outside.0.join("authority.pem");
        std::fs::write(&elsewhere, "made-up certificate text").unwrap();

        // The default is `<project>`: the engine's project folder.
        let settings = Settings::defaults();
        let roots = allowed(&settings, Some(&project.0));
        assert_eq!(read(&inside, &roots, 0).unwrap(), b"made-up certificate text");

        let refused = read(&elsewhere, &roots, 1).unwrap_err();
        assert_eq!(refused.code().as_str(), "trust_roots_unreadable");
        assert!(refused.message().starts_with("File 2 of"), "{refused:?}");
        assert!(refused.message().contains("files.allowed_roots"), "{refused:?}");
        // The path itself is not repeated.
        assert!(!refused.message().contains("outside"), "{refused:?}");

        let relative = read(Path::new("certs/authority.pem"), &roots, 0).unwrap_err();
        assert!(relative.message().contains("not a full path"), "{relative:?}");

        // With no project, `<project>` allows nothing.
        let none = allowed(&settings, None);
        assert!(read(&inside, &none, 0).is_err());

        // A link inside the project that leads out of it is not followed.
        #[cfg(unix)]
        {
            let link = project.0.join("certs").join("linked.pem");
            std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
            assert!(read(&link, &roots, 0).is_err());
        }
    }
}
