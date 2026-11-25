//! The approval surface on one machine, proven both ways against the real
//! local engine (roadmap package 3.6): a person approves on the engine's
//! own page and the run writes once; nothing a program on the same machine
//! can do approves, and each way it tries is refused by name while the
//! test application stores nothing.
//!
//! The engine runs in this process, as `serve` runs it, with the real
//! channel, the real loopback listener and its pages, and the kernel. Only
//! the presence prompt is the test's: [`AtPrompt`] stands for the
//! operating system's prompt with a person in front of it, who passes a
//! prompt only right after pressing Approve, and only for the change the
//! page showed, as the page tells a person to. A program can raise such a
//! prompt by replaying a press; it cannot answer one. That a program on
//! the machine cannot answer the operating system's own prompt is
//! hypothesis H-12, which no test in this repository can show.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_actions::{BoxFuture, Paths, PersonSurface};
use a2m_local::{
    Availability, EngineLock, FAILURE_CODE_HEADER, PresencePrompt, PromptAnswer, PromptName,
    PromptText, StopReason,
};
use anything2mcp::cli::{Finished, Host, run_with};
use anything2mcp::{EngineOptions, LocalEngine};
use common::served::{Bridge, PATIENCE, stateless};
use common::{Machine, Surface};
use example_app::harness::TestApp;
use serde_json::{Value as Json, json};
use tokio::task::JoinHandle;

const PLACEHOLDER: &str = "http://127.0.0.1:4010";
const UPSERT: &str = "example_line_item_upsert";
const ORDER: &str = "A-102";

/// The input of a write of the line `item`.
fn write_input(item: &str) -> String {
    format!(
        r#"{{"order_id": "{ORDER}", "item_code": "{item}", "description": "New rail", "quantity": 3, "length_in": "12' 6 1/2\""}}"#
    )
}

/// The operating system's presence prompt with a person in front of it.
/// The person passes a prompt only when they have just pressed Approve on
/// a page that showed this change (`expect`); any other prompt they
/// cancel, as the page tells them to.
struct AtPrompt {
    expecting: Mutex<Option<String>>,
    raised: AtomicU32,
    passed: AtomicU32,
}

impl AtPrompt {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            expecting: Mutex::new(None),
            raised: AtomicU32::new(0),
            passed: AtomicU32::new(0),
        })
    }

    /// The person is about to press Approve on a page that shows `short`.
    fn expect(&self, short: &str) {
        *self.expecting.lock().unwrap() = Some(short.to_owned());
    }

    fn raised(&self) -> u32 {
        self.raised.load(Ordering::SeqCst)
    }

    fn passed(&self) -> u32 {
        self.passed.load(Ordering::SeqCst)
    }
}

impl PresencePrompt for AtPrompt {
    fn available(&self) -> BoxFuture<'_, Availability> {
        Box::pin(async {
            Availability::ready(PromptName::Polkit, "The prompt of a test, with a person.")
        })
    }

    fn ask(
        &self,
        text: PromptText,
        _deadline: tokio::time::Instant,
    ) -> BoxFuture<'_, PromptAnswer> {
        Box::pin(async move {
            self.raised.fetch_add(1, Ordering::SeqCst);
            let shown = text.detail.split_whitespace().next_back().unwrap_or("");
            let expected = self.expecting.lock().unwrap().take();
            if expected.as_deref() == Some(shown) {
                self.passed.fetch_add(1, Ordering::SeqCst);
                PromptAnswer::Present(PromptName::Polkit)
            } else {
                PromptAnswer::Dismissed
            }
        })
    }
}

/// A machine whose store is on disk and holds the published `example`
/// target. A person publishes on an engine in this process, which then
/// closes. `settings` is added to the installation's settings file.
async fn published(app: &TestApp, settings: &str) -> Machine {
    let machine = Machine::new("orders", &[(PLACEHOLDER, &app.url(""))]).with_passphrase();
    let file = Paths::under(machine.home.path())
        .installation_config
        .join("settings.toml");
    let held = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, format!("{held}{settings}")).unwrap();
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    world.close().await;
    lock_comes_free(&machine).await;
    machine
}

/// Waits, for a bounded time, until the engine lock of the machine is free:
/// a child another test of this process forked keeps a copy of it until it
/// runs its program.
async fn lock_comes_free(machine: &Machine) {
    let paths = Paths::under(machine.home.path());
    let give_up = tokio::time::Instant::now() + PATIENCE;
    while EngineLock::held(&paths).unwrap() {
        assert!(
            tokio::time::Instant::now() < give_up,
            "the engine lock stayed held after the engine closed"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The local engine as `serve` runs it, in this process, serving.
async fn serve(machine: &Machine, prompt: &Arc<AtPrompt>) -> JoinHandle<StopReason> {
    let mut options = EngineOptions::of_machine(
        Paths::under(machine.home.path()),
        machine.environment(),
        Surface::new() as Arc<dyn PersonSurface>,
    );
    options.project = Some(machine.project.path().to_path_buf());
    let local = LocalEngine::start(options, Arc::clone(prompt) as Arc<dyn PresencePrompt>)
        .await
        .unwrap();
    assert!(local.engine().runner_problem().is_none());
    tokio::spawn(local.serve())
}

/// Stops the engine through the channel and waits until it has let go.
async fn stop(machine: &Machine, serving: JoinHandle<StopReason>) {
    let (stopped, _) = command(machine, &["engine", "stop"]).await;
    assert_eq!(stopped.code, 0, "{stopped:?}");
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
    lock_comes_free(machine).await;
}

/// A command of the program, run as the command line runs it: nobody is
/// at a terminal, so the caller is an agent.
async fn command(machine: &Machine, args: &[&str]) -> (Finished, Json) {
    let host = Host {
        cwd: machine.project.path().to_path_buf(),
        environment: machine.environment(),
        session_folder: None,
        stdout_is_terminal: false,
        person_at_terminal: false,
        person: None,
        key_store: None,
        program: None,
        prompt: None,
    };
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let finished = run_with(&args, &host).await;
    let json = serde_json::from_str(finished.stdout.trim())
        .unwrap_or_else(|_| panic!("not one JSON object: {finished:?}"));
    (finished, json)
}

/// A run that rests, waiting for its approval, as an agent called it.
struct Rested {
    run: String,
    link: String,
    item: &'static str,
}

async fn rest(machine: &Machine, item: &'static str) -> Rested {
    let input = write_input(item);
    let (rested, response) = command(machine, &["tool", "call", UPSERT, "--input", &input]).await;
    assert_eq!(rested.code, 3, "{rested:?}");
    let result = &response["result"];
    assert_eq!(result["status"], "needs_approval", "{response}");
    let link = result["link"].as_str().unwrap().to_owned();
    assert!(link.starts_with("http://127.0.0.1:"), "{link}");
    Rested {
        run: result["run_id"].as_str().unwrap().to_owned(),
        link,
        item,
    }
}

/// Resumes a run as its agent does, and gives the answer. The kernel's
/// `run.resume` now and then answers in the moment between the runner's
/// claim of the job and its work on it, with the run still at rest; a
/// caller then resumes again.
async fn resume(machine: &Machine, run: &str) -> (Finished, Json) {
    let mut tries = 0;
    loop {
        let (done, response) = command(machine, &["run", "resume", run]).await;
        tries += 1;
        if response["result"]["status"] != "needs_approval" || tries == 20 {
            return (done, response);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// A run whose approval cleared nothing asks for a person again: it is
/// resumed, as its agent resumes it, until its answer gives a new page and
/// no longer says it was approved, for a bounded time. A resume can answer
/// before the runner has worked on the run.
async fn asked_again(machine: &Machine, rested: &Rested) -> Rested {
    let give_up = tokio::time::Instant::now() + PATIENCE;
    loop {
        let (still, response) = command(machine, &["run", "resume", &rested.run]).await;
        let result = &response["result"];
        assert_eq!(result["status"], "needs_approval", "{response}");
        if let Some(link) = result["link"].as_str()
            && link != rested.link
        {
            assert_eq!(still.code, 3, "{still:?}");
            assert_ne!(result["approved"], true, "{response}");
            return Rested {
                run: rested.run.clone(),
                link: link.to_owned(),
                item: rested.item,
            };
        }
        assert!(tokio::time::Instant::now() < give_up, "{response}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The run still rests: resuming it writes nothing.
async fn still_rests(machine: &Machine, run: &str) {
    let (still, response) = command(machine, &["run", "resume", run]).await;
    assert_eq!(still.code, 3, "{still:?}");
    assert_eq!(response["result"]["status"], "needs_approval", "{response}");
}

fn http() -> reqwest::Client {
    a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
        .no_proxy()
        .build()
        .unwrap()
}

/// What a page carries that a press needs: its one-time value and the
/// short hash of the change it shows.
struct Shown {
    once: String,
    short: String,
}

fn between<'a>(text: &'a str, before: &str, after: char) -> &'a str {
    let from = text
        .find(before)
        .unwrap_or_else(|| panic!("{before}: {text}"))
        + before.len();
    &text[from..from + text[from..].find(after).unwrap()]
}

/// Opens the page as a browser does, and reads what a press needs.
async fn open(http: &reqwest::Client, link: &str) -> Shown {
    let page = http.get(link).send().await.unwrap();
    assert_eq!(page.status().as_u16(), 200);
    let text = page.text().await.unwrap();
    Shown {
        once: between(&text, "name=\"once\" value=\"", '"').to_owned(),
        short: between(&text, "<span>Change <code>", '<').to_owned(),
    }
}

fn origin_of(link: &str) -> &str {
    &link[..link.find("/approve/").unwrap()]
}

/// A press of `which` on the page at `link`, sent as a program sends it:
/// with whatever origin and one-time value it chooses. The status and the
/// failure code the listener named.
async fn post(
    http: &reqwest::Client,
    link: &str,
    which: &str,
    origin: Option<&str>,
    once: Option<&str>,
) -> (u16, String) {
    let mut request = http
        .post(format!("{link}/{which}"))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(once.map(|once| format!("once={once}")).unwrap_or_default());
    if let Some(origin) = origin {
        request = request.header("origin", origin);
    }
    let answer = request.send().await.unwrap();
    (answer.status().as_u16(), code_of(&answer))
}

fn code_of(answer: &reqwest::Response) -> String {
    answer
        .headers()
        .get(FAILURE_CODE_HEADER)
        .map(|value| value.to_str().unwrap().to_owned())
        .unwrap_or_default()
}

/// The person approves on the page, as a browser presses: the page's own
/// origin and its one-time value, then the prompt, which they pass.
async fn person_approves(http: &reqwest::Client, prompt: &AtPrompt, rested: &Rested) {
    let shown = open(http, &rested.link).await;
    prompt.expect(&shown.short);
    let (status, code) = post(
        http,
        &rested.link,
        "approve",
        Some(origin_of(&rested.link)),
        Some(&shown.once),
    )
    .await;
    assert_eq!((status, code.as_str()), (200, ""));
}

async fn writes(app: &TestApp, rested: &Rested) -> usize {
    app.write_count(ORDER, rested.item).await.unwrap()
}

/// A person approves on the page in a real browser and the run writes once;
/// every way a program on the machine can try to approve is refused by
/// name, raises no prompt that passes, and writes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_person_approves_on_the_page_and_no_program_on_the_machine_can() {
    let app = TestApp::start().await.unwrap();
    let machine = published(&app, "").await;
    let prompt = AtPrompt::new();
    let serving = serve(&machine, &prompt).await;
    let http = http();
    let rested = rest(&machine, "L-300").await;
    let run = rested.run.as_str();
    let link = rested.link.as_str();
    let origin = origin_of(link);

    // No command approves. A command line has no such command; resuming
    // approves nothing.
    for args in [
        &["run", "approve", run][..],
        &["approval", "approve", run][..],
        &["approve", run][..],
    ] {
        let (refused, response) = command(&machine, args).await;
        assert_ne!(refused.code, 0, "{args:?}: {refused:?}");
        assert_ne!(response["error"]["code"], Json::Null, "{response}");
    }
    still_rests(&machine, run).await;

    // No route of the local channel approves: the router has none.
    let connector = a2m_local::Connector::new(
        &Paths::under(machine.home.path()),
        anything2mcp::version().parse().unwrap(),
        a2m_local::ClientLabel::CommandLine,
    )
    .unwrap();
    let channel = connector.connect().await.unwrap();
    let prefix = a2m_actions::REST_PREFIX;
    for path in [
        format!("{prefix}/actions/run.approve"),
        format!("{prefix}/actions/approval.record"),
        format!("{prefix}/runs/{run}/approve"),
        format!("{prefix}/runs/{run}/approval"),
        format!("{prefix}/approvals"),
    ] {
        let request = axum::http::Request::builder()
            .method("POST")
            .uri(&path)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(json!({"run_id": run}).to_string()))
            .unwrap();
        let answer = channel.send(request).await.unwrap();
        let status = answer.status().as_u16();
        let body = axum::body::to_bytes(answer.into_body(), 1 << 20)
            .await
            .unwrap();
        let response: Json = serde_json::from_slice(&body).unwrap();
        assert_eq!(status, 404, "{path}: {response}");
        assert_eq!(
            response["error"]["code"], "route_not_found",
            "{path}: {response}"
        );
    }

    // No tool an agent can call approves, in either toolset: none is
    // listed, and a call of one by a guessed name is refused.
    for toolset in ["use", "build"] {
        let mut bridge = Bridge::start(&machine, &["bridge", &format!("--toolset={toolset}")]);
        let listed = bridge.ask(&stateless(1, "tools/list", json!({})));
        let tools = listed["result"]["tools"].as_array().unwrap();
        assert!(!tools.is_empty(), "{listed}");
        for tool in tools {
            let name = tool["name"].as_str().unwrap();
            assert!(!name.contains("approv"), "{toolset}: {name}");
        }
        for (id, name) in [(2, "a2m_run_approve"), (3, "a2m_approval_record")] {
            let called = bridge.ask(&stateless(
                id,
                "tools/call",
                json!({"name": name, "arguments": {"run_id": run}}),
            ));
            let refused = called.get("error").map_or_else(
                || called["result"]["structuredContent"]["error"]["code"].clone(),
                |error| error["data"]["code"].clone(),
            );
            assert!(refused.is_string(), "{toolset}: {called}");
        }
        let (code, _, said) = bridge.close();
        assert_eq!(code, Some(0), "{said}");
    }
    // Nor does the registry the program is built from hold an action that
    // approves.
    let registry = anything2mcp::action_registry().unwrap();
    for action in registry.iter() {
        assert!(!action.name.as_str().contains("approv"), "{}", action.name);
    }

    // The loopback listener answers its pages and their presses, and
    // nothing else.
    let shown = open(&http, link).await;
    for path in [
        "/api/v1/actions/run.approve".to_owned(),
        format!("/approve/{run}/approve"),
        "/approvals".to_owned(),
    ] {
        let (status, code) = post(&http, &format!("{origin}{path}"), "", Some(origin), None).await;
        assert_eq!((status, code.as_str()), (404, "page_not_found"), "{path}");
    }
    // A press with no origin, another origin, or another host is refused.
    let (status, code) = post(&http, link, "approve", None, Some(&shown.once)).await;
    assert_eq!((status, code.as_str()), (403, "request_not_from_page"));
    for other in ["null", "http://127.0.0.1:1", "http://localhost"] {
        let shown = open(&http, link).await;
        let (status, code) = post(&http, link, "approve", Some(other), Some(&shown.once)).await;
        assert_eq!(
            (status, code.as_str()),
            (403, "request_not_from_page"),
            "{other}"
        );
    }
    let forged_host = http
        .get(link)
        .header("host", "localhost")
        .send()
        .await
        .unwrap();
    assert_eq!(forged_host.status().as_u16(), 403);
    assert_eq!(code_of(&forged_host), "request_not_from_page");
    // A press with the page's origin and no one-time value, or a made-up
    // one, or one of a page served before the last, is refused.
    let earlier = open(&http, link).await;
    let last = open(&http, link).await;
    for once in [None, Some("0000"), Some(earlier.once.as_str())] {
        let (status, code) = post(&http, link, "approve", Some(origin), once).await;
        assert_eq!(
            (status, code.as_str()),
            (403, "request_not_from_page"),
            "{once:?}"
        );
    }
    // That spent the last page's value too: a press is one use of a page.
    let (status, code) = post(&http, link, "approve", Some(origin), Some(&last.once)).await;
    assert_eq!((status, code.as_str()), (403, "request_not_from_page"));
    assert_eq!(prompt.raised(), 0);

    // A program that replays the page's own request exactly, as a browser
    // would send it, raises the prompt; the person did not press, so they
    // cancel it, and nothing is approved.
    let replayed = open(&http, link).await;
    let (status, code) = post(&http, link, "approve", Some(origin), Some(&replayed.once)).await;
    assert_eq!((status, code.as_str()), (403, "presence_failed"));
    assert_eq!((prompt.raised(), prompt.passed()), (1, 0));
    still_rests(&machine, run).await;
    assert_eq!(writes(&app, &rested).await, 0);

    // The person opens the link in a real browser, reads the page, and
    // presses Approve, as a person does; the prompt is raised, and passed.
    prompt.expect(&open(&http, link).await.short);
    let person = common::browser::Person::new();
    let (title, detail) = person.press(link, "Approve").await;
    person.end().await;
    assert_eq!(title, "Approved", "{detail}");
    assert_eq!((prompt.raised(), prompt.passed()), (2, 1));
    assert_eq!(writes(&app, &rested).await, 0);

    // The page is gone once answered, and with no page waiting the
    // listener itself is closed: a replay of a press of it reaches nothing.
    let replay = http
        .post(format!("{link}/approve"))
        .header("origin", origin)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("once={}", replayed.once))
        .send()
        .await;
    assert!(
        replay.as_ref().is_err_and(reqwest::Error::is_connect),
        "{replay:?}"
    );

    // The agent resumes; the run writes once, with the values, and a
    // resume again writes nothing.
    let (done, response) = resume(&machine, run).await;
    assert_eq!(done.code, 0, "{done:?}");
    assert_eq!(response["result"]["status"], "completed", "{response}");
    assert_eq!(writes(&app, &rested).await, 1);
    let stored = app.stored_item(ORDER, "L-300").await.unwrap().unwrap();
    assert_eq!(stored.description, "New rail");
    assert_eq!(stored.quantity, 3);
    assert_eq!(stored.length_sixteenths, 2408);
    let (again, _) = command(&machine, &["run", "resume", run]).await;
    assert_eq!(again.code, 0, "{again:?}");
    assert_eq!(writes(&app, &rested).await, 1);
    assert_eq!(prompt.raised(), 2);

    stop(&machine, serving).await;
}

/// An approval is for one run's one diff, is used once, and ends with its
/// window: another run's approval, an approval made stale at the stand or
/// by time, and a press for a change that no longer waits each write
/// nothing and say so by name; an approval survives an engine started
/// again before the write, which is then written once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_approval_is_for_one_run_and_one_diff_used_once_and_survives_a_restart() {
    let app = TestApp::start().await.unwrap();
    // An approval may be used for one minute on this machine, so that one
    // can be seen to end.
    let machine = published(&app, "\n[policy]\napproval_window_minutes = 1\n").await;
    let prompt = AtPrompt::new();
    let serving = serve(&machine, &prompt).await;
    let http = http();

    // Another run's approval. Two runs rest; the person approves the
    // first. The second still rests, and the first page's one-time value
    // pressed on the second page is refused.
    let first = rest(&machine, "L-301").await;
    let second = rest(&machine, "L-302").await;
    let on_first = open(&http, &first.link).await;
    let on_second = open(&http, &second.link).await;
    assert_ne!(on_first.short, on_second.short);
    prompt.expect(&on_first.short);
    let (status, code) = post(
        &http,
        &second.link,
        "approve",
        Some(origin_of(&second.link)),
        Some(&on_first.once),
    )
    .await;
    assert_eq!((status, code.as_str()), (403, "request_not_from_page"));
    let (status, code) = post(
        &http,
        &first.link,
        "approve",
        Some(origin_of(&first.link)),
        Some(&on_first.once),
    )
    .await;
    assert_eq!((status, code.as_str()), (200, ""));
    still_rests(&machine, &second.run).await;
    // The second page, pressed now, is answered by a person who did not
    // press it: nothing is approved.
    let again = open(&http, &second.link).await;
    let (status, code) = post(
        &http,
        &second.link,
        "approve",
        Some(origin_of(&second.link)),
        Some(&again.once),
    )
    .await;
    assert_eq!((status, code.as_str()), (403, "presence_failed"));
    still_rests(&machine, &second.run).await;
    let (done, response) = resume(&machine, &first.run).await;
    assert_eq!(done.code, 0, "{done:?}");
    assert_eq!(response["result"]["status"], "completed", "{response}");
    assert_eq!(writes(&app, &first).await, 1);
    assert_eq!(writes(&app, &second).await, 0);

    // An approval made stale at the stand: after the person approved,
    // somebody changes the record at the system. The diff the person saw
    // is no longer what the write would do, and nothing is written.
    let changed = rest(&machine, "L-303").await;
    let (status, code) = {
        // The record exists before the run writes it, as somebody keyed it.
        let shown = open(&http, &changed.link).await;
        let put = app
            .http()
            .put(app.url(&format!("/api/orders/{ORDER}/items/L-303")))
            .json(&json!({"description": "Changed by hand", "quantity": 7, "length_in": "10"}))
            .send()
            .await
            .unwrap();
        assert!(put.status().is_success(), "{}", put.status());
        prompt.expect(&shown.short);
        post(
            &http,
            &changed.link,
            "approve",
            Some(origin_of(&changed.link)),
            Some(&shown.once),
        )
        .await
    };
    assert_eq!((status, code.as_str()), (200, ""));
    let (stale, response) = resume(&machine, &changed.run).await;
    assert_ne!(stale.code, 0, "{stale:?}");
    assert_eq!(
        response["error"]["code"], "changed_since_preview",
        "{response}"
    );
    assert_eq!(writes(&app, &changed).await, 1);
    let held = app.stored_item(ORDER, "L-303").await.unwrap().unwrap();
    assert_eq!(held.description, "Changed by hand");

    // The engine starts again between the approval and the write. The
    // approval is in the store, not in the engine's memory: the run is
    // written once by the new engine. The page of the old engine is gone.
    let kept = rest(&machine, "L-305").await;
    person_approves(&http, &prompt, &kept).await;
    stop(&machine, serving).await;
    // A `serve` process ends with its engine and takes its listener with
    // it; in this process the listener may outlive the engine for a
    // moment, and then holds no page.
    let gone = http.get(&kept.link).send().await;
    assert!(
        gone.as_ref()
            .map_or_else(reqwest::Error::is_connect, |page| code_of(page)
                == "page_not_found"),
        "{gone:?}"
    );
    let serving = serve(&machine, &prompt).await;
    let (done, response) = resume(&machine, &kept.run).await;
    assert_eq!(done.code, 0, "{done:?}");
    assert_eq!(response["result"]["status"], "completed", "{response}");
    assert_eq!(writes(&app, &kept).await, 1);
    let (again, _) = command(&machine, &["run", "resume", &kept.run]).await;
    assert_eq!(again.code, 0, "{again:?}");
    assert_eq!(writes(&app, &kept).await, 1);

    // An approval made stale by time: approved, and resumed only after
    // its window, one minute on this machine, has ended. The wait is for
    // the time itself to pass: the engine keeps one clock, the machine's.
    // The approval clears nothing; the run's events name the refusal, and
    // the run asks for a person again, on a new page.
    let late = rest(&machine, "L-304").await;
    person_approves(&http, &prompt, &late).await;
    tokio::time::sleep(Duration::from_secs(62)).await;
    let again = asked_again(&machine, &late).await;
    assert_eq!(writes(&app, &late).await, 0);
    let trace = command(&machine, &["trace", "show", &late.run]).await.1;
    let refused: Vec<&Json> = trace["result"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "need" && event["detail"]["refused"].is_string())
        .collect();
    assert!(!refused.is_empty(), "{trace}");
    assert!(
        refused
            .iter()
            .all(|event| event["detail"]["refused"] == "approval_stale"),
        "{trace}"
    );
    assert!(
        !trace["result"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["kind"] == "intent"),
        "{trace}"
    );
    // A person approves the change again, and it is written once.
    person_approves(&http, &prompt, &again).await;
    let (done, response) = resume(&machine, &late.run).await;
    assert_eq!(done.code, 0, "{done:?}");
    assert_eq!(response["result"]["status"], "completed", "{response}");
    assert_eq!(writes(&app, &late).await, 1);

    assert_eq!(writes(&app, &second).await, 0);
    assert_eq!(prompt.passed(), 5);

    stop(&machine, serving).await;
}
