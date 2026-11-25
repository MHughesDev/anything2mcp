//! `run.trace` and `trace.export` (spec section 217; decision 0469): the
//! summary names each step and each kept capture and holds no value; the
//! export is one file in the project's working folder, read back before its
//! digest is given, with a registered secret never in it, and never written
//! through a link.

use std::sync::atomic::Ordering;

use a2m_actions::{CloseReason, Request, Response};
use serde_json::{Value as Json, json};

use super::kernel::engine::ORDER_GET;
use super::kernel::{Options, World};

fn code_of(response: &Response) -> String {
    response
        .error
        .as_ref()
        .unwrap_or_else(|| panic!("no error in {response:?}"))
        .code
        .clone()
}

/// A world whose reads keep every exchange, and whose read of an order
/// captures the application's answer.
async fn capturing() -> World {
    let world = World::ready_with(Options::with_user_settings(
        "[evidence]\ncapture = \"full\"\n",
    ))
    .await;
    world.hooks.tells_acting.store(true, Ordering::SeqCst);
    world
}

impl World {
    async fn read_an_order(&self) -> String {
        let read = self
            .ok(
                "tool.call",
                json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
            )
            .await;
        read["run_id"].as_str().unwrap().to_owned()
    }

    async fn export(&self, run: &str, project: &std::path::Path) -> Response {
        self.kernel
            .dispatch(
                Request::new("trace.export", json!({"run_id": run}), self.caller.clone())
                    .in_project(project.to_path_buf()),
            )
            .await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_runs_trace_is_summarized_by_step_with_its_captures_and_no_value() {
    let world = capturing().await;
    let run = world.read_an_order().await;

    let summary = world.ok("run.trace", json!({"run_id": run})).await;
    assert_eq!(summary["run_id"], run);
    assert_eq!(summary["tool"], ORDER_GET);
    assert_eq!(summary["status"], "completed");
    assert_eq!(summary["trace_check"], "whole");
    assert!(summary["signature"].is_string(), "{summary}");
    assert!(summary["trace"].as_str().unwrap().starts_with("blob:"));
    let steps = summary["steps"].as_array().unwrap();
    assert!(!steps.is_empty(), "{summary}");
    assert_eq!(summary["steps_total"], steps.len());
    assert_eq!(summary["records"], steps.len());
    assert!(
        steps
            .iter()
            .all(|step| step["kind"].is_string() && !step["outcome"].is_null()),
        "{summary}"
    );
    assert_eq!(steps[0]["operation"], ORDER_GET);
    // The read's answer was kept, and is named by its reference alone.
    let captures = summary["captures"].as_array().unwrap();
    assert!(
        captures.iter().any(|capture| capture["name"] == "response"
            && capture["blob"].as_str().unwrap().starts_with("blob:")),
        "{summary}"
    );
    // No value the system answered is in the summary.
    let text = summary.to_string();
    assert!(!text.contains("Example Customer One"), "{text}");

    // Another person's run is no run of theirs; an unknown run neither.
    let stranger = world.stranger();
    let theirs = world
        .call_as(&stranger, "run.trace", json!({"run_id": run}))
        .await;
    assert!(!theirs.ok);
    let unknown = world
        .call(
            "run.trace",
            json!({"run_id": "0123456789abcdef0123456789abcdef"}),
        )
        .await;
    assert!(
        matches!(code_of(&unknown).as_str(), "not_found" | "schema_invalid"),
        "{unknown:?}"
    );
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_runs_evidence_is_exported_to_one_file_read_back_before_its_digest_is_given() {
    let world = capturing().await;
    let run = world.read_an_order().await;
    let project = world.project("project");

    // With no project folder, nothing is written anywhere.
    let nowhere = world.call("trace.export", json!({"run_id": run})).await;
    assert_eq!(code_of(&nowhere), "path_not_allowed", "{nowhere:?}");

    let exported = world.export(&run, &project).await;
    assert!(exported.ok, "{exported:?}");
    let result = exported.result.unwrap();
    let relative = format!(".anything2mcp/exports/{run}.json");
    assert_eq!(result["file"], relative);
    let bytes = std::fs::read(project.join(&relative)).unwrap();
    assert_eq!(result["bytes"], bytes.len());
    assert_eq!(result["digest"], a2m_core::Digest::of(&bytes).to_string());

    let document: Json = serde_json::from_slice(&bytes).unwrap();
    let manifest = &document["manifest"];
    assert_eq!(manifest["format"], "anything2mcp.trace_export");
    assert_eq!(manifest["format_version"], 1);
    assert_eq!(manifest["run_id"], run);
    assert_eq!(manifest["trace_check"], "whole");
    // The events are those `trace.show` gives, and the trace is every line.
    let shown = world.ok("trace.show", json!({"run_id": run})).await;
    assert_eq!(document["events"], shown["events"]);
    let lines = document["trace"].as_array().unwrap();
    assert_eq!(lines.first().unwrap()["type"], "header");
    assert_eq!(lines.last().unwrap()["type"], "end");
    assert_eq!(
        lines.last().unwrap()["signature"],
        manifest["signature"],
        "{manifest}"
    );
    // The kept answer is in the file as text, and listed in the manifest.
    let captures = document["captures"].as_array().unwrap();
    assert!(
        captures.iter().any(|capture| capture["name"] == "response"
            && capture["text"].as_str().unwrap().contains("A-100")),
        "{captures:?}"
    );
    let pieces = manifest["pieces"].as_array().unwrap();
    assert_eq!(result["pieces"], pieces.len());
    assert!(pieces.iter().any(|piece| piece["part"] == "events"));
    assert!(pieces.iter().any(|piece| piece["part"] == "trace"));

    // A second export replaces the first, and gives the same file.
    let again = world.export(&run, &project).await;
    assert!(again.ok, "{again:?}");
    assert_eq!(again.result.unwrap()["digest"], result["digest"]);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_piece_that_holds_a_registered_secret_is_left_out_of_the_export_and_named() {
    let world = capturing().await;
    // The application answers the order's customer with a value that is
    // registered as a secret after the read was kept.
    let run = world.read_an_order().await;
    let stored = world
        .call(
            "secret.set",
            json!({"name": "example_export_secret", "value": "Example Customer One"}),
        )
        .await;
    assert!(stored.ok, "{:?}", stored.error);
    let project = world.project("project");
    let exported = world.export(&run, &project).await;
    assert!(exported.ok, "{exported:?}");
    let result = exported.result.unwrap();
    assert!(result["left_out"].as_u64().unwrap() >= 1, "{result}");
    assert!(
        exported
            .warnings
            .iter()
            .any(|said| said.contains("left out")),
        "{:?}",
        exported.warnings
    );
    let text = std::fs::read_to_string(project.join(result["file"].as_str().unwrap())).unwrap();
    assert!(!text.contains("Example Customer One"), "{text}");
    assert!(text.contains("registered secret"), "{text}");
    world.kernel.close(CloseReason::Asked).await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_export_is_never_written_through_a_link_out_of_the_project() {
    let world = capturing().await;
    let run = world.read_an_order().await;
    let project = world.project("project");
    let outside = world.folder.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::create_dir_all(project.join(".anything2mcp")).unwrap();
    std::os::unix::fs::symlink(&outside, project.join(".anything2mcp").join("exports")).unwrap();

    let refused = world.export(&run, &project).await;
    assert!(!refused.ok, "{refused:?}");
    assert_eq!(code_of(&refused), "path_not_allowed", "{refused:?}");
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    world.kernel.close(CloseReason::Asked).await;
}
