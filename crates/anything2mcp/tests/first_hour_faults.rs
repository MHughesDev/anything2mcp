//! The faults of the first hour that package 5.6a measured (H-18's
//! result), each through the product's commands, as an agent with no
//! knowledge of the repository met it (package 5.6b):
//!
//! - a list on the command line, written as the agents wrote it, a JSON
//!   array or a comma-separated string, and how the help and the refusal
//!   say it;
//! - the `next` after `test.run`, which names `binding.promote` while the
//!   binding that would act is a draft;
//! - a promotion from a try whose run is not in the store, which says so
//!   and names the try to run again;
//! - the answer to a publish asked again after the person published, which
//!   says the version is published, with nothing pending;
//! - the engine's store and the project, which name one project, or the
//!   call is refused;
//! - the web scout of the test application, whose `/` redirects to
//!   `/orders`: the list read is drafted at `/orders`, not at `/{key}`, and
//!   the reads by key are tried on a key read off the list.
//!
//! Every command is the real program. The engine is the `LocalEngine` that
//! `serve` starts, run in this process so that its presence prompt can be
//! passed, as in `first_hour.rs`; the person's press goes through its page.
//! The web scout needs a browser, found as the product's other web tests
//! find one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use a2m_actions::{BoxFuture, Paths, PersonSurface};
use a2m_local::{Availability, PresencePrompt, PromptAnswer, PromptName, PromptText, StopReason};
use anything2mcp::{EngineOptions, LocalEngine, PASSPHRASE_VARIABLE};
use common::{Folder, IN_PROCESS_VARIABLE, PASSPHRASE, Surface, USER};
use example_app::harness::TestApp;
use serde_json::{Value as Json, json};

/// The read this developer keeps, tests, and publishes.
const READ: &str = "example_api_orders_get";
/// Its input: an order the test application holds from its seed.
const ORDER_INPUT: &str = r#"{"order_id": "A-100"}"#;
/// The drafts the read needs: its operation and the description it is
/// pinned to.
const KEPT: [&str; 2] = ["descriptions/http.yaml", "operations/api_orders_get.yaml"];

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

/// A home for the engine, with the passphrase route chosen for its vault.
fn home() -> Folder {
    let home = Folder::new("home");
    let installation = Paths::under(home.path()).installation_config;
    std::fs::create_dir_all(&installation).unwrap();
    std::fs::write(
        installation.join("settings.toml"),
        "[vault]\nbackend = \"passphrase\"\n",
    )
    .unwrap();
    home
}

/// The developer's machine: a project folder, and the engine home each
/// command is run with.
struct Desk {
    home: Folder,
    project: Folder,
}

impl Desk {
    fn new() -> Self {
        Self {
            home: home(),
            project: Folder::new("faults-project"),
        }
    }

    /// The real program, run in the project folder as an agent runs it,
    /// with `home` as its engine home. With `inside`, it runs the engine
    /// inside itself and is given the vault's passphrase.
    fn program(&self, home: &Folder, inside: bool, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_anything2mcp"));
        command
            .arg("--json")
            .args(args)
            .current_dir(self.project.path())
            .env_remove(PASSPHRASE_VARIABLE)
            .env("A2M_HOME", home.path())
            .env("USER", USER)
            .env(IN_PROCESS_VARIABLE, "true")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if inside {
            command.env(PASSPHRASE_VARIABLE, PASSPHRASE);
        }
        command
    }

    async fn run_with(&self, home: &Folder, inside: bool, args: &[&str]) -> (Option<i32>, Json) {
        let mut command = self.program(home, inside, args);
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

    /// A command of the served engine of this desk's home.
    async fn run(&self, args: &[&str]) -> (Option<i32>, Json) {
        self.run_with(&self.home, false, args).await
    }

    async fn ok(&self, args: &[&str]) -> Json {
        let (code, object) = self.run(args).await;
        assert_eq!(code, Some(0), "`{}`: {object}", args.join(" "));
        object
    }

    /// The help of a command, as text.
    fn help(&self, args: &[&str]) -> String {
        let mut command = Command::new(env!("CARGO_BIN_EXE_anything2mcp"));
        let done = command
            .args(args)
            .arg("--help")
            .current_dir(self.project.path())
            .env("A2M_HOME", self.home.path())
            .env("USER", USER)
            .output()
            .unwrap();
        // As one line: the help wraps at the terminal's width.
        String::from_utf8_lossy(&done.stdout)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The scout, continued as its `next` says, to its report.
    async fn scout(&self, inside: bool) -> Json {
        let mut args = vec!["scout", "run", "example", "--wait-seconds", "60"];
        let mut scouted = Json::Null;
        for _ in 0..20 {
            let (code, object) = self.run_with(&self.home, inside, &args).await;
            assert_eq!(code, Some(0), "{object}");
            scouted = object;
            if scouted["result"].get("report_hash").is_some() {
                return scouted;
            }
            assert_eq!(scouted["result"]["state"], "working", "{scouted}");
            args.truncate(5);
        }
        panic!("the scout did not end: {scouted}");
    }

    /// The engine of this desk's home, as `serve` starts it.
    async fn serve(&self, prompt: &Arc<Passed>) -> tokio::task::JoinHandle<StopReason> {
        let mut options = EngineOptions::of_machine(
            Paths::under(self.home.path()),
            std::collections::BTreeMap::from([
                (
                    "A2M_HOME".to_owned(),
                    self.home.path().display().to_string(),
                ),
                ("USER".to_owned(), USER.to_owned()),
                (PASSPHRASE_VARIABLE.to_owned(), PASSPHRASE.to_owned()),
            ]),
            Surface::new() as Arc<dyn PersonSurface>,
        );
        options.project = Some(self.project.path().to_path_buf());
        let local = LocalEngine::start(options, Arc::clone(prompt) as Arc<dyn PresencePrompt>)
            .await
            .unwrap();
        tokio::spawn(local.serve())
    }

    /// The person's press on the page a publish request names: the page is
    /// read, Carry out is pressed from the page's own origin, and the
    /// presence prompt is passed.
    async fn press(link: &str) {
        let http = a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
            .no_proxy()
            .build()
            .unwrap();
        let text = http.get(link).send().await.unwrap().text().await.unwrap();
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
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_faults_of_the_first_hour_are_met_no_more_through_the_commands() {
    let app = TestApp::start().await.unwrap();
    let desk = Desk::new();
    desk.ok(&["init"]).await;
    let prompt = Arc::new(Passed(AtomicU32::new(0)));
    let serving = desk.serve(&prompt).await;
    desk.ok(&[
        "target",
        "add",
        "example",
        "--address",
        &app.url(""),
        "--kind",
        "http",
    ])
    .await;
    desk.scout(false).await;

    // 1. A list on the command line. The help says both forms.
    let help = desk.help(&["draft", "delete"]);
    assert!(
        help.contains("repeat the flag for each item") && help.contains("one JSON array of text"),
        "{help}"
    );
    // A comma-separated string is one name, and its refusal says how a list
    // is written; nothing is removed.
    let (_, untested) = desk.run(&["test", "run", "example"]).await;
    let unwanted: Vec<String> = untested["result"]["missing"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["file"].as_str())
        .filter(|file| !KEPT.contains(file))
        .map(str::to_owned)
        .collect();
    assert!(unwanted.len() >= 2, "{untested}");
    let (code, comma) = desk
        .run(&["draft", "delete", "example", "--files", &unwanted.join(",")])
        .await;
    assert_eq!(code, Some(1), "{comma}");
    assert!(
        comma["error"]["fix"]
            .as_str()
            .unwrap()
            .contains("repeat the flag for each (`--files a --files b`)"),
        "{comma}"
    );
    let folder = desk.project.path().join("targets").join("example");
    assert!(unwanted.iter().all(|file| folder.join(file).exists()));
    // A JSON array, as an agent copies a list from a result, is the list.
    let array = serde_json::to_string(&unwanted).unwrap();
    let removed = desk
        .ok(&["draft", "delete", "example", "--files", &array])
        .await;
    assert_eq!(removed["result"]["removed"], true, "{removed}");
    assert_eq!(
        removed["result"]["files"].as_array().unwrap().len(),
        unwanted.len(),
        "{removed}"
    );
    assert!(unwanted.iter().all(|file| !folder.join(file).exists()));

    // 2. The `next` after `test.run` names the promote while the binding
    // that would act is a draft, and the publish request after it.
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
    let tested = desk.ok(&["test", "run", "example"]).await;
    assert_eq!(tested["result"]["passed"], true, "{tested}");
    assert_eq!(tested["next"]["action"], "binding.promote", "{tested}");
    assert_eq!(tested["next"]["args"]["tool"], READ, "{tested}");
    assert_eq!(tested["next"]["args"]["mode"], "api", "{tested}");
    let status = desk.ok(&["target", "status", "example"]).await;
    assert_eq!(status["next"]["action"], "binding.promote", "{status}");

    // 3. A promotion from a try whose run is not in the store says so, and
    // names the try to run again.
    let (code, gone) = desk
        .run(&[
            "binding",
            "promote",
            "example",
            "--tool",
            READ,
            "--mode",
            "api",
            "--run-id",
            "r_01a20fd0b2db746283a55984644a888c",
        ])
        .await;
    assert_eq!(code, Some(1), "{gone}");
    assert_eq!(gone["error"]["code"], "promotion_unproven", "{gone}");
    assert!(
        gone["error"]["message"]
            .as_str()
            .unwrap()
            .contains("this engine's store holds no such run"),
        "{gone}"
    );
    assert!(
        gone["error"]["fix"].as_str().unwrap().contains(&format!(
            "`draft.try` (target `example`, tool `{READ}`, mode `api`, position 1)"
        )),
        "{gone}"
    );
    // The scout's proof promotes it; the `next` is then the publish.
    let promoted = desk
        .ok(&[
            "binding", "promote", "example", "--tool", READ, "--mode", "api",
        ])
        .await;
    assert_eq!(promoted["result"]["promoted"], true, "{promoted}");
    let tested = desk.ok(&["test", "run", "example"]).await;
    assert_eq!(tested["next"]["action"], "publish.request", "{tested}");

    // 4. The publish: asked, the page waits, and the `next` is the same
    // request; after the person's press, the same request says the version
    // is published, with nothing pending.
    let (code, asked) = desk
        .run(&["publish", "request", "example", "--version", "1.0.0"])
        .await;
    assert_eq!(code, Some(3), "{asked}");
    assert_eq!(asked["next"]["action"], "publish.request", "{asked}");
    Desk::press(asked["result"]["link"].as_str().unwrap()).await;
    assert_eq!(prompt.0.load(Ordering::SeqCst), 1);
    let again = desk
        .ok(&["publish", "request", "example", "--version", "1.0.0"])
        .await;
    assert_eq!(again["result"]["state"], "published", "{again}");
    assert_eq!(again["result"]["version"], "1.0.0", "{again}");
    let warnings = again["warnings"].to_string();
    assert!(!warnings.contains("did nothing more"), "{again}");
    assert!(
        warnings.contains("published version `1.0.0` of `example`")
            && warnings.contains("nothing waits"),
        "{again}"
    );
    assert_eq!(again["next"]["action"], "tool.search", "{again}");
    let called = desk
        .ok(&["tool", "call", READ, "--input", ORDER_INPUT])
        .await;
    assert_eq!(called["result"]["status"], "completed", "{called}");

    // 5. The same project, reached with another engine home: its store is
    // not the one the project was worked from, so every call is refused by
    // name, and no answer of that store is taken for the project's.
    let other = home();
    for args in [
        &["tool", "search"][..],
        &["tool", "call", READ, "--input", ORDER_INPUT][..],
        &["tool", "describe", READ][..],
    ] {
        let (code, refused) = desk.run_with(&other, true, args).await;
        assert_eq!(code, Some(1), "{refused}");
        assert_eq!(
            refused["error"]["code"], "project_of_another_store",
            "{refused}"
        );
        assert!(
            refused["error"]["fix"]
                .as_str()
                .unwrap()
                .contains(".anything2mcp/engine"),
            "{refused}"
        );
    }
    // The engine the project was worked from still answers it.
    let listed = desk.ok(&["tool", "search"]).await;
    assert!(listed.to_string().contains(READ), "{listed}");

    // Nothing wrote to the application.
    assert!(app.writes().await.unwrap().is_empty());
    let (code, _) = desk.run(&["engine", "stop"]).await;
    assert_eq!(code, Some(0));
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_web_scout_takes_a_redirect_as_its_page_and_tries_its_reads_on_a_key_off_a_list() {
    let app = TestApp::start().await.unwrap();
    let desk = Desk::new();
    let (code, made) = desk.run_with(&desk.home, true, &["init"]).await;
    assert_eq!(code, Some(0), "{made}");
    let (code, added) = desk
        .run_with(
            &desk.home,
            true,
            &[
                "target",
                "add",
                "example",
                "--address",
                &app.url(""),
                "--kind",
                "web",
            ],
        )
        .await;
    assert_eq!(code, Some(0), "{added}");
    let report = desk.scout(true).await;
    let content = &report["result"]["content"];

    // The application's `/` redirects to `/orders`: that one page is one
    // screen at `/orders`, never a screen at `/{key}` with a key input.
    let screens = desk
        .project
        .path()
        .join("targets")
        .join("example")
        .join("screens");
    let mut urls = Vec::new();
    for entry in std::fs::read_dir(&screens).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        assert!(!text.contains("{key}"), "{text}");
        if let Some(line) = text
            .lines()
            .find(|line| line.trim_start().starts_with("url:"))
        {
            urls.push(line.trim().to_owned());
        }
    }
    assert!(urls.contains(&"url: \"/orders\"".to_owned()), "{urls:?}");

    // Every drafted read is tried and proven: the list with no input, and
    // the reads by key on the first key the list answered, under its
    // column `order`. The key is kept only in the trial's own call.
    let tools = content["tried"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 3, "{report}");
    for tool in tools {
        assert_eq!(tool["state"], "proven", "{tool}");
    }
    let by_key: Vec<&Json> = tools
        .iter()
        .filter(|tool| tool.get("input_from").is_some())
        .collect();
    assert_eq!(by_key.len(), 2, "{report}");
    for tool in by_key {
        assert_eq!(tool["input_from"]["output"], "order", "{tool}");
    }
    for tool in tools {
        assert!(tool["call"]["input"].get("order_id").is_none(), "{tool}");
    }
    assert_eq!(content["tried"]["counts"]["proven"], json!(3), "{report}");
    assert!(app.writes().await.unwrap().is_empty());
}
