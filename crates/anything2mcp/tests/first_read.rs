//! Milestone 1, the first read: from target files in a project folder to a
//! value read from the test application, through the command line and the
//! HTTP pack.

mod common;

use common::Machine;
use example_app::harness::TestApp;
use serde_json::json;

const PLACEHOLDER: &str = "http://127.0.0.1:4010";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_target_is_added_linted_published_and_read_five_times_with_one_trace_signature() {
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("orders", &[(PLACEHOLDER, &app.url(""))]);
    let world = machine.open().await;

    // The drafts lint and compile, and the plan names the tools.
    let linted = world.ok(&["draft", "lint", "example"]).await;
    assert_eq!(linted["ok"], true, "{linted}");
    assert!(
        linted["tools"]
            .as_array()
            .unwrap()
            .contains(&json!("example_order_get")),
        "{linted}"
    );

    // Publishing is a person's act: an agent is refused, a person is not.
    let refused = world.agent(&["version", "publish", "example"]).await;
    assert_eq!(refused.finished.code, 3, "{:?}", refused.finished);
    assert_eq!(refused.error()["code"], "person_required");
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    assert_eq!(published.result()["version"], "1.0.0");
    assert!(published.result()["plan_hash"].is_string());

    // The tool is found, and called five times.
    let found = world.ok(&["tool", "search", "order"]).await;
    assert!(
        found["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "example_order_get" && tool["effect"] == "read"),
        "{found}"
    );
    let mut signatures = Vec::new();
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
        let result = ran.result();
        assert_eq!(result["status"], "completed");
        assert_eq!(result["mode"], "http");
        // The value is what the test application holds.
        assert_eq!(result["result"]["customer"], "Example Customer One");
        assert_eq!(result["result"]["status"], "draft");

        // The dispatch recorded a decision, an allow, about this action.
        let decision = ran.json["decision"].as_str().unwrap().to_owned();
        let explained = world.ok(&["policy", "explain", &decision]).await;
        assert_eq!(explained["answer"], "allow");
        assert_eq!(explained["resource"], "action:tool.call");

        // The run has a row, events in order, and a stored trace.
        let run = result["run_id"].as_str().unwrap().to_owned();
        let row = world.ok(&["run", "get", &run]).await;
        assert_eq!(row["status"], "completed");
        assert_eq!(row["result"], result["result"]);
        let trace = world.ok(&["trace", "show", &run]).await;
        let kinds: Vec<&str> = trace["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| event["kind"].as_str().unwrap())
            .collect();
        assert!(kinds.contains(&"binding_chosen"), "{kinds:?}");
        assert_eq!(kinds.last(), Some(&"state_changed"));
        assert!(trace["trace"].as_str().unwrap().starts_with("blob:"));
        signatures.push(trace["signature"].as_str().unwrap().to_owned());
    }
    assert!(
        signatures.windows(2).all(|pair| pair[0] == pair[1]),
        "{signatures:?}"
    );
    world.close().await;
}
