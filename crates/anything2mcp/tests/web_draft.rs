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
    Archetype, Environment, Epoch, Failure, Finding, Mode, NoSecrets, PackPayload, RunId,
    SurfaceKind, SurfaceName, SurfacePlan, TargetName, Value, Values,
};
use a2m_files::{CompileMode, DraftReport, Drafter, RelPath, TargetRoot};
use a2m_provider::{
    Authority, BoxFuture, CheckedPaths, Deadline, Destinations, DiscoveryScope, FindingSink,
    Posture, PrepareContext, SandboxPort, StepFailure, StepInput, StepOutcome, SurfaceDescription,
    SurfaceRef,
};
use common::{Folder, Machine};
use example_app::harness::TestApp;

const READ: &str = "example_orders_a_100_line_items_list";
const PAGE: &str = "/orders/A-100";

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_that_shows_an_order_is_scouted_drafted_and_read_through_the_web_pack() {
    let app = TestApp::start().await.unwrap();
    let origin = app.url("");
    // An engine as the product opens it, for its registry: both web
    // crates, joined by the one handle on their browsers.
    let machine = Machine::new("pages", &[("http://127.0.0.1:4010", &origin)]);
    // The browsers' profiles live in the data directory's own folder,
    // which the product hands the web pack only when it exists.
    std::fs::create_dir_all(a2m_actions::Paths::under(machine.home.path()).profiles).unwrap();
    let world = machine.open().await;
    let registry = world.engine.kernel().providers();

    // 1. The probe looks at the one page, and no further.
    let description = SurfaceDescription {
        kind: SurfaceKind::new("web").unwrap(),
        archetype: Archetype::Interface,
        address: origin.clone(),
        facts: Values::new(),
    };
    let discoverer = registry
        .discoverers_for(&description)
        .into_iter()
        .find(|handle| handle.declared().name.as_str() == "web_snapshot")
        .expect("the probe's snapshot discoverer is registered");
    let surface = web_surface(&origin);
    let mut focus = Values::new();
    focus.insert("paths", vec![Value::from(PAGE)]);
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
    // The start page, which leads to the list of orders, and the page
    // asked for: one screen of the order's page.
    let screens: Vec<&Finding> = kept
        .0
        .iter()
        .filter(|finding| {
            finding.kind().as_str() == "screen"
                && matches!(finding.subject(), a2m_core::Subject::Address(at) if at.path() == PAGE)
        })
        .collect();
    assert_eq!(screens.len(), 1, "{:?}", kept.0);

    // 2. Drafted from the findings into a target folder that holds only
    // the target file.
    let folder = Folder::new("web-drafts");
    std::fs::write(
        folder.path().join("target.yaml"),
        std::fs::read_to_string(machine.project.path().join("targets/example/target.yaml"))
            .unwrap(),
    )
    .unwrap();
    let root = TargetRoot::new(folder.path());
    let set = registry.compiler_set();
    let report: DraftReport = Drafter::new(set, &root).draft(&kept.0).unwrap();
    assert!(
        report.refused.is_empty() && report.conflicts.is_empty(),
        "{report:?}"
    );

    // 3. Compiled, leaving out each file that holds an open question.
    let files: Vec<(RelPath, Vec<u8>)> = files_of(folder.path())
        .into_iter()
        .filter(|(file, _)| !report.open_questions.iter().any(|open| open.file == *file))
        .collect();
    let sources =
        a2m_files::load_files(&root, &files, set).unwrap_or_else(|problems| panic!("{problems:?}"));
    let plan = a2m_files::compile(&sources, set, CompileMode::Draft, a2m_files::DRAFT_VERSION)
        .unwrap_or_else(|problems| panic!("{problems:?}"))
        .plan;
    let read = plan
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == READ)
        .unwrap_or_else(|| panic!("no `{READ}` among {:?}", report.written));
    assert_eq!(read.effect, a2m_core::EffectClass::Read);
    let ui = Mode::new("ui").unwrap();
    let binding = read
        .bindings
        .iter()
        .find(|binding| binding.mode == ui)
        .unwrap();

    // 4. Read through the `ui` driver, in a run that may only look: its tab
    // observes and cannot write.
    let handle = registry.driver(&ui).unwrap();
    let mut surface = plan.surfaces[binding.surface.get()].clone();
    surface.address.clone_from(&origin);
    let surface = SurfaceRef {
        target: plan.header.target.clone(),
        environment: Environment::new("test").unwrap(),
        surface,
    };
    let destinations = Destinations::new([origin.clone()]);
    let supervisor = handle.supervisor(Vec::new());
    let cx = PrepareContext {
        run: RunId::from_bytes([7; 16]),
        epoch: Epoch::new(1),
        surface: &surface,
        plan: &plan,
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
        // The read takes no input: the page is the one order's.
        assert!(step.args.is_empty(), "{step:?}");
        let args = Values::new();
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
    if let Some(failure) = failed {
        panic!("{failure}");
    }

    // 5. The same as what the application's control interface,
    // `/__control/orders/A-100`, says it holds, column by column.
    let stored = app.stored_order("A-100").await.unwrap().unwrap();
    assert_eq!(stored.items.len(), 3);
    let column = |of: &dyn Fn(&example_app::store::StoredItem) -> String| {
        Value::List(
            stored
                .items
                .values()
                .map(|item| Value::from(of(item)))
                .collect(),
        )
    };
    assert_eq!(
        values.get("item_code"),
        Some(&column(&|item| item.item_code.clone())),
        "{values:?}"
    );
    assert_eq!(
        values.get("description"),
        Some(&column(&|item| item.description.clone())),
        "{values:?}"
    );
    assert_eq!(
        values.get("quantity"),
        Some(&column(&|item| item.quantity.to_string())),
        "{values:?}"
    );
    assert_eq!(
        values.get("length_in"),
        Some(&column(&|item| example_app::store::format_length_in(
            item.length_sixteenths
        ))),
        "{values:?}"
    );

    // Nothing was written.
    assert!(app.writes().await.unwrap().is_empty());
    world.close().await;
}
