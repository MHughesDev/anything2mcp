//! The process commands: `serve`, which is the local engine in the
//! foreground, and `bridge`, the stdio proxy an MCP client starts. They
//! start a role and are not actions. `runner` is named and not built.

use std::io::Write;
use std::sync::Arc;

use a2m_actions::{BuiltInToolset, Env, ExitClass, OsFamily, PROGRAM, Paths, Response, codes};
use a2m_core::{Failure, FailureClass};
use a2m_local::{ChannelControl, ClientLabel, Connector, StopReason, system_prompt};
use a2m_mcp::stdio_proxy;
use a2m_policy::ToolsetName;
use a2m_telemetry::SystemClock;
use serde_json::json;

use super::parse::{Globals, Tree};
use super::start::{self, PASSPHRASE_FROM_STDIN};
use super::{Finished, Host, client, project_of, render, stopped};
use crate::engine::EngineOptions;
use crate::local::{LocalEngine, program_version};
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
    Paths::resolve(&Env {
        os: OsFamily::current(),
        vars: host.environment.clone(),
    })
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

/// Resolves when the operating system asks the process to stop: the
/// terminal's interrupt everywhere, and the termination signal where
/// there is one. Never resolves when the system offers neither.
async fn asked_to_stop() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    interrupted = tokio::signal::ctrl_c() => {
                        if interrupted.is_err() {
                            terminate.recv().await;
                        }
                    }
                    _ = terminate.recv() => {}
                }
            }
            Err(_) => {
                if tokio::signal::ctrl_c().await.is_err() {
                    std::future::pending::<()>().await;
                }
            }
        }
    }
    #[cfg(not(unix))]
    {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

fn stop_when_asked(control: ChannelControl) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        asked_to_stop().await;
        control.stop(StopReason::Asked);
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
    for word in rest {
        if word == PASSPHRASE_FROM_STDIN && !passphrase_on_stdin {
            passphrase_on_stdin = true;
            continue;
        }
        return Err((
            malformed(
                format!(
                    "`{PROGRAM} serve` takes no argument but `{PASSPHRASE_FROM_STDIN}`, and `{}` was given.",
                    super::shown(word)
                ),
                format!("Run `{PROGRAM} serve`. Its settings are in the settings files."),
            ),
            ExitClass::Malformed,
        ));
    }
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
    let mut options = EngineOptions::of_machine(
        paths,
        environment,
        Arc::new(TerminalSurface::new(
            Arc::new(ProcessTerminal),
            Arc::clone(&clock) as Arc<dyn a2m_telemetry::Clock>,
        )),
    );
    options.clock = clock;
    options.project =
        project_of(globals, host).map_err(|failure| (failure, ExitClass::Malformed))?;
    options.key_store.clone_from(&host.key_store);
    let local = LocalEngine::start(options, system_prompt())
        .await
        .map_err(failed)?;
    let signals = stop_when_asked(local.control());
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
    let word = match reason {
        StopReason::Idle => "idle",
        StopReason::Asked => "asked",
        StopReason::Restart => "restart",
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
    };
    let response = Response {
        ok: true,
        action: "serve".to_owned(),
        result: Some(json!({"stopped": word})),
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
        Ok(_) => {}
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
