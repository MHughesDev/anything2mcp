//! What `init` writes: a project folder's first files.
//!
//! Section 220 gives `init` no action: a project file is what makes a
//! folder a project, so no action can write the first one through the
//! checked project folder, and the client end that runs `init` runs as the
//! person in a folder of theirs. So the shortcut is the client's, and this
//! module is the one place that says what it writes (decision 0164): the
//! project file, the guidance file for agents (the text of
//! `guides/guidance.md`, the same as `docs.get guidance` serves), the
//! `targets` folder, and a `.gitignore` that keeps the engine's working
//! files out of the project's history. A file already there is kept as it
//! is, and nothing is written through a link.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::Path;

use a2m_core::Failure;

use crate::codes;

/// The project file `init` writes: a valid, empty set of project settings.
pub const PROJECT_FILE_TEXT: &str = "# The settings of this project. An empty file is a complete configuration;\n# run `anything2mcp config show` for every key and where it was set.\n";

/// The guidance file for agents that `init` writes into the project.
pub const GUIDANCE_TEXT: &str = include_str!("../guides/guidance.md");

/// What `init` writes into `.gitignore`.
pub const GITIGNORE_TEXT: &str =
    "# The engine's working files: snapshots, captures, traces, diffs.\n.anything2mcp/\n";

/// What `init` did with each file, by its path inside the project folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitReport {
    /// The files and the folder it made.
    pub written: Vec<&'static str>,
    /// The files that were there already and were kept as they are.
    pub kept: Vec<&'static str>,
}

fn refused(why: &str) -> Failure {
    codes::input(
        codes::PATH_NOT_ALLOWED,
        format!("No project was made here: {why}."),
        "Run `init` in a folder of your own, by its full path.",
    )
}

/// The guidance file's text, without the first line that names it as a
/// guide.
fn guidance() -> &'static str {
    GUIDANCE_TEXT
        .split_once('\n')
        .map_or(GUIDANCE_TEXT, |(first, rest)| {
            if first.starts_with("name:") {
                rest.trim_start_matches('\n')
            } else {
                GUIDANCE_TEXT
            }
        })
}

/// Makes `folder` a project: writes each of its first files that is not
/// there yet, new, and never through a link. Looks at the disk and waits
/// on it.
///
/// # Errors
///
/// `input`/`path_not_allowed` for a path that is not a full path to a
/// folder that exists; `input`/`file_unavailable` for a file that could
/// not be written. What was written before a failure stays.
pub fn init_project(folder: &Path) -> Result<InitReport, Failure> {
    if !folder.is_absolute() {
        return Err(refused("the path is not a full path"));
    }
    match std::fs::symlink_metadata(folder) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(refused("the path is not a folder, or it is a link")),
        Err(_) => return Err(refused("the folder does not exist")),
    }
    let mut report = InitReport {
        written: Vec::new(),
        kept: Vec::new(),
    };
    let files: [(&'static str, &str); 3] = [
        ("anything2mcp.toml", PROJECT_FILE_TEXT),
        ("AGENTS.md", guidance()),
        (".gitignore", GITIGNORE_TEXT),
    ];
    for (name, text) in files {
        // Made new, or not at all: a file or a link already at the name is
        // left as it is.
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(folder.join(name))
        {
            Ok(mut file) => {
                file.write_all(text.as_bytes())
                    .and_then(|()| file.flush())
                    .map_err(|_| unwritten(name))?;
                report.written.push(name);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                report.kept.push(name);
            }
            Err(_) => return Err(unwritten(name)),
        }
    }
    let targets = folder.join("targets");
    match std::fs::create_dir(&targets) {
        Ok(()) => report.written.push("targets"),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            report.kept.push("targets");
        }
        Err(_) => return Err(unwritten("targets")),
    }
    Ok(report)
}

fn unwritten(name: &str) -> Failure {
    codes::input(
        a2m_provider::codes::FILE_UNAVAILABLE,
        format!("`{name}` could not be written in the project folder."),
        "Check that you may write the folder, then run `init` again; what is there is kept.",
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "a test")]
mod tests {
    use super::*;

    #[test]
    fn init_writes_a_project_that_loads_and_keeps_what_is_there() {
        let folder = std::env::temp_dir().join(format!("a2m-init-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("AGENTS.md"), "mine\n").unwrap();
        let report = init_project(&folder).unwrap();
        assert_eq!(
            report.written,
            ["anything2mcp.toml", ".gitignore", "targets"]
        );
        assert_eq!(report.kept, ["AGENTS.md"]);
        // What was there is kept as it was.
        assert_eq!(
            std::fs::read_to_string(folder.join("AGENTS.md")).unwrap(),
            "mine\n"
        );
        // The project file loads as a settings file by itself.
        let text = std::fs::read_to_string(folder.join("anything2mcp.toml")).unwrap();
        assert!(toml::from_str::<toml::Table>(&text).unwrap().is_empty());
        assert!(folder.join("targets").is_dir());
        // Again: everything is kept.
        let again = init_project(&folder).unwrap();
        assert!(again.written.is_empty());
        // The guidance file is the guide without its name line, and short.
        assert!(guidance().starts_with("# Building tools"));
        assert!(guidance().lines().count() < 150);
        assert!(init_project(Path::new("relative")).is_err());
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
