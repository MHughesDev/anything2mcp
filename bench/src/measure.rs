//! The scenarios of package 1.7: the budgets of spec section 225 that the
//! product's code can be measured against today.
//!
//! Each in-process scenario opens the engine as the product's own end-to-end
//! tests do, over the test application, and calls a tool through the
//! product's commands. Two scenarios run the real program as a child process:
//! the start of the engine, and its memory while idle. A scenario whose code
//! does not exist is not here; its budget is reported as not measured.
//!
//! What a number includes is said beside each scenario, because a budget is
//! about the product's own time and a test stand's time is never the product's.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use a2m_actions::Paths;
use anything2mcp::PASSPHRASE_VARIABLE;
use anything2mcp::cli::READY_LINE;
use serde_json::Value;

use crate::error::BenchError;
use crate::harness::{Clock, Overhead, Plan, Scenario, ScenarioResult};
use crate::promotion::{Folder, MEASURED, Machine, fail};
use crate::scenarios::Stand;
use crate::stats::Stats;

/// Listing the published tools.
pub const LIST_TOOLS: &str = "list_tools";
/// A read through the request binding, set against the same request made directly.
pub const HTTP_OPERATION: &str = "http_operation";
/// A read through the interface binding on a warm tab, set against the page's own request.
pub const UI_STEP: &str = "ui_step";
/// The first read on an engine that has not started a browser.
pub const COLD_BROWSER_START: &str = "cold_browser_start";
/// Process start to the line that says the engine is ready.
pub const START_TO_READY: &str = "start_to_ready";
/// Process start to ready with the vault closed: no key was given, so the
/// engine serves and runs no job. The floor of the start, before any key is opened.
pub const START_TO_READY_VAULT_CLOSED: &str = "start_to_ready_vault_closed";
/// The resident memory of an engine that has served nothing but a status.
pub const IDLE_MEMORY: &str = "idle_memory";

/// The most iterations a scenario that reads through a browser runs.
pub const MOST_BROWSER_ITERATIONS: u32 = 40;
/// The most engines a scenario that starts one for each iteration starts.
pub const MOST_STARTS: u32 = 20;

/// A passphrase for the vault of the engines the scenarios start: the route a
/// machine with no key store of its own takes.
const PASSPHRASE: &str = "bench-passphrase-for-a-throwaway-vault";

/// The path of the page the interface binding reads, as the application serves it.
fn page_path() -> String {
    format!("/orders/{}/items/{}", MEASURED.0, MEASURED.1)
}

fn operation_error(scenario: &str, message: impl std::fmt::Display) -> BenchError {
    BenchError::operation(scenario, message)
}

// ---- listing tools --------------------------------------------------------

/// What the tool-listing scenario holds: the application and an engine with
/// one published tool.
pub struct Listing {
    _stand: Stand,
    machine: Machine,
}

/// `tool search` with no words, which lists every published tool, as a
/// person's command in an open engine. It is the command front door's time,
/// parse and render included. The MCP `tools/list` is served from the same
/// registry; it is not timed separately here.
#[derive(Debug, Clone, Copy, Default)]
pub struct ListTools;

impl Scenario for ListTools {
    type State = Listing;

    fn name(&self) -> &str {
        LIST_TOOLS
    }

    async fn setup(&self) -> Result<Listing, BenchError> {
        let stand = Stand::start(LIST_TOOLS).await?;
        let address = stand.app().url("");
        let machine = Machine::open(address.trim_end_matches('/')).await?;
        machine.publish().await?;
        let listed = machine.run(&["tool", "search"], true).await?;
        if !listed.to_string().contains(crate::promotion::TOOL) {
            return Err(fail(format!("the published tool is not listed: {listed}")));
        }
        Ok(Listing {
            _stand: stand,
            machine,
        })
    }

    async fn operation(&self, state: &Listing) -> Result<(), BenchError> {
        let listed = state.machine.run(&["tool", "search"], true).await?;
        if listed.is_null() {
            return Err(operation_error(LIST_TOOLS, "the listing was empty"));
        }
        Ok(())
    }
}

// ---- a read through the request binding -----------------------------------

/// What a read scenario holds: the application, an engine, and the answer the
/// first read gave.
pub struct Reading {
    stand: Stand,
    machine: Machine,
    reference: Value,
}

async fn reading(scenario: &str, request: bool) -> Result<Reading, BenchError> {
    let stand = Stand::start(scenario).await?;
    let address = stand.app().url("");
    let address = address.trim_end_matches('/').to_owned();
    let machine = Machine::open(&address).await?;
    if request {
        machine.add_request_binding(&address, "verified")?;
    }
    machine.publish().await?;
    // The application says how the read reached it.
    let before = stand.app().seen().await.map_err(fail)?;
    let reference = machine.read(scenario, MEASURED).await?;
    let heard: Vec<String> = stand
        .app()
        .seen()
        .await
        .map_err(fail)?
        .into_iter()
        .skip(before.len())
        .collect();
    let json_read = format!("GET /api/orders/{}/items/{}", MEASURED.0, MEASURED.1);
    let page_read = format!("GET {}", page_path());
    let reached = if request {
        heard.contains(&json_read) && !heard.contains(&page_read)
    } else {
        heard.contains(&page_read)
    };
    if !reached {
        return Err(fail(format!(
            "the read did not reach the application by the binding the scenario measures: it heard {heard:?}"
        )));
    }
    Ok(Reading {
        stand,
        machine,
        reference,
    })
}

async fn read_again(scenario: &str, state: &Reading) -> Result<(), BenchError> {
    let read = state.machine.read(scenario, MEASURED).await?;
    if read == state.reference {
        Ok(())
    } else {
        Err(operation_error(
            scenario,
            format!("a read returned {read}, not {}", state.reference),
        ))
    }
}

/// A read through a verified `http` binding of an operation that also has a
/// `ui` binding, against the same request made straight to the application.
///
/// The difference is the product's own time for the call: the command, the
/// policy check, scheduling, the ledger's record, the request, and the result.
#[derive(Debug, Clone, Copy, Default)]
pub struct HttpOperation;

impl Scenario for HttpOperation {
    type State = Reading;

    fn name(&self) -> &str {
        HTTP_OPERATION
    }

    async fn setup(&self) -> Result<Reading, BenchError> {
        reading(HTTP_OPERATION, true).await
    }

    async fn operation(&self, state: &Reading) -> Result<(), BenchError> {
        read_again(HTTP_OPERATION, state).await
    }

    fn baseline(&self, state: &Reading) -> Option<impl Future<Output = Result<(), BenchError>>> {
        Some(state.stand.read_directly(HTTP_OPERATION))
    }
}

/// A read through the interface binding on a warm tab, against a request for
/// the same page made straight to the application.
///
/// The difference holds the browser's own work, drawing the page and running
/// its scripts, which no engine can take away; so it is an upper bound on the
/// product's overhead, and the budget row it is held to is read that way.
#[derive(Debug, Clone, Copy, Default)]
pub struct UiStep;

impl Scenario for UiStep {
    type State = Reading;

    fn name(&self) -> &str {
        UI_STEP
    }

    async fn setup(&self) -> Result<Reading, BenchError> {
        reading(UI_STEP, false).await
    }

    async fn operation(&self, state: &Reading) -> Result<(), BenchError> {
        read_again(UI_STEP, state).await
    }

    fn baseline(&self, state: &Reading) -> Option<impl Future<Output = Result<(), BenchError>>> {
        Some(state.stand.read_page_directly(UI_STEP))
    }
}

impl Stand {
    /// Reads the seeded line item's page straight from the application.
    ///
    /// # Errors
    ///
    /// Returns an error, naming the scenario, if the request failed or the
    /// application did not answer with a page.
    pub async fn read_page_directly(&self, scenario: &str) -> Result<(), BenchError> {
        let response = self
            .app()
            .http()
            .get(self.app().url(&page_path()))
            .send()
            .await
            .map_err(|e| operation_error(scenario, e))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|e| operation_error(scenario, e))?;
        if status.is_success() && !body.is_empty() {
            Ok(())
        } else {
            Err(operation_error(
                scenario,
                format!(
                    "the application answered {status} with {} bytes",
                    body.len()
                ),
            ))
        }
    }
}

/// A result of a scenario that was timed by hand: no baseline, no overhead.
fn timed(name: &str, took: &[Duration]) -> Result<ScenarioResult, BenchError> {
    let operation = Stats::from_durations(took).ok_or(BenchError::NoIterations)?;
    Ok(ScenarioResult {
        name: name.to_owned(),
        operation,
        baseline: None,
        overhead: None,
        resident_bytes: None,
    })
}

/// The first read on an engine that has not yet opened a browser.
///
/// Each iteration opens a new engine with a data directory of its own and
/// publishes; the engine has started no browser until the read. The time is
/// that read: the browser's start, its tab, and the read, so it holds the
/// warm read's time as well, and is an upper bound on the browser's start.
///
/// # Errors
///
/// Returns an error if an engine could not be opened or a read failed.
pub async fn cold_browser_start(
    plan: &Plan,
    clock: &impl Clock,
) -> Result<ScenarioResult, BenchError> {
    let stand = Stand::start(COLD_BROWSER_START).await?;
    let address = stand.app().url("");
    let address = address.trim_end_matches('/');
    let mut took = Vec::new();
    for _ in 0..plan.iterations {
        let machine = Machine::open(address).await?;
        machine.publish().await?;
        let started = clock.now();
        machine.read(COLD_BROWSER_START, MEASURED).await?;
        took.push(clock.now().saturating_sub(started));
        // The engine, and its browser with it, ends here.
        drop(machine);
    }
    timed(COLD_BROWSER_START, &took)
}

// ---- the program as a child process ----------------------------------------

/// The program the process scenarios start: the one named by the variable
/// `A2M_BENCH_PROGRAM`, or `anything2mcp` in the folder this program is in.
#[must_use]
pub fn program() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("A2M_BENCH_PROGRAM") {
        return Some(PathBuf::from(named));
    }
    let own = std::env::current_exe().ok()?;
    let name = format!("anything2mcp{}", std::env::consts::EXE_SUFFIX);
    let found = own.parent()?.join(name);
    found.is_file().then_some(found)
}

/// How the engine's vault gets its key.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Vault {
    /// From a passphrase in the environment: the route a machine with no key
    /// store takes, and the only one a container can take.
    Passphrase,
    /// From nothing: the vault stays closed.
    Closed,
}

struct Engine {
    child: Child,
    // Held for the engine's life: its home folder.
    _home: Folder,
}

impl Engine {
    fn command(program: &PathBuf, home: &Folder, args: &[&str]) -> Command {
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(&home.0)
            .env("A2M_HOME", &home.0)
            .env("USER", "pat")
            .env(PASSPHRASE_VARIABLE, PASSPHRASE)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        command
    }

    /// Starts `serve` in a new home and waits for the line that says it is
    /// ready. The time is from just before the start to that line.
    fn start(
        program: &PathBuf,
        clock: &impl Clock,
        scenario: &str,
        vault: Vault,
    ) -> Result<(Self, Duration), BenchError> {
        let home = Folder::new("benchengine")?;
        if vault == Vault::Passphrase {
            let installation = Paths::under(&home.0).installation_config;
            std::fs::create_dir_all(&installation).map_err(fail)?;
            std::fs::write(
                installation.join("settings.toml"),
                "[vault]\nbackend = \"passphrase\"\n",
            )
            .map_err(fail)?;
        }
        let started = clock.now();
        let mut command = Self::command(program, &home, &["serve"]);
        if vault == Vault::Closed {
            command.env_remove(PASSPHRASE_VARIABLE);
        }
        let mut child = command
            .spawn()
            .map_err(|e| operation_error(scenario, format!("{}: {e}", program.display())))?;
        let Some(stderr) = child.stderr.take() else {
            return Err(operation_error(scenario, "the program has no error stream"));
        };
        let mut said = Vec::new();
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if line == READY_LINE {
                let took = clock.now().saturating_sub(started);
                return Ok((Self { child, _home: home }, took));
            }
            said.push(line);
        }
        let _ = child.kill();
        let _ = child.wait();
        Err(operation_error(
            scenario,
            format!("the engine ended before it was ready: it said {said:?}"),
        ))
    }

    /// Asks the engine for its status, as any command does.
    fn status(&self, program: &PathBuf) -> Result<(), BenchError> {
        let output = Self::command(program, &self._home, &["engine", "status", "--json"])
            .output()
            .map_err(|e| operation_error(IDLE_MEMORY, e))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(operation_error(
                IDLE_MEMORY,
                format!("`engine status` stopped: {}", output.status),
            ))
        }
    }

    /// The engine's resident memory in bytes, where the system says it.
    fn resident_bytes(&self) -> Option<u64> {
        if !cfg!(unix) {
            return None;
        }
        let output = Command::new("ps")
            .args(["-o", "rss=", "-p", &self.child.id().to_string()])
            .output()
            .ok()?;
        let kib: u64 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .ok()?;
        Some(kib.saturating_mul(1024))
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts the program's engine as many times as the plan says, each in a new
/// home, and times each from the start to the line that says it is ready.
///
/// With `memory`, each engine is then asked for its status, once, as any
/// command asks, and its resident memory is read.
async fn engines(
    scenario: &str,
    plan: &Plan,
    clock: &impl Clock,
    memory: bool,
    vault: Vault,
) -> Result<Option<(Vec<Duration>, Vec<u64>)>, BenchError> {
    let Some(program) = program() else {
        return Ok(None);
    };
    let mut took = Vec::new();
    let mut resident = Vec::new();
    for turn in 0..plan.warmup_iterations.min(2) + plan.iterations.min(MOST_STARTS) {
        let (engine, time) = Engine::start(&program, clock, scenario, vault)?;
        if memory {
            engine.status(&program)?;
            if let Some(bytes) = engine.resident_bytes() {
                resident.push(bytes);
            }
        }
        drop(engine);
        // The first starts are warm-up: the program's file is read from disk
        // once and the system's caches hold it after.
        if turn >= plan.warmup_iterations.min(2) {
            took.push(time);
        }
    }
    Ok(Some((took, resident)))
}

/// From the start of the program's `serve` to the line that says it is
/// ready, in a new home each time, with the vault's passphrase route. `None`
/// when the program was not built beside the harness.
///
/// # Errors
///
/// Returns an error if an engine did not become ready.
pub async fn start_to_ready(
    plan: &Plan,
    clock: &impl Clock,
) -> Result<Option<ScenarioResult>, BenchError> {
    match engines(START_TO_READY, plan, clock, false, Vault::Passphrase).await? {
        Some((took, _)) => timed(START_TO_READY, &took).map(Some),
        None => Ok(None),
    }
}

/// From the start of `serve` with no key for the vault to the ready line.
///
/// # Errors
///
/// Returns an error if an engine did not become ready.
pub async fn start_to_ready_vault_closed(
    plan: &Plan,
    clock: &impl Clock,
) -> Result<Option<ScenarioResult>, BenchError> {
    match engines(
        START_TO_READY_VAULT_CLOSED,
        plan,
        clock,
        false,
        Vault::Closed,
    )
    .await?
    {
        Some((took, _)) => timed(START_TO_READY_VAULT_CLOSED, &took).map(Some),
        None => Ok(None),
    }
}

/// The resident memory of an engine that has been asked for its status and
/// nothing else: the median over the engines started. `None` when the
/// program was not built beside the harness, or the system gives no reading.
///
/// The scenario's timings are those of the starts it measured memory after.
///
/// # Errors
///
/// Returns an error if an engine did not become ready.
pub async fn idle_memory(
    plan: &Plan,
    clock: &impl Clock,
) -> Result<Option<ScenarioResult>, BenchError> {
    let Some((took, mut resident)) =
        engines(IDLE_MEMORY, plan, clock, true, Vault::Passphrase).await?
    else {
        return Ok(None);
    };
    resident.sort_unstable();
    let Some(median) = resident.get(resident.len().saturating_sub(1) / 2).copied() else {
        return Ok(None);
    };
    let mut result = timed(IDLE_MEMORY, &took)?;
    result.resident_bytes = Some(median);
    Ok(Some(result))
}

/// The overhead of `result` as the harness holds it, for a test's message.
#[must_use]
pub fn overhead_median_ns(result: &ScenarioResult) -> Option<i64> {
    result.overhead.map(|Overhead { median_ns, .. }| median_ns)
}
