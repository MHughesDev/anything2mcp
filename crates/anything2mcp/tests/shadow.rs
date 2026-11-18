//! A binding in shadow, through the product's commands (decisions 0207,
//! 0219, and 0251): the read of an order has its verified `http` binding
//! and a second `http` binding in shadow, through an endpoint of its own.
//! Each live call is answered by the verified binding, the shadow binding
//! reads beside it, `shadow report` counts the distinct inputs that matched
//! in production, one order called again is one input, and only a person
//! promotes the binding that earned it. The test application is asked what
//! it holds and what it heard.
//!
//! The test application holds three orders, and five distinct inputs are
//! needed by default. The threshold is a protected setting, lowered here to
//! three from an installation file taken as an administrator's
//! (`EngineOptions::with_installation_kept_by_an_administrator_for_tests`,
//! decision 0241), as an administrator of a real installation would.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;

use a2m_actions::{Paths, PersonSurface};
use anything2mcp::{Engine, EngineOptions};
use common::{Machine, World};
use example_app::harness::TestApp;

const PLACEHOLDER: &str = "http://127.0.0.1:4010";

/// A second endpoint that reads the same order: its own name gives the
/// binding that uses it a digest of its own.
const SPARE_ENDPOINT: &str = r#"---
endpoint: order_get_spare
surface: api
method: GET
path: "/api/orders/{order_id}"
auth: none
effect: read
headers: {accept: "application/json"}
responses: {absent: [404]}
response_schema:
  type: object
  required: [order_id, customer, status, items]
  properties:
    order_id: {type: string}
    customer: {type: string}
    status: {type: string, enum: [draft, submitted]}
    items: {type: array, items: {type: object}}
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shadow_binding_reads_beside_the_verified_one_and_a_person_promotes_it() {
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("orders", &[(PLACEHOLDER, &app.url(""))]);
    let target = machine.project.path().join("targets").join("example");
    let endpoints = target.join("endpoints").join("orders.yaml");
    let mut text = std::fs::read_to_string(&endpoints).unwrap();
    text.push_str(SPARE_ENDPOINT);
    std::fs::write(&endpoints, text).unwrap();
    let operations = target.join("operations").join("orders.yaml");
    let verified = std::fs::read_to_string(&operations).unwrap();
    let with_shadow = verified.replace(
        "    endpoint: order_get\n",
        "    endpoint: order_get\n  - mode: http\n    verb: read\n    status: shadow\n    endpoint: order_get_spare\n",
    );
    assert_ne!(with_shadow, verified);
    std::fs::write(&operations, &with_shadow).unwrap();

    // The administrator's installation file lowers the threshold to the
    // three orders the application holds.
    let installation = Paths::under(machine.home.path()).installation_config;
    std::fs::create_dir_all(&installation).unwrap();
    std::fs::write(
        installation.join("settings.toml"),
        "[engine.shadow.matches_to_promote]\nread = 3\n",
    )
    .unwrap();
    let mut options = EngineOptions::of_machine(
        Paths::under(machine.home.path()),
        machine.environment(),
        Arc::clone(&machine.surface) as Arc<dyn PersonSurface>,
    )
    .with_installation_kept_by_an_administrator_for_tests();
    options.project = Some(machine.project.path().to_path_buf());
    options.place = machine.place;
    options.key_store.clone_from(&machine.key_store);
    let clock = Arc::clone(&options.clock);
    let world = World {
        engine: Engine::open(options).await.expect("the settings load"),
        surface: Arc::clone(&machine.surface),
        clock,
    };
    let linted = world.ok(&["draft", "lint", "example"]).await;
    assert_eq!(linted["ok"], true, "{linted}");
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);

    let held = app.stored_order("A-100").await.unwrap().unwrap();
    let before = app.seen().await.unwrap();
    let call = |order: &'static str| {
        let world = &world;
        async move {
            let input = format!(r#"{{"order_id": "{order}"}}"#);
            // Shadow evidence counts only from production, which the
            // compiler implies where the target does not declare it.
            let ran = world
                .agent(&[
                    "tool",
                    "call",
                    "example_order_get",
                    "--environment",
                    "production",
                    "--input",
                    &input,
                ])
                .await;
            assert_eq!(ran.finished.code, 0, "{:?}", ran.finished);
            assert_eq!(ran.result()["status"], "completed");
            ran.result()["result"]["customer"].clone()
        }
    };
    let report = || {
        let world = &world;
        async move {
            world
                .ok(&["shadow", "report", "example", "--tool", "example_order_get"])
                .await
        }
    };
    // One order called twice: two matches, one distinct input.
    for _ in 0..2 {
        // The caller is given what the application holds.
        assert_eq!(call("A-100").await, held.customer.as_str());
    }
    let reported = report().await;
    let standing = &reported["bindings"][0];
    assert_eq!(standing["mode"], "http", "{reported}");
    assert_eq!(standing["matches"], 2, "{reported}");
    assert_eq!(standing["distinct_inputs"], 1, "{reported}");
    assert_eq!(standing["threshold"], 3, "{reported}");
    assert_eq!(standing["promotable"], false, "{reported}");
    assert_eq!(standing["not_promotable"], "too_few", "{reported}");
    // Two more orders: three distinct inputs that matched in production.
    call("A-101").await;
    call("A-102").await;
    // The application heard each read twice: once through each binding.
    let heard: Vec<String> = app
        .seen()
        .await
        .unwrap()
        .into_iter()
        .skip(before.len())
        .collect();
    for (order, times) in [("A-100", 4), ("A-101", 2), ("A-102", 2)] {
        let line = format!("GET /api/orders/{order}");
        assert_eq!(
            heard.iter().filter(|seen| **seen == line).count(),
            times,
            "{heard:?}"
        );
    }

    let reported = report().await;
    let standing = &reported["bindings"][0];
    assert_eq!(standing["matches"], 4, "{reported}");
    assert_eq!(standing["distinct_inputs"], 3, "{reported}");
    assert_eq!(
        standing["evidence_from"],
        serde_json::json!(["production"]),
        "{reported}"
    );
    assert_eq!(standing["against_verified_now"], true, "{reported}");
    // The cost is in the rate budget's units, one per step: each shadow
    // read is three steps, of which one reached the application.
    assert_eq!(standing["environments"][0]["requests"], 12, "{reported}");
    assert_eq!(standing["promotable"], true, "{reported}");
    let shown = reported.to_string();
    assert!(!shown.contains(held.customer.as_str()), "{shown}");
    for order in ["A-100", "A-101", "A-102"] {
        assert!(!shown.contains(order), "{shown}");
    }

    let promote = [
        "binding",
        "promote",
        "example",
        "--tool",
        "example_order_get",
        "--mode",
        "http",
        "--position",
        "2",
    ];
    let refused = world.agent(&promote).await;
    assert_eq!(
        refused.error()["code"],
        "person_required",
        "{:?}",
        refused.finished
    );
    assert_eq!(std::fs::read_to_string(&operations).unwrap(), with_shadow);
    let promoted = world.person(&promote).await;
    assert_eq!(promoted.finished.code, 0, "{:?}", promoted.finished);
    assert_eq!(promoted.result()["promoted"], true);
    assert_eq!(
        std::fs::read_to_string(&operations).unwrap(),
        with_shadow.replace("    status: shadow\n", "    status: \"verified\"\n")
    );
    // Reading changed nothing the application holds.
    assert_eq!(app.stored_order("A-100").await.unwrap().unwrap(), held);
    assert!(app.writes().await.unwrap().is_empty());
    world.close().await;
}
