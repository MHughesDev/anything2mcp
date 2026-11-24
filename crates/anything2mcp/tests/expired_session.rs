//! Package 2.4 through the product: a session the system ended returns
//! `needs_login`, and nothing signs in in the person's place.
//!
//! The person signs in to the test application in a window of the engine's
//! own browser (the provider `window_login`), and a `ui` read works. The
//! application then ends every session, as an idle timeout does. The next
//! read meets the application's own sign-in page in place of its orders;
//! the keeper checks the session with the target's probe, a read run
//! through the engine, which meets the same page; and since a sign-in in a
//! window is the person's alone, the read rests at `needs_login` at once.
//! The application's control interface shows that no sign-in was sent
//! after the person's own.
//!
//! A window needs a screen: the engine runs in a child process of this test
//! binary on a virtual display, as in `signed_in_web_read.rs`. Where no
//! screen can be had the test says so and passes, unless
//! `A2M_REQUIRE_DESKTOP=1`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::process::{Command, Output};
use std::sync::Arc;

use a2m_actions::{BoxFuture, Paths, PersonSurface};
use a2m_local::{Availability, PresencePrompt, PromptAnswer, PromptName, PromptText, StopReason};
use a2m_pack_web_browser::test_support::{Screen, screen_required};
use anything2mcp::{EngineOptions, LocalEngine, PASSPHRASE_VARIABLE};
use common::{
    Folder, IN_PROCESS_VARIABLE, PASSPHRASE, Surface, USER, copy_changed, project_source,
};
use example_app::auth::AuthSettings;
use example_app::harness::{PASSWORD, TestApp};
use serde_json::Value as Json;

/// The read the agent makes.
const READ: &str = "example_order_list";

/// The sign-in of the project in a window of the engine's own browser.
const WINDOW_SIGN_IN: &str = "\
auth:
  - surfaces: [app]
    mode: custom
    provider: window_login
    probe: example_account_get
    expiry_signals:
      - {screen: sign_in}
    settings:
      screen: sign_in
      signed_in: orders
      wait_ms: 60000
    attempt_limit: 1
";

/// The name of the test that runs in the child process below.
const CHILD: &str = "an_expired_session_returns_needs_login_on_the_display_it_is_given";

/// A presence prompt that a person passes.
struct Passed;

impl PresencePrompt for Passed {
    fn available(&self) -> BoxFuture<'_, Availability> {
        Box::pin(async { Availability::ready(PromptName::Polkit, "The prompt of a test.") })
    }

    fn ask(
        &self,
        _text: PromptText,
        _deadline: tokio::time::Instant,
    ) -> BoxFuture<'_, PromptAnswer> {
        Box::pin(async { PromptAnswer::Present(PromptName::Polkit) })
    }
}

/// The person's machine, and every text a command printed.
struct Desk {
    home: Folder,
    project: Folder,
    printed: std::sync::Mutex<Vec<String>>,
}

impl Desk {
    /// A machine whose project holds the target files of `signed_in_pages`
    /// with the sign-in in a window, at the address of `app`. A call waits
    /// for no sign-in: one that needs a person returns at once.
    fn new(app: &TestApp) -> Self {
        let home = Folder::new("home");
        let installation = Paths::under(home.path()).installation_config;
        std::fs::create_dir_all(&installation).unwrap();
        std::fs::write(
            installation.join("settings.toml"),
            "[vault]\nbackend = \"passphrase\"\n\n[sessions]\nlogin_wait_minutes = 0\n",
        )
        .unwrap();
        let project = Folder::new("expired-session");
        let origin = app.url("");
        copy_changed(
            &project_source("signed_in_pages"),
            project.path(),
            &[("http://127.0.0.1:4010", origin.trim_end_matches('/'))],
        );
        std::fs::write(
            project.path().join("targets/example/auth.yaml"),
            WINDOW_SIGN_IN,
        )
        .unwrap();
        Self {
            home,
            project,
            printed: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Runs the real program in the project folder, as an agent runs it,
    /// to its end; gives its exit code and its object.
    async fn run(&self, args: &[&str]) -> (Option<i32>, Json) {
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

    /// The state of the person's one session, once it no longer signs in,
    /// as `session status` shows it. It waits for that, for a minute at
    /// most.
    async fn state_once_signed_in(&self) -> String {
        for _ in 0..120 {
            let (_, status) = self.run(&["session", "status"]).await;
            let state = status["result"]["sessions"][0]["state"]
                .as_str()
                .unwrap_or("absent")
                .to_owned();
            if !matches!(state.as_str(), "absent" | "signing_in") {
                return state;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        panic!("the sign-in in the window did not end");
    }

    /// The engine as `serve` starts it in this project.
    async fn engine(&self) -> LocalEngine {
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
        LocalEngine::start(options, Arc::new(Passed) as Arc<dyn PresencePrompt>)
            .await
            .unwrap()
    }
}

/// An HTTP client for the engine's own page, as the person's browser.
fn browser() -> reqwest::Client {
    a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
        .no_proxy()
        .build()
        .unwrap()
}

/// Presses Carry out on the engine's page behind `link`.
async fn carry_out(http: &reqwest::Client, link: &str) -> String {
    let text = http.get(link).send().await.unwrap().text().await.unwrap();
    let marker = "name=\"once\" value=\"";
    let from = text.find(marker).unwrap() + marker.len();
    let once = &text[from..from + text[from..].find('"').unwrap()];
    let origin = &link[..link.find("/act/").unwrap()];
    http.post(format!("{link}/confirm"))
        .header("origin", origin)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("once={once}"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap()
}

#[test]
fn an_expired_session_returns_needs_login() {
    let screen = match Screen::start() {
        Ok(screen) => screen,
        Err(why) if !screen_required() => {
            eprintln!("NOT RUN, no screen on this machine: {why}");
            return;
        }
        Err(why) => panic!("no screen, and A2M_REQUIRE_DESKTOP=1 says one must be had: {why}"),
    };
    let mut child = Command::new(std::env::current_exe().unwrap());
    child.args([
        CHILD,
        "--exact",
        "--ignored",
        "--test-threads=1",
        "--nocapture",
    ]);
    child.env_remove("WAYLAND_DISPLAY");
    match screen.display() {
        Some(display) => child.env("DISPLAY", display),
        None => child.env_remove("DISPLAY"),
    };
    let output = child.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "the child did not finish:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Run only by the test above, in a child process that has a display.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run by `an_expired_session_returns_needs_login` in a child process"]
async fn an_expired_session_returns_needs_login_on_the_display_it_is_given() {
    let app = TestApp::start().await.unwrap();
    // The page of the application types the person's username and password
    // as a person at the window would.
    app.set_auth_settings(&AuthSettings {
        person_at_the_window: true,
        ..AuthSettings::default()
    })
    .await
    .unwrap();
    let desk = Desk::new(&app);
    let local = desk.engine().await;
    let serving = tokio::spawn(local.serve());
    let http = browser();

    // Published by the person, signed in by the person in the window, and
    // read.
    let (code, asked) = desk
        .run(&["publish", "example", "--version", "1.0.0"])
        .await;
    assert_eq!(code, Some(3), "{asked}");
    let said = carry_out(&http, asked["result"]["link"].as_str().unwrap()).await;
    assert!(said.contains("was carried out as you, once"), "{said}");
    let login = ["session", "login", "example"];
    let (code, asked) = desk.run(&login).await;
    assert_eq!(code, Some(3), "{asked}");
    let said = carry_out(&http, asked["result"]["link"].as_str().unwrap()).await;
    assert!(said.contains("was carried out as you, once"), "{said}");
    // A call here waits for no sign-in, so the test waits for the one in
    // the window to end, by the session's own state.
    assert_eq!(desk.state_once_signed_in().await, "valid");
    let call = ["call", READ, "--input", "{}"];
    let (code, read) = desk.run(&call).await;
    assert_eq!(code, Some(0), "{read}");
    assert_eq!(read["result"]["status"], "completed", "{read}");
    let before = app.auth_report().await.unwrap();
    assert_eq!(before.sign_ins, 1, "{before:?}");

    // Nobody is at the window any more, and the application ends every
    // session, as its idle timeout would.
    app.set_auth_settings(&AuthSettings::default())
        .await
        .unwrap();
    app.expire_sessions().await.unwrap();

    // The next read rests at `needs_login`, and says to sign in.
    let (code, rested) = desk.run(&call).await;
    assert_eq!(code, Some(3), "{rested}");
    assert_eq!(rested["result"]["status"], "needs_login", "{rested}");
    assert_eq!(desk.state_once_signed_in().await, "needs_person");

    // Nothing was sent to sign in after the person's own sign-in.
    let after = app.auth_report().await.unwrap();
    assert_eq!(after.sign_in_attempts, before.sign_in_attempts, "{after:?}");
    assert_eq!(after.sign_ins, 1, "{after:?}");
    assert!(app.writes().await.unwrap().is_empty());

    let (code, _) = desk.run(&["engine", "stop"]).await;
    assert_eq!(code, Some(0));
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
    for printed in desk.printed.lock().unwrap().iter() {
        assert!(!printed.contains(PASSWORD), "{printed}");
    }
    drop(desk.home);
}
