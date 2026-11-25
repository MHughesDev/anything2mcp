//! Package 9.1 in a real browser, through the product's commands: the
//! test application's form of a line item is changed on purpose, as a
//! release of a web application changes a screen (`POST /__control/screens`).
//! A canary, the check of a watch of the read through that form, catches a
//! change its locators do not survive; the write through the same form is
//! then refused as drift, naming the tool, before anything reaches the
//! application; a repaired screen model is drafted and published by the
//! test, as a person or an agent would, and the write is then approved,
//! made once, and read back. A change the locators survive, because they
//! name a field by its role and its label and not by its place, its class,
//! or its identifier, shows no drift at all. The application is asked what
//! it stored. It needs a browser, as the web pack's tests find one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_actions::{
    ApprovalAsk, ApprovalDesk, BoxFuture, Paths, PersonActAsk, PersonSurface, PresenceAsk,
    PresenceProof, SessionAsk, SignInAsk, SurfaceLink,
};
use a2m_auth::PersonAnswer;
use a2m_core::{Failure, RunId};
use a2m_logs::{Clock, ManualClock, SystemClock};
use a2m_policy::{Channel, PresenceCheck};
use anything2mcp::{Engine, EngineOptions};
use common::tending::{CHECK_WAIT, PASS_STEP, may_step, next_due_after};
use common::{Machine, World};
use example_app::harness::TestApp;
use example_app::releases::{ChangedPage, ScreenChange};
use serde_json::{Value as Json, json};

const PLACEHOLDER: &str = "http://127.0.0.1:4010";
const GET: &str = "example_line_item_get";
const UPDATE: &str = "example_line_item_update";
/// The screen model of the form, in the project's draft.
const SCREEN: &str = "targets/example/screens/line_item.yaml";

/// A person surface that shows the engine's pages: it keeps the desk, the
/// approvals it was asked for, and each person's act, and the test presses
/// there.
struct Pages {
    desk: Mutex<Option<Arc<ApprovalDesk>>>,
    approvals: Mutex<Vec<ApprovalAsk>>,
    acts: Mutex<Vec<PersonActAsk>>,
    clock: Arc<dyn Clock>,
}

impl PersonSurface for Pages {
    fn take_desk(&self, desk: ApprovalDesk) {
        *self.desk.lock().unwrap() = Some(Arc::new(desk));
    }

    fn ask_approval(&self, ask: ApprovalAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            let run = ask.run;
            self.approvals.lock().unwrap().push(ask);
            Ok(SurfaceLink {
                url: Some(format!("http://127.0.0.1/approve/{run}")),
                text: "Open the approval page.".to_owned(),
            })
        })
    }

    fn ask_sign_in(&self, _ask: SignInAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async {
            Ok(SurfaceLink {
                url: None,
                text: String::new(),
            })
        })
    }

    fn confirm_presence(&self, ask: PresenceAsk) -> BoxFuture<'_, Result<PresenceProof, Failure>> {
        Box::pin(async move {
            Ok(PresenceProof::new(
                PresenceCheck::OsPrompt,
                ask.principal,
                self.clock.now(),
            ))
        })
    }

    fn ask_session(&self, _ask: SessionAsk) -> BoxFuture<'_, PersonAnswer> {
        Box::pin(async { PersonAnswer::NoAnswer })
    }

    fn shows_person_acts(&self) -> bool {
        true
    }

    fn ask_person_act(&self, ask: PersonActAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            let link = format!("http://127.0.0.1/act/{}", ask.act);
            self.acts.lock().unwrap().push(ask);
            Ok(SurfaceLink {
                url: Some(link),
                text: "Open the page to carry it out.".to_owned(),
            })
        })
    }
}

/// The application, a machine whose project uses it through its pages,
/// and an engine on that machine with the page surface and a clock the
/// test moves.
struct Stand {
    app: TestApp,
    machine: Machine,
    world: World,
    pages: Arc<Pages>,
    clock: Arc<ManualClock>,
}

impl Stand {
    /// The `pages` project over a new application, its draft linted and
    /// published by its person.
    async fn published() -> Self {
        Self::published_with(&[]).await
    }

    /// The same, with each of `more` written into the project's copy, by
    /// its path in the project, before the draft is published.
    async fn published_with(more: &[(&str, &str)]) -> Self {
        let app = TestApp::start().await.unwrap();
        let machine = Machine::new("pages", &[(PLACEHOLDER, &app.url(""))]).on_disk();
        for (path, text) in more {
            let path = machine.project.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let clock = Arc::new(ManualClock::starting_at(SystemClock::new().now()));
        let pages = Arc::new(Pages {
            desk: Mutex::new(None),
            approvals: Mutex::new(Vec::new()),
            acts: Mutex::new(Vec::new()),
            clock: Arc::clone(&clock) as Arc<dyn Clock>,
        });
        let mut options = EngineOptions::of_machine(
            Paths::under(machine.home.path()),
            machine.environment(),
            Arc::clone(&pages) as Arc<dyn PersonSurface>,
        );
        options.project = Some(machine.project.path().to_path_buf());
        options.place = machine.place;
        options.key_store.clone_from(&machine.key_store);
        options.clock = Arc::clone(&clock) as Arc<dyn Clock>;
        let engine = Engine::open(options).await.expect("the settings load");
        assert!(
            engine.runner_problem().is_none(),
            "{:?}",
            engine.runner_problem()
        );
        let world = World {
            engine,
            surface: Arc::clone(&machine.surface),
            clock: Arc::clone(&clock) as Arc<dyn Clock>,
        };
        let stand = Self {
            app,
            machine,
            world,
            pages,
            clock,
        };
        stand.publish().await;
        stand
    }

    /// Lints the draft and publishes it, as its person.
    async fn publish(&self) {
        let linted = self.world.ok(&["draft", "lint", "example"]).await;
        assert_eq!(linted["ok"], true, "{linted}");
        let published = self.world.person(&["version", "publish", "example"]).await;
        assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    }

    /// The repair: the screen model in the draft is changed to say what
    /// the form shows now, each of `changes` replacing a text of the file,
    /// and the draft is published again. Nothing is healed in a run.
    async fn repaired(&self, changes: &[(&str, &str)]) {
        let path = self.machine.project.path().join(SCREEN);
        let mut text = std::fs::read_to_string(&path).unwrap();
        for (old, new) in changes {
            assert!(text.contains(old), "{old} is not in the screen model");
            text = text.replace(old, new);
        }
        std::fs::write(&path, text).unwrap();
        self.publish().await;
    }

    /// A canary of the read of line item L-101: a watch of it, every
    /// minute for an hour, confirmed by its person on the page. Gives its
    /// identifier once its first check has ended.
    async fn canary(&self) -> String {
        let input = json!({"order_id": "A-100", "item_code": "L-101"}).to_string();
        let ends_at = (self.clock.now().as_unix_millis() + 3_600_000).to_string();
        let asked = self
            .world
            .agent(&[
                "watch",
                "create",
                GET,
                "--input",
                &input,
                "--interval-seconds",
                "60",
                "--ends-at",
                &ends_at,
            ])
            .await;
        assert_eq!(asked.result()["state"], "asked", "{:?}", asked.finished);
        let id = asked.result()["watch_id"].as_str().unwrap().to_owned();
        let ask = self
            .pages
            .acts
            .lock()
            .unwrap()
            .iter()
            .find(|ask| {
                ask.action.as_str() == "watch.confirm"
                    && ask.input.iter().any(|field| field.value == id)
            })
            .cloned()
            .expect("the person was shown the confirmation");
        let proof = PresenceProof::new(PresenceCheck::OsPrompt, self.person().id, self.clock.now())
            .for_act(ask.act);
        let done = self.desk().carry_out(ask.act, proof).await.unwrap();
        assert!(done.ok, "{done:?}");
        self.checked(Duration::from_secs(10), &id, 1).await;
        id
    }

    /// Moves the clock by `by` and tends until the watch `id` has made
    /// `checks` checks and is not running one. Between two passes the
    /// clock moves on by [`PASS_STEP`], never as far as the watch's next due
    /// time (see `common::tending`); the wait is bounded by [`CHECK_WAIT`]
    /// of real time.
    async fn checked(&self, by: Duration, id: &str, checks: u64) -> Json {
        let before = self.watch(id).await;
        self.clock.advance(by);
        let next = next_due_after(&before, self.clock.now().as_unix_millis());
        let started = std::time::Instant::now();
        let mut first = true;
        while started.elapsed() < CHECK_WAIT {
            if !first && may_step(self.clock.now().as_unix_millis(), next) {
                self.clock.advance(PASS_STEP);
            }
            first = false;
            self.world.engine.kernel().tend().await;
            let watch = self.watch(id).await;
            if watch["checks"] == checks && watch["state"] != "running" {
                return watch;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!(
            "the check never ended in {CHECK_WAIT:?}: {}",
            self.watch(id).await
        );
    }

    async fn watch(&self, id: &str) -> Json {
        self.world.ok(&["watch", "get", id]).await
    }

    async fn changes(&self, id: &str) -> Vec<Json> {
        self.world.ok(&["watch", "changes", id]).await["changes"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn person(&self) -> a2m_policy::PrincipalRef {
        self.world
            .engine
            .caller(a2m_actions::FrontDoor::Command, false)
            .principal
            .principal()
            .clone()
    }

    fn desk(&self) -> Arc<ApprovalDesk> {
        self.pages.desk.lock().unwrap().clone().unwrap()
    }

    /// The write of a line item of order A-100, called by an agent: it
    /// rests, its person approves the diff they were shown, and it is
    /// resumed. Gives what the resume gave.
    async fn written(&self, item: &str, description: &str, quantity: u32) -> Json {
        let input = json!({
            "order_id": "A-100",
            "item_code": item,
            "description": description,
            "quantity": quantity,
            "length_in": "12.5",
        })
        .to_string();
        let rested = self
            .world
            .agent(&["tool", "call", UPDATE, "--input", &input])
            .await;
        assert_eq!(
            rested.result()["status"],
            "needs_approval",
            "{}",
            rested.json
        );
        let run = rested.result()["run_id"].as_str().unwrap().to_owned();
        let run_id: RunId = run.parse().unwrap();
        let asked = self
            .pages
            .approvals
            .lock()
            .unwrap()
            .last()
            .cloned()
            .unwrap();
        assert_eq!(asked.run, run_id);
        let person = self.person();
        let proof = PresenceProof::new(PresenceCheck::OsPrompt, person.id, self.clock.now());
        self.desk()
            .approve(run_id, asked.diff_hash, person, Channel::LocalPrompt, proof)
            .await
            .unwrap();
        let done = self.world.agent(&["run", "resume", &run]).await;
        assert_eq!(done.finished.code, 0, "{}", done.json);
        assert_eq!(done.result()["status"], "completed", "{}", done.json);
        done.result().clone()
    }

    async fn close(self) {
        self.world.close().await;
    }
}

fn kinds(changes: &[Json]) -> Vec<&str> {
    changes
        .iter()
        .map(|change| change["kind"].as_str().unwrap())
        .collect()
}

/// A change the screen model does not survive: the canary catches it and
/// stops the watch as drifted; the write through the same form is refused
/// as drift, naming the tool, with nothing sent and nobody asked; the
/// repair, published, lets a canary through the changed form pass and
/// the write be made once and read back.
async fn caught_and_repaired(change: ScreenChange, repair: &[(&str, &str)], why: &str) {
    let stand = Stand::published().await;
    stand
        .app
        .change_screen(ChangedPage::LineItem, change)
        .await
        .unwrap();

    // The canary's read finds the form changed, and the watch stops on it.
    let id = stand.canary().await;
    let watch = stand.watch(&id).await;
    assert_eq!(watch["state"], "paused", "{change:?}: {watch}");
    assert_eq!(watch["reason"], "drifted", "{change:?}: {watch}");
    // The watch says which failure of the pack it stopped on.
    assert_eq!(watch["failure"]["class"], "drift", "{change:?}: {watch}");
    assert_eq!(watch["failure"]["code"], why, "{change:?}: {watch}");

    // The write through the same form is refused before anything is sent:
    // nobody is asked to approve it, and the application heard nothing of
    // the line item and stored nothing.
    let before = stand.app.seen().await.unwrap();
    let input = json!({
        "order_id": "A-100",
        "item_code": "L-102",
        "description": "Oak rail",
        "quantity": 6,
        "length_in": "12.5",
    })
    .to_string();
    let refused = stand
        .world
        .agent(&["tool", "call", UPDATE, "--input", &input])
        .await;
    assert_ne!(refused.finished.code, 0, "{}", refused.json);
    assert_eq!(
        refused.error()["code"],
        "binding_drifted",
        "{change:?}: {}",
        refused.json
    );
    assert_eq!(refused.error()["class"], "drift", "{}", refused.json);
    // The run is stopped, not failed: a write the mark stops never fails.
    assert_eq!(refused.result()["status"], "stopped", "{}", refused.json);
    let message = refused.error()["message"].as_str().unwrap();
    assert!(message.contains(UPDATE), "{message}");
    assert!(stand.pages.approvals.lock().unwrap().is_empty());
    let heard = stand.app.heard_since(&before).await.unwrap();
    assert!(
        !heard.iter().any(|line| line.contains("L-102")),
        "{heard:?}"
    );
    assert!(stand.app.writes().await.unwrap().is_empty());

    // The repair: the screen model says what the form shows now, drafted
    // and published as any version is. A canary through the changed form
    // passes on it.
    stand.repaired(repair).await;
    let again = stand.canary().await;
    let watch = stand.watch(&again).await;
    assert_eq!(watch["state"], "confirmed", "{change:?}: {watch}");

    // The write is then made once, verified, and read back; the
    // application holds exactly that one write.
    let done = stand.written("L-102", "Oak rail", 6).await;
    assert_eq!(done["result"]["description"], "Oak rail", "{done}");
    assert_eq!(done["result"]["quantity"], 6, "{done}");
    assert_eq!(stand.app.writes().await.unwrap().len(), 1);
    assert_eq!(stand.app.write_count("A-100", "L-102").await.unwrap(), 1);
    let stored = stand
        .app
        .stored_item("A-100", "L-102")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.description, "Oak rail");
    assert_eq!(stored.quantity, 6);
    assert_eq!(stored.length_sixteenths, 200);

    // No run of the writing tool failed: the one the drift refused is
    // stopped, and the one after the repair completed.
    let runs = stand
        .world
        .ok(&["run", "list", "--tool", UPDATE, "--limit", "100"])
        .await;
    let mut statuses: Vec<&str> = runs["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|run| run["status"].as_str().unwrap())
        .collect();
    statuses.sort_unstable();
    assert_eq!(statuses, ["completed", "stopped"], "{runs}");
    stand.close().await;
}

/// A reworded label and button: the field's locators name it by its label,
/// so "Qty" is not "Quantity", and no locator of the list finds it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reworded_form_is_caught_by_a_canary_and_repaired_with_no_failed_write() {
    caught_and_repaired(
        ScreenChange::Reword,
        &[
            (
                "{role: textbox, name: Quantity}",
                "{role: textbox, name: Qty}",
            ),
            (
                "{role: button, name: Save}",
                "{role: button, name: Save line item}",
            ),
        ],
        "locator_not_found",
    )
    .await;
}

/// A form wrapped in a card that takes its heading: every locator still
/// finds its field, but the fingerprint says the field's section shows
/// "Line Item Details", and the form no longer does.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrapped_form_is_caught_by_its_fingerprint_and_repaired_with_no_failed_write() {
    caught_and_repaired(
        ScreenChange::Wrap,
        &[(
            "section: {role: form, name: \"\"}, near_text: \"Line Item Details\"",
            "section: {role: region, name: \"Line Item Details\"}, near_text: \"Line Item Details\"",
        )],
        "fingerprint_mismatch",
    )
    .await;
}

/// A restyled form, every class and identifier renamed, and a reordered
/// one: the locators name each field by its role and its label, and its
/// fingerprint by the section it is in, none of which changed. The canary
/// passes, nothing is marked, and the write is made once under each with
/// the first version, unrepaired.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restyled_or_reordered_form_shows_no_drift_and_takes_the_write() {
    let stand = Stand::published().await;
    stand
        .app
        .change_screen(ChangedPage::LineItem, ScreenChange::Restyle)
        .await
        .unwrap();
    let id = stand.canary().await;
    let watch = stand.watch(&id).await;
    assert_eq!(watch["state"], "confirmed", "restyle: {watch}");
    let done = stand.written("L-102", "Restyled rail", 5).await;
    assert_eq!(done["result"]["quantity"], 5, "{done}");
    assert_eq!(stand.app.write_count("A-100", "L-102").await.unwrap(), 1);

    stand
        .app
        .change_screen(ChangedPage::LineItem, ScreenChange::Reorder)
        .await
        .unwrap();
    let watch = stand.checked(Duration::from_secs(60), &id, 2).await;
    assert_eq!(watch["state"], "confirmed", "reorder: {watch}");
    let done = stand.written("L-103", "Reordered brace", 8).await;
    assert_eq!(done["result"]["quantity"], 8, "{done}");

    // Two checks of a record nothing changed, through two changed forms:
    // nothing but the confirmation. The application holds exactly the two
    // writes, one under each change.
    let changes = stand.changes(&id).await;
    assert_eq!(kinds(&changes), ["confirmed"], "{changes:?}");
    assert_eq!(stand.app.writes().await.unwrap().len(), 2);
    assert_eq!(stand.app.write_count("A-100", "L-103").await.unwrap(), 1);
    let stored = stand
        .app
        .stored_item("A-100", "L-103")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.description, "Reordered brace");
    assert_eq!(stored.quantity, 8);
    stand.close().await;
}

/// The read of every order, through the list of orders, a screen of its
/// own: what a drift of the form of a line item does not touch.
const LIST: &str = "example_order_list";

/// The files of that read, written into the project beside the form's.
const ORDER_LIST_FILES: &[(&str, &str)] = &[
    (
        "targets/example/screens/orders.yaml",
        "# The list of every order.\n\
         screen: orders\n\
         surface: app\n\
         reach: {url: \"/orders\"}\n\
         ready: {visible: {role: heading, name: \"Orders\"}}\n\
         rows:\n  locate:\n    - {role: table, name: \"All orders\"}\n\
         \x20 fingerprint: {section: {role: main, name: \"\"}}\n\
         \x20 header: [\"Order\", \"Customer\", \"Status\", \"Lines\"]\n\
         \x20 columns:\n\
         \x20   order_ids: {header: \"Order\", type: identifier}\n\
         \x20   customers: {header: \"Customer\", type: text}\n",
    ),
    (
        "targets/example/operations/orders.yaml",
        "operation: example_order_list\n\
         title: List every order\n\
         effect: read\n\
         inputs: {}\n\
         outputs:\n\
         \x20 order_ids: {type: identifier, list: true}\n\
         \x20 customers: {type: text, list: true}\n\
         bindings:\n\
         \x20 - mode: ui\n\
         \x20   verb: list\n\
         \x20   status: verified\n\
         \x20   screen: orders\n\
         description: >\n\
         \x20 Lists the number and the customer of every order.\n",
    ),
    (
        "targets/example/fixtures/order_list/fixture.yaml",
        "scrubbed: true\nnote: \"Synthetic data, written by hand for the imaginary target.\"\n",
    ),
    (
        "targets/example/fixtures/order_list/orders.json",
        "{\"order_ids\": [\"X-1\", \"X-2\"], \"customers\": [\"Invented Customer One\", \"Invented Customer Two\"]}\n",
    ),
    (
        "targets/example/tests/order_list.yaml",
        "test: example_order_list\n\
         kind: offline\n\
         fixture: order_list\n\
         inputs: {}\n\
         expect: {order_ids: [\"X-1\", \"X-2\"], customers: [\"Invented Customer One\", \"Invented Customer Two\"]}\n",
    ),
];

impl Stand {
    /// The published tools as the MCP door lists them, every one of them,
    /// to an agent: each by its name.
    async fn mcp_listed(&self) -> std::collections::BTreeMap<String, Json> {
        use a2m_mcp::{Listing, McpService, McpSettings};
        let settings = McpSettings {
            listing: Listing::Full,
            ..McpSettings::local()
        };
        let service = McpService::new(
            self.world.engine.kernel().clone(),
            a2m_policy::ToolsetName::new("use").unwrap(),
            settings,
        )
        .unwrap();
        let revision = "2026-07-28";
        let message = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": {"_meta": {
                "io.modelcontextprotocol/protocolVersion": revision,
                "io.modelcontextprotocol/clientCapabilities": {},
                "io.modelcontextprotocol/clientInfo": {"name": "drift", "version": "0"},
            }},
        });
        let mut request = axum::http::Request::post("/mcp/use")
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", revision)
            .header("mcp-method", "tools/list")
            .body(axum::body::Body::from(message.to_string()))
            .unwrap();
        request
            .extensions_mut()
            .insert(self.world.engine.caller(a2m_actions::FrontDoor::Mcp, true));
        let response = service.handle(request).await;
        assert_eq!(response.status().as_u16(), 200);
        let body = axum::body::to_bytes(response.into_body(), 1 << 24)
            .await
            .unwrap();
        let answer: Json = serde_json::from_slice(&body).unwrap();
        answer["result"]["tools"]
            .as_array()
            .unwrap_or_else(|| panic!("{answer}"))
            .iter()
            .filter(|tool| tool["name"].as_str().unwrap().starts_with("example_"))
            .map(|tool| (tool["name"].as_str().unwrap().to_owned(), tool.clone()))
            .collect()
    }

    /// The published tools as `tool search` gives them on the command
    /// line, each by its name.
    async fn command_listed(&self) -> std::collections::BTreeMap<String, Json> {
        self.world.ok(&["tool", "search"]).await["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| (tool["name"].as_str().unwrap().to_owned(), tool.clone()))
            .collect()
    }
}

/// The sentence the MCP door puts first in the description of a tool a
/// drift stopped (decision 0396).
const UNAVAILABLE: &str =
    "Unavailable: the system changed and this tool was stopped until its target is repaired.";

/// Package 9.1a (decision 0396): a tool a drift stopped stays in every
/// list, marked, with the screen's name; a read through another screen is
/// not marked; the call is refused as drift; and after the repair is
/// published both listings show it available again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tool_a_drift_stopped_is_listed_marked_and_listed_available_after_the_repair() {
    let stand = Stand::published_with(ORDER_LIST_FILES).await;
    let listed = stand.command_listed().await;
    for name in [GET, UPDATE, LIST] {
        assert_eq!(listed[name]["available"], true, "{}", listed[name]);
    }
    stand
        .app
        .change_screen(ChangedPage::LineItem, ScreenChange::Reword)
        .await
        .unwrap();
    let id = stand.canary().await;
    assert_eq!(stand.watch(&id).await["reason"], "drifted");

    // Over MCP: the write through the form is listed, with its schema, its
    // description beginning with the fixed sentence, and why in its
    // `_meta`. The read of the list of orders is listed as it was.
    let mcp = stand.mcp_listed().await;
    let write = &mcp[UPDATE];
    let description = write["description"].as_str().unwrap();
    assert!(description.starts_with(UNAVAILABLE), "{write}");
    assert!(
        write["inputSchema"]["properties"]["quantity"].is_object(),
        "{write}"
    );
    let why = &write["_meta"]["anything2mcp/unavailable"];
    assert_eq!(why["reason"], "drift", "{write}");
    assert_eq!(why["knowledge"], "line_item", "{write}");
    assert!(why["marked_at"].is_i64(), "{write}");
    let list = &mcp[LIST];
    assert!(
        !list["description"].as_str().unwrap().contains(UNAVAILABLE),
        "{list}"
    );
    assert!(
        list["_meta"].get("anything2mcp/unavailable").is_none(),
        "{list}"
    );

    // On the command line: the same, as `tool search` gives it.
    let listed = stand.command_listed().await;
    assert_eq!(listed[UPDATE]["available"], false, "{}", listed[UPDATE]);
    assert_eq!(listed[UPDATE]["unavailable"], *why, "{}", listed[UPDATE]);
    assert_eq!(listed[LIST]["available"], true, "{}", listed[LIST]);
    assert!(listed[LIST].get("unavailable").is_none());
    let described = stand.world.ok(&["tool", "describe", UPDATE]).await;
    assert_eq!(described["unavailable"], *why, "{described}");
    // As a person at a terminal reads it, too.
    let args = ["tool".to_owned(), "search".to_owned()];
    let shown = anything2mcp::cli::run_on(&stand.world.engine, &args, false, false)
        .await
        .stdout;
    assert!(shown.contains("available: false"), "{shown}");
    assert!(shown.contains("knowledge: line_item"), "{shown}");

    // The call is refused as drift, before anything is sent.
    let input = json!({
        "order_id": "A-100",
        "item_code": "L-102",
        "description": "Oak rail",
        "quantity": 6,
        "length_in": "12.5",
    })
    .to_string();
    let refused = stand
        .world
        .agent(&["tool", "call", UPDATE, "--input", &input])
        .await;
    assert_eq!(
        refused.error()["code"],
        "binding_drifted",
        "{}",
        refused.json
    );
    assert!(stand.app.writes().await.unwrap().is_empty());

    // The repair is published, and both listings show the tool available.
    stand
        .repaired(&[
            (
                "{role: textbox, name: Quantity}",
                "{role: textbox, name: Qty}",
            ),
            (
                "{role: button, name: Save}",
                "{role: button, name: Save line item}",
            ),
        ])
        .await;
    let mcp = stand.mcp_listed().await;
    for name in [GET, UPDATE, LIST] {
        let tool = &mcp[name];
        assert!(
            !tool["description"].as_str().unwrap().contains(UNAVAILABLE),
            "{tool}"
        );
        assert!(
            tool["_meta"].get("anything2mcp/unavailable").is_none(),
            "{tool}"
        );
    }
    let listed = stand.command_listed().await;
    for name in [GET, UPDATE, LIST] {
        assert_eq!(listed[name]["available"], true, "{}", listed[name]);
    }
    stand.close().await;
}

impl Stand {
    /// A canary of the read of line item L-101, asked for by an agent on the
    /// command line, every minute for an hour, with no watch (package 9.1b,
    /// decision 0999). Gives its identifier; it is not confirmed.
    async fn ask_canary(&self) -> String {
        let input = json!({"order_id": "A-100", "item_code": "L-101"}).to_string();
        let ends_at = (self.clock.now().as_unix_millis() + 3_600_000).to_string();
        let asked = self
            .world
            .agent(&[
                "canary",
                "create",
                GET,
                "--input",
                &input,
                "--interval-seconds",
                "60",
                "--ends-at",
                &ends_at,
            ])
            .await;
        assert_eq!(asked.result()["state"], "asked", "{}", asked.json);
        asked.result()["canary_id"].as_str().unwrap().to_owned()
    }

    async fn canary_of(&self, id: &str) -> Json {
        self.world.ok(&["canary", "get", id]).await
    }

    /// The canary's person confirms it on the page they were shown, then
    /// the clock moves by `by` and the engine tends until its check ends.
    async fn confirmed_canary(&self, id: &str, by: Duration) -> Json {
        let ask = self
            .pages
            .acts
            .lock()
            .unwrap()
            .iter()
            .find(|ask| {
                ask.action.as_str() == "canary.confirm"
                    && ask.input.iter().any(|field| field.value == id)
            })
            .cloned()
            .expect("the person was shown the canary's confirmation");
        let proof = PresenceProof::new(PresenceCheck::OsPrompt, self.person().id, self.clock.now())
            .for_act(ask.act);
        let done = self.desk().carry_out(ask.act, proof).await.unwrap();
        assert!(done.ok, "{done:?}");
        self.clock.advance(by);
        let started = std::time::Instant::now();
        while started.elapsed() < CHECK_WAIT {
            self.world.engine.kernel().tend().await;
            let canary = self.canary_of(id).await;
            if canary["checks"] == 1 && canary["state"] != "running" {
                return canary;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!(
            "the canary's check never ended in {CHECK_WAIT:?}: {}",
            self.canary_of(id).await
        );
    }
}

/// Package 9.1b in a real browser, through the product's commands: the
/// test application's form of a line item is changed on purpose. A canary
/// an agent asked for and nobody confirmed sends nothing however long it
/// waits, and the write through the form is not stopped by it; once its
/// person confirms it, its first check finds the change, and the write
/// through the same form is refused as drift, naming the tool, before
/// anything reaches the application. There is no watch. The application is
/// asked what it heard and what it stored.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_canary_nobody_confirmed_sends_nothing_and_a_confirmed_one_stops_the_write_before_any_call()
 {
    let stand = Stand::published().await;
    stand
        .app
        .change_screen(ChangedPage::LineItem, ScreenChange::Reword)
        .await
        .unwrap();
    let before = stand.app.seen().await.unwrap();
    let id = stand.ask_canary().await;

    // Ten of its intervals pass, with the engine tending: nothing is sent.
    for _ in 0..10 {
        stand.clock.advance(Duration::from_secs(60));
        stand.world.engine.kernel().tend().await;
    }
    let canary = stand.canary_of(&id).await;
    assert_eq!(canary["state"], "asked", "{canary}");
    assert_eq!(canary["checks"], 0, "{canary}");
    let heard = stand.app.heard_since(&before).await.unwrap();
    assert!(
        !heard.iter().any(|line| line.contains("L-101")),
        "{heard:?}"
    );
    let runs = stand.world.ok(&["run", "list", "--tool", GET]).await;
    assert_eq!(runs["runs"].as_array().unwrap().len(), 0, "{runs}");

    // Its person confirms it: its first check finds the form changed, and
    // the canary goes on.
    let canary = stand.confirmed_canary(&id, Duration::from_secs(10)).await;
    assert_eq!(canary["state"], "confirmed", "{canary}");
    assert_eq!(canary["outcome"], "drifted", "{canary}");
    assert_eq!(canary["failure"]["class"], "drift", "{canary}");
    let watches = stand.world.ok(&["watch", "list"]).await;
    assert_eq!(watches["watches"].as_array().unwrap().len(), 0, "{watches}");

    // The write through the same form is refused before anything is sent:
    // nobody is asked to approve it, and the application heard nothing of
    // the line item and stored nothing.
    let before = stand.app.seen().await.unwrap();
    let input = json!({
        "order_id": "A-100",
        "item_code": "L-102",
        "description": "Oak rail",
        "quantity": 6,
        "length_in": "12.5",
    })
    .to_string();
    let refused = stand
        .world
        .agent(&["tool", "call", UPDATE, "--input", &input])
        .await;
    assert_eq!(
        refused.error()["code"],
        "binding_drifted",
        "{}",
        refused.json
    );
    assert_eq!(refused.result()["status"], "stopped", "{}", refused.json);
    assert!(
        refused.error()["message"]
            .as_str()
            .unwrap()
            .contains(UPDATE)
    );
    assert!(stand.pages.approvals.lock().unwrap().is_empty());
    let heard = stand.app.heard_since(&before).await.unwrap();
    assert!(
        !heard.iter().any(|line| line.contains("L-102")),
        "{heard:?}"
    );
    assert!(stand.app.writes().await.unwrap().is_empty());

    // Its person ends it.
    let ended = stand.world.person(&["canary", "end", &id]).await;
    assert_eq!(ended.result()["ended"], "ended_by_person", "{}", ended.json);
    stand.close().await;
}
