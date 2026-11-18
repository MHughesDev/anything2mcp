//! Watches through the dispatcher (decision 0283), against the built-in
//! test application: an agent's ask runs nothing until its person confirms
//! it on the page; an agent that calls the confirmation is sent to the page;
//! and a watch, and every change it recorded, is its own person's alone.

use std::time::Duration;

use a2m_actions::{
    Caller, CloseReason, ExitClass, FrontDoor, PersonActAsk, PresenceProof, Response,
};
use a2m_policy::{PresenceCheck, Role, RoleBinding, Subject};
use serde_json::{Value as Json, json};

use super::kernel::World;
use super::kernel::engine::ORDER_GET;

/// How many passes of tending a test waits through for a check to end
/// before it fails by name. Each pass waits a little for the runner.
const PASSES: u32 = 1_500;

impl World {
    /// The read of one order, every minute, for an hour.
    fn watch_input(&self) -> Json {
        json!({
            "tool": ORDER_GET,
            "input": {"order_id": "A-100"},
            "interval_seconds": 60,
            "ends_at": self.now().as_unix_millis() + 3_600_000,
        })
    }

    /// How many reads of the watched order the test application heard.
    async fn order_reads(&self) -> usize {
        self.app
            .seen_all()
            .await
            .unwrap()
            .iter()
            .filter(|line| line.contains("/api/orders/A-100"))
            .count()
    }

    /// The watch, as its person's caller reads it.
    async fn watch(&self, id: &str) -> Json {
        self.ok("watch.get", json!({"watch_id": id})).await
    }

    /// Tends until the watch stands as `wanted` says, and gives it.
    async fn tended_until(&self, id: &str, wanted: impl Fn(&Json) -> bool) -> Json {
        for _ in 0..PASSES {
            self.kernel.tend().await;
            let watch = self.watch(id).await;
            if wanted(&watch) {
                return watch;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!(
            "the watch never stood as the test waits for: {}",
            self.watch(id).await
        );
    }

    /// The confirmation the person was shown on the page, last.
    fn confirmation(&self) -> PersonActAsk {
        self.surface
            .acts
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|ask| ask.action.as_str() == "watch.confirm")
            .cloned()
            .expect("the person was shown the confirmation")
    }

    /// The person presses on the page, with a proof given for that act.
    async fn confirmed_on_the_page(&self, ask: &PersonActAsk) -> Response {
        let person = self.caller.principal.principal().id;
        let proof =
            PresenceProof::new(PresenceCheck::OsPrompt, person, self.now()).for_act(ask.act);
        self.surface.desk().carry_out(ask.act, proof).await.unwrap()
    }
}

fn code_of(response: &Response) -> String {
    response
        .error
        .as_ref()
        .unwrap_or_else(|| panic!("no error in {response:?}"))
        .code
        .clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agent_asks_for_a_watch_and_nothing_runs_until_its_person_confirms_on_the_page() {
    let world = World::ready().await;
    *world.surface.pages.lock().unwrap() = true;

    // The agent asks. The watch is asked and waits for its person, whose
    // page the result links; where it reads and what a check costs come
    // from the plan.
    let asked = world.call("watch.create", world.watch_input()).await;
    assert_eq!(asked.exit, ExitClass::PersonNeeded, "{asked:?}");
    let watch = asked.result.clone().unwrap();
    assert_eq!(watch["state"], "asked", "{watch}");
    assert_eq!(watch["surface"], "app", "{watch}");
    assert_eq!(watch["compared"], "whole", "{watch}");
    assert!(
        watch["requests_per_check"].as_u64().unwrap() >= 1,
        "{watch}"
    );
    assert!(watch["link"].as_str().unwrap().contains("/act/"), "{watch}");
    let id = watch["watch_id"].as_str().unwrap().to_owned();
    assert_eq!(asked.next.as_ref().unwrap().action, "watch.changes");

    // Ten minutes pass, and the kernel tends: nothing is read, and no run
    // is made.
    world.clock.advance(Duration::from_secs(600));
    world.kernel.tend().await;
    assert_eq!(world.order_reads().await, 0);
    assert_eq!(world.ok("run.list", json!({})).await["runs"], json!([]));
    assert_eq!(world.watch(&id).await["state"], "asked");

    // The person reads what they confirm, and presses.
    let ask = world.confirmation();
    assert_eq!(ask.input.len(), 1, "the act's input is the watch alone");
    let read = ask.details.join("\n");
    for said in [
        "example_order_get",
        "order_id = A-100",
        "Every 60 seconds",
        "never a value",
    ] {
        assert!(read.contains(said), "`{said}` in {read}");
    }
    let done = world.confirmed_on_the_page(&ask).await;
    assert!(done.ok, "{done:?}");
    assert_eq!(done.result.unwrap()["state"], "confirmed");

    // Its first check comes due within a tenth of the interval: the read is
    // made, as a run of the person's own.
    world.clock.advance(Duration::from_secs(10));
    let watch = world
        .tended_until(&id, |watch| {
            watch["checks"] == 1 && watch["state"] == "confirmed"
        })
        .await;
    assert!(watch["next_due_at"].as_i64().is_some(), "{watch}");
    assert_eq!(world.order_reads().await, 1);
    let runs = world.ok("run.list", json!({})).await;
    assert_eq!(runs["runs"][0]["tool"], ORDER_GET, "{runs}");
    assert_eq!(runs["runs"][0]["status"], "completed", "{runs}");
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agent_calling_watch_confirm_is_sent_to_the_page() {
    let world = World::ready().await;
    let id = {
        // With no page, the ask is kept all the same, and the result says
        // the person confirms it at a terminal.
        let asked = world.call("watch.create", world.watch_input()).await;
        assert_eq!(asked.exit, ExitClass::PersonNeeded, "{asked:?}");
        assert!(
            asked
                .warnings
                .iter()
                .any(|warning| warning.contains("at a terminal"))
        );
        asked.result.unwrap()["watch_id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    // The agent confirms it itself: with no page there is no way, and
    // nothing is asked of the operating system's prompt.
    let refused = world.call("watch.confirm", json!({"watch_id": id})).await;
    assert_eq!(code_of(&refused), "person_required", "{refused:?}");

    // With a page, the agent's call is shown to the person there, and
    // nothing is confirmed by it.
    *world.surface.pages.lock().unwrap() = true;
    let sent = world.call("watch.confirm", json!({"watch_id": id})).await;
    assert_eq!(sent.exit, ExitClass::PersonNeeded, "{sent:?}");
    let result = sent.result.clone().unwrap();
    assert_eq!(result["state"], "needs_person", "{result}");
    assert_eq!(result["action"], "watch.confirm");
    assert_eq!(
        world
            .surface
            .presence_asked
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    assert_eq!(world.watch(&id).await["state"], "asked");
    // The same call again gives back the same page.
    let again = world.call("watch.confirm", json!({"watch_id": id})).await;
    assert_eq!(again.result.unwrap()["act"], result["act"]);
    assert_eq!(
        world.confirmation().act.to_string(),
        result["act"].as_str().unwrap()
    );
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watch_changes_never_names_a_record_from_another_persons_run() {
    let world = World::ready().await;
    *world.surface.pages.lock().unwrap() = true;
    // Everyone in the workspace may look and change, so that what keeps a
    // second person out below is whose watch it is.
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
    let asked = world.call("watch.create", world.watch_input()).await;
    let id = asked.result.unwrap()["watch_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let ask = world.confirmation();
    assert!(world.confirmed_on_the_page(&ask).await.ok);
    world.clock.advance(Duration::from_secs(10));
    world
        .tended_until(&id, |watch| {
            watch["checks"] == 1 && watch["state"] == "confirmed"
        })
        .await;

    // A second person is told there is no such watch, whatever they ask,
    // and changes nothing.
    let second = world.kernel.second_person([0x77; 16]).unwrap();
    let second = Caller::new(second, FrontDoor::Command, false);
    for action in ["watch.get", "watch.changes", "watch.pause", "watch.end"] {
        let theirs = world
            .call_as(&second, action, json!({"watch_id": id}))
            .await;
        assert_eq!(code_of(&theirs), "not_found", "{action}: {theirs:?}");
    }
    let listed = world.call_as(&second, "watch.list", json!({})).await;
    assert_eq!(listed.result.unwrap()["watches"], json!([]));
    assert_eq!(world.watch(&id).await["state"], "confirmed");

    // The person's own changes name the runs of their checks and no value:
    // a whole read names no record at all.
    let changes = world.ok("watch.changes", json!({"watch_id": id})).await;
    let kinds: Vec<&str> = changes["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|change| change["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["confirmed"], "{changes}");
    assert!(
        !changes.to_string().contains("Example Customer"),
        "{changes}"
    );
    for change in changes["changes"].as_array().unwrap() {
        assert!(change.get("record").is_none(), "{change}");
    }
    world.kernel.close(CloseReason::Asked).await;
}
