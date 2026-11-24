//! The web pack's whole path from a page to a read, as the product joins
//! its two crates (decision 0183): the probe's `web_snapshot` discoverer
//! looks at the page of one order of the test application; the drafter
//! writes, from its finding, a screen and a read of the table the page
//! shows; the drafts compile, leaving out any file that holds an open
//! question; and the read runs through the `ui` driver, prepared in a
//! posture that may only look. What it returns is compared with what the
//! application's control interface says it holds, and the application's
//! record of writes stays empty.
//!
//! Only this crate names both web crates, so only here can the probe's
//! finding reach the interface driver's drafting rules through one
//! registry: the registry of an engine opened as the product opens it.
//! It needs a browser, found as the product's other browser tests find
//! one (`A2M_TEST_BROWSER`, `A2M_TEST_BROWSER_NO_SANDBOX`); with none, it
//! fails by name, as they do.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::Path;
use std::time::Duration;

use a2m_core::{
    Archetype, Environment, Epoch, Failure, Finding, Mode, NoSecrets, PackPayload, Plan, RunId,
    SurfaceKind, SurfaceName, SurfacePlan, TargetName, Value, Values,
};
use a2m_files::{CompileMode, DraftReport, Drafter, RelPath, TargetRoot};
use a2m_provider::{
    Authority, BoxFuture, CheckedPaths, Deadline, Destinations, DiscoveryScope, FindingSink,
    Posture, PrepareContext, Registry, SandboxPort, StepFailure, StepInput, StepOutcome,
    SurfaceDescription, SurfaceRef,
};
use common::{Folder, Machine};
use example_app::harness::TestApp;

const READ: &str = "example_orders_a_100_line_items_list";
const PAGE: &str = "/orders/A-100";
/// The reads drafted from the screen two orders' pages share.
const BY_KEY: &str = "example_orders_line_items_list";
const VALUES: &str = "example_orders_get";

/// Keeps what a discoverer finds.
struct Kept(Vec<Finding>);

impl FindingSink for Kept {
    fn found(&mut self, finding: Finding) -> Result<(), Failure> {
        self.0.push(finding);
        Ok(())
    }
}

/// The port the probe's discoverer is handed. It drives its own browser,
/// and sends no step through a port.
struct NoPort;

impl SandboxPort for NoPort {
    fn act<'a>(
        &'a self,
        _step: &'a a2m_core::Step,
        _input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async { panic!("the probe's discoverer sends no step through a port") })
    }
}

fn web_surface(origin: &str) -> SurfaceRef {
    SurfaceRef {
        target: TargetName::new("example").unwrap(),
        environment: Environment::new("test").unwrap(),
        surface: SurfacePlan {
            name: SurfaceName::new("app").unwrap(),
            kind: SurfaceKind::new("web").unwrap(),
            address: origin.to_owned(),
            same_session_as: None,
            stands: None,
            rate: None,
            programs: Vec::new(),
            payload: PackPayload::none(),
        },
    }
}

/// Every file of a target folder, in order of path.
fn files_of(root: &Path) -> Vec<(RelPath, Vec<u8>)> {
    fn walk(root: &Path, at: &Path, out: &mut Vec<(RelPath, Vec<u8>)>) {
        let mut entries: Vec<_> = std::fs::read_dir(at)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for entry in entries {
            if entry.is_dir() {
                walk(root, &entry, out);
            } else {
                let parts: Vec<String> = entry
                    .strip_prefix(root)
                    .unwrap()
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.push((
                    RelPath::new(&parts.join("/")).unwrap(),
                    std::fs::read(&entry).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}

/// Hands `web_snapshot` the pages of `paths`, at depth 0, and keeps what
/// it finds.
async fn discover(registry: &Registry, origin: &str, paths: &[&str]) -> Vec<Finding> {
    discover_with(registry, "web_snapshot", origin, paths, None).await
}

/// Hands the probe's discoverer `name` the pages of `paths`, at depth 0,
/// each looked at once `ready_text` shows where it is given, and keeps
/// what it finds.
async fn discover_with(
    registry: &Registry,
    name: &str,
    origin: &str,
    paths: &[&str],
    ready_text: Option<&str>,
) -> Vec<Finding> {
    let description = SurfaceDescription {
        kind: SurfaceKind::new("web").unwrap(),
        archetype: Archetype::Interface,
        address: origin.to_owned(),
        facts: Values::new(),
    };
    let discoverer = registry
        .discoverers_for(&description)
        .into_iter()
        .find(|handle| handle.declared().name.as_str() == name)
        .unwrap_or_else(|| panic!("the probe's discoverer `{name}` is registered"));
    let surface = web_surface(origin);
    let mut focus = Values::new();
    focus.insert(
        "paths",
        paths
            .iter()
            .map(|path| Value::from(*path))
            .collect::<Vec<_>>(),
    );
    if let Some(text) = ready_text {
        focus.insert("ready_text", text);
    }
    let mut bounds = discoverer.declared().bounds;
    bounds.depth = 0;
    let scope = DiscoveryScope {
        surface: &surface,
        bounds,
        deadline: Deadline::after(Duration::from_secs(90)),
        paths: &CheckedPaths::none(),
        redact: &NoSecrets,
        focus: &focus,
    };
    let mut kept = Kept(Vec::new());
    discoverer
        .discover(&NoPort, &scope, &mut kept)
        .await
        .unwrap_or_else(|failure| panic!("{failure}"));
    kept.0
}

/// The `screen` findings whose subject is the address with `path`.
fn screens_at<'a>(findings: &'a [Finding], path: &str) -> Vec<&'a Finding> {
    findings
        .iter()
        .filter(|finding| {
            finding.kind().as_str() == "screen"
                && matches!(finding.subject(), a2m_core::Subject::Address(at) if at.path() == path)
        })
        .collect()
}

/// Drafts the findings into a target folder that holds only the target
/// file, and compiles the drafts, leaving out each file that holds an open
/// question.
fn draft_and_compile(
    machine: &Machine,
    registry: &Registry,
    findings: &[Finding],
) -> (Folder, Plan) {
    let folder = Folder::new("web-drafts");
    std::fs::write(
        folder.path().join("target.yaml"),
        std::fs::read_to_string(machine.project.path().join("targets/example/target.yaml"))
            .unwrap(),
    )
    .unwrap();
    let root = TargetRoot::new(folder.path());
    let set = registry.compiler_set();
    let report: DraftReport = Drafter::new(set, &root).draft(findings).unwrap();
    assert!(
        report.refused.is_empty() && report.conflicts.is_empty(),
        "{report:?}"
    );
    answer_what_an_answer_may_leave_out(folder.path(), &report);
    let files: Vec<(RelPath, Vec<u8>)> = files_of(folder.path())
        .into_iter()
        .filter(|(file, _)| !file.as_str().starts_with("held/"))
        .filter(|(file, _)| !report.open_questions.iter().any(|open| open.file == *file))
        .collect();
    let sources =
        a2m_files::load_files(&root, &files, set).unwrap_or_else(|problems| panic!("{problems:?}"));
    let plan = a2m_files::compile(&sources, set, CompileMode::Draft, a2m_files::DRAFT_VERSION)
        .unwrap_or_else(|problems| panic!("{problems:?}"))
        .plan;
    (folder, plan)
}

/// A person's answer to the one question a drafted page request holds
/// about what its answer may leave out (decisions 0322 and 0341; a capture
/// never states `required`): the held page request moves into place with
/// `required` listing every value the answer's `properties` declare, and
/// the read held with it moves along. A held file with another question
/// stays held.
fn answer_what_an_answer_may_leave_out(folder: &std::path::Path, report: &DraftReport) {
    let mut answered = false;
    for held in &report.held {
        let from = folder.join(held.as_str());
        let mut text = std::fs::read_to_string(&from).unwrap();
        let Some(start) = text.find("required: {") else {
            continue;
        };
        let end = start + text[start..].find('\n').unwrap();
        let properties = text.find("properties:\n").unwrap();
        let names: Vec<&str> = text[properties..]
            .lines()
            .skip(1)
            .take_while(|line| line.starts_with("    "))
            .filter(|line| !line.starts_with("     "))
            .map(|line| line.trim().trim_end_matches(':'))
            .collect();
        text.replace_range(start..end, &format!("required: [{}]", names.join(", ")));
        let to = folder.join(held.as_str().strip_prefix("held/").unwrap());
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(&to, text).unwrap();
        std::fs::remove_file(&from).unwrap();
        answered = true;
    }
    if !answered {
        return;
    }
    // The read drafted over the request holds no question of its own.
    for held in &report.held {
        let from = folder.join(held.as_str());
        if from.exists()
            && !std::fs::read_to_string(&from)
                .unwrap()
                .contains("open_question")
        {
            let to = folder.join(held.as_str().strip_prefix("held/").unwrap());
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::rename(&from, &to).unwrap();
        }
    }
}

/// Runs the `act` steps of a read's `ui` binding through the `ui` driver
/// of the registry, prepared in a posture that may only look: its tab
/// observes and cannot write. Every argument of a drafted read is
/// `input.<name>`, evaluated here from `inputs` as the engine would.
async fn read_through_ui(
    registry: &Registry,
    plan: &Plan,
    origin: &str,
    name: &str,
    inputs: &Values,
) -> Result<Values, StepFailure> {
    read_through(registry, plan, origin, name, "ui", inputs).await
}

/// The same, through the binding of the mode named.
async fn read_through(
    registry: &Registry,
    plan: &Plan,
    origin: &str,
    name: &str,
    mode: &str,
    inputs: &Values,
) -> Result<Values, StepFailure> {
    let read = plan
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == name)
        .unwrap_or_else(|| panic!("no `{name}` in the compiled plan"));
    assert_eq!(read.effect, a2m_core::EffectClass::Read);
    let ui = Mode::new(mode).unwrap();
    let binding = read
        .bindings
        .iter()
        .find(|binding| binding.mode == ui)
        .unwrap_or_else(|| panic!("`{name}` has no `{mode}` binding"));
    let handle = registry.driver(&ui).unwrap();
    let mut surface = plan.surfaces[binding.surface.get()].clone();
    surface.address = origin.to_owned();
    let surface = SurfaceRef {
        target: plan.header.target.clone(),
        environment: Environment::new("test").unwrap(),
        surface,
    };
    let destinations = Destinations::new([origin.to_owned()]);
    let supervisor = handle.supervisor(Vec::new());
    let cx = PrepareContext {
        run: RunId::from_bytes([7; 16]),
        epoch: Epoch::new(1),
        surface: &surface,
        plan,
        posture: Posture::Sandbox {
            test_records: false,
        },
        deadline: Deadline::after(Duration::from_secs(60)),
        redact: &NoSecrets,
        destinations: &destinations,
        paths: &CheckedPaths::none(),
        supervisor: &supervisor,
    };
    let mut prepared = handle
        .prepare(&cx, None)
        .await
        .unwrap_or_else(|failure| panic!("{failure}"));
    let mut values = Values::new();
    let mut failed = None;
    for step in &binding.act {
        let mut args = Values::new();
        for arg in &step.args {
            let source = &plan.expression(arg.value).unwrap().source;
            let input = source
                .strip_prefix("input.")
                .unwrap_or_else(|| panic!("a drafted read's argument is `{source}`"));
            if let Some(value) = inputs.get(input) {
                args.insert(arg.name.clone(), value.clone());
            }
        }
        let input = StepInput::new(&args, Deadline::after(Duration::from_secs(60)));
        match handle
            .act(&mut prepared, step, &input, Authority::Read)
            .await
        {
            Ok(StepOutcome::Done(observed)) => {
                for (name, value) in observed.values {
                    values.insert(name, value);
                }
            }
            Ok(StepOutcome::Absent(_)) => panic!("the order's page was absent"),
            Err(failure) => {
                failed = Some(failure);
                break;
            }
        }
    }
    assert!(
        handle.release(prepared, failed.is_some()).await.complete,
        "the driver did not release cleanly"
    );
    failed.map_or(Ok(values), Err)
}

/// One column of the order's line items, as the control interface holds it.
fn column(
    stored: &example_app::store::StoredOrder,
    of: &dyn Fn(&example_app::store::StoredItem) -> String,
) -> Value {
    Value::List(
        stored
            .items
            .values()
            .map(|item| Value::from(of(item)))
            .collect(),
    )
}

/// An engine as the product opens it, for its registry: both web crates,
/// joined by the one handle on their browsers.
async fn product_engine(origin: &str) -> (Machine, common::World) {
    let machine = Machine::new("pages", &[("http://127.0.0.1:4010", origin)]);
    // The browsers' profiles live in the data directory's own folder,
    // which the product hands the web pack only when it exists.
    std::fs::create_dir_all(a2m_actions::Paths::under(machine.home.path()).profiles).unwrap();
    let world = machine.open().await;
    (machine, world)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_that_shows_an_order_is_scouted_drafted_and_read_through_the_web_pack() {
    let app = TestApp::start().await.unwrap();
    let origin = app.url("");
    let (machine, world) = product_engine(&origin).await;
    let registry = world.engine.kernel().providers();

    // 1. The probe looks at the one page, and no further: one screen of
    // the order's page.
    let found = discover(registry, &origin, &[PAGE]).await;
    assert_eq!(screens_at(&found, PAGE).len(), 1, "{found:?}");

    // 2 and 3. Drafted and compiled.
    let (_folder, plan) = draft_and_compile(&machine, registry, &found);

    // 4. Read through the `ui` driver. The read takes no input: the page
    // is the one order's.
    let values = read_through_ui(registry, &plan, &origin, READ, &Values::new())
        .await
        .unwrap_or_else(|failure| panic!("{failure}"));

    // 5. The same as what the application's control interface,
    // `/__control/orders/A-100`, says it holds, column by column.
    let stored = app.stored_order("A-100").await.unwrap().unwrap();
    assert_eq!(stored.items.len(), 3);
    assert_eq!(
        values.get("item_code"),
        Some(&column(&stored, &|item| item.item_code.clone())),
        "{values:?}"
    );
    assert_eq!(
        values.get("description"),
        Some(&column(&stored, &|item| item.description.clone())),
        "{values:?}"
    );
    assert_eq!(
        values.get("quantity"),
        Some(&column(&stored, &|item| item.quantity.to_string())),
        "{values:?}"
    );
    assert_eq!(
        values.get("length_in"),
        Some(&column(&stored, &|item| {
            example_app::store::format_length_in(item.length_sixteenths)
        })),
        "{values:?}"
    );

    // Nothing was written.
    assert!(app.writes().await.unwrap().is_empty());
    world.close().await;
}

/// The web pack's matched screens (decision 0187), proven in the product:
/// two pages of one screen give one screen at the address with a
/// placeholder; the reads drafted from it take the order's key, read the
/// order the key names, and refuse a key that is not one plain segment
/// before the application hears a request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_pages_of_one_screen_are_drafted_into_reads_by_key_in_the_product() {
    let app = TestApp::start().await.unwrap();
    let origin = app.url("");
    let (machine, world) = product_engine(&origin).await;
    let registry = world.engine.kernel().providers();

    let found = discover(registry, &origin, &["/orders/A-100", "/orders/A-101"]).await;
    assert_eq!(
        screens_at(&found, "/orders/{order_id}").len(),
        1,
        "{found:?}"
    );
    for page in ["/orders/A-100", "/orders/A-101"] {
        assert!(screens_at(&found, page).is_empty(), "{page}: {found:?}");
    }

    let (_folder, plan) = draft_and_compile(&machine, registry, &found);
    for name in [BY_KEY, VALUES] {
        let read = plan
            .operations
            .iter()
            .find(|operation| operation.name.as_str() == name)
            .unwrap_or_else(|| panic!("no `{name}` in the compiled plan"));
        let inputs: Vec<&str> = read
            .inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect();
        assert_eq!(inputs, ["order_id"], "{name}");
    }

    // The second order, read by its key, is what the control interface
    // holds for it.
    let key = Values::new().with("order_id", "A-101");
    let stored = app.stored_order("A-101").await.unwrap().unwrap();
    let lines = read_through_ui(registry, &plan, &origin, BY_KEY, &key)
        .await
        .unwrap_or_else(|failure| panic!("{failure}"));
    assert_eq!(
        lines.get("item_code"),
        Some(&column(&stored, &|item| item.item_code.clone())),
        "{lines:?}"
    );
    assert_eq!(
        lines.get("quantity"),
        Some(&column(&stored, &|item| item.quantity.to_string())),
        "{lines:?}"
    );
    let order = read_through_ui(registry, &plan, &origin, VALUES, &key)
        .await
        .unwrap_or_else(|failure| panic!("{failure}"));
    assert_eq!(
        order.get("customer"),
        Some(&Value::from(stored.customer.clone())),
        "{order:?}"
    );
    assert_eq!(
        order.get("status"),
        Some(&Value::from(format!("{:?}", stored.status))),
        "{order:?}"
    );

    // A key that is not one plain segment is refused, unsent, and the
    // application hears no request.
    for refused in ["A-101/items/L-101", "..", "A-101?status=x", "%2e%2e"] {
        let before = app.seen().await.unwrap();
        let key = Values::new().with("order_id", refused);
        for name in [BY_KEY, VALUES] {
            let stopped = read_through_ui(registry, &plan, &origin, name, &key)
                .await
                .unwrap_err();
            assert_eq!(
                (stopped.failure.class(), stopped.failure.code().as_str()),
                (a2m_core::FailureClass::Input, "address_value_refused"),
                "{refused:?}: {stopped}"
            );
            assert_eq!(stopped.sent, a2m_provider::Sent::NotSent);
        }
        assert_eq!(
            app.heard_since(&before).await.unwrap(),
            Vec::<String>::new(),
            "{refused:?}"
        );
    }

    // Nothing was written.
    assert!(app.writes().await.unwrap().is_empty());
    world.close().await;
}

/// The totals of an order as the control interface reports what it holds.
async fn totals_of(app: &TestApp, order: &str) -> Values {
    let stored = app.stored_order(order).await.unwrap().unwrap();
    let quantity: u64 = stored
        .items
        .values()
        .map(|item| u64::from(item.quantity))
        .sum();
    let sixteenths: u64 = stored
        .items
        .values()
        .map(|item| u64::from(item.quantity) * u64::from(item.length_sixteenths))
        .sum();
    Values::new()
        .with("lines", i64::try_from(stored.items.len()).unwrap())
        .with("total_quantity", i64::try_from(quantity).unwrap())
        .with(
            "total_length_in",
            example_app::store::format_length_in(u32::try_from(sixteenths).unwrap()),
        )
}

/// The `page_http` mode through the product's registry (decision 0190):
/// the probe's `web_capture` looks at the pages of two orders' totals,
/// which ask for the totals by themselves; the drafter writes a read of
/// that request by key; the drafts compile, leaving out any file that
/// holds an open question; and the read runs through the `page_http`
/// driver in a posture that may only look, for each order, and returns
/// what the control interface says the order holds. A key that is not one
/// plain segment is refused before the application hears a request, and
/// nothing is written.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_two_pages_make_is_drafted_into_a_page_http_read_by_key_in_the_product() {
    const TOTALS: &str = "example_api_orders_totals_get";
    let app = TestApp::start().await.unwrap();
    let origin = app.url("");
    let (machine, world) = product_engine(&origin).await;
    let registry = world.engine.kernel().providers();

    let found = discover_with(
        registry,
        "web_capture",
        &origin,
        &["/orders/A-100/summary", "/orders/A-101/summary"],
        Some("Loaded."),
    )
    .await;
    let exchanges: Vec<&Finding> = found
        .iter()
        .filter(|finding| {
            finding.kind().as_str() == "exchange"
                && matches!(finding.subject(), a2m_core::Subject::Address(at) if at.path() == "/api/orders/{order_id}/totals")
        })
        .collect();
    assert_eq!(exchanges.len(), 1, "{found:?}");

    let (_folder, plan) = draft_and_compile(&machine, registry, &found);
    let read = plan
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == TOTALS)
        .unwrap_or_else(|| panic!("no `{TOTALS}` in the compiled plan"));
    assert_eq!(read.effect, a2m_core::EffectClass::Read);
    assert!(
        read.bindings
            .iter()
            .all(|binding| binding.act.iter().all(|step| !step.commits))
    );

    for order in ["A-100", "A-101"] {
        let key = Values::new().with("order_id", order);
        let before = app.seen().await.unwrap().len();
        let values = read_through(registry, &plan, &origin, TOTALS, "page_http", &key)
            .await
            .unwrap_or_else(|failure| panic!("{order}: {failure}"));
        let wanted = totals_of(&app, order).await;
        for (name, value) in wanted.iter() {
            assert_eq!(values.get(name), Some(value), "{order}: {values:?}");
        }
        let seen = app.seen().await.unwrap();
        assert!(
            seen[before..].contains(&format!("GET /api/orders/{order}/totals")),
            "{seen:?}"
        );
        assert!(
            seen[before..].iter().all(|line| line.starts_with("GET ")),
            "{seen:?}"
        );
    }

    for refused in ["A-100/x", "..", "A-100?view=all", "%2e%2e"] {
        let before = app.seen().await.unwrap();
        let key = Values::new().with("order_id", refused);
        let stopped = read_through(registry, &plan, &origin, TOTALS, "page_http", &key)
            .await
            .unwrap_err();
        assert_eq!(
            (stopped.failure.class(), stopped.failure.code().as_str()),
            (a2m_core::FailureClass::Input, "address_value_refused"),
            "{refused:?}: {stopped}"
        );
        assert_eq!(stopped.sent, a2m_provider::Sent::NotSent);
        assert_eq!(
            app.heard_since(&before).await.unwrap(),
            Vec::<String>::new(),
            "{refused:?}"
        );
    }

    assert!(app.writes().await.unwrap().is_empty());
    world.close().await;
}
