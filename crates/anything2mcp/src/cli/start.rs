//! Whether, and how, a client starts the local engine on demand
//! (decision 0165). The starting itself, under the start lock, is
//! `Connector::connect_or_start` of `a2m-local`; this module decides
//! whether it may happen and says what the started engine is: this
//! program's `serve`, in the data directory, with a reduced environment,
//! and the vault's passphrase on its standard input when the vault takes
//! its key from one.

use std::collections::BTreeMap;
use std::path::Path;

use a2m_actions::{PROGRAM, Paths};
use a2m_core::{Failure, FailureClass, Secret};
use a2m_local::{Launch, codes as local_codes};

use super::Host;
use crate::engine::{PASSPHRASE_VARIABLE, settings_of};

/// The flag of `serve` that has it read the vault's passphrase from its
/// standard input, to its end, before it opens the vault.
pub(crate) const PASSPHRASE_FROM_STDIN: &str = "--passphrase-from-stdin";

/// The names of the environment an engine started on demand keeps: where
/// the directories are, who the user is, the system's own, the language,
/// the desktop's buses, and nothing else. A variable that holds a
/// credential is none of these, so none reaches the engine.
const KEPT_NAMES: [&str; 34] = [
    // Where the program, the directories, and the user are.
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "USERNAME",
    "TMPDIR",
    "TEMP",
    "TMP",
    "A2M_HOME",
    // The language and the time zone.
    "LANG",
    "LANGUAGE",
    "TZ",
    // The desktop: the presence prompt and the key store talk over these.
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "DBUS_SESSION_BUS_ADDRESS",
    "DBUS_SYSTEM_BUS_ADDRESS",
    // Windows: the user's folders and the system's own, which programs
    // there need to start at all.
    "USERPROFILE",
    "USERDOMAIN",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "ALLUSERSPROFILE",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "WINDIR",
    "COMSPEC",
    "PATHEXT",
    "PROGRAMFILES",
    "PROGRAMFILES(X86)",
    "PROGRAMW6432",
    "COMMONPROGRAMFILES",
    "NUMBER_OF_PROCESSORS",
];

/// The beginnings of names that are kept as well.
const KEPT_PREFIXES: [&str; 2] = ["LC_", "XDG_"];

/// Whether an engine started on demand keeps the variable `name`. Names
/// are compared without case on Windows, where the system does so. The
/// settings variables, `A2M_<GROUP>__<KEY>`, are kept, so the started
/// engine loads the settings its caller would; a setting holds a reference
/// to a secret, never the secret.
fn kept(name: &str) -> bool {
    let name = if cfg!(windows) {
        name.to_ascii_uppercase()
    } else {
        name.to_owned()
    };
    if name == PASSPHRASE_VARIABLE {
        return false;
    }
    if let Some(rest) = name.strip_prefix("A2M_")
        && rest.contains("__")
    {
        return true;
    }
    KEPT_NAMES.contains(&name.as_str())
        || KEPT_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

/// The environment an engine started on demand starts with: the caller's,
/// reduced to what [`kept`] keeps.
pub(crate) fn engine_environment(
    environment: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    environment
        .iter()
        .filter(|(name, _)| kept(name))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

/// No engine answers, and none is started: why, and what to do.
fn not_started(why: &str) -> Failure {
    Failure::new(
        FailureClass::Busy,
        local_codes::ENGINE_UNREACHABLE,
        format!("No engine answers on the local channel, and none is started on demand: {why}."),
        format!("Start the engine with `{PROGRAM} serve`, then try again."),
    )
}

/// Whether a client that finds no engine may start one, and if so how.
/// `asked` is true for `engine start`, which starts one by name whatever
/// `engine.in_process` says.
///
/// # Errors
///
/// `busy`/`engine_unreachable`, saying why none is started: the settings
/// do not load, `engine.in_process` is on, the vault takes its key from a
/// passphrase that the caller was not given, or the program cannot name
/// itself or its project.
pub(crate) fn plan(
    host: &Host,
    paths: &Paths,
    project: Option<&Path>,
    asked: bool,
) -> Result<Launch, Failure> {
    plan_for(
        host.program.as_deref(),
        &host.environment,
        paths,
        project,
        asked,
    )
}

/// The same, from the program and the environment alone: what the bridge
/// keeps for as long as it runs.
/// The flag of `serve` that says a command started the engine on demand:
/// such an engine that cannot open its vault ends at once, saying why
/// (decision 0220).
pub(crate) const STARTED_ON_DEMAND: &str = "--started-on-demand";

pub(crate) fn plan_for(
    program: Option<&Path>,
    environment: &BTreeMap<String, String>,
    paths: &Paths,
    project: Option<&Path>,
    asked: bool,
) -> Result<Launch, Failure> {
    let program = program
        .ok_or_else(|| {
            not_started("this command line runs inside another program, which is not one to start")
        })?
        .to_path_buf();
    let settings = settings_of(paths, project, environment).map_err(|failure| {
        not_started(&format!("the settings do not load ({})", failure.code()))
    })?;
    if !asked && settings.flag("engine.in_process") == Some(true) {
        return Err(not_started(
            "the setting `engine.in_process` is on, so a command runs the engine inside itself",
        ));
    }
    // The started engine is given the passphrase on its standard input,
    // which only it reads, and never in its environment or its arguments,
    // which other programs of the user can read for as long as it runs.
    // The route is the setting's, by default the build's (decision 0220);
    // without the passphrase the command stops by the vault's own name for
    // it, and starts no engine that could not open its vault.
    let route = a2m_vault::PassphraseRoute::of_backend(settings.text("vault.backend"));
    let input = if route == a2m_vault::PassphraseRoute::Enabled {
        let given = environment
            .get(PASSPHRASE_VARIABLE)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| a2m_vault::PassphraseKeyStore::not_given(PASSPHRASE_VARIABLE))?;
        Some(Secret::new(given.clone()))
    } else {
        None
    };
    let mut args = Vec::new();
    if let Some(project) = project {
        let folder = project
            .to_str()
            .ok_or_else(|| not_started("the project folder's path is not text"))?;
        args.extend(["--project".to_owned(), folder.to_owned()]);
    }
    args.push("serve".to_owned());
    args.push(STARTED_ON_DEMAND.to_owned());
    if input.is_some() {
        args.push(PASSPHRASE_FROM_STDIN.to_owned());
    }
    // The engine runs in its data directory, which outlives every project
    // folder and is the user's own; it holds no caller's folder open.
    std::fs::create_dir_all(&paths.data)
        .map_err(|_| not_started("the data directory cannot be made"))?;
    Ok(Launch {
        program,
        args,
        cwd: paths.data.clone(),
        environment: engine_environment(environment),
        log: Launch::log_of(paths),
        input,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::engine_environment;

    #[test]
    fn an_engine_started_on_demand_keeps_no_credential_of_its_caller() {
        let caller: BTreeMap<String, String> = [
            ("PATH", "/usr/bin"),
            ("HOME", "/home/example"),
            ("A2M_HOME", "/home/example/a2m"),
            ("A2M_ENGINE__IDLE_STOP_MINUTES", "5"),
            ("LC_ALL", "C"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
            ("A2M_VAULT_PASSPHRASE", "secret-for-tests-passphrase"),
            ("EXAMPLE_API_TOKEN", "example-token-1"),
            ("EXAMPLE_SECRET_KEY", "example-key-1"),
            ("HTTPS_PROXY", "http://user:secret-for-tests@proxy.example"),
            ("A2M_TEST_BROWSER", "/opt/example/browser"),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
        let kept: Vec<String> = engine_environment(&caller).into_keys().collect();
        assert_eq!(
            kept,
            [
                "A2M_ENGINE__IDLE_STOP_MINUTES",
                "A2M_HOME",
                "HOME",
                "LC_ALL",
                "PATH",
                "XDG_RUNTIME_DIR"
            ]
        );
    }
}
