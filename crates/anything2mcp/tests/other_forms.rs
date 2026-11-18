//! The HTTP pack's answers in forms other than JSON (decision 0217),
//! through the product: an XML view and a plain-text status line are read
//! through the real commands, and an answer that declares a document type
//! is refused before anything is read from it. The test application is
//! asked what it holds.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use common::Machine;
use example_app::harness::TestApp;
use serde_json::json;

const PLACEHOLDER: &str = "http://127.0.0.1:4010";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_xml_and_a_text_answer_are_read_through_the_product() {
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("forms", &[(PLACEHOLDER, &app.url(""))]);
    let world = machine.open().await;
    let linted = world.ok(&["draft", "lint", "example"]).await;
    assert_eq!(linted["ok"], true, "{linted}");
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);

    // The XML view of an order: what the application holds for it.
    let ran = world
        .agent(&[
            "tool",
            "call",
            "example_order_xml_get",
            "--input",
            r#"{"order_id": "A-100"}"#,
        ])
        .await;
    assert_eq!(ran.finished.code, 0, "{:?}", ran.finished);
    let read = &ran.result()["result"];
    let held = app.stored_order("A-100").await.unwrap().unwrap();
    assert_eq!(read["customer"], held.customer.as_str(), "{read}");
    assert_eq!(read["status"], serde_json::to_value(held.status).unwrap(), "{read}");
    let codes: Vec<&str> = held.items.keys().map(String::as_str).collect();
    assert_eq!(read["item_codes"], json!(codes), "{read}");
    let quantities: Vec<_> = held.items.values().map(|item| item.quantity).collect();
    assert_eq!(read["quantities"], json!(quantities), "{read}");

    // The status line: its counts are those of the orders the application
    // holds, and its newest order's customer is that order's.
    let ran = world
        .agent(&["tool", "call", "example_status_get", "--input", "{}"])
        .await;
    assert_eq!(ran.finished.code, 0, "{:?}", ran.finished);
    let line = &ran.result()["result"];
    let mut stored = 0;
    for number in 100..110 {
        if app
            .stored_order(&format!("A-{number}"))
            .await
            .unwrap()
            .is_some()
        {
            stored += 1;
        }
    }
    assert_eq!(line["orders"], stored, "{line}");
    assert_eq!(
        line["draft"].as_i64().unwrap() + line["submitted"].as_i64().unwrap(),
        stored,
        "{line}"
    );
    let newest = app
        .stored_order(line["newest"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(line["customer"], newest.customer.as_str(), "{line}");

    // An answer that declares a document type is refused by name.
    let refused = world
        .agent(&["tool", "call", "example_declared_get", "--input", "{}"])
        .await;
    assert_eq!(refused.error()["code"], "document_type_refused", "{:?}", refused.finished);
    assert!(app.writes().await.unwrap().is_empty());
    world.close().await;
}
