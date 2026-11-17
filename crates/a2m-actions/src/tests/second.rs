//! What the crates above the action layer asked of it: a source file that
//! reaches a pipeline, a tool list in one decided step, a run's events to
//! follow, and the smaller things of the registry's second pass.

use std::time::Duration;

use a2m_actions::{
    Caller, CloseReason, ExitClass, FrontDoor, McpResultFace, Request, Response, SHORTCUTS,
    command_faces,
};
use a2m_core::{FailureClass, RunId, RunState};
use serde_json::{Value as Json, json};

use super::kernel::World;
use super::kernel::engine::{self, ITEM_UPSERT, ORDER_FILL, ORDER_GET, sheet};
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
    let mut plan = engine::plan(&world.app.url(""));
    plan.header.version = a2m_core::Version::new(2, 0, 0);
    plan.pipelines[0].source = plan.pipelines[1].source.clone();
    assert!(plan.pipelines[0].source.is_some());
    world.kernel.publish_plan(&plan).await.unwrap();
    let project = world.folder.path().join("project");
    std::fs::create_dir_all(project.join("sheets")).unwrap();
    let file = project.join("sheets").join("items.csv");
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
        "source_file": "sheets/items.csv",
    });

    // What the dispatcher refuses before any run exists.
    let invalid = (FailureClass::Input, "schema_invalid".to_owned());
    let none = call(json!({"tool": ORDER_FILL, "input": fill}), true).await;
    assert_eq!(error_of(&none), invalid);
    assert!(none.error.unwrap().fix.contains("source_file"));
    let not_wanted = call(
        json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}, "source_file": "sheets/items.csv"}),
        true,
    )
    .await;
    assert_eq!(error_of(&not_wanted), invalid);
    let both = call(
        json!({"tool": ORDER_FILL, "source_file": "sheets/items.csv", "source_rows": [{"Code": "L-1"}]}),
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
    for escaping in [
        "../outside.csv".to_owned(),
        outside.to_string_lossy().into_owned(),
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
    missing["source_file"] = json!("sheets/nothing.csv");
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
            "plan_hash",
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
async fn a_secret_comes_from_standard_input_and_a_shortcut_stops_before_what_is_not_built() {
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

    // `add` registers the target and stops before the probe, which is not
    // built, and says so. No step after it is begun.
    let add = SHORTCUTS
        .iter()
        .find(|shortcut| shortcut.word == "add")
        .unwrap();
    let run = add.on(registry);
    let steps: Vec<&str> = run.steps.iter().map(|step| step.action).collect();
    assert_eq!(steps, ["target.add"]);
    assert_eq!(run.stops_before, Some("target.probe"));
    let note = run.note.unwrap();
    assert!(note.contains("stopped before `target.probe`"), "{note}");
    assert!(note.contains("Done: `target.add`"), "{note}");
    assert!(note.contains("`session.login`"), "{note}");
    // A shortcut whose steps are all built runs them all and says nothing.
    let lint = SHORTCUTS
        .iter()
        .find(|shortcut| shortcut.word == "lint")
        .unwrap();
    let run = lint.on(registry);
    assert_eq!(
        (run.steps.len(), run.stops_before, run.note),
        (1, None, None)
    );
    // One whose first step is not built does nothing, and says that.
    let learn = SHORTCUTS
        .iter()
        .find(|shortcut| shortcut.word == "learn")
        .unwrap();
    let run = learn.on(registry);
    assert!(run.steps.is_empty());
    assert!(run.note.unwrap().contains("does nothing yet"));
    // Every shortcut's steps are a prefix of its own, whatever is built.
    for shortcut in SHORTCUTS {
        let run = shortcut.on(registry);
        assert_eq!(run.steps, shortcut.steps[..run.steps.len()]);
        assert_eq!(
            run.stops_before.is_some(),
            run.steps.len() < shortcut.steps.len()
        );
    }
    world.kernel.close(CloseReason::Asked).await;
}
