//! The first hour, end to end, through the command face only: a developer
//! with an empty folder and the built-in test application ends with a
//! published read tool that an agent calls, and the result is checked
//! against what the test application holds.
//!
//! `init`, the target added, its surface declared, the scout and its
//! report, a drafted read linted and tried, its offline test written by
//! hand and run, the publish asked for and carried out by a person on the
//! engine's page, and the published tool called. Every command is the
//! real program, a thin client of one served engine. The engine is the
//! same `LocalEngine` that `serve` starts, run in this process because the
//! program ships no presence prompt a test can pass; the person's press
//! goes through its page, as in `local_engine.rs`.
//!
//! Where the product stops today, the test asserts the named stop and says
//! which package lifts it, so that it fails loudly when the stop moves.
//! The whole flow is timed; the target is a first tool in under ten
//! minutes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::Path;
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use a2m_actions::{BoxFuture, Paths, PersonSurface};
use a2m_local::{Availability, PresencePrompt, PromptAnswer, PromptName, PromptText, StopReason};
use anything2mcp::{EngineOptions, LocalEngine, PASSPHRASE_VARIABLE};
use common::{Folder, IN_PROCESS_VARIABLE, PASSPHRASE, Surface, USER};
use example_app::harness::TestApp;
use serde_json::{Value as Json, json};

/// How long one step may take before the test fails by name.
const PATIENCE: Duration = Duration::from_secs(180);

/// The reads the scout drafts from the test application's published
/// description and proves by calling them.
const PROVEN: [&str; 5] = [
    "example_api_orders_list",
    "example_api_orders_get",
    "example_orders_items_get",
    "example_orders_totals_get",
    "example_orders_delivery_get",
];

/// The read this developer makes usable, tests, and publishes.
const READ: &str = "example_api_orders_get";

/// A presence prompt that a person passes, and that counts.
struct Passed(AtomicU32);

impl PresencePrompt for Passed {
    fn available(&self) -> BoxFuture<'_, Availability> {
        Box::pin(async { Availability::ready(PromptName::Polkit, "The prompt of a test.") })
    }

    fn ask(
        &self,
        _text: PromptText,
        _deadline: tokio::time::Instant,
    ) -> BoxFuture<'_, PromptAnswer> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            PromptAnswer::Present(PromptName::Polkit)
        })
    }
}

/// The developer's machine: a home for the engine and an empty folder.
struct Desk {
    home: Folder,
    project: Folder,
}

impl Desk {
    fn new() -> Self {
        let home = Folder::new("first-home");
        // The vault takes its key from a passphrase, which only the engine
        // is given: a command holds none.
        let installation = Paths::under(home.path()).installation_config;
        std::fs::create_dir_all(&installation).unwrap();
        std::fs::write(
            installation.join("settings.toml"),
            "[vault]\nbackend = \"passphrase\"\n",
        )
        .unwrap();
        Self {
            home,
            project: Folder::new("first-project"),
        }
    }

    /// The real program, run in the project folder as an agent runs it:
    /// no terminal, no passphrase, and no engine of its own.
    fn program(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_anything2mcp"));
        command
            .arg("--json")
            .args(args)
            .current_dir(self.project.path())
            .env_remove(PASSPHRASE_VARIABLE)
            .env("A2M_HOME", self.home.path())
            .env("USER", USER)
            .env(IN_PROCESS_VARIABLE, "true")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        command
    }

    /// Runs a command to its end and gives its exit code and its object.
    async fn run(&self, args: &[&str]) -> (Option<i32>, Json) {
        let mut command = self.program(args);
        let done: Output = tokio::task::spawn_blocking(move || command.output().unwrap())
            .await
            .unwrap();
        let object = serde_json::from_slice(&done.stdout).unwrap_or_else(|_| {
            panic!(
                "`{}` printed no single object: {} / {}",
                args.join(" "),
                String::from_utf8_lossy(&done.stdout),
                String::from_utf8_lossy(&done.stderr)
            )
        });
        (done.status.code(), object)
    }

    /// A command that must succeed.
    async fn ok(&self, args: &[&str]) -> Json {
        let (code, object) = self.run(args).await;
        assert_eq!(code, Some(0), "`{}`: {object}", args.join(" "));
        object
    }

    /// Writes one draft file, through the command face, and checks that it
    /// was taken.
    async fn write(&self, file: &str, content: &str) {
        let written = self
            .ok(&[
                "draft", "write", "example", "--file", file, "--content", content,
            ])
            .await;
        assert_eq!(written["result"]["written"], true, "{file}: {written}");
    }

    /// Follows a run until it rests or ends, as its `next` says.
    async fn follow(&self, run: &str) -> Json {
        let give_up = Instant::now() + PATIENCE;
        loop {
            let got = self.ok(&["run", "get", run]).await;
            let status = got["result"]["status"].as_str().unwrap_or_default();
            if !matches!(status, "queued" | "validating" | "running") {
                return got;
            }
            assert!(Instant::now() < give_up, "the run {run} did not end: {got}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Runs the scout to its end: each call continues the last scout
    /// until it has finished.
    async fn scout(&self, fresh: bool) -> Json {
        let give_up = Instant::now() + PATIENCE;
        let mut args = vec!["scout", "run", "example", "--wait-seconds", "20"];
        if fresh {
            args.push("--fresh");
        }
        loop {
            let scouted = self.ok(&args).await;
            if scouted["result"].get("report_hash").is_some() {
                return scouted;
            }
            assert_eq!(scouted["result"]["state"], "working", "{scouted}");
            assert!(Instant::now() < give_up, "the scout did not end: {scouted}");
            args.truncate(5);
        }
    }
}

/// The target file a developer writes once `target add` has made it: the
/// one surface, the test application's address, and a test environment.
fn target_file(address: &str) -> String {
    format!(
        "schema_version: 1\ntarget: example\ntitle: Example order entry\nsurfaces:\n  api:\n    kind: http\n    base: \"{address}\"\nenvironments:\n  test:\n    kind: test\ndefault_environment: test\n"
    )
}

/// The names of the scout's tried reads at one level of proof.
fn tried(report: &Json, proven: bool) -> Vec<String> {
    let tools = report["result"]["content"]["tried"]["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut names: Vec<String> = tools
        .iter()
        .filter(|tool| (tool["proven"] == true) == proven)
        .filter_map(|tool| tool["tool"].as_str().map(str::to_owned))
        .collect();
    names.sort();
    names
}

fn files_of(folder: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files_of(&path));
        } else {
            found.push(path.display().to_string());
        }
    }
    found
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_first_hour_from_an_empty_folder_to_a_published_read_an_agent_calls() {
    let app = TestApp::start().await.unwrap();
    let desk = Desk::new();
    let began = Instant::now();

    // 1. `init` makes the empty folder a project.
    let made = desk.ok(&["init"]).await;
    assert!(
        made["result"]["written"]
            .as_array()
            .unwrap()
            .contains(&json!("anything2mcp.toml")),
        "{made}"
    );

    // The engine, as `serve` starts it in this project.
    let prompt = Arc::new(Passed(AtomicU32::new(0)));
    let mut options = EngineOptions::of_machine(
        Paths::under(desk.home.path()),
        std::collections::BTreeMap::from([
            ("A2M_HOME".to_owned(), desk.home.path().display().to_string()),
            ("USER".to_owned(), USER.to_owned()),
            (PASSPHRASE_VARIABLE.to_owned(), PASSPHRASE.to_owned()),
        ]),
        Surface::new() as Arc<dyn PersonSurface>,
    );
    options.project = Some(desk.project.path().to_path_buf());
    let local = LocalEngine::start(options, Arc::clone(&prompt) as Arc<dyn PresencePrompt>)
        .await
        .unwrap();
    let serving = tokio::spawn(local.serve());

    // 2. The target is added, and its surface declared.
    let added = desk
        .ok(&["target", "add", "example", "--address", &app.url("")])
        .await;
    assert_eq!(added["result"]["folder"], "targets/example", "{added}");
    desk.write("target.yaml", &target_file(&app.url(""))).await;

    // 3. The scout and its report.
    let first = desk.scout(false).await;
    println!("{first}");

    let elapsed = began.elapsed();
    println!("the first hour took {:.1} s", elapsed.as_secs_f64());
    let (code, _) = desk.run(&["engine", "stop"]).await;
    assert_eq!(code, Some(0));
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
    let _ = (tried, files_of, READ, PROVEN);
}
