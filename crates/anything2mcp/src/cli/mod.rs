//! The command line, shared by the `anything2mcp` command and its short
//! name `a2m`.
//!
//! The command tree is built from the action registry each time the
//! program starts: `anything2mcp <noun> <verb> [arguments]` for every
//! action with a command face, and the shortcuts, which are data. The
//! process commands (`serve`, `runner`, `bridge`) are named and not built.
//!
//! Every command runs the engine inside its own process: it opens the
//! kernel, dispatches, and closes. The local engine and its channel are
//! not in this build, so `--in-process` is accepted and changes nothing.

mod parse;
mod render;

use std::collections::BTreeMap;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use a2m_actions::{
    Env, ExitClass, FrontDoor, OsFamily, PROGRAM, Paths, PersonSurface, codes, command_faces,
};
use a2m_core::{Failure, FailureClass};
use a2m_telemetry::SystemClock;
use a2m_vault::KeyStore;

use crate::engine::{Engine, EngineOptions, action_registry};
use crate::person::{ProcessTerminal, TerminalSurface};
use parse::{Globals, Parsed, Step, Tree};

/// The name of the file that marks a project folder.
pub const PROJECT_FILE: &str = "anything2mcp.toml";

/// What a command line runs in: the process's surroundings, as data, so
/// that a test gives its own.
pub struct Host {
    /// The folder the command was run in.
    pub cwd: PathBuf,
    /// The process environment.
    pub environment: BTreeMap<String, String>,
    /// Whether standard output is a terminal. When it is not, the result is
    /// one JSON object.
    pub stdout_is_terminal: bool,
    /// Whether a person is at the terminal the command was started at. When
    /// not, the caller is taken to be an agent.
    pub person_at_terminal: bool,
    /// How a person is reached. `None` is the terminal the command was
    /// started at.
    pub person: Option<Arc<dyn PersonSurface>>,
    /// Where the vault's root key is kept. `None` is this machine's own.
    pub key_store: Option<Arc<dyn KeyStore>>,
}

impl std::fmt::Debug for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host")
            .field("cwd", &self.cwd)
            .field("stdout_is_terminal", &self.stdout_is_terminal)
            .field("person_at_terminal", &self.person_at_terminal)
            .finish_non_exhaustive()
    }
}

impl Host {
    /// The surroundings of this process.
    #[must_use]
    pub fn of_process() -> Self {
        let env = Env::of_process();
        Self {
            cwd: std::env::current_dir().unwrap_or_default(),
            environment: env.vars,
            stdout_is_terminal: std::io::stdout().is_terminal(),
            person_at_terminal: std::io::stdin().is_terminal() && std::io::stderr().is_terminal(),
            person: None,
            key_store: None,
        }
    }
}

/// What a command line gave: its exit code and what it wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    /// The exit code, from the output contract.
    pub code: u8,
    /// What goes to standard output.
    pub stdout: String,
    /// What goes to standard error.
    pub stderr: String,
}

/// A caller's text, cut and cleaned, for a message.
fn shown(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(80).collect()
}

/// The command tree of this build.
fn tree() -> Result<Tree, Failure> {
    Ok(Tree::new(command_faces(&action_registry()?)))
}

/// The usage line of every action command of this build, in catalog order.
/// The parity test compares it with the action registry.
///
/// # Errors
///
/// As [`action_registry`].
pub fn usage_lines() -> Result<Vec<String>, Failure> {
    Ok(tree()?
        .faces()
        .iter()
        .map(|face| face.usage.clone())
        .collect())
}

/// The nearest folder, from `from` upward, that holds the project file.
fn find_project(from: &Path) -> Option<PathBuf> {
    from.ancestors()
        .find(|folder| folder.join(PROJECT_FILE).is_file())
        .map(Path::to_path_buf)
}

fn project_of(globals: &Globals, host: &Host) -> Option<PathBuf> {
    match &globals.project {
        Some(given) if given.is_absolute() => Some(given.clone()),
        Some(given) => Some(host.cwd.join(given)),
        None => find_project(&host.cwd),
    }
}

/// Dispatches the steps of one command in turn on an open engine, and
/// renders the last response: the first that is not a success, or the
/// final one.
async fn run_steps(
    engine: &Engine,
    tree: &Tree,
    globals: &Globals,
    steps: Vec<Step>,
    json: bool,
    is_agent: bool,
) -> Finished {
    let mut last = None;
    for step in steps {
        let caller = engine.caller(FrontDoor::Command, is_agent);
        let mut request = engine.request(&step.action, step.input, caller);
        request.idempotency_key.clone_from(&globals.idempotency_key);
        let response = engine.dispatch(request).await;
        let go_on = response.exit == ExitClass::Succeeded;
        last = Some(response);
        if !go_on {
            break;
        }
    }
    match last {
        Some(response) => render::render(&response, tree, json),
        None => Finished {
            code: 0,
            stdout: String::new(),
            stderr: String::new(),
        },
    }
}

fn stopped(tree: &Tree, action: &str, failure: &Failure, exit: ExitClass, json: bool) -> Finished {
    render::render(&render::refused(action, failure, exit), tree, json)
}

/// Runs one command line on an engine that is already open, as a person at
/// a terminal or as an agent. For a host that keeps one engine for several
/// commands, and for tests.
pub async fn run_on(engine: &Engine, args: &[String], json: bool, is_agent: bool) -> Finished {
    let tree = Tree::new(command_faces(engine.kernel().actions()));
    match tree.parse(args) {
        Ok(Parsed::Invoke(globals, steps)) => {
            run_steps(
                engine,
                &tree,
                &globals,
                steps,
                json || globals.json,
                is_agent,
            )
            .await
        }
        Ok(other) => without_engine(&tree, other, json),
        Err(failure) => stopped(&tree, "", &failure, ExitClass::Malformed, json),
    }
}

/// What a command line that needs no engine gives.
fn without_engine(tree: &Tree, parsed: Parsed, json: bool) -> Finished {
    let plain = |stdout: String| Finished {
        code: 0,
        stdout,
        stderr: String::new(),
    };
    match parsed {
        Parsed::Version => plain(format!("{PROGRAM} {}\n", crate::version())),
        Parsed::Help(text) => plain(text),
        Parsed::Process(globals, word) => stopped(
            tree,
            word,
            &Failure::new(
                FailureClass::Input,
                codes::ACTION_NOT_BUILT,
                format!(
                    "`{PROGRAM} {word}` starts a long-lived role, which this build does not have."
                ),
                "Run each command by itself; it runs the engine inside its own process.",
            ),
            ExitClass::Failed,
            json || globals.json,
        ),
        Parsed::EmptyShortcut(globals, word) => stopped(
            tree,
            word,
            &Failure::new(
                FailureClass::Input,
                codes::ACTION_NOT_BUILT,
                format!("`{PROGRAM} {word}` has no action beneath it yet."),
                format!(
                    "Make an empty file named `{PROJECT_FILE}` in the project folder, then run `{PROGRAM} target add <name>`."
                ),
            ),
            ExitClass::Failed,
            json || globals.json,
        ),
        // Not reached: the caller dispatches these on an engine.
        Parsed::Invoke(..) => plain(String::new()),
    }
}

/// Runs one command line: parses it against the command tree, and for an
/// action or a shortcut opens the engine in this process, dispatches, and
/// closes.
pub async fn run_with(args: &[String], host: &Host) -> Finished {
    let json = !host.stdout_is_terminal;
    let tree = match tree() {
        Ok(tree) => tree,
        Err(failure) => {
            return stopped(
                &Tree::new(Vec::new()),
                "",
                &failure,
                ExitClass::Failed,
                json,
            );
        }
    };
    let (globals, steps) = match tree.parse(args) {
        Ok(Parsed::Invoke(globals, steps)) => (globals, steps),
        Ok(other) => return without_engine(&tree, other, json),
        Err(failure) => return stopped(&tree, "", &failure, ExitClass::Malformed, json),
    };
    let json = json || globals.json;
    let action = steps
        .first()
        .map(|step| step.action.clone())
        .unwrap_or_default();
    let env = Env {
        os: OsFamily::current(),
        vars: host.environment.clone(),
    };
    let paths = match Paths::resolve(&env) {
        Ok(paths) => paths,
        Err(failure) => return stopped(&tree, &action, &failure, ExitClass::Failed, json),
    };
    let clock = Arc::new(SystemClock::new());
    let person: Arc<dyn PersonSurface> = match &host.person {
        Some(person) => Arc::clone(person),
        None => Arc::new(TerminalSurface::new(
            Arc::new(ProcessTerminal),
            Arc::clone(&clock) as Arc<dyn a2m_telemetry::Clock>,
        )),
    };
    let mut options = EngineOptions::of_machine(paths, host.environment.clone(), person);
    options.clock = clock;
    options.project = project_of(&globals, host);
    options.key_store.clone_from(&host.key_store);
    let engine = match Engine::open(options).await {
        Ok(engine) => engine,
        Err(failure) => return stopped(&tree, &action, &failure, ExitClass::Failed, json),
    };
    let finished = run_steps(
        &engine,
        &tree,
        &globals,
        steps,
        json,
        !host.person_at_terminal,
    )
    .await;
    engine.close().await;
    finished
}

/// Runs the command line with the arguments that follow the program name,
/// in this process's surroundings.
///
/// The output names the product, never the name the program was started
/// under, so both commands behave identically.
pub fn run(args: impl IntoIterator<Item = String>) -> ExitCode {
    let args: Vec<String> = args.into_iter().collect();
    let host = Host::of_process();
    let finished = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(run_with(&args, &host)),
        Err(_) => Finished {
            code: 1,
            stdout: String::new(),
            stderr: format!("{PROGRAM}: the program could not start its runtime.\n"),
        },
    };
    // A closed pipe is not this program's failure to report.
    let _ = std::io::stdout().write_all(finished.stdout.as_bytes());
    let _ = std::io::stderr().write_all(finished.stderr.as_bytes());
    ExitCode::from(finished.code)
}
