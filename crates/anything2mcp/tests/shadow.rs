//! A binding in shadow, through the product's commands (decisions 0207 and
//! 0219): the read of an order has its verified `http` binding and a
//! second `http` binding in shadow, through an endpoint of its own. Each
//! live call is answered by the verified binding, the shadow binding reads
//! beside it, `shadow report` counts the matches, and only a person
//! promotes the binding that earned it. The test application is asked what
//! it holds and what it heard.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use common::Machine;
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

    let world = machine.open().await;
    let linted = world.ok(&["draft", "lint", "example"]).await;
    assert_eq!(linted["ok"], true, "{linted}");
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);

    let held = app.stored_order("A-100").await.unwrap().unwrap();
    let before = app.seen().await.unwrap();
    for _ in 0..5 {
        let ran = world
            .agent(&[
                "tool",
                "call",
                "example_order_get",
                "--input",
                r#"{"order_id": "A-100"}"#,
            ])
            .await;
        assert_eq!(ran.finished.code, 0, "{:?}", ran.finished);
        assert_eq!(ran.result()["status"], "completed");
        // The caller is given what the application holds.
        assert_eq!(ran.result()["result"]["customer"], held.customer.as_str());
    }
    // The application heard each read twice: once through each binding.
    let heard: Vec<String> = app
        .seen()
        .await
        .unwrap()
        .into_iter()
        .skip(before.len())
        .filter(|line| line == "GET /api/orders/A-100")
        .collect();
    assert_eq!(heard.len(), 10, "{heard:?}");

    let report = world
        .ok(&["shadow", "report", "example", "--tool", "example_order_get"])
        .await;
    let standing = &report["bindings"][0];
    assert_eq!(standing["mode"], "http", "{report}");
    assert_eq!(standing["matches"], 5, "{report}");
    assert_eq!(standing["promotable"], true, "{report}");
    assert!(!report.to_string().contains(held.customer.as_str()));

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
