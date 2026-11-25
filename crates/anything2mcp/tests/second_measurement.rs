//! The faults the second measurement of the first hour found (H-18's
//! result, the second measurement), each through the product's commands,
//! as an agent with no knowledge of the repository met it (package 5.6c),
//! on the web scout of the test application that the measurement used:
//!
//! - `ready` with every gate passed and only the person's press left
//!   answers `ok: true`, exit code 3, with `state: needs_person`, and its
//!   text says first that it succeeded;
//! - `ready --tools` publishes only the read named, and says so;
//! - the scout report lists each finding not drafted once, with a reason;
//! - `next` after the scout, and from `target status`, is `ready`, and
//!   `next` after `ready` is the call of the read it named;
//! - `tool describe` gives an input that holds every required input;
//! - the guidance lists the report's part names, and the refusal of a
//!   wrong one names them;
//! - `init --help` says what `init` does, and the guidance does not say
//!   that it has been run.
//!
//! Every command is the real program, and the engine is the `LocalEngine`
//! that `serve` starts, run in this process so that its presence prompt
//! can be passed; the person's press goes through its page.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use a2m_actions::{BoxFuture, Paths, PersonSurface};
use a2m_local::{Availability, PresencePrompt, PromptAnswer, PromptName, PromptText, StopReason};
use anything2mcp::{EngineOptions, LocalEngine, PASSPHRASE_VARIABLE};
use common::{Folder, IN_PROCESS_VARIABLE, PASSPHRASE, Surface, USER};
use example_app::harness::TestApp;
use serde_json::{Value as Json, json};

/// The read of one order the agent wants, and the only one it publishes.
const READ: &str = "example_orders_get";
/// An order the test application holds from its seed.
const ORDER: &str = "A-101";

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
            project: Folder::new("second-project"),
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
async fn the_faults_of_the_second_measurement_are_met_no_more_through_the_commands() {
    let app = TestApp::start().await.unwrap();
    let desk = Desk::new();

    // 7. `init --help` says what it does; the guidance does not say it
    // has been run, and says how to start.
    let help = desk.help(&["init"]);
    assert!(!help.contains("No action is beneath it yet"), "{help}");
    assert!(
        help.contains("a project")
            && help.contains("AGENTS.md")
            && help.contains("anything2mcp.toml"),
        "{help}"
    );
    let (code, guidance) = desk
        .run_with(&desk.home, true, &["docs", "get", "guidance"])
        .await;
    assert_eq!(code, Some(0), "{guidance}");
    let guidance = guidance.to_string();
    assert!(
        !guidance.contains("made the project folder and wrote this file"),
        "{guidance}"
    );
    assert!(
        guidance.contains("Start with `anything2mcp init`"),
        "{guidance}"
    );
    // 6. It lists the report's part names.
    for part in [
        "tried.tools",
        "drafted.not_drafted",
        "checked.tools",
        "not_looked_at",
    ] {
        assert!(guidance.contains(&format!("`{part}`")), "{part}");
    }

    desk.ok(&["init"]).await;
    let prompt = Arc::new(Passed(AtomicU32::new(0)));
    let serving = desk.serve(&prompt).await;
    desk.ok(&["add", "example", "--address", &app.url(""), "--kind", "web"])
        .await;
    desk.scout(false).await;
    let report = desk.ok(&["scout", "report", "example"]).await;
    let content = &report["result"]["content"];
    assert_eq!(content["tried"]["counts"]["proven"], json!(3), "{report}");

    // 3. Each finding not drafted is listed once, with a reason.
    // The page's own request, `GET /orders` answered with a document, is
    // covered by two rules of the web pack and drafted by neither.
    let not_drafted = content["drafted"]["not_drafted"].as_array().unwrap();
    assert!(!not_drafted.is_empty(), "{report}");
    let mut seen = Vec::new();
    for left in not_drafted {
        let id = left["finding"].as_str().unwrap();
        assert!(!seen.contains(&id), "listed twice: {left}");
        seen.push(id);
        let reason = left["reason"].as_str().unwrap();
        assert_ne!(reason, "the rule gave no reason", "{left}");
        assert!(reason.len() > 40, "{left}");
    }

    // 4. After the scout proved its reads, `next` is `ready`, from the
    // report and from the target's status alike.
    assert_eq!(report["next"]["action"], "draft.ready", "{report}");
    let status = desk.ok(&["target", "status", "example"]).await;
    assert_eq!(status["next"]["action"], "draft.ready", "{status}");

    // 6. A part named as the agent named it is refused, naming the parts.
    let (code, refused) = desk
        .run(&["scout", "report", "example", "--part", "tried"])
        .await;
    assert_eq!(code, Some(2), "{refused}");
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("tried.tools"),
        "{refused}"
    );

    // 1 and 2. `ready` of the one read wanted: every step before the
    // publish succeeded, so the answer is `ok`, waiting for a person, and
    // its text says so first. The version asked for holds that read alone.
    let (code, asked) = desk.run(&["ready", "example", "--tools", READ]).await;
    assert_eq!(code, Some(3), "{asked}");
    assert_eq!(asked["ok"], true, "{asked}");
    assert!(asked.get("error").is_none(), "{asked}");
    let result = &asked["result"];
    assert_eq!(result["state"], "needs_person", "{asked}");
    assert!(
        result["text"].as_str().unwrap().starts_with("Succeeded:"),
        "{asked}"
    );
    assert_eq!(result["publish"]["state"], "needs_person", "{asked}");
    assert_eq!(result["publish"]["tools"], json!([READ]), "{asked}");
    // 4. `next` is the call of the read it named, with its required input.
    assert_eq!(asked["next"]["action"], "tool.call", "{asked}");
    assert_eq!(asked["next"]["args"]["tool"], READ, "{asked}");
    assert!(
        asked["next"]["args"]["input"].get("order_id").is_some(),
        "{asked}"
    );
    let link = result["publish"]["link"].as_str().unwrap().to_owned();
    Desk::press(&link).await;

    // Asked again, it says the version is published, holding the one read.
    let again = desk.ok(&["ready", "example", "--tools", READ]).await;
    assert_eq!(again["result"]["state"], "published", "{again}");
    assert_eq!(
        again["result"]["publish"]["tools"],
        json!([READ]),
        "{again}"
    );
    let found = desk.ok(&["tool", "search", "order"]).await;
    let published: Vec<&str> = found["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(published, [READ], "{found}");
    // The other drafts stay in the folder, untouched.
    let operations = desk
        .project
        .path()
        .join("targets")
        .join("example")
        .join("operations");
    assert_eq!(std::fs::read_dir(&operations).unwrap().count(), 3);

    // 5. `tool describe` gives an input that holds every required input.
    let described = desk.ok(&["tool", "describe", READ]).await;
    let required = described["result"]["input_schema"]["required"]
        .as_array()
        .unwrap()
        .clone();
    assert!(required.contains(&json!("order_id")), "{described}");
    let input = &described["next"]["args"]["input"];
    for name in &required {
        assert!(input.get(name.as_str().unwrap()).is_some(), "{described}");
    }

    // The read, called with an order of the application's, answers what
    // it holds.
    let called = desk
        .ok(&[
            "call",
            READ,
            "--input",
            &json!({"order_id": ORDER}).to_string(),
        ])
        .await;
    assert_eq!(called["result"]["status"], "completed", "{called}");
    let held = app.stored_order(ORDER).await.unwrap().unwrap();
    assert_eq!(
        called["result"]["result"]["customer"],
        held.customer.as_str(),
        "{called}"
    );
    assert_eq!(prompt.0.load(Ordering::SeqCst), 1);
    assert!(app.writes().await.unwrap().is_empty());
    let (code, _) = desk.run(&["engine", "stop"]).await;
    assert_eq!(code, Some(0));
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
}
