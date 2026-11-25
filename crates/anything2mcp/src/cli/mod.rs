//! The command line, shared by the `anything2mcp` command and its short
//! name `a2m`.
//!
//! The command tree is built from the action registry each time the
//! program starts: `anything2mcp <noun> <verb> [arguments]` for every
//! action with a command face, and the shortcuts, which are data. The
//! process commands are written by hand: `serve` is the local engine and
//! `bridge` the stdio proxy of an MCP client; `runner` is named and not
//! built.
//!
//! A command is a thin client of the local engine: it sends each action
//! over the local channel and renders the answer. When none runs it starts
//! one on demand, detached, and then talks to it (decision 0165). With
//! `engine.in_process` on, or where the start is refused, it runs the
//! engine inside its own process instead: it opens the kernel, dispatches,
//! and closes. `--in-process` asks for that form by name, and is refused
//! while an engine runs.

mod client;
mod complete;
mod parse;
mod process;
mod render;
mod start;

use std::collections::BTreeMap;
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use a2m_actions::{Env, ExitClass, FrontDoor, OsFamily, PROGRAM, Paths, PersonSurface, codes};
use a2m_core::{Failure, FailureClass};
use a2m_local::{ClientLabel, Kept, kept_by};
use a2m_logs::SystemClock;
use a2m_vault::KeyStore;

use crate::engine::{Engine, EngineOptions, action_registry};
use crate::person::{ProcessTerminal, TerminalSurface};
pub use parse::COMPLETION_COMMAND;
pub(crate) use parse::Tree;
use parse::{Globals, Parsed, Step};
pub use process::READY_LINE;
use serde_json::Value as Json;

/// The name of the file that marks a project folder.
pub const PROJECT_FILE: &str = "anything2mcp.toml";

/// What a command line runs in: the process's surroundings, as data, so
/// that a test gives its own.
pub struct Host {
    /// The folder the command was run in.
    pub cwd: PathBuf,
    /// The process environment.
    pub environment: BTreeMap<String, String>,
    /// The session manager's folder of this user, on Linux, when it is the
    /// user's and closed: where the runtime folder is when
    /// `XDG_RUNTIME_DIR` is not set (decision 0432). Read from the machine
    /// by [`Host::of_process`]; a test gives a stand-in, or `None`.
    pub session_folder: Option<PathBuf>,
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
    /// The program a client starts, as `serve`, when it finds no engine:
    /// this program's own file. `None` starts nothing, and a command runs
    /// the engine inside itself: a host that embeds the command line, and a
    /// test, are no program to start (decision 0165).
    pub program: Option<PathBuf>,
    /// The presence prompt `serve` asks a person's presence with. `None` is
    /// this machine's own, which the operating system gives. The shipped
    /// program never sets it; only the copy test harness does (decision 0310).
    pub prompt: Option<Arc<dyn a2m_local::PresencePrompt>>,
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
    /// The environment the layout is resolved from.
    pub(crate) fn env(&self) -> Env {
        Env {
            os: OsFamily::current(),
            vars: self.environment.clone(),
            session_folder: self.session_folder.clone(),
        }
    }

    /// The surroundings of this process.
    #[must_use]
    pub fn of_process() -> Self {
        let env = Env::of_process();
        Self {
            cwd: std::env::current_dir().unwrap_or_default(),
            environment: env.vars,
            session_folder: env.session_folder,
            stdout_is_terminal: std::io::stdout().is_terminal(),
            person_at_terminal: std::io::stdin().is_terminal() && std::io::stderr().is_terminal(),
            person: None,
            key_store: None,
            program: std::env::current_exe().ok(),
            prompt: None,
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
    Ok(Tree::of(&action_registry()?))
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

/// A project file, or a folder a project is looked for in, that others may
/// change: it is not taken as this person's project.
fn not_yours(path: &Path, why: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        codes::PATH_NOT_ALLOWED,
        format!(
            "`{}` is not taken as a project: {why}. A project's file decides which settings, target files, and folders a command uses, so it is taken only from a folder that only you, or the administrator, can change.",
            shown_path(path)
        ),
        "Make the folder and its project file yours and closed to others, or run the command in a folder of your own.",
    )
}

fn shown_path(path: &Path) -> String {
    render::visible(&path.display().to_string())
}

/// Whether a folder, or a project file, is this person's own.
fn kept(path: &Path) -> Kept {
    // A path that cannot be read is nobody's to trust.
    kept_by(path).unwrap_or(Kept::Open("it cannot be read"))
}

/// The nearest folder, from `from` upward, that holds the project file.
///
/// The walk looks only in folders that only this person, or the
/// administrator, can change, and stops at the first that others can:
/// a shared folder such as the system's temporary folder is never looked
/// in, nor anything above it. A project file found in a folder of the
/// person's own must be closed to others as well, or it is refused by name.
/// Where this build cannot tell who may change a file (Windows), the walk
/// looks in the working folder, and above it only inside `home`.
fn find_project(from: &Path, home: Option<&Path>) -> Result<Option<PathBuf>, Failure> {
    for folder in from.ancestors() {
        match kept(folder) {
            Kept::Private => {}
            Kept::NotChecked => {
                let inside_home = home.is_some_and(|home| folder.starts_with(home));
                if folder != from && !inside_home {
                    return Ok(None);
                }
            }
            Kept::Open(_) => return Ok(None),
        }
        let file = folder.join(PROJECT_FILE);
        if std::fs::symlink_metadata(&file).is_err() {
            continue;
        }
        return match kept(&file) {
            Kept::Private | Kept::NotChecked if file.is_file() => Ok(Some(folder.to_path_buf())),
            Kept::Open(why) => Err(not_yours(&file, why)),
            _ => Err(not_yours(&file, "it is not a file")),
        };
    }
    Ok(None)
}

/// The project a command names with `--project`: the folder and its file
/// must be the person's own, as for one found by walking up.
fn named_project(folder: PathBuf) -> Result<Option<PathBuf>, Failure> {
    if let Kept::Open(why) = kept(&folder) {
        return Err(not_yours(&folder, why));
    }
    let file = folder.join(PROJECT_FILE);
    if std::fs::symlink_metadata(&file).is_ok()
        && let Kept::Open(why) = kept(&file)
    {
        return Err(not_yours(&file, why));
    }
    Ok(Some(folder))
}

fn project_of(globals: &Globals, host: &Host) -> Result<Option<PathBuf>, Failure> {
    match &globals.project {
        Some(given) if given.is_absolute() => named_project(given.clone()),
        Some(given) => named_project(host.cwd.join(given)),
        None => {
            let home = ["USERPROFILE", "HOME"]
                .iter()
                .find_map(|name| host.environment.get(*name))
                .filter(|home| !home.is_empty())
                .map(PathBuf::from);
            find_project(&host.cwd, home.as_deref())
        }
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

fn stdin_unreadable() -> Failure {
    // Nothing of what was read is in the message, and nothing of the
    // system's own error, which could quote it.
    Failure::new(
        FailureClass::Input,
        codes::SCHEMA_INVALID,
        "Standard input could not be read as text.",
        "Give the value on standard input as UTF-8 text, such as `printf %s \"$VALUE\" | anything2mcp ...`.",
    )
}

/// Standard input to its end, as text, without one line ending at its
/// end. What is read is returned and written nowhere.
fn read_stdin() -> Result<String, Failure> {
    let mut input = std::io::stdin().lock();
    if input.is_terminal() {
        // A person types the value. The terminal shows what is typed;
        // this program repeats none of it.
        let mut error = std::io::stderr().lock();
        let _ = writeln!(
            error,
            "{PROGRAM}: type the value, then end the input (Ctrl-D; on Windows, Ctrl-Z and Enter). The terminal shows what you type."
        );
    }
    let mut text = String::new();
    input
        .read_to_string(&mut text)
        .map_err(|_| stdin_unreadable())?;
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    Ok(text)
}

/// Fills, for each step whose command asked for it, the field that
/// standard input fills: read to its end, off the async threads. The flag
/// that asked for it is not part of the input.
async fn take_stdin(steps: &mut [Step]) -> Result<(), Failure> {
    for step in steps {
        let Some(field) = step.stdin_for.take() else {
            continue;
        };
        let text = tokio::task::spawn_blocking(read_stdin)
            .await
            .map_err(|_| stdin_unreadable())??;
        if let Json::Object(input) = &mut step.input {
            input.insert(field, Json::String(text));
        }
    }
    Ok(())
}

/// Runs one command line on an engine that is already open, as a person at
/// a terminal or as an agent. For a host that keeps one engine for several
/// commands, and for tests.
pub async fn run_on(engine: &Engine, args: &[String], json: bool, is_agent: bool) -> Finished {
    // The tree is made once for the engine, whose registry is fixed while
    // it is open.
    let tree = engine.command_tree();
    match tree.parse(args) {
        Ok(Parsed::Invoke(globals, mut steps)) => {
            let json = json || globals.json;
            if let Err(failure) = take_stdin(&mut steps).await {
                return stopped(tree, "", &failure, ExitClass::Malformed, json);
            }
            run_steps(engine, tree, &globals, steps, json, is_agent).await
        }
        Ok(other) => without_engine(tree, other, json),
        Err(failure) => stopped(tree, "", &failure, ExitClass::Malformed, json),
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
        // The program answers it itself, engine or not.
        Parsed::Process(globals, COMPLETION_COMMAND, rest) => {
            complete::run(tree, &rest, json || globals.json)
        }
        // Reached only on an engine that is open already; a process
        // command of the program itself is run by `run_with`.
        Parsed::Process(globals, word, _) => stopped(
            tree,
            word,
            &Failure::new(
                FailureClass::Input,
                codes::ACTION_NOT_BUILT,
                format!(
                    "`{PROGRAM} {word}` starts a process of its own, and an engine is open here already."
                ),
                format!("Run `{PROGRAM} {word}` as a command."),
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
/// action or a shortcut sends it to the local engine when one runs, or
/// else opens the engine in this process, dispatches, and closes. A
/// process command starts its role.
pub async fn run_with(args: &[String], host: &Host) -> Finished {
    let json = !host.stdout_is_terminal;
    let tree = match tree() {
        Ok(tree) => tree.with_width(parse::width_of(&host.environment)),
        Err(failure) => {
            return stopped(&Tree::empty(), "", &failure, ExitClass::Failed, json);
        }
    };
    let (globals, mut steps) = match tree.parse(args) {
        Ok(Parsed::Invoke(globals, steps)) => (globals, steps),
        Ok(Parsed::Process(globals, word, rest)) => {
            return process::run(&tree, &globals, word, &rest, host, json || globals.json).await;
        }
        Ok(Parsed::EmptyShortcut(globals, INIT_WORD)) => {
            return init(&tree, &globals, host, json || globals.json);
        }
        Ok(other) => return without_engine(&tree, other, json),
        Err(failure) => return stopped(&tree, "", &failure, ExitClass::Malformed, json),
    };
    let json = json || globals.json;
    let action = steps
        .first()
        .map(|step| step.action.clone())
        .unwrap_or_default();
    if let Err(failure) = take_stdin(&mut steps).await {
        return stopped(&tree, &action, &failure, ExitClass::Malformed, json);
    }
    let paths = match Paths::resolve(&host.env()) {
        Ok(paths) => paths,
        Err(failure) => return stopped(&tree, &action, &failure, ExitClass::Failed, json),
    };
    // The engine's log is read where the engine runs, by the client end:
    // it starts no engine, opens no kernel, and makes nothing.
    if steps.len() == 1 && steps[0].action == LOGS_ACTION {
        return engine_logs(&tree, &paths, &steps[0].input, json);
    }
    let project = match project_of(&globals, host) {
        Ok(project) => project,
        Err(failure) => return stopped(&tree, &action, &failure, ExitClass::Malformed, json),
    };
    if steps.len() == 1 && steps[0].action == START_ACTION && !globals.in_process {
        return start_engine(&tree, &paths, host, project.as_deref(), json).await;
    }
    // No engine is started for `--in-process`, which asks for none, nor for
    // the engine's status or its stop, which mean the engine that runs.
    let wants_engine = !globals.in_process && !NO_START.contains(&action.as_str());
    let launch = || {
        wants_engine
            .then(|| start::plan(host, &paths, project.as_deref(), false).ok())
            .flatten()
    };
    let (lock, refused) = match client::find(&paths, ClientLabel::CommandLine, launch).await {
        Ok(client::Found::Engine(_)) if globals.in_process => {
            return stopped(
                &tree,
                &action,
                &Failure::new(
                    FailureClass::Busy,
                    a2m_local::codes::ENGINE_RUNNING,
                    "An engine already runs for this user and data directory, so the command cannot run one inside itself.",
                    "Run the command without `--in-process`: it then uses the running engine.",
                ),
                ExitClass::Failed,
                json,
            );
        }
        Ok(client::Found::Engine(link)) => {
            let warning =
                crate::engine::settings_warning(link.start_variables_differing(
                    &crate::engine::settings_variables(&host.environment),
                ));
            let asking = client::Asking {
                globals: &globals,
                project: project.as_deref(),
                paths: &paths,
            };
            return client::run_steps(&link, &tree, &asking, steps, json, warning.as_deref()).await;
        }
        // No engine runs and none was started: `engine.in_process` is on,
        // no passphrase was given for a vault that needs one, or the
        // machine refused the start. The command runs the engine inside
        // its own process, holding the engine lock it was handed until the
        // kernel has closed.
        Ok(client::Found::Nothing(lock, refused)) => (lock, refused),
        Err(failure) => return client::not_reached(&tree, &action, &failure, json),
    };
    let clock = Arc::new(SystemClock::new());
    let person: Arc<dyn PersonSurface> = match &host.person {
        Some(person) => Arc::clone(person),
        None => Arc::new(TerminalSurface::new(
            Arc::new(ProcessTerminal),
            Arc::clone(&clock) as Arc<dyn a2m_logs::Clock>,
        )),
    };
    let mut options = EngineOptions::of_machine(paths, host.environment.clone(), person);
    options.clock = clock;
    options.project = project;
    options.key_store.clone_from(&host.key_store);
    let engine = match Engine::open_holding(options, lock).await {
        Ok(engine) => engine,
        Err(failure) => return stopped(&tree, &action, &failure, ExitClass::Failed, json),
    };
    // In one process the caller is this process: whether a person is at
    // its terminal is what the process can see of itself. It is no proof
    // of a person (a program can give itself a terminal), and it is never
    // carried over the local channel; a person's act here is asked for at
    // the terminal and recorded as proving nobody (decision 0158).
    let mut finished = run_steps(
        &engine,
        &tree,
        &globals,
        steps,
        json,
        !host.person_at_terminal,
    )
    .await;
    engine.close().await;
    if let Some(refused) = refused {
        finished.stderr.insert_str(
            0,
            &render::visible(&format!(
                "{PROGRAM}: no engine could be started on demand, so this command ran the engine inside itself: {}\n",
                refused.message()
            )),
        );
    }
    finished
}

/// The shortcut that makes a project folder. It has no action beneath it:
/// a project file is what makes a folder a project, so the client, which
/// runs as the person in a folder of theirs, writes the first files
/// (decision 0164).
const INIT_WORD: &str = "init";

/// `init`: makes the folder the command was run in, or the one `--project`
/// names, a project, with `a2m_actions::init::init_project`. A folder that
/// others can change is not made one.
fn init(tree: &Tree, globals: &Globals, host: &Host, json: bool) -> Finished {
    let folder = match &globals.project {
        Some(given) if given.is_absolute() => given.clone(),
        Some(given) => host.cwd.join(given),
        None => host.cwd.clone(),
    };
    if let Kept::Open(why) = kept(&folder) {
        return stopped(
            tree,
            INIT_WORD,
            &not_yours(&folder, why),
            ExitClass::Malformed,
            json,
        );
    }
    match a2m_actions::init::init_project(&folder) {
        Ok(report) => {
            let response = a2m_actions::Response {
                ok: true,
                action: INIT_WORD.to_owned(),
                result: Some(serde_json::json!({
                    "folder": folder.display().to_string(),
                    "written": report.written,
                    "kept": report.kept,
                })),
                error: None,
                warnings: Vec::new(),
                next: None,
                exit: ExitClass::Succeeded,
                decision: None,
            };
            render::render(&response, tree, json)
        }
        Err(failure) => {
            let exit = if failure.class() == FailureClass::Input {
                ExitClass::Malformed
            } else {
                ExitClass::Failed
            };
            stopped(tree, INIT_WORD, &failure, exit, json)
        }
    }
}

/// The action that starts the local engine, carried out here, in the
/// client end.
const START_ACTION: &str = "engine.start";

/// The action that reads the engine's recent log lines, carried out here,
/// in the client end.
const LOGS_ACTION: &str = "engine.logs";

/// How many lines `engine logs` gives when it is not told, and the most it
/// gives.
const LOG_LINES: u64 = 50;
const LOG_LINES_MOST: u64 = 1_000;

/// `engine logs`: the engine's recent log lines, from the log files of this
/// machine's data directory, oldest first. Each line is the object the
/// engine wrote, or its text when it is not one. Whether an engine runs
/// does not matter, and nothing is made where nothing is.
fn engine_logs(tree: &Tree, paths: &Paths, input: &Json, json: bool) -> Finished {
    let asked = match input.get("lines") {
        None => Some(LOG_LINES),
        Some(value) => value.as_u64().filter(|n| (1..=LOG_LINES_MOST).contains(n)),
    };
    let Some(count) = asked.and_then(|count| usize::try_from(count).ok()) else {
        return stopped(
            tree,
            LOGS_ACTION,
            &Failure::new(
                FailureClass::Input,
                codes::SCHEMA_INVALID,
                format!("`--lines` takes a whole number from 1 to {LOG_LINES_MOST}."),
                format!("Write `--lines {LOG_LINES}`, or leave it out for {LOG_LINES}."),
            ),
            ExitClass::Malformed,
            json,
        );
    };
    let file = paths.logs.join(a2m_logs::LOG_FILE_NAME);
    match a2m_logs::recent_lines(&paths.logs, count) {
        Ok(read) => {
            let lines: Vec<Json> = read
                .into_iter()
                .map(|line| match serde_json::from_str::<Json>(&line) {
                    Ok(object @ Json::Object(_)) => object,
                    _ => Json::String(line),
                })
                .collect();
            let response = a2m_actions::Response {
                ok: true,
                action: LOGS_ACTION.to_owned(),
                result: Some(serde_json::json!({
                    "file": file.display().to_string(),
                    // A look at the file's entry, which makes nothing.
                    "exists": std::fs::symlink_metadata(&file).is_ok(),
                    "lines": lines,
                })),
                error: None,
                warnings: Vec::new(),
                next: None,
                exit: ExitClass::Succeeded,
                decision: None,
            };
            render::render(&response, tree, json)
        }
        Err(failure) => stopped(tree, LOGS_ACTION, &failure, ExitClass::Failed, json),
    }
}

/// The actions for which no engine is started on demand: they ask about,
/// or stop, the engine that runs.
const NO_START: [&str; 2] = ["engine.status", "engine.stop"];

/// `engine start`: starts the local engine, detached, when none answers,
/// whatever `engine.in_process` says, and says whether it started one.
async fn start_engine(
    tree: &Tree,
    paths: &Paths,
    host: &Host,
    project: Option<&Path>,
    json: bool,
) -> Finished {
    let not_reached = |failure: &Failure| client::not_reached(tree, START_ACTION, failure, json);
    let started = async {
        let connector = a2m_local::Connector::new(
            paths,
            crate::local::program_version()?,
            ClientLabel::CommandLine,
        )?;
        match connector.connect().await {
            Ok(link) => return Ok((link, false)),
            Err(failure) if *failure.code() == a2m_local::codes::ENGINE_UNREACHABLE => {}
            Err(failure) => return Err(failure),
        }
        let launch = start::plan(host, paths, project, true)?;
        client::start(&connector, paths, launch).await
    };
    match started.await {
        Ok((link, started)) => {
            let response = a2m_actions::Response {
                ok: true,
                action: START_ACTION.to_owned(),
                result: Some(serde_json::json!({
                    "started": started,
                    "engine_version": link.engine_version().to_string(),
                })),
                error: None,
                warnings: Vec::new(),
                next: None,
                exit: ExitClass::Succeeded,
                decision: None,
            };
            render::render(&response, tree, json)
        }
        Err(failure) => not_reached(&failure),
    }
}

/// Takes the vault's passphrase out of this process's environment, once
/// the host has copied it, so that nothing this process starts can inherit
/// it, whatever a target lists to pass on.
///
/// # Safety
///
/// No other thread may run, or read or change the environment, while it
/// runs: the caller is `main`, before anything else.
#[allow(
    unsafe_code,
    reason = "the environment is changed once, at the start of `main`, while the process has one thread"
)]
unsafe fn forget_passphrase() {
    // SAFETY: the caller holds that no other thread exists yet (see the
    // function's contract); with one thread nothing reads the environment
    // while it changes.
    unsafe { std::env::remove_var(crate::PASSPHRASE_VARIABLE) };
}

/// What the program writes when a thread panics: where, never the
/// message. A panic's message may hold a value (a dependency's assertion
/// prints what it compared), and a started engine's standard error is its
/// output file, which no redactor reads (PR-1 of the third audit, decision
/// 0194).
fn panic_report(location: Option<&std::panic::Location<'_>>) -> String {
    let at = location.map_or_else(
        || "an unknown place".to_owned(),
        |at| format!("{}:{}", at.file(), at.line()),
    );
    format!(
        "{PROGRAM}: an internal fault stopped a part of the program at {at}. Its message is not shown, because it may hold a value. Report this with the place.\n"
    )
}

/// Replaces the standard panic report with [`panic_report`], for the whole
/// process.
fn report_panics_by_place_alone() {
    std::panic::set_hook(Box::new(|info| {
        let _ = std::io::stderr().write_all(panic_report(info.location()).as_bytes());
    }));
}

/// Runs the command line with the arguments that follow the program name,
/// in this process's surroundings.
///
/// The output names the product, never the name the program was started
/// under, so both commands behave identically.
///
/// It copies the process environment into the host first, and then takes
/// the vault's passphrase out of the process's own environment.
///
/// # Safety
///
/// It is the program's `main`, and is called before any other thread
/// starts: it changes the process environment, which is sound only while
/// the process has one thread.
#[allow(
    unsafe_code,
    reason = "the program's main changes the environment before any thread starts"
)]
pub unsafe fn run(args: impl IntoIterator<Item = String>) -> ExitCode {
    // SAFETY: the caller holds this function's contract, which is the
    // same.
    unsafe { run_in(args, Host::of_process()) }
}

/// [`run`], in the surroundings `host` gives: a program that sets what the
/// shipped one never does, such as the copy test harness's presence prompt,
/// builds its own host from [`Host::of_process`] and starts here.
///
/// # Safety
///
/// As [`run`]: called before any other thread starts.
#[allow(
    unsafe_code,
    reason = "it changes the environment before any thread starts, as `run` does"
)]
pub unsafe fn run_in(args: impl IntoIterator<Item = String>, host: Host) -> ExitCode {
    report_panics_by_place_alone();
    let args: Vec<String> = args.into_iter().collect();
    // SAFETY: the caller holds this function's contract, which is the
    // same: no other thread runs yet.
    unsafe { forget_passphrase() };
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

#[cfg(test)]
mod tests {
    /// A panic is reported by its place alone: the message, which may
    /// hold a value, is never written where the output file would keep it.
    #[test]
    fn a_panic_is_reported_by_its_place_and_never_its_message() {
        let here = std::panic::Location::caller();
        let report = super::panic_report(Some(here));
        assert!(
            report.contains(&format!("{}:{}", here.file(), here.line())),
            "{report}"
        );
        assert!(report.starts_with("anything2mcp: "), "{report}");
        assert!(super::panic_report(None).contains("an unknown place"));
    }

    #[cfg(unix)]
    #[test]
    fn a_project_file_that_others_can_change_is_never_taken_as_a_project() {
        use std::os::unix::fs::PermissionsExt;

        use super::{PROJECT_FILE, find_project};

        let mode = |path: &std::path::Path, mode: u32| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        let base = std::env::temp_dir().join(format!("anything2mcp-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        mode(&base, 0o755);

        // A folder every account may write to, as the system's temporary
        // folder is, holds a project file another account planted; the
        // person works in a folder of their own beneath it.
        let shared = base.join("shared");
        let work = shared.join("work");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(shared.join(PROJECT_FILE), "").unwrap();
        mode(&shared, 0o777);
        mode(&work, 0o755);
        assert_eq!(find_project(&work, None).unwrap(), None);
        // In the shared folder itself nothing is taken either.
        assert_eq!(find_project(&shared, None).unwrap(), None);

        // A project file in the person's own folder that others may change
        // is refused by name; closed to others, it is the project.
        let own = base.join("own");
        std::fs::create_dir_all(&own).unwrap();
        mode(&own, 0o755);
        let file = own.join(PROJECT_FILE);
        std::fs::write(&file, "").unwrap();
        mode(&file, 0o666);
        let refused = find_project(&own, None).unwrap_err();
        assert_eq!(refused.code().as_str(), "path_not_allowed");
        assert!(
            refused.message().contains("every account may change it"),
            "{refused:?}"
        );
        mode(&file, 0o644);
        assert_eq!(find_project(&own, None).unwrap(), Some(own.clone()));
        let below = own.join("targets");
        std::fs::create_dir_all(&below).unwrap();
        mode(&below, 0o755);
        assert_eq!(find_project(&below, None).unwrap(), Some(own.clone()));

        mode(&shared, 0o755);
        let _ = std::fs::remove_dir_all(&base);
    }
}
