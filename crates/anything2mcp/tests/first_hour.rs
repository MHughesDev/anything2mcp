//! The first hour, end to end, through the command face only: a developer
//! with an empty folder and the built-in test application ends with a
//! published read tool that an agent calls, and the result is checked
//! against what the test application holds.
//!
//! `init`, the target added with its address and the surface's kind, the
//! scout and its report, the drafts not wanted removed, the read kept given
//! its offline test recorded and run, its first binding promoted on that
//! evidence, the publish asked for and carried out by a person on the
//! engine's page, and the published tool called. No file is written by
//! hand. Every command is the real program, a thin client
//! of one served engine. The engine is the same `LocalEngine` that `serve`
//! starts, run in this process because the program ships no presence prompt
//! a test can pass; the person's press goes through its page, as in
//! `local_engine.rs`.
//!
//! The target file's surface and test environment are written by
//! `target.add`, which asks the compiler set under which key a surface of
//! the kind states its address (decision 0199). The offline test and its
//! fixture are recorded by `fixture.record` from one real read, every value
//! scrubbed by rule (decision 0196). The whole flow is timed; the target is
//! a first tool in under ten minutes. A second test scouts the same application with an
//! engine that runs inside each command.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

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

/// The reads the scout drafts from the test application's published
/// description and proves by calling them.
const PROVEN: [&str; 5] = [
    "example_api_orders_list",
    "example_api_orders_get",
    "example_orders_items_get",
    "example_orders_totals_get",
    "example_orders_delivery_get",
];

/// The kind of the test application's surface, as the developer names it
/// to `target add`: never guessed from the address.
const SURFACE_KIND: &str = "http";

/// The read this developer makes usable, tests, and publishes.
const READ: &str = "example_api_orders_get";
/// Its input: an order the test application holds from its seed.
const ORDER_INPUT: &str = r#"{"order_id": "A-100"}"#;

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
    /// Whether each command runs the engine inside itself, and so is given
    /// the vault's passphrase, as a person running it at a terminal would.
    inside: bool,
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
            inside: false,
        }
    }

    /// A machine on which no engine is served: each command runs one.
    fn inside() -> Self {
        Self {
            inside: true,
            ..Self::new()
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
        if self.inside {
            command.env(PASSPHRASE_VARIABLE, PASSPHRASE);
        }
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

    /// Adds the target `example` with the test application's address and
    /// the kind of its surface, and checks the file it wrote.
    async fn add(&self, address: &str) -> Json {
        let added = self
            .ok(&[
                "target",
                "add",
                "example",
                "--address",
                address,
                "--kind",
                SURFACE_KIND,
            ])
            .await;
        assert_eq!(added["next"]["action"], "scout.run", "{added}");
        let written = std::fs::read_to_string(
            self.project
                .path()
                .join("targets")
                .join("example")
                .join("target.yaml"),
        )
        .unwrap();
        assert!(
            written.contains(&format!(
                "surfaces:\n  http:\n    kind: http\n    base: \"{address}\"\nenvironments:\n  test:\n    kind: test\ndefault_environment: test\n"
            )),
            "{written}"
        );
        added
    }

    /// Asks to publish, as the developer's agent does, and carries the
    /// publish out as the person: the page is read, Carry out is pressed
    /// from the page's own origin, and the presence prompt is passed, for
    /// the `presses`-th time. The page says the surface leads to `address`.
    async fn publish_as_a_person(
        &self,
        prompt: &Passed,
        version: &str,
        presses: u32,
        address: &str,
    ) {
        let (code, asked) = self
            .run(&["publish", "example", "--version", version])
            .await;
        assert_eq!(code, Some(3), "{asked}");
        assert_eq!(asked["result"]["state"], "needs_person", "{asked}");
        let link = asked["result"]["link"].as_str().unwrap().to_owned();
        let http = a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
            .no_proxy()
            .build()
            .unwrap();
        let page = http.get(&link).send().await.unwrap();
        assert_eq!(page.status().as_u16(), 200);
        let text = page.text().await.unwrap();
        assert!(
            text.contains("version.publish") && text.contains(READ),
            "{text}"
        );
        // The page says where the version's surface leads, in each
        // environment, by name, as the plan holds the address (decision
        // 0199).
        assert!(
            text.contains(&format!(
                "<h2>Where it leads</h2><ul><li><code>http</code>, a surface of the kind <code>http</code>, leads to <code>{}</code> in the environment <code>test</code>;",
                address.trim_end_matches('/')
            )),
            "{text}"
        );
        // The page names the binding that acts for the first time in this
        // version, by operation and mode.
        assert!(
            text.contains(&format!(
                "First to act in this version: {READ} (read), through the mode http."
            )),
            "{text}"
        );
        let marker = "name=\"once\" value=\"";
        let from = text.find(marker).unwrap() + marker.len();
        let once = &text[from..from + text[from..].find('"').unwrap()];
        let origin = &link[..link.find("/act/").unwrap()];
        let pressed = http
            .post(format!("{link}/confirm"))
            .header("origin", origin)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!("once={once}"))
            .send()
            .await
            .unwrap();
        let status = pressed.status().as_u16();
        let said = pressed.text().await.unwrap();
        assert_eq!(status, 200, "{said}");
        assert!(said.contains("was carried out as you, once"), "{said}");
        assert_eq!(prompt.0.load(Ordering::SeqCst), presses);
    }

    /// Runs the scout and continues it, as its `next` says, at most
    /// `calls` times; gives the last answer, with its report once the
    /// scout has finished.
    async fn scout(&self, fresh: bool, calls: usize) -> Json {
        let mut args = vec!["scout", "run", "example", "--wait-seconds", "10"];
        if fresh {
            args.push("--fresh");
        }
        let mut scouted = Json::Null;
        for _ in 0..calls {
            scouted = self.ok(&args).await;
            if scouted["result"].get("report_hash").is_some() {
                break;
            }
            assert_eq!(scouted["result"]["state"], "working", "{scouted}");
            args.truncate(5);
        }
        scouted
    }
}

/// The names of the scout's tried reads that were proven, or not.
fn tried(report: &Json, proven: bool) -> Vec<String> {
    let tools = report["result"]["content"]["tried"]["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut names: Vec<String> = tools
        .iter()
        .filter(|tool| (tool["state"] == "proven") == proven)
        .filter_map(|tool| tool["tool"].as_str().map(str::to_owned))
        .collect();
    names.sort();
    names
}

/// The drafted files a scout of the test application writes for the read
/// this hour is for: its endpoint and its operation.
const KEPT: [&str; 2] = ["endpoints/get_order.yaml", "operations/api_orders_get.yaml"];

/// What a scout's report says, checked the same way for every scout of the
/// test application: what was found, what was drafted, what was proven.
fn check_report(report: &Json) {
    let content = &report["result"]["content"];
    let surface = &content["surfaces"][0];
    assert_eq!(surface["state"], "identified", "{report}");
    // Found through the surface's own mode, with no binding written first:
    // the operations the published description states, and the endpoints
    // the live look found.
    assert_eq!(
        surface["findings_by_kind"],
        json!({"described_operation": {"inferred": 8}, "endpoint": {"discovered": 2}}),
        "{report}"
    );
    let written: Vec<&str> = content["drafted"]["written"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|file| file["file"].as_str())
        .collect();
    for file in KEPT {
        assert!(written.contains(&file), "{file} in {written:?}");
    }
    // Every drafted read tried, five times each or to its first failure:
    // the reads with an example of their own proven at `executed`.
    let mut proven = PROVEN.map(str::to_owned).to_vec();
    proven.sort();
    assert_eq!(tried(report, true), proven, "{report}");
    assert_eq!(tried(report, false), ["example_api_stock_get"], "{report}");
    let tools = content["tried"]["tools"].as_array().unwrap();
    let stock = tools
        .iter()
        .find(|tool| tool["tool"] == "example_api_stock_get")
        .unwrap();
    // The stand fails every third call: two completed, the third failed by
    // name, and the read is tried and not proven.
    assert_eq!(stock["state"], "failed", "{stock}");
    assert_eq!(stock["tries"], 3, "{stock}");
    assert_eq!(stock["completed"], 2, "{stock}");
    assert_eq!(stock["failure"]["code"], "unexpected_status", "{stock}");
    for tool in tools.iter().filter(|tool| tool["state"] == "proven") {
        assert_eq!(tool["level"], "executed", "{tool}");
        assert_eq!(tool["tries"], 5, "{tool}");
    }
    // An agent asked: no consent is recorded on the person's behalf.
    assert_eq!(content["consent"]["recorded"], false, "{report}");
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
            (
                "A2M_HOME".to_owned(),
                desk.home.path().display().to_string(),
            ),
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

    // 2. The target is added with the application's address and the kind
    // of its surface: the target file declares that one surface, under the
    // key its pack states, and a test environment. Nothing else.
    let added = desk.add(&app.url("")).await;
    assert_eq!(added["result"]["folder"], "targets/example", "{added}");

    // 3. The scout and its report. It observes the surface through the
    // surface's own mode, drafts by the packs' rules, and tries every
    // drafted read; each trial is submitted once, and the stage ends.
    let first = desk.scout(false, 20).await;
    assert!(first["result"]["report_hash"].is_string(), "{first}");
    check_report(&first);
    // A second scout of the same application in the same state, with the
    // same drafts, gives the same report.
    let again = desk.scout(true, 20).await;
    check_report(&again);
    assert_ne!(again["result"]["scout_id"], first["result"]["scout_id"]);
    assert_eq!(
        again["result"]["report_hash"], first["result"]["report_hash"],
        "{first}\n{again}"
    );

    // 4. The drafted read looked at: linted, and tried in the sandbox.
    let linted = desk.ok(&["lint", "example"]).await;
    assert_eq!(linted["result"]["ok"], true, "{linted}");
    assert!(
        linted["result"]["tools"]
            .as_array()
            .unwrap()
            .contains(&json!(READ)),
        "{linted}"
    );
    let tried = desk
        .ok(&[
            "draft",
            "try",
            "example",
            "--tool",
            READ,
            "--input",
            ORDER_INPUT,
        ])
        .await;
    assert_eq!(tried["result"]["status"], "completed", "{tried}");
    let held = app.stored_order("A-100").await.unwrap().unwrap();
    assert_eq!(
        tried["result"]["result"]["customer"],
        held.customer.as_str(),
        "{tried}"
    );

    // 5. Its tests. The gates refuse every operation that has no offline
    // test, and name both ways on: a test, or the draft removed.
    let untested = desk.ok(&["test", "example"]).await;
    let missing = untested["result"]["missing"].as_array().unwrap().clone();
    let entry = missing
        .iter()
        .find(|entry| entry["file"] == "operations/api_orders_get.yaml")
        .unwrap_or_else(|| panic!("{untested}"));
    assert_eq!(entry["next"][0]["action"], "test.run", "{entry}");
    assert_eq!(entry["next"][1]["action"], "draft.delete", "{entry}");
    // The developer keeps the one read this hour is for, and removes the
    // other drafted operations, as the refusal says.
    for entry in &missing {
        let file = entry["file"].as_str().unwrap();
        if KEPT.contains(&file) {
            continue;
        }
        let removed = desk
            .ok(&["draft", "delete", "example", "--file", file])
            .await;
        assert_eq!(removed["result"]["removed"], true, "{removed}");
        assert!(
            !desk
                .project
                .path()
                .join("targets")
                .join("example")
                .join(file)
                .exists()
        );
    }
    // The read's offline test and its fixture, recorded from one real read
    // in the sandbox, every value scrubbed by rule (decision 0196).
    let recorded = desk
        .ok(&[
            "fixture",
            "record",
            "example",
            "--tool",
            READ,
            "--input",
            ORDER_INPUT,
        ])
        .await;
    assert_eq!(recorded["result"]["test_state"], "passed", "{recorded}");
    assert_eq!(
        recorded["result"]["fixture"], "fixtures/api_orders_get",
        "{recorded}"
    );
    // No value the application holds of the order is in any byte written.
    let target_dir = desk.project.path().join("targets").join("example");
    let mut held_values = vec![held.order_id.clone(), held.customer.clone()];
    if let Some(status) = serde_json::to_value(held.status).unwrap().as_str() {
        held_values.push(status.to_owned());
    }
    for file in recorded["result"]["files"].as_array().unwrap() {
        let bytes = std::fs::read(target_dir.join(file.as_str().unwrap())).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        for value in &held_values {
            assert!(!text.contains(value.as_str()), "{value} in {file}: {text}");
        }
    }
    let tested = desk.ok(&["test", "example"]).await;
    assert_eq!(tested["result"]["passed"], true, "{tested}");
    assert_eq!(tested["result"]["gates"][1]["state"], "passed", "{tested}");

    // 6. The read's first binding made able to act: its offline test
    // passes, and the scout proved it at `executed`. An agent may promote
    // a read; the file's status is rewritten through the checked write.
    let promoted = desk
        .ok(&[
            "binding", "promote", "example", "--tool", READ, "--mode", "http",
        ])
        .await;
    assert_eq!(promoted["result"]["promoted"], true, "{promoted}");
    assert_eq!(
        promoted["result"]["evidence"]["tried"]["by"], "scout",
        "{promoted}"
    );
    let operation = desk
        .project
        .path()
        .join("targets")
        .join("example")
        .join("operations")
        .join("api_orders_get.yaml");
    assert!(
        std::fs::read_to_string(&operation)
            .unwrap()
            .contains("status: \"verified\""),
    );
    let tested = desk.ok(&["test", "example"]).await;
    assert_eq!(tested["result"]["passed"], true, "{tested}");

    // 7. The publish, asked for by the developer's agent, waits for a
    // person on the engine's page; the page names the binding that acts
    // for the first time; the person presses Carry out and passes the
    // presence prompt.
    desk.publish_as_a_person(&prompt, "1.0.0", 1, &app.url(""))
        .await;
    // 8. The published tool, called by an agent, reads what the
    // application holds.
    let called = desk.ok(&["call", READ, "--input", ORDER_INPUT]).await;
    assert_eq!(called["result"]["status"], "completed", "{called}");
    let read = &called["result"]["result"];
    assert_eq!(read["customer"], held.customer.as_str(), "{called}");
    assert_eq!(read["order_id"], held.order_id.as_str(), "{called}");
    assert_eq!(
        read["status"],
        serde_json::to_value(held.status).unwrap(),
        "{called}"
    );
    // Nothing in the hour wrote to the application.
    assert!(app.writes().await.unwrap().is_empty());
    assert_eq!(app.stored_order("A-100").await.unwrap().unwrap(), held);

    let elapsed = began.elapsed();
    // The target is a first tool in under ten minutes.
    println!(
        "the first hour, from an empty folder to a published read called, took {:.1} s",
        elapsed.as_secs_f64()
    );
    assert!(elapsed < Duration::from_secs(600), "{elapsed:?}");
    let (code, _) = desk.run(&["engine", "stop"]).await;
    assert_eq!(code, Some(0));
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
}

/// The scout with an engine that runs inside each command, as
/// `engine.in_process` makes it: every call is a new engine, which ends
/// with its command. A trial a call starts is waited for to its end within
/// that call, so none is left claimed by an engine that has gone, and a
/// later call finds each trial made before and goes on from there.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scout_with_an_engine_inside_each_command_ends_with_its_report() {
    let app = TestApp::start().await.unwrap();
    let desk = Desk::inside();
    desk.ok(&["init"]).await;
    desk.add(&app.url("")).await;
    // No wait between trials: each command makes one trial at least, ends,
    // and the next command, a new engine, continues the same scout.
    let first = desk
        .ok(&["scout", "run", "example", "--wait-seconds", "0"])
        .await;
    assert_eq!(first["result"]["state"], "working", "{first}");
    assert_eq!(first["result"]["stage"], "try", "{first}");
    let second = desk
        .ok(&["scout", "run", "example", "--wait-seconds", "0"])
        .await;
    assert_eq!(second["result"]["state"], "working", "{second}");
    assert_eq!(second["result"]["scout_id"], first["result"]["scout_id"]);
    // Then to its end.
    let mut scouted = Json::Null;
    for _ in 0..5 {
        scouted = desk
            .ok(&["scout", "run", "example", "--wait-seconds", "60"])
            .await;
        if scouted["result"].get("report_hash").is_some() {
            break;
        }
    }
    check_report(&scouted);
    assert_eq!(scouted["result"]["scout_id"], first["result"]["scout_id"]);
    // Each trial was submitted once, by whichever engine made it: five runs
    // of each proven read, three of the read that failed on its third.
    for (tool, count) in PROVEN
        .iter()
        .map(|tool| (*tool, 5))
        .chain([("example_api_stock_get", 3)])
    {
        let listed = desk
            .ok(&["run", "list", "--tool", tool, "--limit", "100"])
            .await;
        assert_eq!(
            listed["result"]["runs"].as_array().unwrap().len(),
            count,
            "{tool}: {listed}"
        );
    }
    assert!(app.writes().await.unwrap().is_empty());
}
