//! Faults of a person's confirmation of an asserted read (decision 0999),
//! through the product, against the test application's case 39, whose
//! cases page asks for its rows with `POST /api/cases/rows/search`.
//!
//! An agent's ask stores nothing, and a prompt the person does not pass
//! confirms nothing; a press after the drafts changed is refused, and stores
//! nothing; and a confirmation made for one body, or for one address,
//! covers the request no longer once the body or the surface's address
//! changes. Each time the try under the drafts stops, and the route hears no
//! search it did not let through.
//!
//! The engine is the same `LocalEngine` that `serve` starts, run in this
//! process because the program ships no presence prompt a test can pass;
//! every command is the real program. It needs a browser, as the web
//! pack's tests find one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use a2m_actions::{BoxFuture, Paths, PersonSurface};
use a2m_local::{Availability, PresencePrompt, PromptAnswer, PromptName, PromptText, StopReason};
use anything2mcp::{EngineOptions, LocalEngine, PASSPHRASE_VARIABLE};
use common::{
    Folder, IN_PROCESS_VARIABLE, PASSPHRASE, Surface, USER, copy_changed, project_source,
};
use example_app::harness::TestApp;
use serde_json::Value as Json;

const PLACEHOLDER: &str = "http://127.0.0.1:4010";
const LIST: &str = "example_case_list";
const ENTRY: &str = "targets/example/page_requests/cases_rows_search.yaml";

/// A presence prompt that a person passes, or declines while `declines`
/// is set, and that counts.
struct Prompt {
    asked: AtomicU32,
    declines: AtomicBool,
}

impl PresencePrompt for Prompt {
    fn available(&self) -> BoxFuture<'_, Availability> {
        Box::pin(async { Availability::ready(PromptName::Polkit, "The prompt of a test.") })
    }

    fn ask(
        &self,
        _text: PromptText,
        _deadline: tokio::time::Instant,
    ) -> BoxFuture<'_, PromptAnswer> {
        Box::pin(async move {
            self.asked.fetch_add(1, Ordering::SeqCst);
            if self.declines.load(Ordering::SeqCst) {
                PromptAnswer::Denied
            } else {
                PromptAnswer::Present(PromptName::Polkit)
            }
        })
    }
}

/// The real program, run in the project folder as an agent runs it.
async fn run(home: &Path, project: &Path, args: &[&str]) -> (Option<i32>, Json) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_anything2mcp"));
    command
        .arg("--json")
        .args(args)
        .current_dir(project)
        .env_remove(PASSPHRASE_VARIABLE)
        .env("A2M_HOME", home)
        .env("USER", USER)
        .env(IN_PROCESS_VARIABLE, "true")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let done = tokio::task::spawn_blocking(move || command.output().unwrap())
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

/// The person's press on the page at `link`: the status and what the page
/// said after.
async fn press(http: &reqwest::Client, link: &str) -> (u16, String) {
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
    (status, pressed.text().await.unwrap())
}

/// Replaces `from` with `to` in the project's file `path`.
fn change(project: &Path, path: &str, from: &str, to: &str) {
    let at = project.join(path);
    let text = std::fs::read_to_string(&at).unwrap();
    assert!(text.contains(from), "{from}: {text}");
    std::fs::write(&at, text.replace(from, to)).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_asserted_read_is_tried_only_as_its_person_confirmed_it() {
    let app = TestApp::start().await.unwrap();
    let seeded = app.case_rows().await.unwrap();
    let home = Folder::new("confirm-home");
    let project = Folder::new("confirm");
    let address = app.url("").trim_end_matches('/').to_owned();
    copy_changed(
        &project_source("rows"),
        project.path(),
        &[(PLACEHOLDER, &address)],
    );
    let installation = Paths::under(home.path()).installation_config;
    std::fs::create_dir_all(&installation).unwrap();
    std::fs::write(
        installation.join("settings.toml"),
        "[vault]\nbackend = \"passphrase\"\n",
    )
    .unwrap();
    let mut options = EngineOptions::of_machine(
        Paths::under(home.path()),
        std::collections::BTreeMap::from([
            ("A2M_HOME".to_owned(), home.path().display().to_string()),
            ("USER".to_owned(), USER.to_owned()),
            (PASSPHRASE_VARIABLE.to_owned(), PASSPHRASE.to_owned()),
        ]),
        Surface::new() as Arc<dyn PersonSurface>,
    );
    options.project = Some(project.path().to_path_buf());
    let prompt = Arc::new(Prompt {
        asked: AtomicU32::new(0),
        declines: AtomicBool::new(true),
    });
    let local = LocalEngine::start(options, Arc::clone(&prompt) as Arc<dyn PresencePrompt>)
        .await
        .unwrap();
    let serving = tokio::spawn(local.serve());
    let (home_path, project_path) = (home.path(), project.path());
    let http = a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
        .no_proxy()
        .build()
        .unwrap();
    let try_list = || {
        run(
            home_path,
            project_path,
            &["draft", "try", "example", "--tool", LIST],
        )
    };
    let stops = |tried: &Json| {
        assert_eq!(tried["result"]["status"], "failed", "{tried}");
        let failure = tried["result"]["failure"].to_string() + &tried["error"].to_string();
        assert!(failure.contains("page_write_refused"), "{tried}");
    };
    let ask = || async {
        let (code, asked) = run(
            home_path,
            project_path,
            &["assertion", "confirm", "example"],
        )
        .await;
        assert_eq!(code, Some(3), "{asked}");
        assert_eq!(asked["result"]["state"], "needs_person", "{asked}");
        asked["result"]["link"].as_str().unwrap().to_owned()
    };

    // The agent asks; the person's prompt is not passed. Nothing is
    // confirmed, and the try stops.
    let link = ask().await;
    let (status, said) = press(&http, &link).await;
    assert_ne!(status, 200, "{said}");
    assert!(!said.contains("was carried out as you"), "{said}");
    assert_eq!(prompt.asked.load(Ordering::SeqCst), 1);
    stops(&try_list().await.1);
    prompt.declines.store(false, Ordering::SeqCst);

    // The page is shown, then the drafts change before the press: the
    // press is refused, and nothing is confirmed, for the old body or the
    // new.
    let link = ask().await;
    change(project_path, ENTRY, "enum: [open]", "enum: [open, closed]");
    let (_, said) = press(&http, &link).await;
    assert!(
        said.contains("changed after the page was shown") && said.contains("Nothing was confirmed"),
        "{said}"
    );
    stops(&try_list().await.1);
    change(project_path, ENTRY, "enum: [open, closed]", "enum: [open]");
    stops(&try_list().await.1);

    // Confirmed as the drafts are: the try reads the open rows.
    let link = ask().await;
    let (status, said) = press(&http, &link).await;
    assert_eq!(status, 200, "{said}");
    assert!(said.contains("was carried out as you, once"), "{said}");
    let (code, tried) = try_list().await;
    assert_eq!(code, Some(0), "{tried}");
    assert_eq!(tried["result"]["status"], "completed", "{tried}");
    let heard = app.case_rows().await.unwrap().received;
    assert_eq!(heard, vec!["search".to_owned()], "{heard:?}");

    // A confirmation for one body does not cover another: the body may now
    // carry another status, and the try stops.
    change(project_path, ENTRY, "enum: [open]", "enum: [open, closed]");
    stops(&try_list().await.1);
    change(project_path, ENTRY, "enum: [open, closed]", "enum: [open]");

    // Nor one for one address another: the same application, named by
    // another address, and the try stops.
    let other = address.replace("127.0.0.1", "localhost");
    change(
        project_path,
        "targets/example/target.yaml",
        &address,
        &other,
    );
    stops(&try_list().await.1);
    change(
        project_path,
        "targets/example/target.yaml",
        &other,
        &address,
    );

    // As confirmed, it reads again.
    let (_, tried) = try_list().await;
    assert_eq!(tried["result"]["status"], "completed", "{tried}");

    let (code, _) = run(home_path, project_path, &["engine", "stop"]).await;
    assert_eq!(code, Some(0));
    assert_eq!(serving.await.unwrap(), StopReason::Asked);

    // The route heard only the searches the two tries let through; no row
    // changed and nothing was written.
    let after = app.case_rows().await.unwrap();
    assert_eq!(after.rows, seeded.rows);
    assert_eq!(after.received, vec!["search".to_owned(); 2], "{after:?}");
    assert!(app.writes().await.unwrap().is_empty());
}
