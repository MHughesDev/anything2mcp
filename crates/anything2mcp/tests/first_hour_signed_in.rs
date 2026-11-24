//! The first hour for a system that needs a sign-in, end to end, through
//! the command face only: a developer with an empty folder and the part of
//! the built-in test application that takes a bearer token ends with a
//! published read that an agent calls under the developer's own session.
//!
//! `init`; the target added with its address and kind; a first scout,
//! whose drafted reads the system refuses for want of a credential; the
//! sign-in declared in `auth.yaml` through `draft.write`, which names the
//! provider and a vault reference and holds no secret; the scout again,
//! which stops by name before it drafts, because the surface is observed
//! only under a session nobody holds; `session.login` asked by the agent
//! and carried out by the person on the engine's own page, where the token
//! is typed into the page's form, never into an action's input; the scout
//! once more, the same scout, now under the session; the drafts not wanted
//! removed; the read's offline test recorded from one signed-in read; its
//! binding promoted; the publish carried out by the person; and the tool
//! called under the session. No file is written by hand.
//!
//! The token appears in no action's result, no event, no log, no file
//! under the engine's home, and no file of the project. The engine is the
//! same `LocalEngine` that `serve` starts, run in this process because the
//! program ships no presence prompt a test can pass (as in
//! `first_hour.rs`). The test stands in for the person exactly as the
//! publish step's tests do.
//!
//! The application's form sign-in with a second factor is not used here:
//! its provider, `form_login`, belongs to the web pack and is proven in
//! that pack's own tests (decision 0245).
//! The page's question for a second factor is proven in `a2m-local`.

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
use common::prefixed::Prefixed;
use common::{Folder, IN_PROCESS_VARIABLE, PASSPHRASE, Surface, USER};
use example_app::harness::{BEARER_TOKEN, TestApp};
use serde_json::{Value as Json, json};

/// The read this developer makes usable, tests, and publishes.
const READ: &str = "example_api_orders_get";
/// Its input: an order the test application holds from its seed.
const ORDER_INPUT: &str = r#"{"order_id": "A-100"}"#;
/// The drafted files the hour keeps: the read's operation, the description
/// it is pinned to in the `api` mode, the one place its request is
/// stated (decision 0399).
const KEPT: [&str; 2] = ["descriptions/http.yaml", "operations/api_orders_get.yaml"];

/// The sign-in, as the developer's agent declares it: the provider and the
/// vault reference the token is kept under. No secret is in it.
const AUTH_FILE: &str = "auth:\n  - surfaces: [http]\n    mode: custom\n    provider: bearer_static\n    probe: example_api_orders_get\n    probe_inputs: {order_id: \"A-100\"}\n    credentials:\n      token: \"vault://example/service/token\"\n    attempt_limit: 1\n";

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

/// The developer's machine, and every text a command printed.
struct Desk {
    home: Folder,
    project: Folder,
    printed: std::sync::Mutex<Vec<String>>,
}

impl Desk {
    fn new() -> Self {
        let home = Folder::new("home");
        let installation = Paths::under(home.path()).installation_config;
        std::fs::create_dir_all(&installation).unwrap();
        std::fs::write(
            installation.join("settings.toml"),
            "[vault]\nbackend = \"passphrase\"\n",
        )
        .unwrap();
        Self {
            home,
            project: Folder::new("signed-in"),
            printed: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// The real program, run in the project folder as an agent runs it.
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

    /// Runs a command to its end; gives its exit code and its object, and
    /// keeps everything it printed.
    async fn run(&self, args: &[&str]) -> (Option<i32>, Json) {
        let mut command = self.program(args);
        let done: Output = tokio::task::spawn_blocking(move || command.output().unwrap())
            .await
            .unwrap();
        let stdout = String::from_utf8_lossy(&done.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&done.stderr).into_owned();
        let object = serde_json::from_str(&stdout).unwrap_or_else(|_| {
            panic!(
                "`{}` printed no single object: {stdout} / {stderr}",
                args.join(" ")
            )
        });
        self.printed.lock().unwrap().extend([stdout, stderr]);
        (done.status.code(), object)
    }

    async fn ok(&self, args: &[&str]) -> Json {
        let (code, object) = self.run(args).await;
        assert_eq!(code, Some(0), "`{}`: {object}", args.join(" "));
        object
    }

    fn target_file(&self, file: &str) -> std::path::PathBuf {
        self.project
            .path()
            .join("targets")
            .join("example")
            .join(file)
    }

    /// The scout, continued as its `next` says, to its report.
    async fn scout(&self, fresh: bool) -> Json {
        let mut args = vec!["scout", "run", "example", "--wait-seconds", "10"];
        if fresh {
            args.push("--fresh");
        }
        let mut scouted = Json::Null;
        for _ in 0..20 {
            scouted = self.ok(&args).await;
            if scouted["result"].get("report_hash").is_some() {
                return scouted;
            }
            assert_eq!(scouted["result"]["state"], "working", "{scouted}");
            args.truncate(5);
        }
        panic!("the scout did not end: {scouted}");
    }
}

/// An HTTP client for the engine's own page, as the person's browser.
fn browser() -> reqwest::Client {
    a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
        .no_proxy()
        .build()
        .unwrap()
}

/// The page's one-time value.
fn once_of(page: &str) -> String {
    let marker = "name=\"once\" value=\"";
    let from = page.find(marker).unwrap() + marker.len();
    page[from..from + page[from..].find('"').unwrap()].to_owned()
}

/// Every file under a folder, as text.
fn texts_under(folder: &std::path::Path, out: &mut Vec<(std::path::PathBuf, String)>) {
    let Ok(listing) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in listing {
        let path = entry.unwrap().path();
        if path.is_dir() {
            texts_under(&path, out);
        } else if let Ok(bytes) = std::fs::read(&path) {
            out.push((path, String::from_utf8_lossy(&bytes).into_owned()));
        }
    }
}

/// The names of the scout's tried reads that were proven, or not.
fn tried(report: &Json, proven: bool) -> Vec<String> {
    let mut names: Vec<String> = report["result"]["content"]["tried"]["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|tool| (tool["state"] == "proven") == proven)
        .filter_map(|tool| tool["tool"].as_str().map(str::to_owned))
        .collect();
    names.sort();
    names
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_signed_in_first_hour_from_an_empty_folder_to_a_read_called_under_the_session() {
    let app = TestApp::start().await.unwrap();
    // The application's part that takes a bearer token, at an origin of
    // its own, as such a system is usually reached: the web packs' drafting
    // places a surface by its origin, and does not yet place one whose
    // address has a path.
    let relay = Prefixed::start(&app.url(""), "/secure/bearer").await;
    let desk = Desk::new();
    let address = relay.base().to_owned();
    let began = Instant::now();

    // 1. `init`, and the engine as `serve` starts it in this project.
    desk.ok(&["init"]).await;
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

    // 2. The target, with the address of the signed-in part and its kind.
    let added = desk
        .ok(&[
            "target",
            "add",
            "example",
            "--address",
            &address,
            "--kind",
            "http",
        ])
        .await;
    assert_eq!(added["next"]["action"], "scout.run", "{added}");

    // 3. A first scout. The description is served openly and its reads are
    // drafted, and the system refuses every one of them for want of a
    // credential: nothing is proven.
    let first = desk.scout(false).await;
    assert!(tried(&first, true).is_empty(), "{first}");
    assert!(!tried(&first, false).is_empty(), "{first}");
    assert_eq!(
        first["result"]["content"]["surfaces"][0]["session"]["needed"], false,
        "{first}"
    );

    // 4. The sign-in declared, by the agent, through `draft.write`: the
    // provider and the reference the token is kept under, and no secret.
    let wrote = desk
        .ok(&[
            "draft",
            "write",
            "example",
            "--file",
            "auth.yaml",
            "--content",
            AUTH_FILE,
        ])
        .await;
    assert_eq!(wrote["ok"], true, "{wrote}");

    // 5. The scout stops by name before it drafts: the surface is observed
    // only under a session, and nobody is signed in.
    let (code, stopped) = desk.run(&["scout", "run", "example", "--fresh"]).await;
    assert_eq!(code, Some(3), "{stopped}");
    assert_eq!(stopped["result"]["state"], "needs_sign_in", "{stopped}");
    assert_eq!(stopped["result"]["surface"], "http", "{stopped}");
    assert_eq!(stopped["next"]["action"], "session.login", "{stopped}");

    // 6. The agent asks for the sign-in. It names the target; no secret is
    // in its input. The person is asked on the engine's own page.
    let login = ["session", "login", "example"];
    let (code, asked) = desk.run(&login).await;
    assert_eq!(code, Some(3), "{asked}");
    assert_eq!(asked["result"]["state"], "needs_person", "{asked}");
    let link = asked["result"]["link"].as_str().unwrap().to_owned();
    let http = browser();
    let page = http.get(&link).send().await.unwrap();
    assert_eq!(page.status().as_u16(), 200);
    let text = page.text().await.unwrap();
    assert!(text.contains("<code>session.login</code>"), "{text}");
    assert!(text.contains("name=\"secret.token\""), "{text}");
    assert!(text.contains("vault://example/service/token"), "{text}");
    assert!(!text.contains(BEARER_TOKEN));
    // The person types the token into the page's form and presses Carry
    // out; the presence prompt is passed.
    let origin = &link[..link.find("/act/").unwrap()];
    let pressed = http
        .post(format!("{link}/confirm"))
        .header("origin", origin)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!(
            "once={}&secret.token={}",
            once_of(&text),
            BEARER_TOKEN
        ))
        .send()
        .await
        .unwrap();
    let status = pressed.status().as_u16();
    let said = pressed.text().await.unwrap();
    assert_eq!(status, 200, "{said}");
    assert!(said.contains("was carried out as you, once"), "{said}");
    assert!(!said.contains(BEARER_TOKEN), "{said}");
    assert_eq!(prompt.0.load(Ordering::SeqCst), 1);
    // The agent's same call is told what became of it: a session exists,
    // for whom, for which environment, and at which level. The provider
    // holds the token and does not try it, so the level is `held`.
    let signed = desk.ok(&login).await;
    let result = &signed["result"];
    assert_eq!(result["state"], "signed_in", "{signed}");
    assert_eq!(result["environment"], "test", "{signed}");
    assert_eq!(result["surface"], "http", "{signed}");
    assert_eq!(result["level"], "held", "{signed}");
    assert!(result["session"].is_string(), "{signed}");
    let sessions = desk.ok(&["session", "status", "example"]).await;
    assert_eq!(
        sessions["result"]["sessions"][0]["state"], "valid",
        "{sessions}"
    );

    // 7. The same scout goes on, under the session: its discovery and its
    // trials. The report says the surface's findings needed the session.
    let report = desk.scout(false).await;
    assert_eq!(
        report["result"]["scout_id"], stopped["result"]["scout_id"],
        "{report}"
    );
    let surface = &report["result"]["content"]["surfaces"][0];
    assert_eq!(surface["state"], "identified", "{report}");
    assert_eq!(
        surface["session"],
        json!({"needed": true, "provider": "bearer_static"}),
        "{report}"
    );
    assert!(tried(&report, true).contains(&READ.to_owned()), "{report}");

    // 8. The drafts not wanted removed, as the gates say; the read's
    // offline test recorded from one signed-in read.
    // Gates that did not pass are a failed action: exit code 1, with the
    // refusal a publish would give (decision 0219).
    let (code, untested) = desk.run(&["test", "example"]).await;
    assert_eq!(code, Some(1), "{untested}");
    assert_eq!(untested["error"]["code"], "publish_gates_not_passed");
    for entry in untested["result"]["missing"].as_array().unwrap() {
        let file = entry["file"].as_str().unwrap();
        if KEPT.contains(&file) {
            continue;
        }
        desk.ok(&["draft", "delete", "example", "--file", file])
            .await;
    }
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
    // Nothing of the session is in any byte the recording wrote.
    for file in recorded["result"]["files"].as_array().unwrap() {
        let bytes = std::fs::read(desk.target_file(file.as_str().unwrap())).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains(BEARER_TOKEN), "{file}: {text}");
        assert!(
            !text.to_ascii_lowercase().contains("bearer "),
            "{file}: {text}"
        );
    }
    let tested = desk.ok(&["test", "example"]).await;
    assert_eq!(tested["result"]["passed"], true, "{tested}");

    // 9. Promoted on the scout's proof, and published by the person.
    let promoted = desk
        .ok(&[
            "binding", "promote", "example", "--tool", READ, "--mode", "api",
        ])
        .await;
    assert_eq!(promoted["result"]["promoted"], true, "{promoted}");
    let (code, asked) = desk
        .run(&["publish", "example", "--version", "1.0.0"])
        .await;
    assert_eq!(code, Some(3), "{asked}");
    let link = asked["result"]["link"].as_str().unwrap().to_owned();
    let text = http.get(&link).send().await.unwrap().text().await.unwrap();
    // A publish's page asks for no secret.
    assert!(!text.contains("name=\"secret."), "{text}");
    let origin = &link[..link.find("/act/").unwrap()];
    let pressed = http
        .post(format!("{link}/confirm"))
        .header("origin", origin)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("once={}", once_of(&text)))
        .send()
        .await
        .unwrap();
    let said = pressed.text().await.unwrap();
    assert!(said.contains("was carried out as you, once"), "{said}");

    // 10. The published tool, called by the agent, reads under the session
    // what the application holds.
    let called = desk.ok(&["call", READ, "--input", ORDER_INPUT]).await;
    assert_eq!(called["result"]["status"], "completed", "{called}");
    let held = app.stored_order("A-100").await.unwrap().unwrap();
    assert_eq!(
        called["result"]["result"]["customer"],
        held.customer.as_str(),
        "{called}"
    );
    // No token was refused: every read the system was sent under the
    // session carried the right one, and none was sent without it once
    // the sign-in was declared.
    assert!(app.writes().await.unwrap().is_empty());

    let elapsed = began.elapsed();

    // The person's session ended on purpose: a run is then told to sign in,
    // and nothing signs in behind the person's back, though the token is
    // stored. Asked again, the sign-in needs no page: the token is stored
    // and the provider needs no person.
    let session = result["session"].as_str().unwrap().to_owned();
    let revoked = desk.ok(&["session", "revoke", &session]).await;
    assert_eq!(revoked["result"]["state"], "revoked", "{revoked}");
    let (code, waits) = desk.run(&["call", READ, "--input", ORDER_INPUT]).await;
    assert_eq!(code, Some(3), "{waits}");
    assert_eq!(waits["result"]["status"], "needs_login", "{waits}");
    assert_eq!(waits["next"]["action"], "run.resume", "{waits}");
    let waiting = waits["result"]["run_id"].as_str().unwrap().to_owned();
    let again = desk.ok(&login).await;
    assert_eq!(again["result"]["state"], "signed_in", "{again}");
    // The same session of the same person, signed in anew.
    assert_eq!(again["result"]["session"], json!(session), "{again}");
    assert_eq!(
        prompt.0.load(Ordering::SeqCst),
        2,
        "no page was pressed for it"
    );
    // The run that waited goes on under the new sign-in.
    let resumed = desk
        .ok(&["run", "resume", &waiting, "--wait-seconds", "30"])
        .await;
    assert_eq!(resumed["result"]["status"], "completed", "{resumed}");
    println!(
        "the signed-in first hour, from an empty folder to a read called under the session, took {:.1} s",
        elapsed.as_secs_f64()
    );
    assert!(elapsed < Duration::from_secs(600), "{elapsed:?}");

    // The token is in nothing a command printed, and in no file of the
    // engine's home or of the project: no store row, event, log, trace,
    // fixture, or draft.
    let (code, _) = desk.run(&["engine", "stop"]).await;
    assert_eq!(code, Some(0));
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
    for printed in desk.printed.lock().unwrap().iter() {
        assert!(!printed.contains(BEARER_TOKEN), "{printed}");
    }
    let mut files = Vec::new();
    texts_under(desk.home.path(), &mut files);
    texts_under(desk.project.path(), &mut files);
    assert!(!files.is_empty());
    for (path, text) in &files {
        assert!(!text.contains(BEARER_TOKEN), "{}", path.display());
    }
}
