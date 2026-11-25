//! `client.connect` (decision 0219): the configuration block an MCP client
//! needs to start `anything2mcp bridge` for this project.
//!
//! No client is special. The one profile this build knows is `generic`: the
//! block is printed, and no file is written. Every other profile is data,
//! a file the local user keeps in the `clients` folder of the user's
//! settings folder, `<name>.toml`:
//!
//! ```toml
//! format = "json"                 # the only format built
//! key = "servers"                 # where the entry goes, dot-separated
//! [path]
//! linux = "~/.config/example-client/mcp.json"
//! macos = "~/Library/Application Support/example-client/mcp.json"
//! windows = "~/AppData/Roaming/example-client/mcp.json"
//! ```
//!
//! A path that starts with `~/` is under the person's home folder; any
//! other must be a full path. Writing is a local person's act, never an
//! agent's: the change is shown first, and written only when `yes` is
//! given. Only the entry of this name under the key is set; every other
//! entry of the file is kept as it was. The file is written whole under
//! another name beside it and renamed over it. A file that does not read
//! as a JSON object is refused, and nothing is written.

use std::path::{Path, PathBuf};

use a2m_core::Failure;
use serde_json::{Map, Value as Json, json};

use super::build::off_thread;
use super::handler;
use crate::codes;
use crate::dispatch::{Next, Output};
use crate::paths::OsFamily;

/// The profile every build has: the block on standard output, no file.
pub(crate) const GENERIC: &str = "generic";

/// The largest client configuration file read.
const LARGEST_FILE: u64 = 1024 * 1024;

/// A client profile, as its data file states it.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientProfile {
    format: String,
    key: String,
    path: ClientProfilePaths,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientProfilePaths {
    linux: Option<String>,
    macos: Option<String>,
    windows: Option<String>,
}

fn refused(message: impl Into<String>, fix: impl Into<String>) -> Failure {
    codes::input(codes::CLIENT_CONFIG_REFUSED, message, fix)
}

/// A profile's name: a plain word, so that it names a file of the
/// profiles folder and nothing else.
fn checked_name(name: &str) -> Result<&str, Failure> {
    let plain = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    // A name Windows keeps for a device names the device, not a file, as
    // `con.toml` does (decision 0431).
    if a2m_core::is_kept_for_a_device(name) {
        return Err(codes::input(
            codes::SCHEMA_INVALID,
            format!(
                "`{name}` is not a valid client profile name; Windows keeps `{name}` for a device, so a file of that name cannot be made there, and the name is refused on every system."
            ),
            format!(
                "Give the client profile a name Windows does not keep for a device, such as `{name}_main`."
            ),
        ));
    }
    if plain {
        Ok(name)
    } else {
        Err(codes::input(
            codes::SCHEMA_INVALID,
            "A client profile's name is a plain word of lowercase letters, digits, `_`, and `-`.",
            format!(
                "Name a profile kept in the `clients` folder of the settings folder, or `{GENERIC}`."
            ),
        ))
    }
}

/// Reads the profile `name` from `folder`.
fn client_profile(folder: &Path, name: &str) -> Result<ClientProfile, Failure> {
    let file = folder.join(format!("{name}.toml"));
    let text = std::fs::read_to_string(&file).map_err(|_| {
        codes::input(
            codes::NOT_FOUND,
            format!("No client profile `{name}` is kept in `{}`.", folder.display()),
            format!(
                "Write `{name}.toml` there, with `format`, `key`, and a `[path]` for this system; or use `{GENERIC}`, which prints the block."
            ),
        )
    })?;
    let read: ClientProfile = toml::from_str(&text).map_err(|_| {
        refused(
            format!("The client profile `{}` does not read as a profile.", file.display()),
            "Give it `format = \"json\"`, `key`, and a `[path]` table with `linux`, `macos`, or `windows`.",
        )
    })?;
    if read.format != "json" {
        return Err(refused(
            format!(
                "The client profile `{name}` names the format `{}`, and only `json` is built.",
                read.format
            ),
            "Use a client whose configuration is a JSON file, or `generic`, which prints the block.",
        ));
    }
    Ok(read)
}

/// The file a profile names on this system.
fn file_of(profile: &ClientProfile, name: &str, home: Option<&Path>) -> Result<PathBuf, Failure> {
    let given = match OsFamily::current() {
        OsFamily::Linux => profile.path.linux.as_deref(),
        OsFamily::MacOs => profile.path.macos.as_deref(),
        OsFamily::Windows => profile.path.windows.as_deref(),
    }
    .ok_or_else(|| {
        refused(
            format!("The client profile `{name}` names no file for this system."),
            "Add the file's path for this system under `[path]` in the profile.",
        )
    })?;
    let path = match given.strip_prefix("~/") {
        Some(rest) => home
            .ok_or_else(|| {
                refused(
                    "The profile's path is under the home folder, and this system names none.",
                    "Give the path in full in the profile.",
                )
            })?
            .join(rest),
        None => PathBuf::from(given),
    };
    if !path.is_absolute() {
        return Err(refused(
            format!("The client profile `{name}` names a path that is not a full path."),
            "Give the path in full, or under `~/`.",
        ));
    }
    Ok(path)
}

/// The JSON object at `key`, dot-separated, inside `root`, made where it
/// is missing. `None` when something on the way is not an object.
fn object_at<'a>(root: &'a mut Map<String, Json>, key: &str) -> Option<&'a mut Map<String, Json>> {
    let mut at = root;
    for part in key.split('.').filter(|part| !part.is_empty()) {
        at = at
            .entry(part.to_owned())
            .or_insert_with(|| Json::Object(Map::new()))
            .as_object_mut()?;
    }
    Some(at)
}

/// Writes `text` to `file` whole: under another name beside it, then
/// renamed over it.
fn write_whole(file: &Path, text: &str) -> Result<(), Failure> {
    let cannot = |what: &str| {
        refused(
            format!(
                "The client's file `{}` could not be {what}.",
                file.display()
            ),
            "Check that the folder exists and is yours, then run the command again.",
        )
    };
    let folder = file.parent().ok_or_else(|| cannot("written"))?;
    std::fs::create_dir_all(folder).map_err(|_| cannot("given its folder"))?;
    let name = file
        .file_name()
        .ok_or_else(|| cannot("written"))?
        .to_string_lossy()
        .into_owned();
    let beside = folder.join(format!(".{name}.anything2mcp-new"));
    std::fs::write(&beside, text).map_err(|_| cannot("written"))?;
    std::fs::rename(&beside, file).map_err(|_| {
        let _ = std::fs::remove_file(&beside);
        cannot("renamed into place")
    })
}

handler!(
    /// `client.connect`: the configuration an MCP client needs to start the
    /// bridge for this project, printed, or set in the client's file a
    /// profile names, as a local person's act.
    client_connect,
    |kernel, call| {
        let client = checked_name(call.text("client").unwrap_or(GENERIC))?.to_owned();
        let entry_name = call.text("name").unwrap_or("anything2mcp").to_owned();
        if entry_name.is_empty() || entry_name.contains('.') {
            return Err(codes::input(
                codes::SCHEMA_INVALID,
                "The server entry's name is a word with no dot in it.",
                "Leave `name` out to use `anything2mcp`.",
            ));
        }
        let toolset = call.text("toolset").unwrap_or("use").to_owned();
        let project = call.project_folder()?.to_path_buf();
        let program = std::env::current_exe().map_err(|_| {
            refused(
                "This engine cannot name its own program, so no client can be told how to start it.",
                "Run the command from the installed `anything2mcp`.",
            )
        })?;
        let mut args = vec![
            json!("--project"),
            json!(project.display().to_string()),
            json!("bridge"),
        ];
        if toolset != "use" {
            args.extend([json!("--toolset"), json!(toolset)]);
        }
        let mut entry = json!({"command": program.display().to_string(), "args": args});
        // An engine whose folders are all under one home was given it by
        // `A2M_HOME`: the client is given it too, or the bridge it starts
        // would look for another engine. Never the passphrase.
        let paths = kernel.paths();
        if let Some(home) = paths
            .data
            .parent()
            .filter(|home| crate::paths::Paths::under(home) == *paths)
        {
            entry["env"] = json!({crate::paths::HOME_VARIABLE: home.display().to_string()});
        }
        let block = json!({ entry_name.clone(): entry.clone() });
        let print = call.flag("print") == Some(true);
        if client == GENERIC || print {
            // Printed, and no file is read or written.
            return Ok(Output::ok(json!({
                "client": client,
                "name": entry_name,
                "entry": entry,
                "block": block,
                "written": false,
            })));
        }
        // Writing a client's file changes what a client starts: a local
        // person's act, never an agent's.
        if call.caller.is_agent {
            return Err(codes::policy(
                codes::PERSON_REQUIRED,
                "Setting a client's configuration changes what that client starts, so it is a person's act, never an agent's.",
                format!(
                    "Ask the person to run `anything2mcp connect {client}` at a terminal; `--print` shows the block without writing it."
                ),
            ));
        }
        let folder = kernel.paths().config.join("clients");
        let home = kernel.inner.places.home().map(Path::to_path_buf);
        let yes = call.flag("yes") == Some(true);
        let (shown_entry, named) = (entry.clone(), entry_name.clone());
        let (file, before, change, written) = off_thread(move || {
            let profile = client_profile(&folder, &client)?;
            let file = file_of(&profile, &client, home.as_deref())?;
            let mut root = match std::fs::metadata(&file) {
                Ok(found) if found.is_file() && found.len() <= LARGEST_FILE => {
                    let text = std::fs::read_to_string(&file).map_err(|_| {
                        refused(
                            format!("The client's file `{}` cannot be read as text.", file.display()),
                            "Correct the file, or move it aside, and run the command again.",
                        )
                    })?;
                    match serde_json::from_str::<Json>(&text) {
                        Ok(Json::Object(map)) => map,
                        _ => {
                            return Err(refused(
                                format!("The client's file `{}` does not read as a JSON object, so nothing was written.", file.display()),
                                "Correct the file, or move it aside, and run the command again.",
                            ));
                        }
                    }
                }
                Ok(_) => {
                    return Err(refused(
                        format!("`{}` is not a file this command reads.", file.display()),
                        "Name a client's configuration file of at most a megabyte in the profile.",
                    ));
                }
                Err(_) => Map::new(),
            };
            let under = object_at(&mut root, &profile.key).ok_or_else(|| {
                refused(
                    format!("In `{}`, `{}` is not an object, so nothing was written.", file.display(), profile.key),
                    "Correct the profile's `key`, or the file.",
                )
            })?;
            let before = under.get(&named).cloned();
            let change = match &before {
                None => "add",
                Some(found) if *found == shown_entry => "same",
                Some(_) => "replace",
            };
            under.insert(named, shown_entry);
            let written = yes && change != "same";
            if written {
                let text = serde_json::to_string_pretty(&Json::Object(root)).unwrap_or_default();
                write_whole(&file, &format!("{text}\n"))?;
            }
            Ok((file, before, change, written))
        })
        .await?;
        let mut output = Output::ok(json!({
            "client": call.text("client").unwrap_or(GENERIC),
            "name": entry_name,
            "entry": entry,
            "block": block,
            "file": file.display().to_string(),
            "before": before,
            "change": change,
            "written": written,
        }));
        if !written && change != "same" {
            let mut input = call.input.clone();
            input.insert("yes".to_owned(), json!(true));
            output = output.then(Next::new(
                "client.connect",
                Json::Object(input),
                "to write the change shown, once the person has read it",
            ));
        }
        Ok(output)
    }
);

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test: a helper that cannot do its work stops the test"
    )]

    use super::checked_name;
    use crate::codes;

    /// Decision 0431: a profile is the file `<name>.toml`, and a name
    /// Windows keeps for a device names the device there, so it is refused
    /// on every system with the reason; a word that only begins like one is
    /// a name.
    #[test]
    fn a_profile_named_as_a_windows_device_is_refused_with_the_reason() {
        for device in ["con", "prn", "aux", "nul", "com0", "com9", "lpt1"] {
            let refused = checked_name(device).unwrap_err();
            assert_eq!(refused.code(), &codes::SCHEMA_INVALID, "{device}");
            assert!(
                refused.message().contains("Windows keeps"),
                "{device}: {refused:?}"
            );
            assert!(
                refused.next_step().contains(&format!("{device}_main")),
                "{device}: {refused:?}"
            );
        }
        for name in ["console", "nulls", "com10", "generic", "my-client", "2nd"] {
            assert_eq!(checked_name(name).unwrap(), name);
        }
        assert!(checked_name("Con").is_err());
        assert!(checked_name("../con").is_err());
    }
}
