//! A web application behind its own sign-in page, signed in to once
//! through the product, and then read through its pages (package 2.3).
//!
//! The agent asks for the sign-in with `session.login`, naming only the
//! target. The person is asked on the engine's own page, which asks for
//! each credential the provider `form_login` uses, the username and the
//! password, and shows neither back. The person types both into that page
//! and presses Carry out; the session keeper signs in on the application's
//! own sign-in page, in a browser of the engine's own; and a `ui` read of
//! the signed-in orders page then returns what the application holds. The
//! application's control interface says that it was sent one sign-in form,
//! and the password is in no command's output and no file under the
//! engine's home or the project, nor is the application's session cookie.
//!
//! The engine is the same `LocalEngine` that `serve` starts, run in this
//! process because the program ships no presence prompt a test can pass
//! (as in `first_hour_signed_in.rs`). The sign-in is declared by the files
//! of a version the person publishes first: a stored password is filled in
//! only on a sign-in page a published version declares (decision 0245).
//!
//! Not here: a sign-in the person carries out in a window of the engine's
//! own browser through the product. The provider `window_login` asks the
//! person, through the keeper, whether they are there before it opens the
//! window, and the person's page on one machine answers only a request for
//! a one-time code; it is proven in the web pack's own tests, with the
//! keeper driven directly.

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
use common::{
    Folder, IN_PROCESS_VARIABLE, PASSPHRASE, Surface, USER, copy_changed, project_source,
};
use example_app::harness::{PASSWORD, TestApp, USERNAME};
use serde_json::Value as Json;

/// The read the agent makes once the person is signed in.
const READ: &str = "example_order_list";
/// What every session cookie of the test application starts with.
const COOKIE_PREFIX: &str = "example-session-";

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

/// The person's machine, and every text a command printed.
struct Desk {
    home: Folder,
    project: Folder,
    printed: std::sync::Mutex<Vec<String>>,
}

impl Desk {
    /// A machine whose project holds the target files of `signed_in_pages`,
    /// with the surface at the address of `app`.
    fn new(app: &TestApp) -> Self {
        let home = Folder::new("home");
        let installation = Paths::under(home.path()).installation_config;
        std::fs::create_dir_all(&installation).unwrap();
        std::fs::write(
            installation.join("settings.toml"),
            "[vault]\nbackend = \"passphrase\"\n",
        )
        .unwrap();
        let project = Folder::new("signed-in-pages");
        let origin = app.url("");
        copy_changed(
            &project_source("signed_in_pages"),
            project.path(),
            &[("http://127.0.0.1:4010", origin.trim_end_matches('/'))],
        );
        Self {
            home,
            project,
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

    /// The engine as `serve` starts it in this project, with `prompt` as
    /// the person's presence prompt.
    async fn engine(&self, prompt: &Arc<Passed>) -> LocalEngine {
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
        LocalEngine::start(options, Arc::clone(prompt) as Arc<dyn PresencePrompt>)
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

/// The page's one-time value.
fn once_of(page: &str) -> String {
    let marker = "name=\"once\" value=\"";
    let from = page.find(marker).unwrap() + marker.len();
    page[from..from + page[from..].find('"').unwrap()].to_owned()
}

/// Every file under a folder, as text; a file that is not text is read
/// lossily, which keeps any text inside it.
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_person_signs_in_on_the_engines_page_by_form_once_and_a_ui_read_then_works() {
    let app = TestApp::start().await.unwrap();
    let desk = Desk::new(&app);
    let prompt = Arc::new(Passed(AtomicU32::new(0)));
    let local = desk.engine(&prompt).await;
    let serving = tokio::spawn(local.serve());

    let http = browser();
    // The target is published first, by the person on the engine's page: a
    // stored password is filled in only on a sign-in page a published
    // version declares (decision 0245).
    let (code, asked) = desk
        .run(&["publish", "example", "--version", "1.0.0"])
        .await;
    assert_eq!(code, Some(3), "{asked}");
    let link = asked["result"]["link"].as_str().unwrap().to_owned();
    let text = http.get(&link).send().await.unwrap().text().await.unwrap();
    let origin = &link[..link.find("/act/").unwrap()];
    let said = http
        .post(format!("{link}/confirm"))
        .header("origin", origin)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("once={}", once_of(&text)))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(said.contains("was carried out as you, once"), "{said}");
    assert_eq!(prompt.0.load(Ordering::SeqCst), 1);

    // Before anyone signs in, the read stops by name: nothing is stored
    // to sign in with, and the application is sent no sign-in form.
    let call = ["call", READ, "--input", "{}"];
    let (code, refused) = desk.run(&call).await;
    assert_eq!(code, Some(1), "{refused}");
    assert_eq!(refused["error"]["code"], "credential_missing", "{refused}");
    assert_eq!(app.auth_report().await.unwrap().sign_in_attempts, 0);

    // The agent asks for the sign-in, naming the target and no secret. The
    // person is asked on the engine's own page.
    let login = ["session", "login", "example"];
    let (code, asked) = desk.run(&login).await;
    assert_eq!(code, Some(3), "{asked}");
    assert_eq!(asked["result"]["state"], "needs_person", "{asked}");
    let link = asked["result"]["link"].as_str().unwrap().to_owned();
    let page = http.get(&link).send().await.unwrap();
    assert_eq!(page.status().as_u16(), 200);
    let text = page.text().await.unwrap();
    assert!(text.contains("<code>session.login</code>"), "{text}");
    assert!(text.contains("form_login"), "{text}");
    // A field for each credential the provider uses, empty, and the
    // reference each is sealed under.
    for (role, reference) in [
        ("user", "vault://example/app/user"),
        ("password", "vault://example/app/password"),
    ] {
        assert!(
            text.contains(&format!("name=\"secret.{role}\" type=\"password\"")),
            "{text}"
        );
        assert!(text.contains(reference), "{text}");
    }
    assert!(!text.contains(PASSWORD), "{text}");
    assert!(!text.contains(USERNAME), "{text}");

    // The person types the username and the password into the page's
    // form and presses Carry out; the presence prompt is passed.
    let origin = &link[..link.find("/act/").unwrap()];
    let pressed = http
        .post(format!("{link}/confirm"))
        .header("origin", origin)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!(
            "once={}&secret.user={USERNAME}&secret.password={PASSWORD}",
            once_of(&text),
        ))
        .send()
        .await
        .unwrap();
    let status = pressed.status().as_u16();
    let said = pressed.text().await.unwrap();
    assert_eq!(status, 200, "{said}");
    assert!(said.contains("was carried out as you, once"), "{said}");
    assert!(!said.contains(PASSWORD), "{said}");
    assert_eq!(prompt.0.load(Ordering::SeqCst), 2);

    // The agent's same call is told a session holds. The provider saw the
    // application's own sign that the sign-in worked, so the level is
    // `confirmed`.
    let signed = desk.ok(&login).await;
    let result = &signed["result"];
    assert_eq!(result["state"], "signed_in", "{signed}");
    assert_eq!(result["surface"], "app", "{signed}");
    assert_eq!(result["level"], "confirmed", "{signed}");

    // The application was sent one sign-in form, and it ended in a
    // session.
    let report = app.auth_report().await.unwrap();
    assert_eq!(report.sign_in_attempts, 1, "{report:?}");
    assert_eq!(report.sign_ins, 1, "{report:?}");

    // The `ui` read of the signed-in orders page returns what the
    // application holds, under the session, and signs in no more.
    let read = desk.ok(&call).await;
    assert_eq!(read["result"]["status"], "completed", "{read}");
    let values = &read["result"]["result"];
    let held = app.stored_order("A-100").await.unwrap().unwrap();
    let orders = values["order_ids"].as_array().unwrap();
    let at = orders
        .iter()
        .position(|order| order == "A-100")
        .unwrap_or_else(|| panic!("{read}"));
    assert_eq!(values["customers"][at], held.customer.as_str(), "{read}");
    let report = app.auth_report().await.unwrap();
    assert_eq!(report.sign_in_attempts, 1, "{report:?}");
    assert!(app.writes().await.unwrap().is_empty());

    // The password and the application's session cookie are in nothing a
    // command printed, and in no file of the engine's home or of the
    // project: no store row, event, log, trace, or browser profile.
    let (code, _) = desk.run(&["engine", "stop"]).await;
    assert_eq!(code, Some(0));
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
    for printed in desk.printed.lock().unwrap().iter() {
        assert!(!printed.contains(PASSWORD), "{printed}");
        assert!(!printed.contains(COOKIE_PREFIX), "{printed}");
    }
    let mut files = Vec::new();
    texts_under(desk.home.path(), &mut files);
    texts_under(desk.project.path(), &mut files);
    assert!(!files.is_empty());
    for (path, text) in &files {
        assert!(!text.contains(PASSWORD), "{}", path.display());
        assert!(!text.contains(COOKIE_PREFIX), "{}", path.display());
    }
}
