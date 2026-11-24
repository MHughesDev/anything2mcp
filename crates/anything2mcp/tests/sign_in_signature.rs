//! The proof of package 2.7: five identical runs of a signed-in read give
//! one trace signature (spec section 217, hypothesis H-16).
//!
//! The first of the five finds no session and has the keeper sign in; the
//! other four find one and use it. A trace signature is a hash over what
//! should not vary: step order, step kinds, and failure classes. If the
//! sign-in or the session's age got into it, the first run and the fifth
//! would differ, and a repair that compares traces would call a session's
//! first use a change. They do not differ. The token the keeper placed is in
//! no trace, and the application is told so by what it counts: five reads
//! sent, none refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use common::Machine;
use example_app::harness::{BEARER_TOKEN, TestApp};

const READ: &str = "example_order_get";
const ORDER: &str = r#"{"order_id": "A-100"}"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn five_signed_in_reads_give_one_trace_signature() {
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("secured", &[("http://127.0.0.1:4010", &app.url(""))]).on_disk();
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    let stored = world
        .agent(&[
            "secret",
            "set",
            "example/service/token",
            "--value",
            BEARER_TOKEN,
        ])
        .await;
    assert_eq!(stored.finished.code, 0, "{:?}", stored.finished);

    let mut signatures = Vec::new();
    let mut kinds_of_each = Vec::new();
    for round in 0..5 {
        let ran = world.agent(&["tool", "call", READ, "--input", ORDER]).await;
        assert_eq!(ran.finished.code, 0, "round {round}: {:?}", ran.finished);
        assert_eq!(ran.result()["status"], "completed", "round {round}");
        assert_eq!(
            ran.result()["result"]["customer"],
            "Example Customer One",
            "round {round}"
        );
        let run = ran.result()["run_id"].as_str().unwrap().to_owned();
        let trace = world.ok(&["trace", "show", &run]).await;
        signatures.push(trace["signature"].as_str().unwrap().to_owned());
        kinds_of_each.push(
            trace["events"]
                .as_array()
                .unwrap()
                .iter()
                .map(|event| event["kind"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>(),
        );
        // The token is in no part of what the trace shows.
        assert!(!trace.to_string().contains(BEARER_TOKEN), "round {round}");
    }

    assert!(
        signatures.windows(2).all(|pair| pair[0] == pair[1]),
        "{signatures:?}"
    );
    // The events are the same, in the same order, whether the keeper signed
    // in for the run or found a session held.
    assert!(
        kinds_of_each.windows(2).all(|pair| pair[0] == pair[1]),
        "{kinds_of_each:?}"
    );
    // Five reads reached the application with the token, and it refused none.
    assert_eq!(app.auth_report().await.unwrap().refused_tokens, 0);
    world.close().await;
}
