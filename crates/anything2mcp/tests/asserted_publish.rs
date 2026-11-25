//! Package E7d through the product (decision 0394, point 5): the page a
//! person publishes from names each read a person asserted, after the
//! surfaces and before the tools, with the request, the fields its body may
//! carry, and the fixed sentence that the person's word is the only
//! evidence that it changes nothing. The version is the test application's
//! case 39, whose cases page asks for its rows with
//! `POST /api/cases/rows/search`; once the person has published it, the
//! read returns the rows the application holds, live, and the route heard a
//! search and never an archive.
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
use std::sync::atomic::{AtomicU32, Ordering};

use a2m_actions::{BoxFuture, Paths, PersonSurface};
use a2m_local::{Availability, PresencePrompt, PromptAnswer, PromptName, PromptText, StopReason};
use anything2mcp::{EngineOptions, LocalEngine, PASSPHRASE_VARIABLE};
use common::{
    Folder, IN_PROCESS_VARIABLE, PASSPHRASE, Surface, USER, copy_changed, project_source,
};
use example_app::harness::TestApp;
use serde_json::{Value as Json, json};

const PLACEHOLDER: &str = "http://127.0.0.1:4010";
const LIST: &str = "example_case_list";

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_publish_page_names_the_asserted_search_above_the_tools_and_the_read_then_returns_the_rows()
 {
    let app = TestApp::start().await.unwrap();
    let seeded = app.case_rows().await.unwrap();
    let home = Folder::new("home");
    let project = Folder::new("asserted");
    copy_changed(
        &project_source("rows"),
        project.path(),
        &[(PLACEHOLDER, app.url("").trim_end_matches('/'))],
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
    let prompt = Arc::new(Passed(AtomicU32::new(0)));
    let local = LocalEngine::start(options, Arc::clone(&prompt) as Arc<dyn PresencePrompt>)
        .await
        .unwrap();
    let serving = tokio::spawn(local.serve());
    let (home_path, project_path) = (home.path(), project.path());

    // The agent asks to publish; a person's page waits.
    let (code, asked) = run(
        home_path,
        project_path,
        &["publish", "example", "--version", "1.0.0"],
    )
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

    // The surfaces first, then the reads a person asserted, then the tools
    // (decisions 0394 and 0418). The search is named by its method, its
    // path, its entry, and each field with its `enum`, beside the sentence.
    let at = |heading: &str| {
        text.find(heading)
            .unwrap_or_else(|| panic!("{heading}: {text}"))
    };
    let surfaces = at("<h2>Where it leads</h2>");
    let asserted = at("<h2>The reads a person asserted</h2>");
    let tools = at("<h2>What each tool may do</h2>");
    assert!(surfaces < asserted && asserted < tools, "{text}");
    let section = &text[asserted..tools];
    assert!(
        section.contains(
            "<code>POST</code> <code>/api/cases/rows/search</code> on the surface <code>app</code>, asserted by the <code>page_request</code> <code>cases_rows_search</code>"
        ),
        "{section}"
    );
    assert!(
        section.contains(&format!(
            "<strong>{}</strong>",
            a2m_files::ASSERTED_NOTICE.replace('\'', "&#39;")
        )),
        "{section}"
    );
    for field in [
        "<code>op</code>, a <code>string</code>, one of <code>search</code>",
        "<code>status</code>, a <code>string</code>, one of <code>open</code>",
    ] {
        assert!(section.contains(field), "{field}: {section}");
    }
    assert!(text[tools..].contains(LIST), "{text}");
    // Nothing was sent to publish the page.
    assert_eq!(app.case_rows().await.unwrap(), seeded);

    // The person carries it out.
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
    assert_eq!(prompt.0.load(Ordering::SeqCst), 1);

    // The published read returns the open rows the application holds,
    // live: the tab let the asserted search through.
    let (code, called) = run(
        home_path,
        project_path,
        &["call", LIST, "--input", &json!({}).to_string()],
    )
    .await;
    assert_eq!(code, Some(0), "{called}");
    assert_eq!(called["result"]["status"], "completed", "{called}");
    let open: Vec<&str> = seeded
        .rows
        .iter()
        .filter(|row| row.status == "open")
        .map(|row| row.case_id.as_str())
        .collect();
    assert_eq!(open.len(), 2);
    assert_eq!(
        called["result"]["result"]["case_ids"],
        json!(open),
        "{called}"
    );

    // The route heard one search and never an archive; nothing changed.
    let after = app.case_rows().await.unwrap();
    assert_eq!(after.rows, seeded.rows);
    assert_eq!(after.received, vec!["search".to_owned()], "{after:?}");
    assert!(app.writes().await.unwrap().is_empty());

    let (code, _) = run(home_path, project_path, &["engine", "stop"]).await;
    assert_eq!(code, Some(0));
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
}
