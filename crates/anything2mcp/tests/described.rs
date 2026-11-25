//! Described against observed (decision 0999), as the product joins the
//! HTTP pack and the web probe through one registry, against the test
//! application, which serves both a description of its JSON interface and
//! pages that ask that interface for their data.
//!
//! The first test reads the description with `http_description` and looks
//! at three pages with `web_capture`, and compares the two: the totals the
//! summary pages ask for are described and seen, `matched` at
//! `discovered`; every other described endpoint is `described_only` at
//! `inferred`; the lengths a page asks for are in no description, and are
//! named. It needs a browser, found as the product's other browser tests
//! find one. The second runs the scout of an HTTP target, whose stage 3
//! and report keep the comparison and whose unknowns ledger holds a
//! question for each entry that is not `matched`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::time::Duration;

use a2m_core::{
    AccessKind, ComparisonResult, CompiledSurface, Environment, EvidenceLevel, Failure, Finding,
    NoSecrets, PackPayload, SurfaceKind, SurfaceName, TargetName, Value, Values,
};
use a2m_provider::{
    Access, BoxFuture, CheckedPaths, Comparison, Deadline, DiscoveryScope, FindingSink, Registry,
    RootName, Roots, SandboxPort, StepFailure, StepInput, StepOutcome, SurfaceDescription,
    SurfaceRef,
};
use common::{Folder, Machine};
use example_app::harness::TestApp;
use serde_json::Value as Json;

/// Where the test application serves its description.
const DESCRIPTION_PATH: &str = "/api/openapi.json";

/// Keeps what a discoverer finds.
struct Kept(Vec<Finding>);

impl FindingSink for Kept {
    fn found(&mut self, finding: Finding) -> Result<(), Failure> {
        self.0.push(finding);
        Ok(())
    }
}

/// The port the discoverers here are handed: the description is read from
/// a file and the probe drives its own browser, so neither sends a step.
struct NoPort;

impl SandboxPort for NoPort {
    fn act<'a>(
        &'a self,
        _step: &'a a2m_core::Step,
        _input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async { panic!("no discoverer here sends a step through a port") })
    }
}

fn surface(kind: &str, address: &str) -> SurfaceRef {
    SurfaceRef {
        target: TargetName::new("example").unwrap(),
        environment: Environment::new("test").unwrap(),
        surface: CompiledSurface {
            name: SurfaceName::new(kind).unwrap(),
            kind: SurfaceKind::new(kind).unwrap(),
            address: address.to_owned(),
            same_session_as: None,
            stands: None,
            rate: None,
            programs: Vec::new(),
            payload: PackPayload::none(),
        },
    }
}

/// Runs the discoverer `name` of a surface of `kind` at `address`, with
/// the focus and the files given, and keeps what it finds.
async fn discover(
    registry: &Registry,
    (kind, access_kind): (&str, AccessKind),
    name: &str,
    address: &str,
    focus: &Values,
    paths: &CheckedPaths,
) -> Vec<Finding> {
    let description = SurfaceDescription {
        kind: SurfaceKind::new(kind).unwrap(),
        access_kind,
        address: address.to_owned(),
        facts: Values::new(),
    };
    let discoverer = registry
        .discoverers_for(&description)
        .into_iter()
        .find(|handle| handle.declared().name.as_str() == name)
        .unwrap_or_else(|| panic!("the discoverer `{name}` is registered"));
    let surface = surface(kind, address);
    let mut bounds = discoverer.declared().bounds;
    bounds.depth = 0;
    let scope = DiscoveryScope {
        surface: &surface,
        bounds,
        deadline: Deadline::after(Duration::from_secs(90)),
        paths,
        redact: &NoSecrets,
        focus,
    };
    let mut kept = Kept(Vec::new());
    discoverer
        .discover(&NoPort, &scope, &mut kept)
        .await
        .unwrap_or_else(|failure| panic!("{name}: {failure}"));
    kept.0
}

/// The entries of a comparison with the result, as verb and path.
fn named(comparison: &Comparison, result: ComparisonResult) -> Vec<(String, String)> {
    comparison
        .entries
        .iter()
        .filter(|entry| entry.result == result)
        .map(|entry| {
            let key = entry.key.as_ref().unwrap();
            (key.verb().to_owned(), key.path())
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn what_the_test_application_describes_is_set_against_what_its_pages_were_seen_to_ask() {
    let app = TestApp::start().await.unwrap();
    let origin = app.url("");
    let machine = Machine::new("pages", &[("http://127.0.0.1:4010", &origin)]);
    std::fs::create_dir_all(a2m_actions::Paths::under(machine.home.path()).profiles).unwrap();
    let world = machine.open().await;
    let registry = world.engine.kernel().providers();

    // The description the scout reads, as a file a person hands it.
    let inputs = Folder::new("inputs");
    let text = app
        .http()
        .get(app.url(DESCRIPTION_PATH))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    std::fs::write(inputs.path().join("openapi.json"), text).unwrap();
    let root = RootName::new("inputs").unwrap();
    let mut roots = Roots::none();
    roots
        .add(root.clone(), inputs.path(), Access::Read)
        .unwrap();
    let mut paths = CheckedPaths::none();
    paths.insert(
        "description",
        roots.check(&root, "openapi.json", Access::Read).unwrap(),
    );
    let mut findings = discover(
        registry,
        ("http", AccessKind::Request),
        "http_description",
        &origin,
        &Values::new(),
        &paths,
    )
    .await;
    let described = findings
        .iter()
        .filter(|finding| finding.kind().as_str() == "described_operation")
        .count();
    assert!(described > 5, "{findings:?}");

    // What two summary pages and a page of lengths ask for, seen.
    let mut focus = Values::new();
    focus.insert(
        "paths",
        [
            "/orders/A-100/summary",
            "/orders/A-101/summary",
            "/orders/A-100/lengths-by-query",
        ]
        .iter()
        .map(|path| Value::from(*path))
        .collect::<Vec<_>>(),
    );
    focus.insert("ready_text", "Loaded.");
    let seen = discover(
        registry,
        ("web", AccessKind::Interface),
        "web_capture",
        &origin,
        &focus,
        &CheckedPaths::none(),
    )
    .await;
    assert!(
        seen.iter()
            .any(|finding| finding.kind().as_str() == "endpoint"),
        "{seen:?}"
    );
    findings.extend(seen);

    let comparison = registry.compare(&findings);
    assert_eq!(
        named(&comparison, ComparisonResult::Matched),
        [("GET".to_owned(), "/api/orders/{}/totals".to_owned())],
        "{comparison:?}"
    );
    let matched = comparison
        .entries
        .iter()
        .find(|entry| entry.result == ComparisonResult::Matched)
        .unwrap();
    assert_eq!(matched.level, EvidenceLevel::Discovered, "{matched:?}");
    assert!(!matched.described.is_empty() && !matched.observed.is_empty());

    // Seen once, the lengths request keeps its literal path, and no
    // description states it. The pages themselves were seen too: the
    // description is of the JSON interface alone, so they are named as
    // well, and nothing else under `/api` is.
    let alone = named(&comparison, ComparisonResult::ObservedOnly);
    assert!(
        alone.contains(&("GET".to_owned(), "/api/orders/A-100/lengths".to_owned())),
        "{comparison:?}"
    );
    for page in [
        "/orders/A-100/summary",
        "/orders/A-101/summary",
        "/orders/A-100/lengths-by-query",
    ] {
        assert!(
            alone.contains(&("GET".to_owned(), page.to_owned())),
            "{page}: {comparison:?}"
        );
    }
    assert_eq!(
        alone
            .iter()
            .filter(|(_, path)| path.starts_with("/api/"))
            .count(),
        1,
        "{comparison:?}"
    );
    assert_eq!(
        comparison.count(ComparisonResult::DescribedOnly),
        described - 1,
        "{comparison:?}"
    );
    for entry in &comparison.entries {
        if entry.result == ComparisonResult::DescribedOnly {
            assert_eq!(entry.level, EvidenceLevel::Inferred, "{entry:?}");
            assert!(entry.observed.is_empty());
        }
    }
    assert_eq!(comparison.count(ComparisonResult::Unknown), 0);
    assert!(app.writes().await.unwrap().is_empty());
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scout_keeps_the_comparison_in_its_report_and_asks_of_what_it_could_not_match() {
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("described", &[]);
    let world = machine.open().await;
    world
        .ok(&[
            "target",
            "add",
            "example",
            "--address",
            &app.url(""),
            "--kind",
            "http",
        ])
        .await;
    let mut args = vec!["scout", "run", "example", "--wait-seconds", "10", "--fresh"];
    let mut report = Json::Null;
    for _ in 0..30 {
        report = world.ok(&args).await;
        if report.get("report_hash").is_some() {
            break;
        }
        assert_eq!(report["state"], "working", "{report}");
        args.truncate(5);
    }
    let compared = &report["content"]["compared"]["described_against_observed"];
    // The surface's own look sees its root, which no description states;
    // every operation the description states is at a place that look
    // covered and was not seen.
    let counts = &compared["counts"];
    assert_eq!(counts["matched"], 0, "{compared}");
    assert_eq!(counts["observed_only"], 2, "{compared}");
    assert_eq!(counts["unknown"], 0, "{compared}");
    let described_only = counts["described_only"].as_u64().unwrap();
    assert!(described_only > 5, "{compared}");
    let entries = compared["entries"].as_array().unwrap();
    for entry in entries {
        // The system's words only under the mark.
        assert!(
            entry["endpoint"]["untrusted"]["path"].is_string(),
            "{entry}"
        );
        if entry["result"] == "described_only" {
            assert_eq!(entry["level"], "inferred", "{entry}");
        }
        if entry["result"] == "observed_only" {
            assert_eq!(entry["level"], "discovered", "{entry}");
            assert_eq!(entry["endpoint"]["untrusted"]["path"], "/", "{entry}");
        }
    }

    // One question for each entry, scoped to its first finding, asked once
    // when the scout runs again.
    let scopes = |listed: &Json| -> Vec<String> {
        let mut scopes: Vec<String> = listed["unknowns"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|row| row["scope"].as_str().map(str::to_owned))
            .collect();
        scopes.sort();
        scopes
    };
    let listed = world.ok(&["unknown", "list", "example"]).await;
    let asked = scopes(&listed);
    for entry in entries {
        let first = entry["described"][0]
            .as_str()
            .or(entry["observed"][0].as_str())
            .unwrap();
        assert!(asked.contains(&first.to_owned()), "{first}: {listed}");
    }
    let mut args = vec!["scout", "run", "example", "--wait-seconds", "10", "--fresh"];
    for _ in 0..30 {
        if world.ok(&args).await.get("report_hash").is_some() {
            break;
        }
        args.truncate(5);
    }
    assert_eq!(
        scopes(&world.ok(&["unknown", "list", "example"]).await),
        asked
    );
    world.close().await;
}
