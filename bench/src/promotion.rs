//! The exit scenario of Phase 4 (roadmap package 4.5): a read gains a request
//! binding through shadow, and the request binding returns what the interface
//! binding returned, many times faster.
//!
//! The operation of the scenario is a read of one line item of the test
//! application. It begins with one binding, a `ui` binding that reads the
//! item's form in a browser. The request binding, an `http` read of the item's
//! JSON route, is added in shadow, as a person adding a binding would. The
//! scenario then does what a person does, through the product's own commands
//! and no other route: it publishes, calls the tool until the shadow binding
//! has matched on the number of distinct inputs the engine asks for, reads the
//! shadow report, promotes the binding as a person with `binding promote`, and
//! publishes again. It holds two engines over the same application: one whose
//! operation still has only the `ui` binding, which is the baseline, and one
//! whose operation was promoted, which is the measured operation.
//!
//! Every call, measured or not, is checked against the answer the interface
//! binding gave for the same input before the scenario began, so the two are
//! held to identical results on every iteration. The setup also asks the
//! application what it heard: after promotion a read must reach it as a
//! request for the item's JSON and not as the opening of its page.
//!
//! The engines are opened in process with its store on disk in a temporary home, as the product's
//! own end-to-end tests open them. Nothing here names a real application.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use a2m_actions::{
    ApprovalAsk, ApprovalDesk, BoxFuture, Paths, PersonSurface, PresenceAsk, PresenceProof,
    SessionAsk, SignInAsk, StorePlace, SurfaceLink,
};
use a2m_auth::PersonAnswer;
use a2m_core::Failure;
use a2m_policy::PresenceCheck;
use a2m_telemetry::{Clock, SystemClock};
use a2m_vault::MemoryKeyStore;
use anything2mcp::cli::run_on;
use anything2mcp::{Engine, EngineOptions};
use example_app::harness::TestApp;
use serde_json::{Value, json};

use crate::error::BenchError;
use crate::harness::{Scenario, ScenarioResult};

/// The scenario's name.
pub const PROMOTION_SPEEDUP: &str = "promotion_speedup";

/// The factor the exit test of Phase 4 asks for: the request binding runs at
/// least this many times faster than the interface binding.
pub const REQUIRED_FACTOR: u64 = 10;

/// The most iterations the scenario runs, whatever the plan says: a read
/// through a browser takes tens of milliseconds, and a few dozen of them make
/// a median.
pub const MOST_ITERATIONS: u32 = 40;

/// The tool the scenario calls.
pub(crate) const TOOL: &str = "example_line_item_get";

/// The address the project's files give the application; the scenario
/// replaces it with the address of the application it started.
const PLACEHOLDER: &str = "http://127.0.0.1:4010";

/// The project of the scenario: the `ui` read of a line item, as files.
const PROJECT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/projects/promotion");

/// The surface of the request binding, added to the target's own.
const API_SURFACE: &str = "  api:\n    kind: http\n    base: \"http://127.0.0.1:4010\"\n";

/// The endpoint the request binding reads: the JSON of the same line item.
const ENDPOINT: &str = r#"endpoint: line_item_get
surface: api
method: GET
path: "/api/orders/{order_id}/items/{item_code}"
auth: none
effect: read
headers: {accept: "application/json"}
responses: {absent: [404]}
response_schema:
  type: object
  required: [order_id, item_code, description, quantity, length_in, version]
  properties:
    order_id: {type: string}
    item_code: {type: string}
    description: {type: string}
    quantity: {type: integer}
    length_in: {type: string}
    version: {type: integer}
"#;

/// The offline test of the request binding, with the fixture it replays: the
/// publish gate asks a passing test of every binding that may act.
const HTTP_TEST: &str = r#"test: example_line_item_get
kind: offline
fixture: line_item_get_http
binding: {mode: http}
inputs: {order_id: "A-100", item_code: "L-101"}
expect: {description: "Left rail", quantity: 2, length_in: "96.5"}
"#;
const HTTP_FIXTURE: &str =
    "scrubbed: true\nnote: \"Synthetic data, written by hand for the imaginary target.\"\n";
const HTTP_ANSWER: &str = r#"{"order_id": "A-100", "item_code": "L-101", "description": "Left rail", "quantity": 2, "length_in": "96.5", "version": 1}"#;

/// The binding added in shadow, after the `ui` one.
const SHADOW_BINDING: &str =
    "  - mode: http\n    verb: read\n    status: shadow\n    endpoint: line_item_get\n";

/// The inputs the shadow binding is shown. The product asks for five distinct
/// ones by default, and the application is seeded with four line items, so
/// the scenario adds two through the application's own interface.
const EVIDENCE: [(&str, &str); 6] = [
    ("A-100", "L-101"),
    ("A-100", "L-102"),
    ("A-100", "L-103"),
    ("A-101", "L-201"),
    ("A-100", "L-110"),
    ("A-100", "L-111"),
];

/// The input every iteration reads.
pub(crate) const MEASURED: (&str, &str) = ("A-100", "L-101");

pub(crate) fn fail(message: impl std::fmt::Display) -> BenchError {
    BenchError::setup(PROMOTION_SPEEDUP, message)
}

/// The person at the machine: it keeps the desk and says yes to a presence
/// prompt, as the product's own tests do.
struct Person {
    desk: Mutex<Option<Arc<ApprovalDesk>>>,
    clock: Arc<dyn Clock>,
}

impl PersonSurface for Person {
    fn take_desk(&self, desk: ApprovalDesk) {
        if let Ok(mut held) = self.desk.lock() {
            *held = Some(Arc::new(desk));
        }
    }

    fn ask_approval(&self, ask: ApprovalAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            Ok(SurfaceLink {
                url: Some(format!("http://127.0.0.1/approve/{}", ask.run)),
                text: "Open the approval page.".to_owned(),
            })
        })
    }

    fn ask_sign_in(&self, _ask: SignInAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            Ok(SurfaceLink {
                url: None,
                text: "Sign in at the engine's own window.".to_owned(),
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
        Box::pin(async move { PersonAnswer::NoAnswer })
    }
}

/// A folder under the system's temporary folder, removed when dropped.
pub(crate) struct Folder(pub(crate) PathBuf);

impl Folder {
    pub(crate) fn new(name: &str) -> Result<Self, BenchError> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "a2m-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).map_err(fail)?;
        Ok(Self(path))
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Copies a folder of text files, putting `address` in place of the
/// placeholder address.
fn copy_project(from: &Path, to: &Path, address: &str) -> Result<(), BenchError> {
    std::fs::create_dir_all(to).map_err(fail)?;
    for entry in std::fs::read_dir(from).map_err(fail)? {
        let entry = entry.map_err(fail)?.path();
        let Some(name) = entry.file_name() else {
            continue;
        };
        let target = to.join(name);
        if entry.is_dir() {
            copy_project(&entry, &target, address)?;
        } else {
            let text = std::fs::read_to_string(&entry).map_err(fail)?;
            std::fs::write(&target, text.replace(PLACEHOLDER, address)).map_err(fail)?;
        }
    }
    Ok(())
}

/// One machine: a home, a project of target files, and an open engine.
pub(crate) struct Machine {
    engine: Engine,
    _home: Folder,
    project: Folder,
}

impl Machine {
    pub(crate) async fn open(address: &str) -> Result<Self, BenchError> {
        Self::open_project(address, Path::new(PROJECT)).await
    }

    /// The same machine over another project of target files, whose files
    /// give the application's address as the placeholder.
    pub(crate) async fn open_project(address: &str, from: &Path) -> Result<Self, BenchError> {
        let home = Folder::new("benchhome")?;
        let project = Folder::new("benchproject")?;
        copy_project(from, &project.0, address)?;
        let person = Arc::new(Person {
            desk: Mutex::new(None),
            clock: Arc::new(SystemClock::new()),
        });
        let environment = BTreeMap::from([
            ("A2M_HOME".to_owned(), home.0.display().to_string()),
            ("USER".to_owned(), "pat".to_owned()),
        ]);
        let mut options = EngineOptions::of_machine(
            Paths::under(&home.0),
            environment,
            person as Arc<dyn PersonSurface>,
        );
        options.project = Some(project.0.clone());
        // The browser keeps its profile in a folder of the home, so the store is on disk.
        // `A2M_BENCH_MEMORY_STORE` keeps the ledger in memory, to see how much
        // of a call is the durable store; a run made so is never recorded.
        options.place = if std::env::var_os("A2M_BENCH_MEMORY_STORE").is_some() {
            StorePlace::Memory
        } else {
            StorePlace::Disk
        };
        options.key_store = Some(Arc::new(MemoryKeyStore::new()));
        let engine = Engine::open(options).await.map_err(fail)?;
        if let Some(problem) = engine.runner_problem() {
            return Err(fail(format!(
                "the engine's runner is not ready: {problem:?}"
            )));
        }
        Ok(Self {
            engine,
            _home: home,
            project,
        })
    }

    /// Runs one command line as an agent or as a person, and gives the
    /// `result` of its JSON answer.
    pub(crate) async fn run(&self, args: &[&str], as_agent: bool) -> Result<Value, BenchError> {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let finished = run_on(&self.engine, &args, true, as_agent).await;
        let json: Value = serde_json::from_str(finished.stdout.trim()).map_err(|e| {
            fail(format!(
                "{args:?} did not answer with JSON: {e}: {finished:?}"
            ))
        })?;
        if finished.code != 0 {
            return Err(fail(format!("{args:?} stopped: {finished:?}")));
        }
        Ok(json["result"].clone())
    }

    /// Calls the tool for one line item, as an agent, in production.
    pub(crate) async fn read(
        &self,
        scenario: &str,
        input: (&str, &str),
    ) -> Result<Value, BenchError> {
        let input = json!({"order_id": input.0, "item_code": input.1});
        self.read_tool(scenario, TOOL, &input).await
    }

    /// Calls a tool with an input, as an agent, in production, and gives
    /// the result it completed with.
    pub(crate) async fn read_tool(
        &self,
        scenario: &str,
        tool: &str,
        input: &Value,
    ) -> Result<Value, BenchError> {
        let text = input.to_string();
        let args: Vec<String> = [
            "tool",
            "call",
            tool,
            "--environment",
            "production",
            "--input",
            &text,
        ]
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect();
        let finished = run_on(&self.engine, &args, true, true).await;
        let json: Value = serde_json::from_str(finished.stdout.trim())
            .map_err(|e| BenchError::operation(scenario, format!("not JSON: {e}: {finished:?}")))?;
        let answer = &json["result"];
        if finished.code != 0 || answer["status"] != "completed" {
            return Err(BenchError::operation(
                scenario,
                format!("the call did not complete: {finished:?}"),
            ));
        }
        Ok(answer["result"].clone())
    }

    pub(crate) async fn publish(&self) -> Result<(), BenchError> {
        let linted = self.run(&["draft", "lint", "example"], true).await?;
        if linted["ok"] != true {
            return Err(fail(format!("the draft did not lint: {linted}")));
        }
        self.run(&["version", "publish", "example"], false).await?;
        Ok(())
    }

    /// The request binding is added to the operation in shadow, with the
    /// surface and the endpoint it reads, as a person adds one.
    fn add_shadow_binding(&self, address: &str) -> Result<(), BenchError> {
        self.add_request_binding(address, "shadow")
    }

    /// The same edit with the binding's status named: `shadow` for the
    /// promotion scenario, `verified` for a measure of the request mode alone.
    pub(crate) fn add_request_binding(
        &self,
        address: &str,
        status: &str,
    ) -> Result<(), BenchError> {
        let target = self.project.0.join("targets").join("example");
        let file = target.join("target.yaml");
        let text = std::fs::read_to_string(&file).map_err(fail)?;
        let surface = API_SURFACE.replace(PLACEHOLDER, address);
        let with_api = text.replace("environments:\n", &format!("{surface}environments:\n"));
        if with_api == text {
            return Err(fail("the target file has no place for a surface"));
        }
        std::fs::write(&file, with_api).map_err(fail)?;
        std::fs::create_dir_all(target.join("endpoints")).map_err(fail)?;
        std::fs::write(target.join("endpoints").join("line_items.yaml"), ENDPOINT).map_err(fail)?;
        let fixture = target.join("fixtures").join("line_item_get_http");
        std::fs::create_dir_all(&fixture).map_err(fail)?;
        std::fs::write(fixture.join("fixture.yaml"), HTTP_FIXTURE).map_err(fail)?;
        std::fs::write(fixture.join("line_item_get.json"), HTTP_ANSWER).map_err(fail)?;
        std::fs::write(
            target.join("tests").join("line_item_get_http.yaml"),
            HTTP_TEST,
        )
        .map_err(fail)?;
        let operations = target.join("operations").join("line_items.yaml");
        let text = std::fs::read_to_string(&operations).map_err(fail)?;
        let with_shadow = text.replace(
            "    screen: line_item\n",
            &format!(
                "    screen: line_item\n{}",
                SHADOW_BINDING.replace("status: shadow", &format!("status: {status}"))
            ),
        );
        if with_shadow == text {
            return Err(fail("the operation has no interface binding to shadow"));
        }
        std::fs::write(&operations, with_shadow).map_err(fail)
    }
}

/// What the scenario holds: a machine that reads through the interface, a
/// machine whose operation gained the request binding through shadow, both
/// over one application, and the answer the interface gave for the measured
/// input.
pub struct Promoted {
    _app: TestApp,
    interface: Machine,
    request: Machine,
    reference: Value,
}

/// Creates the two line items the application is not seeded with, through the
/// application's own interface, so that five distinct inputs exist.
async fn seed_more_items(app: &TestApp) -> Result<(), BenchError> {
    for item in ["L-110", "L-111"] {
        let body = json!({"description": "Spare rail", "quantity": 1, "length_in": "12.5"});
        let response = app
            .http()
            .put(app.url(&format!("/api/orders/A-100/items/{item}")))
            .json(&body)
            .send()
            .await
            .map_err(fail)?;
        if !response.status().is_success() {
            return Err(fail(format!(
                "the application refused to add {item}: {}",
                response.status()
            )));
        }
    }
    Ok(())
}

/// A read through the request binding that was gained through shadow, set
/// against the same read through the interface binding it replaced.
///
/// The operation is the read after promotion; the baseline is the read before
/// the request binding existed. The overhead the harness reports is therefore
/// negative: it is the time saved. [`speedup`] gives the factor.
#[derive(Debug, Clone, Copy, Default)]
pub struct PromotionSpeedup;

impl Scenario for PromotionSpeedup {
    type State = Promoted;

    fn name(&self) -> &str {
        PROMOTION_SPEEDUP
    }

    async fn setup(&self) -> Result<Promoted, BenchError> {
        let app = TestApp::start().await.map_err(fail)?;
        seed_more_items(&app).await?;
        let address = app.url("");
        let address = address.trim_end_matches('/');

        // The interface binding alone: the baseline.
        let interface = Machine::open(address).await?;
        interface.publish().await?;

        // The same operation, given a request binding in shadow.
        let request = Machine::open(address).await?;
        request.add_shadow_binding(address)?;
        request.publish().await?;

        // Live calls: each is answered by the interface binding, and the
        // shadow binding reads the same item beside it.
        let mut reference = Value::Null;
        for input in EVIDENCE {
            let held = interface.read(PROMOTION_SPEEDUP, input).await?;
            let answered = request.read(PROMOTION_SPEEDUP, input).await?;
            if held != answered {
                return Err(fail(format!(
                    "a call with a binding in shadow changed its answer: {held} against {answered}"
                )));
            }
            if input == MEASURED {
                reference = held;
            }
        }
        let report = request
            .run(&["shadow", "report", "example", "--tool", TOOL], true)
            .await?;
        let standing = &report["bindings"][0];
        if standing["mode"] != "http" || standing["promotable"] != true {
            return Err(fail(format!(
                "the shadow binding did not earn promotion: {report}"
            )));
        }
        let promoted = request
            .run(
                &[
                    "binding",
                    "promote",
                    "example",
                    "--tool",
                    TOOL,
                    "--mode",
                    "http",
                    "--position",
                    "2",
                ],
                false,
            )
            .await?;
        if promoted["promoted"] != true {
            return Err(fail(format!("the binding was not promoted: {promoted}")));
        }
        // The promotion changes the binding's status and nothing else; the
        // publish that follows is the person's act. The compiler orders an
        // operation's bindings by the rank of their modes, so the request
        // binding acts first with no edit by hand (package 4.6).
        request.publish().await?;

        // The application says how the promoted read reached it: as a
        // request for the item's JSON, and not as its page being opened.
        let before = app.seen().await.map_err(fail)?;
        let answer = request.read(PROMOTION_SPEEDUP, MEASURED).await?;
        let heard: Vec<String> = app
            .seen()
            .await
            .map_err(fail)?
            .into_iter()
            .skip(before.len())
            .collect();
        let json_read = format!("GET /api/orders/{}/items/{}", MEASURED.0, MEASURED.1);
        let page_read = format!("GET /orders/{}/items/{}", MEASURED.0, MEASURED.1);
        if answer != reference || !heard.contains(&json_read) || heard.contains(&page_read) {
            return Err(fail(format!(
                "the promoted read did not run as a request: it answered {answer} and the application heard {heard:?}"
            )));
        }
        Ok(Promoted {
            _app: app,
            interface,
            request,
            reference,
        })
    }

    async fn operation(&self, state: &Promoted) -> Result<(), BenchError> {
        let read = state.request.read(PROMOTION_SPEEDUP, MEASURED).await?;
        identical(&read, &state.reference, "the request binding")
    }

    fn baseline(&self, state: &Promoted) -> Option<impl Future<Output = Result<(), BenchError>>> {
        Some(async move {
            let read = state.interface.read(PROMOTION_SPEEDUP, MEASURED).await?;
            identical(&read, &state.reference, "the interface binding")
        })
    }
}

fn identical(read: &Value, reference: &Value, who: &str) -> Result<(), BenchError> {
    if read == reference {
        Ok(())
    } else {
        Err(BenchError::operation(
            PROMOTION_SPEEDUP,
            format!("{who} returned {read}, not what the interface returned first: {reference}"),
        ))
    }
}

/// How many times faster the request binding is than the interface binding,
/// by the median, as a ratio of the two medians; `None` for a result with no
/// baseline or no time.
#[must_use]
pub fn speedup(result: &ScenarioResult) -> Option<f64> {
    let baseline = result.baseline.as_ref()?;
    if result.operation.median_ns == 0 {
        return None;
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "a ratio of two timings in nanoseconds, shown to one decimal"
    )]
    Some(baseline.median_ns as f64 / result.operation.median_ns as f64)
}
