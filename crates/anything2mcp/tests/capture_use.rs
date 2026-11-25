//! A capture of a person's use of a web application, through the product
//! (package E2; decision 0392, points 1, 3, and 4; decision 0394).
//!
//! The agent asks for a capture with `capture start`; the person begins it
//! on the engine's own page; the engine opens a watching tab in a window of
//! its own browser, on the surface's address, and sends nothing after that.
//! The person is a script of the test application, which every page carries
//! while the test has set it: it opens the page that asks for its rows by
//! `POST` twice, the summaries of two orders, whose script reads by `GET`,
//! and the form of a line item, where it saves once and sends one search
//! whose body names a planted credential. The agent ends the capture with
//! `capture stop`; the findings are read with `finding list` and `finding
//! describe`, and drafted by the rules of the engine's own registry.
//!
//! What the test asks of it: the read by `GET` the pages made twice is
//! drafted, held, as a page request and a read; the search behind `POST`
//! is drafted, held, as a page request with the question in place of its
//! effect, its body's schema closed and without a value, and no read; the
//! save is stored once by the application and drafted as nothing; nothing
//! is drafted outside the holding folder; the application heard nothing
//! the engine did not open itself; and the planted credential is in no
//! file under the engine's home or the project.
//!
//! A window needs a screen: the engine runs in a child process of this test
//! binary with the display of a virtual screen of its own. Where no screen
//! can be had the test says so and passes, unless `A2M_REQUIRE_DESKTOP=1`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use a2m_actions::{BoxFuture, Paths, PersonSurface};
use a2m_core::Finding;
use a2m_files::{Drafter, TargetRoot};
use a2m_local::{Availability, PresencePrompt, PromptAnswer, PromptName, PromptText, StopReason};
use a2m_pack_web_browser::test_support::{Screen, screen_required};
use anything2mcp::{Engine, EngineOptions, LocalEngine, PASSPHRASE_VARIABLE};
use common::{Folder, IN_PROCESS_VARIABLE, PASSPHRASE, Surface, USER};
use example_app::harness::TestApp;
use example_app::person::WalkStep;
use serde_json::{Value as Json, json};

/// A made-up credential, stored in the vault, that the person types into a
/// form and that names a field of a body. A plain word, so that only the
/// redactor, and no rule of form, can tell it is one.
const PLANTED: &str = "examplecapturedword";

const ROWS: &str = "/cases/rows-by-post";
const ROWS_SEARCH: &str = "/api/cases/rows/search";
const FIRST: &str = "/orders/A-100/summary";
const SECOND: &str = "/orders/A-101/summary";
const FORM: &str = "/orders/A-100/items/L-101";
const ITEM: &str = "/api/orders/A-100/items/L-101";
const LAST: &str = "/orders";

/// The child test that runs with a display.
const WINDOW_CHILD: &str = "capture_use::a_person_s_use_is_captured_in_a_watching_tab_and_drafted_held_on_the_display_it_is_given";

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
    /// A machine whose project holds one target, `example`, with one web
    /// surface at the address of `app`.
    fn new(app: &TestApp) -> Self {
        let home = Folder::new("capture-use-home");
        let paths = Paths::under(home.path());
        std::fs::create_dir_all(&paths.installation_config).unwrap();
        std::fs::create_dir_all(&paths.profiles).unwrap();
        std::fs::write(
            paths.installation_config.join("settings.toml"),
            "[vault]\nbackend = \"passphrase\"\n",
        )
        .unwrap();
        let project = Folder::new("capture-use");
        let target = project.path().join("targets").join("example");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(project.path().join("anything2mcp.toml"), "").unwrap();
        std::fs::write(
            target.join("target.yaml"),
            format!(
                "schema_version: 1\ntarget: example\ntitle: Example orders\nsurfaces:\n  app:\n    kind: web\n    origin: \"{}\"\nenvironments:\n  test:\n    kind: test\ndefault_environment: test\nenvironment: {{locale: en-US, timezone: UTC}}\ndrift_risk: low\n",
                app.url("").trim_end_matches('/')
            ),
        )
        .unwrap();
        Self {
            home,
            project,
            printed: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn environment(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            (
                "A2M_HOME".to_owned(),
                self.home.path().display().to_string(),
            ),
            ("USER".to_owned(), USER.to_owned()),
            (PASSPHRASE_VARIABLE.to_owned(), PASSPHRASE.to_owned()),
        ])
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

    /// The engine as `serve` starts it in this project.
    async fn engine(&self, prompt: &Arc<Passed>) -> LocalEngine {
        let mut options = EngineOptions::of_machine(
            Paths::under(self.home.path()),
            self.environment(),
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

/// Every file under a folder, as text.
fn texts_under(folder: &Path, out: &mut Vec<(std::path::PathBuf, String)>) {
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

/// The lines the application heard, each with how often.
fn counted(lines: impl IntoIterator<Item = String>) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for line in lines {
        *out.entry(line).or_insert(0) += 1;
    }
    out
}

/// A page the person opens and reads once it says it has loaded.
fn visit(path: &str) -> [WalkStep; 3] {
    [
        WalkStep::Go(path.to_owned()),
        WalkStep::WaitFor {
            id: "state".to_owned(),
            text: "Loaded.".to_owned(),
        },
        WalkStep::Linger(LINGER),
    ]
}

/// How long the person looks at each page, in milliseconds, before they
/// move on: long enough for the watching tab to see the page's own requests
/// end and take its snapshot.
const LINGER: u64 = 3000;

/// What the person does in the window, from the page the engine opened.
fn walk() -> Vec<WalkStep> {
    let mut steps = vec![WalkStep::Linger(LINGER)];
    for page in [ROWS, FIRST, ROWS, SECOND] {
        steps.extend(visit(page));
    }
    steps.extend([
        WalkStep::Go(FORM.to_owned()),
        WalkStep::Linger(LINGER),
        WalkStep::Type {
            form: "line-item".to_owned(),
            field: "description".to_owned(),
            text: format!("Seen in the capture {PLANTED}"),
        },
        WalkStep::Submit("line-item".to_owned()),
        WalkStep::WaitFor {
            id: "status".to_owned(),
            text: "Saved.".to_owned(),
        },
        WalkStep::PostJson {
            to: ROWS_SEARCH.to_owned(),
            body: json!({"status": "open", "op": "search", PLANTED: "named"}),
        },
        WalkStep::Linger(LINGER),
        WalkStep::Go(LAST.to_owned()),
        WalkStep::Linger(LINGER),
    ]);
    steps
}

/// A person uses the application in a watching tab through the product,
/// and what was captured is drafted, held. A window needs a screen: the
/// engine runs in a child process of this test binary with the display of
/// a virtual screen of its own. Where no screen can be had the test says so
/// and passes, unless `A2M_REQUIRE_DESKTOP=1`.
#[test]
fn a_person_s_use_is_captured_in_a_watching_tab_and_drafted_held() {
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
        WINDOW_CHILD,
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
        "the child did not capture the person's use:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Run only by the test above, in a child process that has a display.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run by `a_person_s_use_is_captured_in_a_watching_tab_and_drafted_held` in a child process"]
async fn a_person_s_use_is_captured_in_a_watching_tab_and_drafted_held_on_the_display_it_is_given()
{
    let app = TestApp::start().await.unwrap();
    let desk = Desk::new(&app);
    let prompt = Arc::new(Passed(AtomicU32::new(0)));
    let local = desk.engine(&prompt).await;
    let serving = tokio::spawn(local.serve());
    let http = browser();

    // The credential is the vault's, so the redactor knows it.
    desk.ok(&["secret", "set", "example/planted", "--value", PLANTED])
        .await;
    app.set_person_walk(&walk()).await.unwrap();
    let before = app.seen().await.unwrap();

    // The agent asks for a capture. It waits for its person, on the
    // engine's page, and nothing is opened yet.
    let (code, asked) = desk.run(&["capture", "start", "example"]).await;
    assert_eq!(code, Some(3), "{asked}");
    assert_eq!(asked["result"]["state"], "asked", "{asked}");
    let link = asked["result"]["link"].as_str().unwrap().to_owned();
    assert_eq!(app.seen().await.unwrap(), before);

    // The person begins it on the page, past the prompt. The engine opens
    // the watching tab on the surface's address, and the person's walk
    // starts there.
    let text = http.get(&link).send().await.unwrap().text().await.unwrap();
    assert!(text.contains("What is kept"), "{text}");
    let origin = link[..link.find("/act/").unwrap()].to_owned();
    let said = http
        .post(format!("{link}/confirm"))
        .header("origin", &origin)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("once={}", once_of(&text)))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(said.contains("Stop recording"), "{said}");

    // The walk ends at the list of orders, after the save.
    let last = format!("GET {LAST}");
    let saved = format!("PUT {ITEM}");
    let walked = tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            let heard = app.seen().await.unwrap();
            let at_save = heard.iter().position(|line| *line == saved);
            if at_save.is_some_and(|at| heard[at..].contains(&last)) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await;
    assert!(walked.is_ok(), "the person's walk did not end");

    // The agent ends the capture. What was found is stored.
    let stopped = desk.ok(&["capture", "stop", "example"]).await;
    let stopped = &stopped["result"];
    assert_eq!(stopped["state"], "ended", "{stopped}");
    assert_eq!(stopped["ended_by"], "asked", "{stopped}");
    assert!(stopped["findings"].as_u64().unwrap() > 0, "{stopped}");

    // The findings, as the product lists and describes them.
    let listed = desk
        .ok(&["finding", "list", "example", "--limit", "200"])
        .await;
    let mut findings: Vec<Finding> = Vec::new();
    for one in listed["result"]["findings"].as_array().unwrap() {
        let id = one["id"].as_str().unwrap();
        let described = desk
            .ok(&["finding", "describe", id, "--target", "example"])
            .await;
        findings.push(serde_json::from_value(described["result"]["finding"].clone()).unwrap());
    }
    let kinds: Vec<&str> = findings.iter().map(|one| one.kind().as_str()).collect();
    for kind in ["exchange", "screen", "endpoint"] {
        assert!(kinds.contains(&kind), "{kind}: {kinds:?}");
    }

    let (code, _) = desk.run(&["engine", "stop"]).await;
    assert_eq!(code, Some(0));
    assert_eq!(serving.await.unwrap(), StopReason::Asked);

    // The save was the person's: stored once, and nothing archived.
    assert_eq!(app.write_count("A-100", "L-101").await.unwrap(), 1);
    let rows = app.case_rows().await.unwrap();
    assert!(
        rows.received.iter().all(|op| op != "archive"),
        "{:?}",
        rows.received
    );

    // The application heard the address the engine opened and what the
    // person did, and nothing of the engine's own.
    let mut heard = app.seen().await.unwrap();
    heard.drain(..before.len());
    let mut expected = vec!["GET /".to_owned(), format!("GET {LAST}")];
    for page in [ROWS, FIRST, ROWS, SECOND] {
        expected.push(format!("GET {page}"));
    }
    expected.extend([
        format!("POST {ROWS_SEARCH}"),
        format!("POST {ROWS_SEARCH}"),
        "GET /api/orders/A-100/totals".to_owned(),
        "GET /api/orders/A-101/totals".to_owned(),
        format!("GET {FORM}"),
        format!("PUT {ITEM}"),
        format!("POST {ROWS_SEARCH}"),
        format!("GET {LAST}"),
    ]);
    assert_eq!(counted(heard), counted(expected));

    // Drafted by the rules of the engine's own registry, into the target.
    let options = {
        let mut options = EngineOptions::of_machine(
            Paths::under(desk.home.path()),
            desk.environment(),
            Surface::new() as Arc<dyn PersonSurface>,
        );
        options.project = Some(desk.project.path().to_path_buf());
        options
    };
    let engine = Engine::open(options).await.unwrap();
    let folder = desk.project.path().join("targets").join("example");
    let root = TargetRoot::new(&folder);
    let redact = engine.kernel().redact();
    let report = Drafter::new(engine.kernel().providers().compiler_set(), &root)
        .with_redact(redact.as_ref())
        .draft(&findings)
        .unwrap();
    engine.close().await;
    let held: Vec<&str> = report.held.iter().map(|path| path.as_str()).collect();

    // Every draft from a capture waits in the holding folder.
    assert!(report.written.is_empty(), "{:?}", report.written);

    // The read by `GET` the pages of two orders made, each once: one page
    // request and one read over it, both held.
    for file in [
        "held/page_requests/api_orders_totals.yaml",
        "held/operations/api_orders_totals_request.yaml",
    ] {
        assert!(held.contains(&file), "{file}: {held:?}");
    }
    let totals =
        std::fs::read_to_string(folder.join("held/page_requests/api_orders_totals.yaml")).unwrap();
    assert!(totals.contains("method: \"GET\""), "{totals}");
    assert!(
        totals.contains("page: \"/orders/{order_id}/summary\""),
        "{totals}"
    );

    // The search behind `POST`, seen twice in one shape: a page request
    // with the question in place of its effect, a closed body with no
    // value, and no read.
    let search_file = "held/page_requests/api_cases_rows_search.yaml";
    assert!(held.contains(&search_file), "{held:?}");
    let search = std::fs::read_to_string(folder.join(search_file)).unwrap();
    assert!(search.contains("method: \"POST\""), "{search}");
    assert!(search.contains(&format!("page: \"{ROWS}\"")), "{search}");
    assert!(search.contains("additionalProperties: false"), "{search}");
    assert!(search.contains("op:"), "{search}");
    assert!(search.contains("status:"), "{search}");
    // The drafter writes neither the person's word nor a value.
    let keys: Vec<&str> = search
        .lines()
        .map(|line| line.trim_start().split(':').next().unwrap_or_default())
        .collect();
    assert!(!keys.contains(&"read_asserted"), "{search}");
    assert!(!keys.contains(&"enum"), "{search}");
    assert!(search.contains("effect: {open_question:"), "{search}");
    assert!(
        search.contains("is not evidence that a request of this shape never does"),
        "{search}"
    );
    assert!(
        held.iter()
            .all(|file| !(file.starts_with("held/operations/") && file.contains("rows_search"))),
        "{held:?}"
    );

    // The save, seen once, is drafted as nothing, and said to be.
    let mut drafted = Vec::new();
    texts_under(&folder, &mut drafted);
    for (path, text) in &drafted {
        assert!(!text.contains("/items/"), "{}: {text}", path.display());
        assert!(
            !text.contains("method: \"PUT\""),
            "{}: {text}",
            path.display()
        );
    }
    assert!(
        report
            .not_drafted
            .iter()
            .any(|left| left.reason.contains("seen once")),
        "{:?}",
        report.not_drafted
    );

    // The credential is in nothing a command printed, and in no file under
    // the engine's home or the project.
    for printed in desk.printed.lock().unwrap().iter() {
        assert!(!printed.contains(PLANTED), "{printed}");
    }
    let mut files = Vec::new();
    texts_under(desk.home.path(), &mut files);
    texts_under(desk.project.path(), &mut files);
    for (path, text) in &files {
        assert!(!text.contains(PLANTED), "{}", path.display());
    }
}
