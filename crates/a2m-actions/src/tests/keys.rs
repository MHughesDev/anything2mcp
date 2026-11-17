//! What a caller needs after a lost answer (decision 0160): an idempotency
//! key that gives back the run its first call started, and `run.list`,
//! which shows a caller its own runs.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use a2m_actions::{Caller, CloseReason, ExitClass, FrontDoor, Request, Response};
use a2m_core::{FailureClass, RunId};
use a2m_policy::{Role, RoleBinding, Subject};
use serde_json::{Value as Json, json};

use super::kernel::World;
use super::kernel::engine::{ITEM_UPSERT, ORDER_GET};
use super::runs::{item, status};

fn run_of(response: &Response) -> String {
    response.result.as_ref().unwrap()["run_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no run in {response:?}"))
        .to_owned()
}

fn error_of(response: &Response) -> (FailureClass, String) {
    let error = response
        .error
        .as_ref()
        .unwrap_or_else(|| panic!("no error in {response:?}"));
    (error.class, error.code.clone())
}

fn listed(result: &Json) -> Vec<String> {
    result["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|run| run["run_id"].as_str().unwrap().to_owned())
        .collect()
}

impl World {
    /// The write, with a key in its own field.
    async fn keyed_write(&self, code: &str, key: &str) -> Response {
        self.call(
            "tool.call",
            json!({"tool": ITEM_UPSERT, "input": item(code), "idempotency_key": key}),
        )
        .await
    }

    /// The same, with the key beside the input, as the REST door and the
    /// MCP door's `_meta` carry it.
    async fn keyed_beside(&self, caller: &Caller, code: &str, key: &str) -> Response {
        let mut request = Request::new(
            "tool.call",
            json!({"tool": ITEM_UPSERT, "input": item(code)}),
            caller.clone(),
        );
        request.idempotency_key = Some(key.to_owned());
        self.kernel.dispatch(request).await
    }

    /// Everyone in the workspace may call, read, and resume, so that what
    /// keeps a second person out below is whose run it is.
    async fn everyone_operates(&self) {
        self.publish(|layers| {
            for role in [Role::Viewer, Role::Operator] {
                layers.roles.push(RoleBinding {
                    who: Subject::Anyone,
                    role,
                });
            }
        })
        .await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_same_key_gives_back_one_run_and_one_write_finished_or_not() {
    let world = World::ready().await;
    let key = "example-key-501";
    let first = world.keyed_write("L-501", key).await;
    assert_eq!(status(&first), "needs_approval", "{first:?}");
    let run = run_of(&first);
    assert!(first.result.as_ref().unwrap().get("repeated").is_none());
    let asked = world.surface.approvals.lock().unwrap().len();

    // The same call again, the key beside it this time: the same run, said
    // to be given back, and nobody is asked again.
    let again = world.keyed_beside(&world.caller, "L-501", key).await;
    assert_eq!(run_of(&again), run, "{again:?}");
    assert_eq!(again.result.as_ref().unwrap()["repeated"], true);
    assert!(!again.warnings.is_empty());
    assert_eq!(world.surface.approvals.lock().unwrap().len(), asked);

    // Approved and resumed: written once, as the application says.
    world.approve(run.parse().unwrap()).await;
    let done = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(status(&done), "completed", "{done:?}");
    assert_eq!(world.app.write_count("A-102", "L-501").await.unwrap(), 1);

    // A finished run is given back, not run again: its result, and no
    // second run of the tool.
    let finished = world.keyed_write("L-501", key).await;
    assert_eq!(run_of(&finished), run);
    assert_eq!(status(&finished), "completed");
    assert_eq!(finished.result.as_ref().unwrap()["repeated"], true);
    assert_eq!(world.app.write_count("A-102", "L-501").await.unwrap(), 1);
    let runs = world.ok("run.list", json!({"tool": ITEM_UPSERT})).await;
    assert_eq!(listed(&runs), vec![run.clone()]);
    // The key finds its run in the list.
    let by_key = world.ok("run.list", json!({"idempotency_key": key})).await;
    assert_eq!(listed(&by_key), vec![run.clone()]);
    // A door with one place for a key carries it beside the listing: it is
    // the key to look for, and the listing says nothing of an unused key.
    let mut beside = Request::new("run.list", json!({}), world.caller.clone());
    beside.idempotency_key = Some(key.to_owned());
    let beside = world.kernel.dispatch(beside).await;
    assert!(beside.warnings.is_empty(), "{:?}", beside.warnings);
    assert_eq!(listed(beside.result.as_ref().unwrap()), vec![run]);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_key_given_with_another_call_is_refused_and_another_persons_key_is_theirs() {
    let world = World::ready().await;
    world.everyone_operates().await;
    let key = "example-key-510";
    let first = world.keyed_write("L-510", key).await;
    let run = run_of(&first);

    // The same key with another input: refused by name, nothing started.
    let other = world.keyed_write("L-511", key).await;
    assert_eq!(
        error_of(&other),
        (FailureClass::Input, "idempotency_key_reused".to_owned()),
        "{other:?}"
    );
    let runs = world.ok("run.list", json!({})).await;
    assert_eq!(listed(&runs), vec![run.clone()]);

    // Another person's same key and same call is their own: it reaches no
    // run of the first person's, and their list holds only their run.
    let stranger = world.stranger();
    let theirs = world.keyed_beside(&stranger, "L-510", key).await;
    let their_run = run_of(&theirs);
    assert_ne!(their_run, run, "{theirs:?}");
    assert!(theirs.result.as_ref().unwrap().get("repeated").is_none());
    let their_list = world
        .call_as(&stranger, "run.list", json!({"idempotency_key": key}))
        .await;
    assert_eq!(listed(their_list.result.as_ref().unwrap()), vec![their_run]);
    let own = world.ok("run.list", json!({"idempotency_key": key})).await;
    assert_eq!(listed(&own), vec![run]);

    // A key is checked: printable, bounded, and one to a call.
    let spaced = world.keyed_write("L-512", "has space").await;
    assert_eq!(error_of(&spaced).1, "idempotency_key_invalid");
    assert_eq!(spaced.exit, ExitClass::Malformed);
    let long = world.keyed_write("L-512", &"k".repeat(201)).await;
    assert_eq!(error_of(&long).1, "idempotency_key_invalid");
    let mut two = Request::new(
        "tool.call",
        json!({"tool": ITEM_UPSERT, "input": item("L-512"), "idempotency_key": "example-key-a"}),
        world.caller.clone(),
    );
    two.idempotency_key = Some("example-key-b".to_owned());
    let two = world.kernel.dispatch(two).await;
    assert_eq!(error_of(&two).1, "idempotency_key_invalid");
    // A key on an action that starts no run is said to be unused.
    let mut read = Request::new("engine.status", json!({}), world.caller.clone());
    read.idempotency_key = Some("example-key-c".to_owned());
    let read = world.kernel.dispatch(read).await;
    assert!(read.ok);
    assert!(
        read.warnings
            .iter()
            .any(|warning| warning.contains("not used"))
    );
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_run_list_shows_only_the_callers_runs_filtered_paged_and_marked() {
    let world = World::ready().await;
    world.everyone_operates().await;
    let read = world
        .ok(
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
        .await;
    let read_run = read["run_id"].as_str().unwrap().to_owned();
    let (write_run, _) = world.rest_a_write("L-520").await;

    // Newest first, each with what to do next.
    let all = world.ok("run.list", json!({})).await;
    assert_eq!(listed(&all), vec![write_run.clone(), read_run.clone()]);
    assert_eq!(all["complete"], true);
    let first = &all["runs"][0];
    assert_eq!(first["status"], "needs_approval");
    assert_eq!(first["waits_for_person"], true);
    assert_eq!(first["in_doubt"], false);
    assert_eq!(first["next"]["action"], "run.resume");
    assert!(first["created_at"].as_i64().unwrap() > 0);
    let second = &all["runs"][1];
    assert_eq!(second["status"], "completed");
    assert_eq!(second["waits_for_person"], false);
    assert!(second["ended_at"].is_i64());

    // Filtered by state, by tool, and by time.
    let waiting = world
        .ok("run.list", json!({"state": ["needs_approval"]}))
        .await;
    assert_eq!(listed(&waiting), vec![write_run.clone()]);
    let reads = world.ok("run.list", json!({"tool": ORDER_GET})).await;
    assert_eq!(listed(&reads), vec![read_run.clone()]);
    let later = world.now().as_unix_millis() + 60_000;
    let none = world.ok("run.list", json!({"since": later})).await;
    assert!(listed(&none).is_empty());
    let bad = world.call("run.list", json!({"state": ["asleep"]})).await;
    assert_eq!(error_of(&bad).1, "schema_invalid");

    // Paged: a page of one, then the page after it, then the end.
    let page = world.call("run.list", json!({"limit": 1})).await;
    let result = page.result.as_ref().unwrap();
    assert_eq!(listed(result), vec![write_run.clone()]);
    assert_eq!(result["complete"], false);
    let next = page.next.as_ref().unwrap();
    assert_eq!(next.action, "run.list");
    let rest = world.ok("run.list", next.args.clone()).await;
    assert_eq!(listed(&rest), vec![read_run]);
    assert_eq!(rest["complete"], true);

    // A second person sees none of it, and cannot page from it.
    let second = world.kernel.second_person([0x77; 16]).unwrap();
    let second = Caller::new(second, FrontDoor::Command, false);
    let theirs = world.call_as(&second, "run.list", json!({})).await;
    assert!(
        listed(theirs.result.as_ref().unwrap()).is_empty(),
        "{theirs:?}"
    );
    let from_ours = world
        .call_as(&second, "run.list", json!({"after": write_run}))
        .await;
    assert_eq!(error_of(&from_ours).1, "not_found");
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_run_whose_write_is_in_doubt_is_marked_in_the_list() {
    let world = World::ready().await;
    let (run, id) = world.rest_a_write("L-530").await;
    world.approve(id).await;
    // The save is sent and its answer is lost; every read after it fails
    // as well, so the record stays in doubt.
    let hooks = Arc::clone(&world.hooks);
    let sent = std::sync::atomic::AtomicBool::new(false);
    world.hooks.on_step(move |kind| {
        if kind == "put_item" {
            sent.store(true, Ordering::SeqCst);
        }
        if sent.load(Ordering::SeqCst) && (kind == "put_item" || kind == "get_item") {
            *hooks.fails_after.lock().unwrap() = Some((
                "get_item".to_owned(),
                FailureClass::Transient,
                "unavailable",
            ));
        }
    });
    world.hooks.lose_answer.store(true, Ordering::SeqCst);
    let lost = world
        .call("run.resume", json!({"run_id": run, "wait_seconds": 5}))
        .await;
    assert_eq!(status(&lost), "stopped", "{lost:?}");
    let list = world.ok("run.list", json!({})).await;
    let entry = &list["runs"][0];
    assert_eq!(entry["run_id"], run.as_str());
    assert_eq!(entry["status"], "stopped");
    assert_eq!(entry["in_doubt"], true, "{list}");
    assert_eq!(entry["next"]["action"], "run.resume");
    assert!(entry["failure"]["class"].is_string());
    let _: RunId = id;
    world.kernel.close(CloseReason::Asked).await;
}
