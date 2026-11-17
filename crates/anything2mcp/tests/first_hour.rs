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
                "draft",
                "write",
                "example",
                "--file",
                file,
                "--content",
                content,
            ])
            .await;
        assert_eq!(written["result"]["written"], true, "{file}: {written}");
    }

    /// Asks to publish, as the developer's agent does, and carries the
    /// publish out as the person: the page is read, Carry out is pressed
    /// from the page's own origin, and the presence prompt is passed, for
    /// the `presses`-th time.
    async fn publish_as_a_person(&self, prompt: &Passed, version: &str, presses: u32) {
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

/// The one read a developer writes by hand before the scout can observe
/// the surface (see the stop below): the test application's order, by its
/// number.
const SEED_ENDPOINT: &str = "endpoint: order_get\nsurface: api\nmethod: GET\npath: \"/api/orders/{order_id}\"\nauth: none\neffect: read\nheaders: {accept: \"application/json\"}\nresponses: {absent: [404]}\nresponse_schema:\n  type: object\n  required: [order_id, customer, status]\n  properties:\n    order_id: {type: string}\n    customer: {type: string}\n    status: {type: string, enum: [draft, submitted]}\n";
const SEED_OPERATION: &str = "operation: example_order_get\ntitle: Get an order\neffect: read\nkey: order_id\ninputs:\n  order_id: {type: identifier, description: \"Order number shown in the application\"}\noutputs:\n  status: {type: identifier}\n  customer: {type: text}\nexamples:\n  - {order_id: \"A-100\"}\nbindings:\n  - mode: http\n    verb: read\n    endpoint: order_get\ndescription: >\n  Reads the status and the customer of one order.\n";

/// The offline test a developer writes for the read once the publish
/// asks for one, with a fixture of synthetic values that match the test
/// application's seed: the answer the read's endpoint gives for A-100.
const FIXTURE: &str =
    "scrubbed: true\nnote: \"Synthetic data, written by hand for the imaginary target.\"\n";
const FIXTURE_ANSWER: &str =
    r#"{"order_id":"A-100","customer":"Example Customer One","status":"draft","items":[]}"#;
const OFFLINE_TEST: &str = "test: example_api_orders_get\nkind: offline\nfixture: api_orders_get\ninputs: {order_id: \"A-100\"}\nexpect: {status: draft, customer: \"Example Customer One\"}\n";

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

    // 2. The target is added, and its surface declared.
    let added = desk
        .ok(&["target", "add", "example", "--address", &app.url("")])
        .await;
    assert_eq!(added["result"]["folder"], "targets/example", "{added}");
    desk.write("target.yaml", &target_file(&app.url(""))).await;

    // 3. The scout and its report.
    let first = desk.scout(false, 3).await;
    let surfaces = &first["result"]["content"]["surfaces"];
    let ended = &surfaces[0]["ended"];
    // The named stop of today: a target that declares only its surface
    // has nothing the scout's sandbox can observe it through. The engine
    // prepares the driver of the first binding in the plan that acts on
    // the surface (`a2m-engine`, `sandbox.rs`), and a new target has none,
    // so the published description is not read (`busy`/`no_binding`) and
    // the live look finds nothing. This lifts when discovery prepares the
    // surface kind's driver without a binding: the engine's package of the
    // scout (decision 0179's stage 3). Until then a developer writes one
    // read by hand first, as the stop's fix says.
    assert_eq!(ended[0]["discoverer"], "http_description", "{first}");
    assert_eq!(
        ended[0]["report"]["failure"]["code"], "no_binding",
        "{first}"
    );
    assert_eq!(ended[1]["findings"], 0, "{first}");
    assert_eq!(
        first["result"]["content"]["drafted"]["written"],
        json!([]),
        "{first}"
    );
    desk.write("endpoints/orders.yaml", SEED_ENDPOINT).await;
    desk.write("operations/orders.yaml", SEED_OPERATION).await;

    let scouted = desk.scout(true, 3).await;
    // The named stop of today: the scout drafts the reads of the published
    // description and checks that they compile, and its try stage then
    // never ends. Each continuation submits the trials again, they
    // complete, and the scout still answers that a trial has not ended
    // (decision 0179's stage 6; the package that is changing the scout's
    // try stage and its report lifts it). When it lifts, the report is
    // asserted here: the five reads in `PROVEN` proven at level `executed`,
    // `example_api_stock_get` tried and not proven, and a second scout
    // giving the same `report_hash`.
    assert_eq!(scouted["result"]["stage"], "try", "{scouted}");
    assert_eq!(
        scouted["result"]["why"], "A trial has not ended yet.",
        "{scouted}"
    );
    let _ = (PROVEN, tried as fn(&Json, bool) -> Vec<String>);
    // What it drafted is in the target's folder: an endpoint and a read
    // for each read the description states.
    let drafted = files_of(&desk.project.path().join("targets").join("example"));
    for file in ["operations/api_orders_get.yaml", "endpoints/get_order.yaml"] {
        assert!(
            drafted
                .iter()
                .any(|path| path.replace('\\', "/").ends_with(file)),
            "{file} in {drafted:?}"
        );
    }

    // 4. The drafted read made usable: linted, and tried in the sandbox.
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

    // 5. Its tests. Nothing drafted one: an observed answer is business
    // data until a fixture is scrubbed, and `fixture.record` is not built.
    // The gates refuse every operation that has no offline test.
    let untested = desk.ok(&["test", "example"]).await;
    let problems = &untested["result"]["gates"][0]["problems"];
    assert!(
        problems.as_array().unwrap().iter().any(|problem| {
            problem["code"] == "kernel.operation_without_test"
                && problem["file"] == Json::Null
                && problem["at"]["file"] == "operations/api_orders_get.yaml"
        }),
        "{untested}"
    );
    // The developer keeps the one read this hour is for. No command removes
    // a draft file, so they do it in their editor: every other drafted
    // file, and the read written by hand before the scout.
    let folder = desk.project.path().join("targets").join("example");
    for path in files_of(&folder) {
        let name = path.replace('\\', "/");
        if ![
            "target.yaml",
            "endpoints/get_order.yaml",
            "operations/api_orders_get.yaml",
        ]
        .iter()
        .any(|kept| name.ends_with(&format!("/{kept}")))
        {
            std::fs::remove_file(&path).unwrap();
        }
    }
    // And writes its offline test, with a fixture of synthetic values that
    // match the application's seed, as the refusal's fix says.
    desk.write("fixtures/api_orders_get/fixture.yaml", FIXTURE)
        .await;
    desk.write("fixtures/api_orders_get/get_order.json", FIXTURE_ANSWER)
        .await;
    desk.write("tests/api_orders_get.yaml", OFFLINE_TEST).await;
    let tested = desk.ok(&["test", "example"]).await;
    assert_eq!(tested["result"]["passed"], true, "{tested}");
    assert_eq!(tested["result"]["gates"][1]["state"], "passed", "{tested}");

    // 6. The publish, asked for by the developer's agent, waits for a
    // person on the engine's page; the person reads it, presses Carry out,
    // and passes the presence prompt.
    desk.publish_as_a_person(&prompt, "1.0.0", 1).await;
    // 7. The published tool, called by an agent, reads what the
    // application holds.
    let (code, refused) = desk.run(&["call", READ, "--input", ORDER_INPUT]).await;
    // The named stop of today: a drafted binding is `draft`, and a
    // published binding that is `draft` may not act (`busy`/`no_binding`).
    // Nothing turns a tried draft binding into a verified one: `binding
    // promote` takes a `shadow` binding, and the scout's proof of a read
    // does not reach the file. It lifts when a read proven at `executed`
    // is drafted, or promoted, as verified (the scout's package and the
    // action registry's, decision 0179). Until then the developer, a
    // person, marks the binding verified in their editor, tests again, and
    // publishes the next version.
    assert_eq!(code, Some(1), "{refused}");
    assert_eq!(refused["error"]["code"], "no_binding", "{refused}");
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("is `draft` and may not act"),
        "{refused}"
    );
    let operation = folder.join("operations").join("api_orders_get.yaml");
    let text = std::fs::read_to_string(&operation).unwrap();
    let marked = text.replacen(
        "    verb: \"read\"\n",
        "    verb: \"read\"\n    status: \"verified\"\n",
        1,
    );
    assert_ne!(marked, text, "{text}");
    std::fs::write(&operation, marked).unwrap();
    let tested = desk.ok(&["test", "example"]).await;
    assert_eq!(tested["result"]["passed"], true, "{tested}");
    // The same call again would be given what became of the first act
    // for the approval window, so the next version is named.
    desk.publish_as_a_person(&prompt, "1.1.0", 2).await;

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
