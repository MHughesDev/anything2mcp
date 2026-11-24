//! Section 502 and 224's manipulation row in the action registry: an input
//! that names another person's run, or another workspace, is refused, and
//! changes nothing.

use a2m_actions::{Caller, FrontDoor, Principal, Response};
use a2m_core::WorkspaceId;
use a2m_policy::{Role, RoleBinding, Subject};
use serde_json::json;

use super::kernel::World;
use super::kernel::engine::ITEM_UPSERT;
use super::runs::{item, status};

fn code_of(response: &Response) -> String {
    response
        .error
        .as_ref()
        .unwrap_or_else(|| panic!("no error in {response:?}"))
        .code
        .clone()
}

/// Every built action whose input names a run (`run.undo`, `run.trace`, and
/// `trace.export` are not built and answer `action_not_built` to everyone).
const NAMING_A_RUN: [&str; 4] = ["run.get", "run.resume", "run.cancel", "trace.show"];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_input_naming_another_persons_run_or_workspace_is_refused_and_changes_nothing() {
    let world = World::ready().await;
    // Everyone in the workspace may look and change, so that what keeps a
    // second person out below is whose run it is, not a missing grant.
    world
        .publish(|layers| {
            for role in [Role::Viewer, Role::Operator] {
                layers.roles.push(RoleBinding {
                    who: Subject::Anyone,
                    role,
                });
            }
        })
        .await;
    let (run, id) = world.rest_a_write("L-610").await;
    let asked = world.surface.approvals.lock().unwrap().len();

    // A second person of the same workspace, and a caller of another
    // workspace with the user's own identifier, each name the run.
    let second = world.kernel.second_person([0x77; 16]).unwrap();
    let second = Caller::new(second, FrontDoor::Command, false);
    let elsewhere = Caller::new(
        Principal {
            principal: world.caller.principal.principal.clone(),
            workspace: WorkspaceId::from_bytes([0x3c; 16]),
            acting_for: None,
        },
        FrontDoor::Command,
        false,
    );
    for (caller, same_workspace) in [(&second, true), (&elsewhere, false)] {
        for action in NAMING_A_RUN {
            let theirs = world.call_as(caller, action, json!({"run_id": run})).await;
            assert!(!theirs.ok, "{action}: {theirs:?}");
            let code = code_of(&theirs);
            if same_workspace && action.starts_with("trace.") {
                // A trace of another person's run is the auditor's to read
                // (section 214), and this person is not one.
                assert_eq!(code, "not_granted", "{action}: {theirs:?}");
            } else if same_workspace {
                // Allowed by policy, the run is still not theirs: there is
                // no such run, for them.
                assert_eq!(code, "not_found", "{action}: {theirs:?}");
            } else {
                // A caller of another workspace is refused before any
                // handler, or finds no such run.
                assert!(
                    matches!(
                        code.as_str(),
                        "not_found" | "identity_not_allowed" | "not_granted"
                    ),
                    "{action}: {code} {theirs:?}"
                );
            }
            // Nothing of the run is told: not its status, not its tool.
            let told = format!("{theirs:?}");
            assert!(!told.contains(ITEM_UPSERT), "{action}: {told}");
            assert!(!told.contains("L-610"), "{action}: {told}");
        }
    }

    // An input may not name a workspace or a person to act as: the schema of
    // every action is strict, and the caller is the one the door verified.
    for field in [
        "workspace",
        "workspace_id",
        "acting_for",
        "principal",
        "person",
    ] {
        let steered = world
            .call(
                "run.get",
                json!({"run_id": run, field: "0000000000000000000000000000003c"}),
            )
            .await;
        assert_eq!(code_of(&steered), "schema_invalid", "{field}: {steered:?}");
        let steered = world
            .call(
                "tool.call",
                json!({"tool": ITEM_UPSERT, "input": item("L-611"), field: "someone"}),
            )
            .await;
        assert_eq!(code_of(&steered), "schema_invalid", "{field}: {steered:?}");
    }
    // Inside a tool's own input, an undeclared field naming a workspace is
    // refused by the tool's schema in the same way.
    let mut input = item("L-612");
    input["workspace"] = json!("0000000000000000000000000000003c");
    let steered = world
        .call("tool.call", json!({"tool": ITEM_UPSERT, "input": input}))
        .await;
    assert!(!steered.ok, "{steered:?}");

    // The run is as it was: still the person's, still waiting, nobody asked
    // again, and the test application stored nothing.
    let mine = world.call("run.get", json!({"run_id": run})).await;
    assert_eq!(status(&mine), "needs_approval", "{mine:?}");
    assert!(world.surface.desk().waiting(id).await.is_ok());
    assert_eq!(world.surface.approvals.lock().unwrap().len(), asked);
    for code in ["L-610", "L-611", "L-612"] {
        assert_eq!(world.app.write_count("A-102", code).await.unwrap(), 0);
    }
    // And the person's own resume after their approval completes it once.
    world.approve(id).await;
    let done = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(status(&done), "completed", "{done:?}");
    assert_eq!(world.app.write_count("A-102", "L-610").await.unwrap(), 1);
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}
