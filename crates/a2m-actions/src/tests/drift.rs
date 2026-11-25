//! Package 9.1 through the kernel (decision 0380): the test application's
//! order screen is changed on purpose; a canary, the check of a watch of
//! the read that goes through that screen, catches it; the write through
//! the same screen is then refused before anything reaches the system, and
//! says which tool and binding drifted; a repaired version, whose screen
//! model says what the screen now shows, is published, and the write is
//! made, approved, and verified, with no write ever failed. What was
//! written is asked of the test application.

use std::time::Duration;

use a2m_actions::{CloseReason, ExitClass, PresenceProof};
use a2m_core::Version;
use a2m_policy::PresenceCheck;
use serde_json::json;

use super::kernel::engine::{
    self, ITEM_UPSERT, ORDER_GET, SCREEN, SHEET_ENTER, STOCK_GET, screens,
};
use super::kernel::{Options, World, world};
use super::runs::{item, status};

/// How many passes of tending a test waits through for the canary to end
/// before it fails by name.
const PASSES: u32 = 1_500;

impl World {
    /// A kernel with its runner started and the `example` target published
    /// with its screens modelled as `layout`.
    async fn with_screens(layout: &str) -> Self {
        let world = world(Options::default()).await;
        world.kernel.start_runner_loop(4).await.unwrap();
        world.publish_screens(layout, 0).await;
        *world.surface.pages.lock().unwrap() = true;
        world
    }

    /// Publishes a version of the target whose screens are modelled as
    /// `layout`, numbered by `patch`.
    async fn publish_screens(&self, layout: &str, patch: u32) {
        let mut compiled_target = engine::compiled_target(&self.app.url(""));
        screens(&mut compiled_target, layout);
        compiled_target.header.version = Version::new(1, 0, patch);
        self.kernel
            .publish_compiled_target(&compiled_target)
            .await
            .unwrap();
    }

    /// A canary of the read of an order: a watch of it, confirmed by its
    /// person on the page, whose first check is tended until it ends.
    async fn canary(&self) -> serde_json::Value {
        let asked = self
            .call(
                "watch.create",
                json!({
                    "tool": ORDER_GET,
                    "input": {"order_id": "A-100"},
                    "interval_seconds": 60,
                    "ends_at": self.now().as_unix_millis() + 3_600_000,
                }),
            )
            .await;
        let id = asked.result.unwrap()["watch_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let ask = self
            .surface
            .acts
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|ask| ask.action.as_str() == "watch.confirm")
            .cloned()
            .unwrap();
        let person = self.caller.principal.principal().id;
        let proof =
            PresenceProof::new(PresenceCheck::OsPrompt, person, self.now()).for_act(ask.act);
        assert!(
            self.surface
                .desk()
                .carry_out(ask.act, proof)
                .await
                .unwrap()
                .ok
        );
        self.clock.advance(Duration::from_secs(10));
        for _ in 0..PASSES {
            self.kernel.tend().await;
            let watch = self.ok("watch.get", json!({"watch_id": id})).await;
            // The check has ended: the watch is no longer running it.
            if watch["checks"] == 1 && watch["state"] != "running" {
                return watch;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the canary never ended");
    }

    /// Each published tool as `tool.search` lists it, and as
    /// `tool.describe` describes it, by name: both say the same of whether
    /// it is available.
    async fn listed(&self) -> std::collections::BTreeMap<String, serde_json::Value> {
        let found = self.ok("tool.search", json!({})).await;
        let mut tools = std::collections::BTreeMap::new();
        for tool in found["tools"].as_array().unwrap() {
            let name = tool["name"].as_str().unwrap().to_owned();
            let described = self.ok("tool.describe", json!({"tool": name})).await;
            assert_eq!(described["available"], tool["available"], "{described}");
            assert_eq!(described["unavailable"], tool["unavailable"], "{described}");
            tools.insert(name, tool.clone());
        }
        tools
    }

    /// The requests the test application heard for the line item `code`.
    async fn heard_of(&self, code: &str) -> usize {
        self.app
            .seen_all()
            .await
            .unwrap()
            .iter()
            .filter(|line| line.contains(&format!("/items/{code}")))
            .count()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_changed_screen_is_caught_by_a_canary_stops_the_write_and_is_repaired_with_no_failed_write()
 {
    let world = World::with_screens("v1").await;

    // The screen is changed on purpose: it no longer shows what its model
    // names. The canary's read finds it, and the watch stops on it.
    *world.hooks.screen_shows.lock().unwrap() = Some("v2".to_owned());
    let watch = world.canary().await;
    assert_eq!(watch["state"], "paused", "{watch}");
    assert_eq!(watch["reason"], "drifted", "{watch}");

    // The read the canary found drifted, through both its bindings, the
    // write through the same screen, and the pipeline that writes through
    // it stay listed, marked unavailable with the screen's name and when
    // the mark was made (decision 0396). The read through no screen is
    // available.
    let listed = world.listed().await;
    for stopped in [ORDER_GET, ITEM_UPSERT, SHEET_ENTER] {
        let tool = &listed[stopped];
        assert_eq!(tool["available"], false, "{tool}");
        assert_eq!(tool["unavailable"]["reason"], "drift", "{tool}");
        assert_eq!(tool["unavailable"]["knowledge"], SCREEN, "{tool}");
        assert!(tool["unavailable"]["marked_at"].is_i64(), "{tool}");
    }
    assert_eq!(
        listed[STOCK_GET]["available"], true,
        "{}",
        listed[STOCK_GET]
    );
    assert!(listed[STOCK_GET].get("unavailable").is_none());

    // The write through the same screen is refused before anything is
    // sent: nothing is asked of the person, and the application heard
    // nothing of the line item.
    let refused = world
        .call(
            "tool.call",
            json!({"tool": ITEM_UPSERT, "input": item("L-910")}),
        )
        .await;
    assert!(!refused.ok, "{refused:?}");
    let error = refused.error.as_ref().unwrap();
    assert_eq!(error.code, "binding_drifted", "{refused:?}");
    assert!(error.message.contains(ITEM_UPSERT), "{refused:?}");
    assert!(world.surface.approvals.lock().unwrap().is_empty());
    assert_eq!(world.heard_of("L-910").await, 0);
    assert_eq!(world.app.write_count("A-102", "L-910").await.unwrap(), 0);

    // The repair: a new version whose screen model says what the screen
    // shows now, authored and published as any version is. Nothing was
    // healed in a run.
    world.publish_screens("v2", 1).await;
    let listed = world.listed().await;
    assert!(
        listed.values().all(|tool| tool["available"] == true),
        "{listed:?}"
    );
    let rested = world
        .call(
            "tool.call",
            json!({"tool": ITEM_UPSERT, "input": item("L-910")}),
        )
        .await;
    assert_eq!(rested.exit, ExitClass::PersonNeeded, "{rested:?}");
    assert_eq!(status(&rested), "needs_approval");
    let run = rested.result.as_ref().unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    world.approve(run.parse().unwrap()).await;
    let done = world.call("run.resume", json!({"run_id": run})).await;
    assert!(done.ok, "{:?} {:?}", done.error, done.result);
    assert_eq!(status(&done), "completed");

    // The application holds the one write, with its values; no run of the
    // writing tool failed.
    assert_eq!(world.app.write_count("A-102", "L-910").await.unwrap(), 1);
    let stored = world
        .app
        .stored_item("A-102", "L-910")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.quantity, 3);
    let runs = world.ok("run.list", json!({"tool": ITEM_UPSERT})).await;
    let statuses: Vec<&str> = runs["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|run| run["status"].as_str().unwrap())
        .collect();
    assert!(statuses.contains(&"completed"), "{runs}");
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_canary_that_passes_again_lifts_the_mark_and_a_version_that_kept_the_screen_stays_stopped()
 {
    let world = World::with_screens("v1").await;
    *world.hooks.screen_shows.lock().unwrap() = Some("v2".to_owned());
    let watch = world.canary().await;
    assert_eq!(watch["reason"], "drifted", "{watch}");

    // A version that changed nothing of the screen is stopped all the same,
    // and listed so.
    world.publish_screens("v1", 1).await;
    assert_eq!(world.listed().await[ITEM_UPSERT]["available"], false);
    let refused = world
        .call(
            "tool.call",
            json!({"tool": ITEM_UPSERT, "input": item("L-911")}),
        )
        .await;
    assert_eq!(
        refused.error.as_ref().map(|error| error.code.as_str()),
        Some("binding_drifted"),
        "{refused:?}"
    );

    // The run the drift stopped is stopped, not failed, and wrote nothing.
    let stopped = refused.result.as_ref().unwrap();
    assert_eq!(stopped["status"], "stopped", "{refused:?}");
    let run = stopped["run_id"].as_str().unwrap().to_owned();
    assert_eq!(world.heard_of("L-911").await, 0);

    // The screen shows what its model says again, and a canary through it
    // passes: its pass lifts the mark, and the stopped run, resumed, goes
    // as far as the person's approval.
    *world.hooks.screen_shows.lock().unwrap() = None;
    let watch = world.canary().await;
    assert_eq!(watch["state"], "confirmed", "{watch}");
    assert_eq!(world.listed().await[ITEM_UPSERT]["available"], true);
    let rested = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(rested.exit, ExitClass::PersonNeeded, "{rested:?}");
    assert_eq!(status(&rested), "needs_approval");
    world.kernel.close(CloseReason::Asked).await;
}

/// Which tools the marks make unavailable, without a run (decision 0396,
/// point 1): a read with a second binding no mark covers is available; a
/// write whose one binding a mark covers is not, and names the latest mark
/// that covers it; a pipeline is unavailable when a step's operation is; a
/// mark that cannot be read makes every tool unavailable and names nothing.
#[test]
fn a_tool_is_unavailable_when_every_binding_of_what_it_runs_is_stopped() {
    use crate::drift::{Held, Marks, unavailable};
    use a2m_engine::{DriftKnowledge, DriftMark, section_digest};

    let mut compiled_target = engine::compiled_target("http://127.0.0.1:1");
    screens(&mut compiled_target, "v1");
    let get = compiled_target
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == ORDER_GET)
        .unwrap();
    let first = &get.bindings[0];
    let reference = first.act[0].refers_to.clone().unwrap();
    let mark = |found_at: i64| Held {
        mark: DriftMark {
            operation: get.name.clone(),
            mode: first.mode.clone(),
            surface: compiled_target.surface(first.surface).unwrap().name.clone(),
            knowledge: DriftKnowledge::Entry {
                section: section_digest(&compiled_target, &reference).unwrap(),
                entry: SCREEN.to_owned(),
            },
        },
        entry: SCREEN.to_owned(),
        found_at,
    };
    let marks = Marks::Read(vec![mark(5), mark(9)]);
    let stopped = unavailable(&compiled_target, &marks);
    let name = |text: &str| a2m_core::OperationName::new(text).unwrap();
    assert!(!stopped.contains_key(&name(ORDER_GET)), "{stopped:?}");
    assert!(!stopped.contains_key(&name(STOCK_GET)), "{stopped:?}");
    let write = &stopped[&name(ITEM_UPSERT)];
    assert_eq!(write.knowledge, SCREEN);
    assert_eq!(write.marked_at, Some(9));
    assert_eq!(stopped[&name(SHEET_ENTER)], *write);

    let every = unavailable(&compiled_target, &Marks::Unreadable);
    for tool in &compiled_target.tools {
        let why = every.get(&tool.name);
        assert!(why.is_some(), "{} is listed available", tool.name);
        assert_eq!(why.unwrap().knowledge, "");
        assert_eq!(why.unwrap().marked_at, None);
    }
    assert!(unavailable(&compiled_target, &Marks::Read(Vec::new())).is_empty());
}

impl World {
    /// Asks for a canary of the read of an order, every minute for an
    /// hour, as an agent would (decision 0999). Gives its identifier; it is
    /// not confirmed.
    async fn ask_canary(&self) -> String {
        let asked = self
            .call(
                "canary.create",
                json!({
                    "tool": ORDER_GET,
                    "input": {"order_id": "A-100"},
                    "interval_seconds": 60,
                    "ends_at": self.now().as_unix_millis() + 3_600_000,
                }),
            )
            .await;
        assert_eq!(asked.exit, ExitClass::PersonNeeded, "{asked:?}");
        let result = asked.result.unwrap();
        assert_eq!(result["state"], "asked", "{result}");
        result["canary_id"].as_str().unwrap().to_owned()
    }

    /// The canary's person confirms it on the page they were shown.
    async fn confirm_canary(&self, id: &str) {
        let ask = self
            .surface
            .acts
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|ask| {
                ask.action.as_str() == "canary.confirm"
                    && ask.input.iter().any(|field| field.value == id)
            })
            .cloned()
            .unwrap();
        // The page shows what the canary sends and what its drift stops.
        assert!(
            ask.details.iter().any(|line| line.contains(ORDER_GET)),
            "{:?}",
            ask.details
        );
        let person = self.caller.principal.principal().id;
        let proof =
            PresenceProof::new(PresenceCheck::OsPrompt, person, self.now()).for_act(ask.act);
        let done = self.surface.desk().carry_out(ask.act, proof).await.unwrap();
        assert!(done.ok, "{done:?}");
    }

    /// Moves the clock by `by` and tends until the canary `id` has made
    /// `checks` checks and is not running one.
    async fn canary_checked(&self, id: &str, by: Duration, checks: u64) -> serde_json::Value {
        self.clock.advance(by);
        for _ in 0..PASSES {
            self.kernel.tend().await;
            let canary = self.ok("canary.get", json!({"canary_id": id})).await;
            if canary["checks"] == checks && canary["state"] != "running" {
                return canary;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the canary's check never ended");
    }
}

/// Package 9.1b (decision 0999): a canary an agent asked for and nobody
/// confirmed sends nothing, whatever time passes, and stops nothing,
/// although the screen it would read has changed; it ends at its end, and
/// it can no longer be confirmed then. What the application heard is asked
/// of the application.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_canary_nobody_confirmed_sends_nothing_and_stops_nothing() {
    let world = World::with_screens("v1").await;
    *world.hooks.screen_shows.lock().unwrap() = Some("v2".to_owned());
    let heard = world.app.seen_all().await.unwrap().len();
    let id = world.ask_canary().await;

    // Past every due time it would have had, and past its end.
    for _ in 0..62 {
        world.clock.advance(Duration::from_secs(60));
        world.kernel.tend().await;
    }
    let canary = world.ok("canary.get", json!({"canary_id": id})).await;
    assert_eq!(canary["state"], "ended", "{canary}");
    assert_eq!(canary["ended"], "end_reached", "{canary}");
    assert_eq!(canary["checks"], 0, "{canary}");
    assert!(canary.get("outcome").is_none(), "{canary}");
    assert_eq!(world.app.seen_all().await.unwrap().len(), heard);
    let runs = world.ok("run.list", json!({"tool": ORDER_GET})).await;
    assert_eq!(runs["runs"].as_array().unwrap().len(), 0, "{runs}");

    // Nothing was marked: every tool is listed available.
    let listed = world.listed().await;
    assert!(
        listed.values().all(|tool| tool["available"] == true),
        "{listed:?}"
    );

    // The person's page, pressed now, confirms nothing: the canary ended.
    let ask = world
        .surface
        .acts
        .lock()
        .unwrap()
        .iter()
        .find(|ask| ask.action.as_str() == "canary.confirm")
        .cloned()
        .unwrap();
    let person = world.caller.principal.principal().id;
    let proof = PresenceProof::new(PresenceCheck::OsPrompt, person, world.now()).for_act(ask.act);
    let pressed = world.surface.desk().carry_out(ask.act, proof).await;
    assert!(!pressed.is_ok_and(|done| done.ok));
    let canary = world.ok("canary.get", json!({"canary_id": id})).await;
    assert_eq!(canary["state"], "ended", "{canary}");
    assert_eq!(world.app.seen_all().await.unwrap().len(), heard);
    world.kernel.close(CloseReason::Asked).await;
}

/// Package 9.1b (decision 0999): a canary its person confirmed, on its own
/// schedule and with no watch, finds the order screen changed on purpose;
/// the write through the same screen is then refused before any call, with
/// nothing sent and nobody asked. The canary goes on at its interval, and
/// when the screen shows what its model says again its next check passes
/// and lifts the mark.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_confirmed_canary_finds_a_changed_screen_and_stops_its_bindings_before_any_call() {
    let world = World::with_screens("v1").await;
    *world.hooks.screen_shows.lock().unwrap() = Some("v2".to_owned());
    let id = world.ask_canary().await;
    world.confirm_canary(&id).await;
    let canary = world.canary_checked(&id, Duration::from_secs(10), 1).await;
    assert_eq!(canary["state"], "confirmed", "{canary}");
    assert_eq!(canary["outcome"], "drifted", "{canary}");
    assert_eq!(canary["failure"]["class"], "drift", "{canary}");
    assert!(canary["next_due_at"].is_i64(), "{canary}");
    // No watch was asked for or made.
    let watches = world.ok("watch.list", json!({})).await;
    assert_eq!(watches["watches"].as_array().unwrap().len(), 0);

    // The read, the write through the same screen, and the pipeline that
    // writes through it are listed unavailable, with the screen's name.
    let listed = world.listed().await;
    for stopped in [ORDER_GET, ITEM_UPSERT, SHEET_ENTER] {
        let tool = &listed[stopped];
        assert_eq!(tool["available"], false, "{tool}");
        assert_eq!(tool["unavailable"]["knowledge"], SCREEN, "{tool}");
    }
    assert_eq!(listed[STOCK_GET]["available"], true);

    // The write is refused before any call: nothing is asked of the
    // person, and the application heard nothing of the line item.
    let refused = world
        .call(
            "tool.call",
            json!({"tool": ITEM_UPSERT, "input": item("L-912")}),
        )
        .await;
    assert_eq!(
        refused.error.as_ref().map(|error| error.code.as_str()),
        Some("binding_drifted"),
        "{refused:?}"
    );
    assert!(world.surface.approvals.lock().unwrap().is_empty());
    assert_eq!(world.heard_of("L-912").await, 0);
    assert_eq!(world.app.write_count("A-102", "L-912").await.unwrap(), 0);

    // The screen shows what its model says again: the canary's next check,
    // a minute on, passes and lifts the mark.
    *world.hooks.screen_shows.lock().unwrap() = None;
    let canary = world.canary_checked(&id, Duration::from_secs(60), 2).await;
    let runs = world.ok("run.list", json!({"tool": ORDER_GET})).await;
    assert_eq!(canary["outcome"], "passed", "{canary} {runs}");
    assert!(canary.get("failure").is_none(), "{canary}");
    assert_eq!(world.listed().await[ITEM_UPSERT]["available"], true);

    // Its person ends it; nothing more is sent.
    let ended = world.ok("canary.end", json!({"canary_id": id})).await;
    assert_eq!(ended["ended"], "ended_by_person", "{ended}");
    world.kernel.close(CloseReason::Asked).await;
}
