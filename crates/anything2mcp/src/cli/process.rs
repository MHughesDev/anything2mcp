//! The process commands: `serve`, which is the local engine in the
//! foreground, and `bridge`, the stdio proxy an MCP client starts. They
//! start a role and are not actions. `runner` is named and not built.

use std::io::Write;
use std::sync::Arc;

use a2m_actions::{BuiltInToolset, ExitClass, PROGRAM, Paths, Response, codes};
use a2m_core::{Failure, FailureClass};
use a2m_local::{ClientLabel, Connector, StopReason};
use a2m_logs::SystemClock;
use a2m_mcp::stdio_proxy;
use a2m_policy::ToolsetName;
use serde_json::json;

use super::parse::{COMPLETION_COMMAND, Globals, Tree};
use super::start::{self, PASSPHRASE_FROM_STDIN, STARTED_ON_DEMAND};
use super::{Finished, Host, client, project_of, render, stopped};
use crate::engine::EngineOptions;
use crate::local::{LocalEngine, StopAsk, StopBy, program_version, stop_word};
use crate::person::{ProcessTerminal, TerminalSurface};
use crate::wire;

/// The line `serve` writes to standard error once the engine answers on
/// the local channel. A program that starts `serve` waits for it.
pub const READY_LINE: &str = "anything2mcp: the local engine is ready.";

/// The name the bridge gives itself on the local channel. The MCP client
/// behind it names itself in the protocol, which the bridge does not read.
const BRIDGE_LABEL: &str = "stdio";

fn malformed(message: String, fix: String) -> Failure {
    Failure::new(FailureClass::Input, codes::SCHEMA_INVALID, message, fix)
}

/// A failure as one line for standard error.
fn told(failure: &Failure) -> String {
    format!("{PROGRAM}: {} {}\n", failure.message(), failure.next_step())
}

/// Writes one line to standard error now, not when the command ends.
fn say(line: &str) {
    let mut error = std::io::stderr().lock();
    // A closed pipe is not this program's failure to report.
    let _ = writeln!(error, "{line}");
    let _ = error.flush();
}

fn paths_of(host: &Host) -> Result<Paths, Failure> {
    Paths::resolve(&host.env())
}

/// Runs one process command.
pub(crate) async fn run(
    tree: &Tree,
    globals: &Globals,
    word: &'static str,
    rest: &[String],
    host: &Host,
    json: bool,
) -> Finished {
    let done = match word {
        // Answered by the program itself: it starts nothing.
        COMPLETION_COMMAND => return super::complete::run(tree, rest, json),
        "serve" => serve(tree, globals, rest, host, json).await,
        // Standard output of the bridge carries protocol messages and
        // nothing else, so what stops it is told on standard error alone.
        "bridge" => {
            return bridge(globals, rest, host)
                .await
                .unwrap_or_else(|(failure, exit)| Finished {
                    code: exit.exit_code(),
                    stdout: String::new(),
                    stderr: told(&failure),
                });
        }
        _ => Err((
            Failure::new(
                FailureClass::Input,
                codes::ACTION_NOT_BUILT,
                format!(
                    "`{PROGRAM} {word}` starts a role on a server, which this build does not have."
                ),
                "Run the engine on this machine with `anything2mcp serve`.",
            ),
            ExitClass::Failed,
        )),
    };
    match done {
        Ok(finished) => finished,
        Err((failure, exit)) => stopped(tree, word, &failure, exit, json),
    }
}

/// The operating system's ways of asking this process to stop, listened
/// for from the start of `serve`, before the kernel opens: one that comes
/// meanwhile is kept, and seen at once when the engine serves. On Unix the
/// interrupt, the termination signal, and the hang-up, which is taken as a
/// termination; on Windows the console's interrupt, break, close, and
/// shutdown events. One the system will not let the process listen for is
/// left out, and the others still stop it.
struct Signals {
    #[cfg(unix)]
    unix: [(&'static str, Option<tokio::signal::unix::Signal>); 3],
    #[cfg(windows)]
    interrupt: Option<tokio::signal::windows::CtrlC>,
    #[cfg(windows)]
    broken: Option<tokio::signal::windows::CtrlBreak>,
    #[cfg(windows)]
    closed: Option<tokio::signal::windows::CtrlClose>,
    #[cfg(windows)]
    shut_down: Option<tokio::signal::windows::CtrlShutdown>,
}

impl Signals {
    /// Starts listening. Must run inside the runtime.
    fn listen() -> Self {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            Self {
                unix: [
                    ("SIGINT", signal(SignalKind::interrupt()).ok()),
                    ("SIGTERM", signal(SignalKind::terminate()).ok()),
                    ("SIGHUP", signal(SignalKind::hangup()).ok()),
                ],
            }
        }
        #[cfg(windows)]
        {
            use tokio::signal::windows;
            Self {
                interrupt: windows::ctrl_c().ok(),
                broken: windows::ctrl_break().ok(),
                closed: windows::ctrl_close().ok(),
                shut_down: windows::ctrl_shutdown().ok(),
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            Self {}
        }
    }

    /// The name of the first that comes. Never resolves when the process
    /// listens for none.
    async fn first(mut self) -> &'static str {
        #[cfg(unix)]
        {
            let [(int, a), (term, b), (hup, c)] = &mut self.unix;
            tokio::select! {
                () = came(a.as_mut().map(|s| s.recv())) => *int,
                () = came(b.as_mut().map(|s| s.recv())) => *term,
                () = came(c.as_mut().map(|s| s.recv())) => *hup,
            }
        }
        #[cfg(windows)]
        {
            tokio::select! {
                () = came(self.interrupt.as_mut().map(|s| s.recv())) => "CTRL_C_EVENT",
                () = came(self.broken.as_mut().map(|s| s.recv())) => "CTRL_BREAK_EVENT",
                () = came(self.closed.as_mut().map(|s| s.recv())) => "CTRL_CLOSE_EVENT",
                () = came(self.shut_down.as_mut().map(|s| s.recv())) => "CTRL_SHUTDOWN_EVENT",
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            std::future::pending::<&'static str>().await
        }
    }
}

/// Resolves when the signal a receiver waits on has come. Never resolves
/// when there is no receiver, or when it can deliver no more.
async fn came(receiver: Option<impl Future<Output = Option<()>>>) {
    match receiver {
        Some(receiver) => {
            if receiver.await.is_none() {
                std::future::pending::<()>().await;
            }
        }
        None => std::future::pending::<()>().await,
    }
}

/// Once the engine serves: stops it when the first signal comes, and
/// records which (decision 0266).
fn stop_when_asked(signals: Signals, ask: StopAsk) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let name = signals.first().await;
        ask.ask(StopBy::Signal(name));
    })
}

type Stopped = (Failure, ExitClass);

/// `serve`: the local engine in the foreground, until it is asked to stop
/// or has been idle for the configured time.
async fn serve(
    tree: &Tree,
    globals: &Globals,
    rest: &[String],
    host: &Host,
    json: bool,
) -> Result<Finished, Stopped> {
    let mut passphrase_on_stdin = false;
    let mut on_demand = false;
    for word in rest {
        if word == PASSPHRASE_FROM_STDIN && !passphrase_on_stdin {
            passphrase_on_stdin = true;
            continue;
        }
        // Said by the command that started this engine on demand, and by
        // nothing else; it changes only what happens when the vault does
        // not open (decision 0220).
        if word == STARTED_ON_DEMAND && !on_demand {
            on_demand = true;
            continue;
        }
        return Err((
            malformed(
                format!(
                    "`{PROGRAM} serve` takes no argument but `{PASSPHRASE_FROM_STDIN}` and `{STARTED_ON_DEMAND}`, and `{}` was given.",
                    super::shown(word)
                ),
                format!("Run `{PROGRAM} serve`. Its settings are in the settings files."),
            ),
            ExitClass::Malformed,
        ));
    }
    // Listened for before anything is opened, so that a signal sent while
    // the kernel opens stops the engine cleanly once it serves, instead of
    // ending the process with its socket left behind.
    let signals = Signals::listen();
    let failed = |failure| (failure, ExitClass::Failed);
    let paths = paths_of(host).map_err(failed)?;
    let mut environment = host.environment.clone();
    if passphrase_on_stdin {
        // A started engine is given the vault's passphrase here, on a pipe
        // only it reads, and never in its environment (decision 0165). What
        // is read stays in this process's memory, as a passphrase from the
        // environment does.
        let read = tokio::task::spawn_blocking(read_passphrase)
            .await
            .map_err(|_| failed(passphrase_unreadable()))?
            .map_err(failed)?;
        environment.insert(crate::PASSPHRASE_VARIABLE.to_owned(), read);
    }
    let clock = Arc::new(SystemClock::new());
    // The person surface of the options is replaced by the local engine's
    // own; the terminal's stands here only because the options need one.
    let prompt = host
        .prompt
        .clone()
        .unwrap_or_else(|| a2m_local::system_prompt_for(&paths));
    let mut options = EngineOptions::of_machine(
        paths,
        environment,
        Arc::new(TerminalSurface::new(
            Arc::new(ProcessTerminal),
            Arc::clone(&clock) as Arc<dyn a2m_logs::Clock>,
        )),
    );
    options.clock = clock;
    options.project =
        project_of(globals, host).map_err(|failure| (failure, ExitClass::Malformed))?;
    options.key_store.clone_from(&host.key_store);
    let local = LocalEngine::start(options, prompt).await.map_err(failed)?;
    // The kernel's logs and traces now receives the process's log lines; the
    // engine's log says that it started, and how (decision 0266).
    tracing::info!(
        pid = std::process::id(),
        version = crate::version(),
        on_demand,
        by = if on_demand { "on_demand" } else { "serve" },
        "engine.started"
    );
    // An engine a command started on demand that cannot open its vault
    // ends at once and says why, on a line the command that started it
    // reads; it does not stay up degraded for every later command to meet
    // (decision 0220). One started by hand stays, so that `doctor` can
    // reach it.
    if on_demand
        && let Some(vault) = local
            .engine()
            .kernel()
            .health()
            .reasons
            .into_iter()
            .find(|reason| reason.part == a2m_actions::Part::Vault)
    {
        say(&a2m_local::stopped_line(&vault.failure));
        // Never served, not even for the moment it takes to stop: a client
        // that were answered would learn of the stop only as a lost
        // connection, and not why.
        local.stop_unserved().await;
        return Err(failed(vault.failure));
    }
    let ask = local.stop_ask();
    let signals = stop_when_asked(signals, ask.clone());
    if let Some(problem) = local.engine().runner_problem() {
        say(&format!(
            "{PROGRAM}: the engine runs no job: {} {}",
            problem.message(),
            problem.next_step()
        ));
    }
    say(&local.presence_line().await);
    say(READY_LINE);
    let reason = local.serve().await;
    signals.abort();
    match reason {
        StopReason::Idle | StopReason::Asked | StopReason::Restart => {}
        StopReason::DataGone => {
            return Err(failed(a2m_local::codes::data_directory_gone()));
        }
        StopReason::Failed => {
            return Err(failed(Failure::new(
                FailureClass::Busy,
                a2m_local::codes::CHANNEL_UNAVAILABLE,
                "The local channel stopped serving.",
                "Start the engine again with `anything2mcp serve`.",
            )));
        }
    }
    let mut stopped = json!({"stopped": stop_word(reason)});
    // What asked, beside `asked` alone: an engine that stopped by itself
    // names nothing.
    if let Some(by) = ask.by().filter(|_| reason == StopReason::Asked) {
        stopped["by"] = json!(by.word());
        if let Some(process) = by.process() {
            stopped["process"] = json!(process);
        }
    }
    let response = Response {
        ok: true,
        action: "serve".to_owned(),
        result: Some(stopped),
        error: None,
        warnings: Vec::new(),
        next: None,
        exit: ExitClass::Succeeded,
        decision: None,
    };
    Ok(render::render(&response, tree, json))
}

/// The most a passphrase given on standard input may hold.
const PASSPHRASE_LIMIT: u64 = 4096;

fn passphrase_unreadable() -> Failure {
    // Nothing of what was read is in the message.
    Failure::new(
        FailureClass::Input,
        codes::SCHEMA_INVALID,
        format!(
            "`{PASSPHRASE_FROM_STDIN}` was given, and standard input did not hold a passphrase: text of at most {PASSPHRASE_LIMIT} bytes."
        ),
        format!(
            "Give the passphrase on standard input, or start `{PROGRAM} serve` without the flag."
        ),
    )
}

/// The vault's passphrase, read from standard input to its end, without
/// one line ending at its end.
fn read_passphrase() -> Result<String, Failure> {
    use std::io::Read;
    let mut text = String::new();
    let read = std::io::stdin()
        .lock()
        .take(PASSPHRASE_LIMIT + 1)
        .read_to_string(&mut text);
    if read.is_err() || text.len() as u64 > PASSPHRASE_LIMIT {
        return Err(passphrase_unreadable());
    }
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    if text.is_empty() {
        return Err(passphrase_unreadable());
    }
    Ok(text)
}

/// The toolset `bridge` was asked for: `use` unless `--toolset` names the
/// other built-in one.
fn toolset_of(rest: &[String]) -> Result<ToolsetName, Failure> {
    let usage = || format!("Run `{PROGRAM} bridge`, or `{PROGRAM} bridge --toolset build`.");
    let mut chosen = BuiltInToolset::Use.as_str().to_owned();
    let mut words = rest.iter();
    while let Some(word) = words.next() {
        let value = match word.split_once('=') {
            Some(("--toolset", value)) => Some(value.to_owned()),
            None if word == "--toolset" => words.next().cloned(),
            _ => {
                return Err(malformed(
                    format!(
                        "`{PROGRAM} bridge` has no argument `{}`.",
                        super::shown(word)
                    ),
                    usage(),
                ));
            }
        };
        chosen =
            value.ok_or_else(|| malformed("`--toolset` needs a value.".to_owned(), usage()))?;
    }
    let served = [BuiltInToolset::Use, BuiltInToolset::Build]
        .iter()
        .any(|toolset| toolset.as_str() == chosen);
    if !served {
        return Err(malformed(
            format!(
                "`{}` is not a toolset the local engine serves over MCP.",
                super::shown(&chosen)
            ),
            usage(),
        ));
    }
    ToolsetName::new(&chosen)
}

/// `bridge`: copies MCP messages between standard input and output and
/// the local engine's MCP endpoint, until input ends. Standard output is
/// written by the proxy alone. A request sent while no engine runs starts
/// one on demand; where none may be started, it is answered with an error
/// that names the failure, and the next one is carried as soon as an
/// engine answers.
async fn bridge(globals: &Globals, rest: &[String], host: &Host) -> Result<Finished, Stopped> {
    let toolset = toolset_of(rest).map_err(|failure| (failure, ExitClass::Malformed))?;
    let failed = |failure| (failure, ExitClass::Failed);
    let paths = paths_of(host).map_err(failed)?;
    let connector = Connector::new(
        &paths,
        program_version().map_err(failed)?,
        ClientLabel::mcp(BRIDGE_LABEL),
    )
    .map_err(failed)?;
    let folder = project_of(globals, host).map_err(|failure| (failure, ExitClass::Malformed))?;
    // When nothing answers, an engine is started on demand for the request
    // that finds none, and again after one has stopped (decision 0165).
    // Where none may be started, one look now tells a person who starts the
    // bridge by hand at once; the client learns it with its first request.
    let starts = Arc::new(Starts {
        program: host.program.clone(),
        paths: paths.clone(),
        environment: host.environment.clone(),
        project: folder.clone(),
    });
    match connector.connect().await {
        Err(failure) if *failure.code() == a2m_local::codes::ENGINE_UNREACHABLE => {
            if let Err(why) = starts.plan() {
                say(told(&why).trim_end());
            }
        }
        Err(failure) => say(told(&failure).trim_end()),
        // An engine that runs with other settings variables serves this
        // bridge with its own, and the person who starts it is told.
        Ok(link) => {
            if let Some(warning) =
                crate::engine::settings_warning(link.start_variables_differing(
                    &crate::engine::settings_variables(&host.environment),
                ))
            {
                say(&format!("{PROGRAM}: warning: {warning}"));
            }
        }
    }
    // The project folder the bridge was started in, or was given with
    // `--project`, named to the engine on every request, as a command names
    // it. The engine checks the folder before any action uses it; nothing
    // an MCP message says can name one.
    let project = folder.and_then(|folder| {
        let named = wire::encode_path(&folder)
            .and_then(|text| axum::http::HeaderValue::from_str(&text).ok());
        if named.is_none() {
            say(&format!(
                "{PROGRAM}: the project folder's path is not text, so it is not named to the engine. Actions that work on project files will say that they need one."
            ));
        }
        named
    });
    let upstream = move |mut request: a2m_mcp::HttpRequest| {
        let connector = connector.clone();
        let starts = Arc::clone(&starts);
        if let Some(project) = &project {
            request
                .headers_mut()
                .insert(wire::PROJECT_HEADER, project.clone());
        }
        async move {
            // A request is sent only once an engine answers, so nothing is
            // sent twice: a start happens before the request is written.
            if let Err(failure) = connector.connect().await {
                if *failure.code() != a2m_local::codes::ENGINE_UNREACHABLE {
                    return Err(failure);
                }
                // The settings files are read off the async threads.
                let planning = Arc::clone(&starts);
                let launch = tokio::task::spawn_blocking(move || planning.plan())
                    .await
                    .map_err(|_| {
                        Failure::new(
                            FailureClass::Busy,
                            a2m_local::codes::ENGINE_UNREACHABLE,
                            "No engine answers, and the start of one stopped before it began.",
                            "Start the engine with `anything2mcp serve`, then try again.",
                        )
                    })??;
                client::start(&connector, &starts.paths, launch).await?;
            }
            client::carry(&connector, request).await
        }
    };
    let copied = stdio_proxy(upstream, &toolset, tokio::io::stdin(), tokio::io::stdout()).await;
    Ok(Finished {
        code: u8::from(copied.is_err()),
        stdout: String::new(),
        stderr: copied
            .err()
            .map_or_else(String::new, |failure| told(&failure)),
    })
}

/// What the bridge needs to start an engine on demand.
struct Starts {
    program: Option<std::path::PathBuf>,
    paths: Paths,
    environment: std::collections::BTreeMap<String, String>,
    project: Option<std::path::PathBuf>,
}

impl Starts {
    /// How an engine is started now, or why none may be. The settings
    /// are read each time, because they may change while the bridge runs.
    fn plan(&self) -> Result<a2m_local::Launch, Failure> {
        start::plan_for(
            self.program.as_deref(),
            &self.environment,
            &self.paths,
            self.project.as_deref(),
            false,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::toolset_of;

    fn words(text: &[&str]) -> Vec<String> {
        text.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn the_bridge_serves_the_two_built_in_toolsets_and_names_no_other() {
        assert_eq!(toolset_of(&[]).unwrap().as_str(), "use");
        assert_eq!(
            toolset_of(&words(&["--toolset", "build"]))
                .unwrap()
                .as_str(),
            "build"
        );
        assert_eq!(
            toolset_of(&words(&["--toolset=use"])).unwrap().as_str(),
            "use"
        );
        for wrong in [
            words(&["--toolset"]),
            words(&["--toolset", "admin"]),
            words(&["build"]),
            words(&["--tool=use"]),
        ] {
            let refused = toolset_of(&wrong).unwrap_err();
            assert_eq!(refused.code().as_str(), "schema_invalid", "{wrong:?}");
        }
    }
}
