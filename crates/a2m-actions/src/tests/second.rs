//! What the crates above the action layer asked of it: a source file that
//! reaches a pipeline, a tool list in one decided step, a run's events to
//! follow, and the smaller things of the registry's second pass.

use std::time::Duration;

use a2m_actions::{
    Caller, CloseReason, ExitClass, FrontDoor, McpResultFace, ProjectFolder, Request, Response,
    RunsInHand, SHORTCUTS, Shortcut, command_faces, shortcuts_offered,
};
use a2m_core::{FailureClass, RunId, RunState};
use serde_json::{Value as Json, json};

use super::kernel::World;
use std::sync::atomic::Ordering;

use super::kernel::engine::{
    self, ITEM_UPSERT, KIT_ENTER, ORDER_FILL, ORDER_GET, SHEET_ENTER, SLOT_ADD, sheet,
};
use super::runs::{item, status};

fn error_of(response: &Response) -> (FailureClass, String) {
    let error = response
        .error
        .as_ref()
        .unwrap_or_else(|| panic!("no error in {response:?}"));
    (error.class, error.code.clone())
}

fn run_of(response: &Response) -> (String, RunId) {
    let run = response.result.as_ref().unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let id = run.parse().unwrap();
    (run, id)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_source_file_reaches_a_pipeline_and_a_resume_reads_the_same_bytes() {
    let world = World::ready().await;
    // The pipeline of fixed steps is given the sheet as its source, so that
    // this proof stands on reading a source and on nothing else a pipeline
    // may do with its rows.
    let mut compiled_target = engine::compiled_target(&world.app.url(""));
    compiled_target.header.version = a2m_core::Version::new(2, 0, 0);
    compiled_target.pipelines[0].source = compiled_target.pipelines[1].source.clone();
    assert!(compiled_target.pipelines[0].source.is_some());
    world
        .kernel
        .publish_compiled_target(&compiled_target)
        .await
        .unwrap();
    let project = world.project("project");
    std::fs::create_dir_all(project.join("sheets")).unwrap();
    std::fs::write(project.join("sheets").join("items.csv"), b"Code\n").unwrap();
    std::fs::write(project.join("sources").join(".hidden.csv"), b"Code\n").unwrap();
    let file = project.join("sources").join("items.csv");
    std::fs::write(
        &file,
        sheet(&[
            ("L-501", "Rail one", "2", "10.5"),
            ("L-502", "Rail two", "4", "20"),
        ]),
    )
    .unwrap();
    let call = |input: Json, in_project: bool| {
        let request = Request::new("tool.call", input, world.caller.clone());
        let request = if in_project {
            request.in_project(project.clone())
        } else {
            request
        };
        world.kernel.dispatch(request)
    };
    let fill = json!({
        "order_id": "A-102",
        "first_code": "L-501",
        "second_code": "L-502",
        "description": "New rail",
        "quantity": 2,
        "length_in": "10.5",
    });
    let sheet_call = json!({
        "tool": ORDER_FILL,
        "input": fill,
        "source_file": "sources/items.csv",
    });

    // What the dispatcher refuses before any run exists.
    let invalid = (FailureClass::Input, "schema_invalid".to_owned());
    let none = call(json!({"tool": ORDER_FILL, "input": fill}), true).await;
    assert_eq!(error_of(&none), invalid);
    assert!(none.error.unwrap().fix.contains("source_file"));
    let not_wanted = call(
        json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}, "source_file": "sources/items.csv"}),
        true,
    )
    .await;
    assert_eq!(error_of(&not_wanted), invalid);
    let both = call(
        json!({"tool": ORDER_FILL, "source_file": "sources/items.csv", "source_rows": [{"Code": "L-1"}]}),
        true,
    )
    .await;
    assert_eq!(error_of(&both), invalid);
    // Rows given inline are a JSON document; this source is read as
    // delimited text, and the refusal names the reader.
    let rows = call(
        json!({"tool": ORDER_FILL, "source_rows": [{"Code": "L-1"}]}),
        true,
    )
    .await;
    assert_eq!(error_of(&rows), invalid);
    assert!(rows.error.unwrap().message.contains("`csv`"));
    // A file is found only through the project folder, and only inside it.
    let no_project = call(sheet_call.clone(), false).await;
    assert_eq!(
        error_of(&no_project),
        (FailureClass::Input, "path_not_allowed".to_owned())
    );
    let outside = std::env::temp_dir().join("a2m-outside.csv");
    // Nor from anywhere in the project but its `sources` folder: not
    // another folder of it, not its project file, not its target files,
    // not a hidden file, and not a folder.
    for escaping in [
        "../outside.csv".to_owned(),
        "sources/../sheets/items.csv".to_owned(),
        outside.to_string_lossy().into_owned(),
        "sheets/items.csv".to_owned(),
        "anything2mcp.toml".to_owned(),
        "targets/example/target.yaml".to_owned(),
        "sources/.hidden.csv".to_owned(),
        "sources".to_owned(),
    ] {
        let mut input = sheet_call.clone();
        input["source_file"] = json!(escaping);
        let refused = call(input, true).await;
        assert_eq!(
            error_of(&refused),
            (FailureClass::Input, "path_not_allowed".to_owned()),
            "{escaping}"
        );
    }
    let mut missing = sheet_call.clone();
    missing["source_file"] = json!("sources/nothing.csv");
    assert_eq!(
        error_of(&call(missing, true).await),
        (FailureClass::Input, "file_unavailable".to_owned())
    );

    // The file reaches the pipeline: the engine read and validated it, went
    // on to the steps, and built a diff of two records, which waits for a
    // person.
    let rested = call(sheet_call.clone(), true).await;
    assert_eq!(rested.exit, ExitClass::PersonNeeded, "{rested:?}");
    assert_eq!(status(&rested), "needs_approval");
    let (run, id) = run_of(&rested);
    let preview = world.surface.desk().preview(id, 0).await.unwrap();
    assert_eq!((preview.create, preview.total), (2, 2), "{preview:?}");

    // The same call while the file is unchanged is the same run.
    let again = call(sheet_call.clone(), true).await;
    assert_eq!(run_of(&again).1, id);

    // The file changes on disk after the call, to one the source would
    // refuse. The run is bound to the bytes it was called with: the
    // resume, which names no project and no file, reads those bytes again
    // and writes what the person approved.
    std::fs::write(&file, b"Code,Name\nL-1,Rail\n").unwrap();
    world.approve(id).await;
    let done = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(status(&done), "completed", "{done:?}");
    // The test application holds both records, each written once.
    for code in ["L-501", "L-502"] {
        assert_eq!(world.app.write_count("A-102", code).await.unwrap(), 1);
        let stored = world.app.stored_item("A-102", code).await.unwrap().unwrap();
        assert_eq!(stored.description, "New rail");
        assert_eq!(stored.quantity, 2);
    }

    // The same call over the changed file is another run, and the engine
    // refuses the file before anything is planned: it is not the template
    // the source knows.
    let unknown = call(sheet_call, true).await;
    assert_ne!(run_of(&unknown).1, id);
    assert!(!unknown.ok, "{unknown:?}");
    assert_eq!(unknown.exit, ExitClass::Failed);
    assert_eq!(status(&unknown), "rejected", "{unknown:?}");
    assert_eq!(
        error_of(&unknown),
        (FailureClass::Input, "source_template_unknown".to_owned())
    );
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tool_list_in_full_is_one_decided_step() {
    let world = World::ready().await;
    let listed = world.call("tool.search", json!({"full": true})).await;
    assert!(listed.ok, "{:?}", listed.error);
    // One dispatch, one decision.
    let decision = world.explained(&listed).await;
    assert_eq!(decision["resource"], "action:tool.search");
    let result = listed.result.unwrap();
    assert_eq!(result["complete"], true);
    let tools = result["tools"].as_array().unwrap();
    assert!(tools.len() >= 4);
    // Each entry is what `tool.describe` gives for the tool.
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        let described = world.ok("tool.describe", json!({"tool": name})).await;
        assert_eq!(tool, &described, "{name}");
        for key in [
            "input_schema",
            "output_schema",
            "annotations",
            "compiled_target_hash",
            "hazard",
        ] {
            assert!(tool.get(key).is_some(), "{name}: {key}");
        }
    }
    // Without the flag the list stays short.
    let short = world.ok("tool.search", json!({})).await;
    assert!(short["tools"][0].get("input_schema").is_none());
    // Both fit the published output schema.
    let def = world.kernel.actions().find("tool.search").unwrap();
    def.output.check_json(&result).unwrap();
    def.output.check_json(&short).unwrap();
    let def = world.kernel.actions().find("tool.describe").unwrap();
    def.output.check_json(&tools[0]).unwrap();
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_run_is_followed_by_its_events_without_polling() {
    let world = World::ready().await;
    let principal = world.caller.principal.clone();

    // A follower waits for the first event of a run that does not rest
    // yet: the wait ends when one is appended, not when its time is up.
    let (run, id) = world.rest_a_write("L-430").await;
    let all = world.kernel.events(&principal, id, 0, 500).await.unwrap();
    assert_eq!(all.state, RunState::NeedsApproval);
    // It rests: nothing more comes until someone acts.
    assert!(!all.ended && !all.working && all.settled());
    let seqs: Vec<u64> = all.events.iter().map(|event| event.seq).collect();
    assert_eq!(seqs, (1..=all.last).collect::<Vec<u64>>());
    assert!(all.events.iter().any(|event| event.kind == "lease_granted"));
    assert_eq!(all.events.last().unwrap().kind, "state_changed");
    // A page is bounded, and the next one goes on from its last number.
    let first = world.kernel.events(&principal, id, 0, 3).await.unwrap();
    assert_eq!((first.events.len(), first.last), (3, 3));
    let rest = world
        .kernel
        .events(&principal, id, first.last, 500)
        .await
        .unwrap();
    assert_eq!(rest.events.first().unwrap().seq, 4);
    assert_eq!(rest.last, all.last);

    // Nothing new: the wait runs out and says so with an empty page.
    let nothing = world
        .kernel
        .next_events(&principal, id, all.last, 50, Duration::from_millis(20))
        .await
        .unwrap();
    assert!(nothing.events.is_empty() && !nothing.ended);
    assert_eq!(nothing.last, all.last);

    // A follower that waits is woken by the work itself.
    let follower = {
        let kernel = world.kernel.clone();
        let principal = principal.clone();
        let after = all.last;
        tokio::spawn(async move {
            let mut after = after;
            let mut kinds = Vec::new();
            loop {
                let page = kernel
                    .next_events(&principal, id, after, 50, Duration::from_secs(60))
                    .await
                    .unwrap();
                kinds.extend(page.events.iter().map(|event| event.kind.clone()));
                after = page.last;
                if page.ended && page.events.is_empty() {
                    return (kinds, after, page.state);
                }
            }
        })
    };
    world.approve(id).await;
    let done = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(status(&done), "completed");
    let (kinds, last, state) = tokio::time::timeout(Duration::from_secs(30), follower)
        .await
        .expect("the follower was woken")
        .unwrap();
    assert_eq!(state, RunState::Completed);
    for kind in ["approval_bound", "intent", "acted", "verified"] {
        assert!(kinds.iter().any(|found| found == kind), "{kind}: {kinds:?}");
    }
    // The follower saw every event, in order, with no gap.
    let whole = world.kernel.events(&principal, id, 0, 500).await.unwrap();
    assert_eq!(whole.last, last);
    assert!(whole.ended && whole.settled());

    // Only the caller that started a run follows it.
    let mut other = principal.clone();
    other.workspace = a2m_core::WorkspaceId::from_bytes([7; 16]);
    let refused = world.kernel.events(&other, id, 0, 10).await.unwrap_err();
    assert_eq!(refused.code().as_str(), "not_found");
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_write_refused_for_want_of_approval_is_recorded_as_a_decision() {
    let world = World::ready().await;
    let (run, _) = world.rest_a_write("L-440").await;
    // The run's own events name the decision that would not let it write
    // without a person.
    let trace = world.ok("trace.show", json!({"run_id": run})).await;
    let need = trace["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["kind"] == "need" && event["detail"]["needs"] == "approval")
        .unwrap_or_else(|| panic!("no need in {trace}"))
        .clone();
    let decision = need["detail"]["decision"].as_str().unwrap();
    let explained = world
        .ok("policy.explain", json!({"decision": decision}))
        .await;
    assert_eq!(explained["answer"], "deny", "{explained}");
    assert_eq!(explained["question"], "write_without_person");
    assert!(
        explained["resource"]
            .as_str()
            .unwrap()
            .contains(ITEM_UPSERT),
        "{explained}"
    );
    // It is the decision about this run's diff, and nothing was written.
    let read = world.call("run.get", json!({"run_id": run})).await;
    assert_eq!(
        need["detail"]["diff"],
        read.result.as_ref().unwrap()["diff"]["hash"]
    );
    assert_eq!(world.app.write_count("A-102", "L-440").await.unwrap(), 0);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_kill_switch_is_one_refusal_wherever_it_is_met() {
    let world = World::ready().await;
    let (run, id) = world.rest_a_write("L-450").await;
    world.approve(id).await;
    world
        .ok("killswitch.set", json!({"scope": "everything", "on": true}))
        .await;
    let switched = (FailureClass::Policy, "kill_switch_on".to_owned());
    // New work, a read as much as a write, and a run that was waiting.
    for (action, input) in [
        (
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        ),
        (
            "tool.call",
            json!({"tool": ITEM_UPSERT, "input": item("L-451")}),
        ),
        ("run.resume", json!({"run_id": run})),
    ] {
        let held = world.call(action, input).await;
        assert_eq!(error_of(&held), switched, "{action}");
        assert_eq!(held.exit, ExitClass::Failed);
        assert!(held.error.unwrap().message.contains("kill switch"));
    }
    assert_eq!(world.app.write_count("A-102", "L-450").await.unwrap(), 0);
    // Reading a run and approving are not work on a system: they go on.
    let read = world.call("run.get", json!({"run_id": run})).await;
    assert_eq!(status(&read), "needs_approval");

    // Released by a person, the approved run is resumed and completes.
    let person = Caller::new(world.caller.principal.clone(), FrontDoor::Command, false);
    let released = world
        .kernel
        .dispatch(Request::new(
            "killswitch.set",
            json!({"scope": "everything", "on": false}),
            person,
        ))
        .await;
    assert!(released.ok, "{:?}", released.error);
    let done = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(status(&done), "completed", "{done:?}");
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_result_shows_exact_numbers_as_text_and_carries_its_next_step() {
    let world = World::ready().await;
    let (run, id) = world.rest_a_write("L-460").await;

    // A run that rests names what to call next, inside the result the MCP
    // face returns, and the result still fits the output schema.
    let rested = world.call("run.get", json!({"run_id": run})).await;
    let face = McpResultFace::of(&rested);
    let structured = face.structured.clone().unwrap();
    assert_eq!(structured["next"]["action"], "run.resume");
    assert_eq!(structured["next"]["tool"], "a2m_run_resume");
    assert_eq!(structured["next"]["args"]["run_id"], run);
    assert_eq!(
        serde_json::from_str::<Json>(&face.text).unwrap(),
        structured
    );
    let def = world.kernel.actions().find("run.get").unwrap();
    def.output.check_json(&structured).unwrap();

    world.approve(id).await;
    let done = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(status(&done), "completed");
    // A result shows values as `render` writes them, which a test of its
    // own holds to exact text for an exact number.
    let result = &done.result.as_ref().unwrap()["result"];
    assert_eq!(result["version"], json!(1), "{result}");
    let def = world.kernel.actions().find("run.resume").unwrap();
    def.output
        .check_json(&McpResultFace::of(&done).structured.unwrap())
        .unwrap();

    // Every strict output schema admits `next`, and results that carry one
    // fit: a list, a description, and a status among them.
    for (action, input) in [
        ("tool.search", json!({})),
        ("tool.describe", json!({"tool": ORDER_GET})),
        ("engine.status", json!({})),
        ("target.list", json!({})),
        (
            "killswitch.set",
            json!({"scope": "target", "target": "example", "on": true}),
        ),
    ] {
        let response = world.call(action, input).await;
        assert!(response.ok, "{action}: {:?}", response.error);
        let structured = McpResultFace::of(&response).structured.unwrap();
        assert_eq!(
            structured.get("next").is_some(),
            response.next.is_some(),
            "{action}"
        );
        world
            .kernel
            .actions()
            .find(action)
            .unwrap()
            .output
            .check_json(&structured)
            .unwrap_or_else(|misfits| panic!("{action}: {misfits:?}"));
    }
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_secret_comes_from_standard_input_and_only_a_shortcut_whose_steps_are_built_is_offered() {
    let world = World::ready().await;
    let registry = world.kernel.actions();

    // The command face of `secret.set` says which flag standard input
    // fills, and the secret need not be an argument.
    let faces = command_faces(registry);
    let face = faces
        .iter()
        .find(|face| face.action == "secret.set")
        .unwrap();
    let flag = face.args.iter().find(|arg| arg.flag == "--stdin").unwrap();
    assert_eq!(flag.stdin_for.as_deref(), Some("value"));
    assert_eq!(flag.ty, "flag");
    let value = face.args.iter().find(|arg| arg.field == "value").unwrap();
    assert!(!value.required && value.stdin_for.is_none());
    assert_eq!(
        faces
            .iter()
            .flat_map(|face| &face.args)
            .filter(|arg| arg.stdin_for.is_some())
            .count(),
        1
    );
    // What the command line sends after it read standard input.
    let stored = world
        .call(
            "secret.set",
            json!({"name": "example_password", "value": "secret-for-tests-1"}),
        )
        .await;
    assert!(stored.ok, "{:?}", stored.error);
    // A call that still carries the flag was not read for by anyone; and
    // one with neither has no secret.
    for input in [
        json!({"name": "example_password", "stdin": true}),
        json!({"name": "example_password"}),
        json!({"name": "example_password", "stdin": true, "value": "secret-for-tests-2"}),
    ] {
        let refused = world.call("secret.set", input).await;
        assert_eq!(
            error_of(&refused),
            (FailureClass::Input, "schema_invalid".to_owned())
        );
        assert!(!refused.to_json().to_string().contains("secret-for-tests"));
    }

    // A shortcut is offered only when every one of its steps is built
    // (decision 0456): each step of each offered shortcut has a handler,
    // and no offered shortcut can stop before a step that is not.
    let offered = shortcuts_offered(registry);
    for shortcut in &offered {
        for step in shortcut.steps {
            let def = registry.find(step.action).unwrap();
            assert!(def.built(), "`{}` offers `{}`", shortcut.word, step.action);
        }
    }
    // The ones not offered today: each runs one action of a version that
    // is not built. Each is offered by itself once its action is.
    let words = |list: &[&Shortcut]| list.iter().map(|s| s.word).collect::<Vec<_>>();
    let withheld: Vec<&Shortcut> = SHORTCUTS
        .iter()
        .filter(|shortcut| !shortcut.offered(registry))
        .collect();
    assert_eq!(words(&withheld), ["bundle", "rollback", "migrate"]);
    // `add` registers the target, scouts it, and shows the report, which
    // asks for a sign-in only where a read waits for one (decision 0306);
    // `diff` is built (package YL1); `init` has no step and is offered.
    for word in ["init", "add", "diff", "learn", "ready"] {
        assert!(words(&offered).contains(&word), "{word}");
    }
    // `learn` is `capture.start` alone: it asks for a capture, which waits
    // for its person; it never ends the capture it asked for.
    let learn = offered
        .iter()
        .find(|shortcut| shortcut.word == "learn")
        .unwrap();
    let steps: Vec<&str> = learn.steps.iter().map(|step| step.action).collect();
    assert_eq!(steps, ["capture.start"]);
    world.kernel.close(CloseReason::Asked).await;
}

impl World {
    /// A project folder under the test's own folder, holding one sheet.
    fn project_with_sheet(&self, rows: &[(&str, &str, &str, &str)]) -> std::path::PathBuf {
        let project = self.project("project");
        std::fs::write(project.join("sources").join("sheet.csv"), sheet(rows)).unwrap();
        project
    }

    /// The codes the test application holds for the order, in order.
    async fn stored_codes(&self, order: &str) -> Vec<String> {
        let mut codes: Vec<String> = self
            .app
            .writes()
            .await
            .unwrap()
            .into_iter()
            .filter(|write| write.order_id == order)
            .map(|write| write.item_code)
            .collect();
        codes.sort();
        codes
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sheet_is_entered_row_by_row_from_a_file_of_the_project() {
    let world = World::ready().await;
    let project = world.project_with_sheet(&[
        ("L-601", "Rail one", "2", "10.5"),
        ("L-602", "Rail two", "4", "20"),
    ]);
    let rested = world
        .kernel
        .dispatch(
            Request::new(
                "tool.call",
                json!({
                    "tool": SHEET_ENTER,
                    "input": {"order_id": "A-102"},
                    "source_file": "sources/sheet.csv",
                }),
                world.caller.clone(),
            )
            .in_project(project),
        )
        .await;
    assert_eq!(status(&rested), "needs_approval", "{rested:?}");
    let (run, id) = run_of(&rested);
    // One record for each row of the file, with the row's own values.
    let preview = world.surface.desk().preview(id, 0).await.unwrap();
    let keys: Vec<&str> = preview
        .records
        .iter()
        .map(|record| record.key.as_str())
        .collect();
    assert_eq!(keys, ["A-102 / L-601", "A-102 / L-602"], "{preview:?}");
    let length = preview.records[1]
        .fields
        .iter()
        .find(|field| field.name == "length_in")
        .unwrap();
    assert_eq!(length.proposed, "20");

    world.approve(id).await;
    let done = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(status(&done), "completed", "{done:?}");
    // The test application holds each row, written once.
    for (code, description, quantity) in [("L-601", "Rail one", 2), ("L-602", "Rail two", 4)] {
        assert_eq!(world.app.write_count("A-102", code).await.unwrap(), 1);
        let stored = world.app.stored_item("A-102", code).await.unwrap().unwrap();
        assert_eq!(stored.description, description);
        assert_eq!(stored.quantity, quantity);
    }
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_key_assigned_before_the_commit_point_is_handed_back_when_the_run_is_resumed() {
    let world = World::ready().await;
    let rested = world
        .call(
            "tool.call",
            json!({
                "tool": SLOT_ADD,
                "input": {"order_id": "A-102", "quantity": 4, "length_in": "12"},
            }),
        )
        .await;
    assert_eq!(status(&rested), "needs_approval", "{rested:?}");
    let (run, id) = run_of(&rested);
    world.approve(id).await;

    // The save is sent and its answer is lost. The run stops in doubt, and
    // the reconcile the scheduler queues settles it before the call
    // returns.
    world.hooks.lose_answer.store(true, Ordering::SeqCst);
    let lost = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(status(&lost), "stopped", "{lost:?}");
    assert_eq!(error_of(&lost).0, FailureClass::PossibleCommit, "{lost:?}");
    assert_eq!(world.hooks.count("reserve_code"), 1);
    let lookups = world.hooks.count("find_item");

    // Resumed: the code the system assigned before the save was recorded
    // in the ledger and is handed back, so the record is read by that code.
    // It is not looked up again, no second code is reserved, and nothing
    // is saved twice.
    let done = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(status(&done), "completed", "{done:?}");
    assert_eq!(world.hooks.count("reserve_code"), 1);
    assert_eq!(world.hooks.count("add_item"), 1);
    assert_eq!(world.hooks.count("find_item"), lookups);
    assert_eq!(world.stored_codes("A-102").await, ["N-001"]);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lines_whose_header_failed_are_recorded_as_blocked_and_written_once_it_is_settled() {
    let world = World::ready().await;
    let project =
        world.project_with_sheet(&[("A", "Left leg", "2", "30"), ("B", "Right leg", "2", "30")]);
    let rested = world
        .kernel
        .dispatch(
            Request::new(
                "tool.call",
                json!({
                    "tool": KIT_ENTER,
                    "input": {"order_id": "A-102", "title": "Table kit"},
                    "source_file": "sources/sheet.csv",
                }),
                world.caller.clone(),
            )
            .in_project(project),
        )
        .await;
    assert_eq!(status(&rested), "needs_approval", "{rested:?}");
    let (run, id) = run_of(&rested);
    world.approve(id).await;

    // The header's save loses its answer: the header is in doubt, and the
    // two lines under it, still planned, are recorded as blocked by it.
    world.hooks.lose_answer.store(true, Ordering::SeqCst);
    let lost = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(status(&lost), "stopped", "{lost:?}");
    let trace = world.ok("trace.show", json!({"run_id": run})).await;
    let failed: Vec<&str> = trace["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "failed")
        .map(|event| event["detail"]["code"].as_str().unwrap())
        .collect();
    assert_eq!(
        failed,
        ["no_confirmation", "blocked_by_parent", "blocked_by_parent"],
        "{trace}"
    );
    assert_eq!(world.hooks.count("put_item"), 0);

    // Resumed: the header is not created again, and the lines are written
    // under the code the system gave it.
    let done = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(status(&done), "completed", "{done:?}");
    assert_eq!(world.hooks.count("add_item"), 1);
    assert_eq!(
        world.stored_codes("A-102").await,
        ["N-001", "N-001-A", "N-001-B"]
    );
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stated_project_folder_is_checked_before_any_handler_sees_it() {
    let world = World::ready().await;
    let home = world.folder.path().to_path_buf();
    let good = world.project("project");
    // The same kind of folder with no project file in it.
    let bare = home.join("bare");
    std::fs::create_dir_all(&bare).unwrap();
    let lint = |folder: Option<ProjectFolder>| {
        world.kernel.dispatch(
            Request::new(
                "draft.lint",
                json!({"target": "example"}),
                world.caller.clone(),
            )
            .in_project_folder(folder.as_ref()),
        )
    };
    let refused = (FailureClass::Input, "path_not_allowed".to_owned());

    // A folder of the user's own is taken: the action looks in it, and
    // finds no such target there.
    let looked = lint(Some(ProjectFolder::new(&good))).await;
    assert!(!looked.ok, "{looked:?}");
    assert_eq!(looked.error.as_ref().unwrap().code, "lint_failed");
    assert_eq!(
        looked.result.unwrap()["problems"][0]["code"],
        "kernel.file_missing"
    );
    // No folder stated: the action says it needs one.
    let none = lint(None).await;
    assert_eq!(error_of(&none), refused);
    assert!(none.error.unwrap().message.contains("names none"));

    // The engine's own folders, a folder that holds them, a path that is
    // not a full one, and a folder that is not there are not used, and the
    // refusal says why.
    let paths = world.kernel.paths().clone();
    // This test's store is in memory, so its folders are made here.
    std::fs::create_dir_all(&paths.config).unwrap();
    std::fs::create_dir_all(&paths.store).unwrap();
    for (folder, why) in [
        (paths.data.clone(), "engine's own"),
        (paths.store.clone(), "engine's own"),
        (paths.config.clone(), "engine's own"),
        (home.clone(), "engine's own"),
        (bare.clone(), "no project file"),
        (std::path::PathBuf::from("project"), "not a full path"),
        (home.join("nothing-here"), "no such folder"),
    ] {
        let response = lint(Some(ProjectFolder::new(&folder))).await;
        assert_eq!(error_of(&response), refused, "{folder:?}");
        let error = response.error.unwrap();
        let message = error.message;
        assert!(message.contains(why), "{folder:?}: {message}");
        // The engine's home and a project are kept apart, and the refusal
        // says why and how (decision 0219).
        if why == "engine's own" {
            assert!(message.contains("kept apart"), "{message}");
            assert!(error.fix.contains("A2M_HOME"), "{}", error.fix);
        }
    }
    // The same for a source file: the store is not read through a project.
    let source = world
        .kernel
        .dispatch(
            Request::new(
                "tool.call",
                json!({"tool": SHEET_ENTER, "input": {"order_id": "A-102"}, "source_file": "store/x"}),
                world.caller.clone(),
            )
            .in_project_folder(Some(&ProjectFolder::new(&paths.data))),
        )
        .await;
    assert_eq!(error_of(&source), refused);

    // An action that needs no project is served whatever was stated.
    let status = world
        .kernel
        .dispatch(
            Request::new("engine.status", json!({}), world.caller.clone())
                .in_project_folder(Some(&ProjectFolder::new(&paths.data))),
        )
        .await;
    assert!(status.ok, "{:?}", status.error);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_kernel_says_how_much_work_it_has_in_hand() {
    let world = World::ready().await;
    let nothing = RunsInHand::default();
    assert_eq!(world.kernel.runs_in_hand().await.unwrap(), nothing);
    assert!(!nothing.any());

    // A run that rests for a person is in hand until the person answers.
    let (_, id) = world.rest_a_write("L-470").await;
    let resting = world.kernel.runs_in_hand().await.unwrap();
    assert_eq!((resting.working, resting.waiting_for_person), (0, 1));
    assert!(resting.any());
    world
        .surface
        .desk()
        .decline(id, None, a2m_policy::Channel::LocalPrompt)
        .await
        .unwrap();
    assert_eq!(world.kernel.runs_in_hand().await.unwrap(), nothing);

    // A run that waits for a runner is in hand, and so is one a runner
    // holds; when it has ended, nothing is.
    world
        .kernel
        .start_runner_loop(1)
        .await
        .unwrap()
        .stop()
        .await;
    let queued = world
        .call(
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}, "wait_seconds": 0}),
        )
        .await;
    assert_eq!(status(&queued), "queued", "{queued:?}");
    let (run, _) = run_of(&queued);
    let waiting = world.kernel.runs_in_hand().await.unwrap();
    assert_eq!((waiting.working, waiting.waiting_for_person), (1, 0));
    world.kernel.start_runner_loop(1).await.unwrap();
    // The run is followed to its end by its events.
    let principal = world.caller.principal.clone();
    let id: RunId = run.parse().unwrap();
    let mut after = 0;
    loop {
        let page = world
            .kernel
            .next_events(&principal, id, after, 50, Duration::from_secs(60))
            .await
            .unwrap();
        after = page.last;
        if page.settled() {
            break;
        }
        assert!(!page.events.is_empty(), "the wait ran out");
    }
    let done = world.call("run.get", json!({"run_id": run})).await;
    assert_eq!(status(&done), "completed", "{done:?}");
    // The engine's write ends the run; the runner records the job's end in
    // a transaction of its own after it, and then says that a run changed
    // (decision 0132). Until then the one job is still in a runner's hands
    // and is counted, and nothing else is; once it is recorded, nothing is.
    let mut changed = world.kernel.run_watch();
    let job_ended = async {
        loop {
            // Marked seen before the read, so that the runner's notice
            // given between the read and the wait still wakes this.
            changed.borrow_and_update();
            let in_hand = world.kernel.runs_in_hand().await.unwrap();
            if in_hand == nothing {
                break;
            }
            assert_eq!((in_hand.working, in_hand.waiting_for_person), (1, 0));
            changed.changed().await.unwrap();
        }
    };
    tokio::time::timeout(Duration::from_secs(60), job_ended)
        .await
        .unwrap();
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sign_in_is_asked_for_by_name_and_a_session_is_ended_only_by_whose_it_is() {
    let world = World::ready().await;
    // The published target signs in through a provider no pack of this
    // engine registers: the sign-in stops by name before anyone is asked.
    let login = world
        .call("session.login", json!({"target": "example"}))
        .await;
    assert_eq!(
        error_of(&login),
        (FailureClass::Input, "auth_provider_unknown".to_owned())
    );
    // A surface the target does not declare is refused by name.
    let refused = world
        .call(
            "session.login",
            json!({"target": "example", "surface": "elsewhere"}),
        )
        .await;
    assert_eq!(
        error_of(&refused),
        (FailureClass::Input, "not_found".to_owned())
    );
    // No credential is an input of the action: one given is refused as a
    // field the schema does not know, and is in nothing the caller is told.
    let given = world
        .call(
            "session.login",
            json!({"target": "example", "password": "secret-for-tests-login"}),
        )
        .await;
    assert!(!given.ok);
    assert!(
        !given
            .to_json()
            .to_string()
            .contains("secret-for-tests-login")
    );
    // A session that is nobody's, or another person's, is not found.
    let revoked = world
        .call(
            "session.revoke",
            json!({"session_id": "s_01a1ec44a011702e856226a0336b125a"}),
        )
        .await;
    assert_eq!(
        error_of(&revoked),
        (FailureClass::Input, "not_found".to_owned())
    );
    let listed = world.ok("session.list", json!({})).await;
    assert_eq!(listed["complete"], true, "{listed}");
    assert_eq!(listed["sessions"], json!([]), "{listed}");
}

/// Decision 0241, the smaller step of decision 0237's point 3: the engine
/// applies one project file to every request, the one of the folder it
/// started in. A request from a project whose file says otherwise is told
/// so in its warnings, with the keys and both files; one whose file says
/// the same is told nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_project_file_the_engine_does_not_apply_is_named_in_the_warnings() {
    let strict = "[authoring.scout]\ntry_reads = false\n";
    async fn lint(world: &World, folder: &std::path::Path) -> Response {
        world
            .kernel
            .dispatch(
                Request::new(
                    "draft.lint",
                    json!({"target": "example"}),
                    world.caller.clone(),
                )
                .in_project_folder(Some(&ProjectFolder::new(folder))),
            )
            .await
    }
    let differ = |response: &Response| {
        response
            .warnings
            .iter()
            .find(|warning| warning.starts_with(a2m_actions::PROJECT_SETTINGS_DIFFER))
            .cloned()
    };

    // Started outside any project: an empty project file says nothing
    // other, and one that sets a key is named.
    let world = World::ready().await;
    let plain = world.project("plain");
    let set = world.project("strict");
    std::fs::write(set.join("anything2mcp.toml"), strict).unwrap();
    let quiet = lint(&world, &plain).await;
    assert!(!quiet.ok, "{quiet:?}");
    assert_eq!(differ(&quiet), None, "{quiet:?}");
    let told = lint(&world, &set).await;
    assert!(!told.ok, "{told:?}");
    let warning = differ(&told).expect("the warning");
    assert!(
        warning.contains("`authoring.scout.try_reads`")
            && warning.contains("started outside any project")
            && warning.contains("anything2mcp engine stop"),
        "{warning}"
    );
    world.kernel.close(CloseReason::Asked).await;

    // Started in a project whose file sets the key: a project whose file
    // says the same is told nothing, and the empty one is named.
    let start = std::env::temp_dir()
        .join(format!("a2m-actions-start-{}", std::process::id()))
        .join("anything2mcp.toml");
    let settings = a2m_actions::Settings::load(&a2m_actions::Sources {
        project: Some(a2m_actions::settings::SourceFile::new(&start, strict)),
        ..a2m_actions::Sources::none()
    })
    .unwrap();
    assert_eq!(settings.project_file(), Some(start.as_path()));
    let world = World::ready_with(super::kernel::Options::with_settings(settings)).await;
    let plain = world.project("plain");
    let same = world.project("same");
    std::fs::write(same.join("anything2mcp.toml"), strict).unwrap();
    assert_eq!(differ(&lint(&world, &same).await), None);
    let warning = differ(&lint(&world, &plain).await).expect("the warning");
    assert!(
        warning.contains("`authoring.scout.try_reads`")
            && warning.contains(&start.display().to_string()),
        "{warning}"
    );
    world.kernel.close(CloseReason::Asked).await;
}
