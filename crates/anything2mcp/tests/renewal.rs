//! Package 2.5 through the product: renewal, with one renewer.
//!
//! The person signs in once to the test application's own sign-in page, on
//! the engine's page, with the provider `form_login` (decision 0245). The
//! application then ends every session, as an idle timeout does, and ten
//! reads are made at once. Each meets the application's sign-in page; the
//! session keeper, the one renewer, signs in again once with what the
//! person stored, and every read completes under the new session. The
//! application counts one sign-in form more, not ten.
//!
//! A second test stores a wrong password. The keeper sends it once; the
//! application refuses it and counts one failure; every read after it is
//! told to have the person sign in, and nothing is sent again, so the
//! account, which the application locks after five failures in a row, is
//! not locked.
//!
//! The engine is the same `LocalEngine` that `serve` starts, run in this
//! process because the program ships no presence prompt a test can pass
//! (as in `signed_in_web_read.rs`).

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
use anything2mcp::{EngineOptions, LocalEngine, PASSPHRASE_VARIABLE};
use common::{
    Folder, IN_PROCESS_VARIABLE, PASSPHRASE, Surface, USER, copy_changed, project_source,
};
use example_app::harness::{LOCKOUT_AFTER, PASSWORD, TestApp, USERNAME};
use serde_json::Value as Json;

/// The read the agent makes once the person is signed in.
const READ: &str = "example_order_list";
/// How many reads meet the ended session at once.
const AT_ONCE: usize = 10;

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
    /// A machine whose project holds the target files of `signed_in_pages`,
    /// with the surface at the address of `app`.
    fn new(app: &TestApp) -> Arc<Self> {
        let home = Folder::new("home");
        let installation = Paths::under(home.path()).installation_config;
        std::fs::create_dir_all(&installation).unwrap();
        std::fs::write(
            installation.join("settings.toml"),
            "[vault]\nbackend = \"passphrase\"\n",
        )
        .unwrap();
        let project = Folder::new("renewal");
        let origin = app.url("");
        copy_changed(
            &project_source("signed_in_pages"),
            project.path(),
            &[("http://127.0.0.1:4010", origin.trim_end_matches('/'))],
        );
        Arc::new(Self {
            home,
            project,
            printed: std::sync::Mutex::new(Vec::new()),
        })
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

    /// Runs a command to its end; gives its exit code and its object.
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

    /// The person presses Carry out on the page at `link`, with `fields`
    /// typed into its form.
    async fn carry_out(&self, link: &str, fields: &str) -> String {
        let http = browser();
        let text = http.get(link).send().await.unwrap().text().await.unwrap();
        let origin = &link[..link.find("/act/").unwrap()];
        http.post(format!("{link}/confirm"))
            .header("origin", origin)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!("once={}{fields}", once_of(&text)))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
    }

    /// The person publishes the target, and then asks for the sign-in and
    /// types `password` on the engine's page. Gives what the page said.
    async fn publish_and_sign_in(&self, password: &str) -> String {
        let (code, asked) = self
            .run(&["publish", "example", "--version", "1.0.0"])
            .await;
        assert_eq!(code, Some(3), "{asked}");
        let said = self
            .carry_out(asked["result"]["link"].as_str().unwrap(), "")
            .await;
        assert!(said.contains("was carried out as you, once"), "{said}");
        let (code, asked) = self.run(&["session", "login", "example"]).await;
        assert_eq!(code, Some(3), "{asked}");
        self.carry_out(
            asked["result"]["link"].as_str().unwrap(),
            &format!("&secret.user={USERNAME}&secret.password={password}"),
        )
        .await
    }

    /// Nothing any command printed holds `secret`.
    fn printed_nothing_of(&self, secret: &str) {
        for printed in self.printed.lock().unwrap().iter() {
            assert!(!printed.contains(secret), "{printed}");
        }
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ten_runs_meet_an_expired_session_through_the_engine_and_the_application_counts_one_sign_in()
 {
    let app = TestApp::start().await.unwrap();
    let desk = Desk::new(&app);
    let local = desk.engine().await;
    let serving = tokio::spawn(local.serve());
    let said = desk.publish_and_sign_in(PASSWORD).await;
    assert!(said.contains("was carried out as you, once"), "{said}");
    let call = ["call", READ, "--input", "{}"];
    let (code, read) = desk.run(&call).await;
    assert_eq!(code, Some(0), "{read}");
    let before = app.auth_report().await.unwrap();
    assert_eq!(
        (before.sign_in_attempts, before.sign_ins),
        (1, 1),
        "{before:?}"
    );

    // The application ends every session, as an idle timeout does, and ten
    // reads are made at once.
    app.expire_sessions().await.unwrap();
    let mut reads = Vec::new();
    for _ in 0..AT_ONCE {
        let desk = Arc::clone(&desk);
        reads.push(tokio::spawn(async move { desk.run(&call).await }));
    }
    let held = app.stored_order("A-100").await.unwrap().unwrap();
    for read in reads {
        let (code, read) = read.await.unwrap();
        assert_eq!(code, Some(0), "{read}");
        assert_eq!(read["result"]["status"], "completed", "{read}");
        let values = &read["result"]["result"];
        let orders = values["order_ids"].as_array().unwrap();
        let at = orders.iter().position(|order| order == "A-100").unwrap();
        assert_eq!(values["customers"][at], held.customer.as_str(), "{read}");
    }

    // One renewal: one sign-in form more, which ended in a session.
    let after = app.auth_report().await.unwrap();
    assert_eq!(
        after.sign_in_attempts,
        before.sign_in_attempts + 1,
        "{after:?}"
    );
    assert_eq!(after.sign_ins, before.sign_ins + 1, "{after:?}");
    assert_eq!(after.failed_sign_ins, 0, "{after:?}");
    assert!(app.writes().await.unwrap().is_empty());

    let (code, _) = desk.run(&["engine", "stop"]).await;
    assert_eq!(code, Some(0));
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
    desk.printed_nothing_of(PASSWORD);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wrong_stored_password_locks_nothing_the_stand_counts_one_failure() {
    let app = TestApp::start().await.unwrap();
    let desk = Desk::new(&app);
    let local = desk.engine().await;
    let serving = tokio::spawn(local.serve());
    let wrong = "not-the-password-for-tests";
    // The page tells the person it did not succeed, and shows nothing of
    // what they typed.
    let said = desk.publish_and_sign_in(wrong).await;
    assert!(said.contains("did not succeed"), "{said}");
    assert!(!said.contains(wrong), "{said}");
    let refused = app.auth_report().await.unwrap();
    assert_eq!(refused.sign_in_attempts, 1, "{refused:?}");
    assert_eq!(refused.failed_sign_ins, 1, "{refused:?}");

    // More reads than the application's lockout: each is told the person
    // must sign in, and the refused password is not sent again.
    let call = ["call", READ, "--input", "{}"];
    for _ in 0..=LOCKOUT_AFTER {
        let (code, read) = desk.run(&call).await;
        assert_ne!(code, Some(0), "{read}");
        let said = read.to_string();
        assert!(
            said.contains("needs_login") || said.contains("credential_refused"),
            "{read}"
        );
    }
    let (_, login) = desk.run(&["session", "login", "example"]).await;
    assert_ne!(login["result"]["state"], "signed_in", "{login}");

    let after = app.auth_report().await.unwrap();
    assert_eq!(after.sign_in_attempts, 1, "{after:?}");
    assert_eq!(after.failed_sign_ins, 1, "{after:?}");
    assert_eq!(after.sign_ins, 0, "{after:?}");
    assert!(!after.locked, "{after:?}");

    let (code, _) = desk.run(&["engine", "stop"]).await;
    assert_eq!(code, Some(0));
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
    desk.printed_nothing_of(wrong);
}
