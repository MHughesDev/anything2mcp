//! Package 9.1 through the kernel (decision 0399): the test application's
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

use super::kernel::engine::{self, ITEM_UPSERT, ORDER_GET, screens};
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
        let mut plan = engine::plan(&self.app.url(""));
        screens(&mut plan, layout);
        plan.header.version = Version::new(1, 0, patch);
        self.kernel.publish_plan(&plan).await.unwrap();
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

    // A version that changed nothing of the screen is stopped all the same.
    world.publish_screens("v1", 1).await;
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
    let rested = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(rested.exit, ExitClass::PersonNeeded, "{rested:?}");
    assert_eq!(status(&rested), "needs_approval");
    world.kernel.close(CloseReason::Asked).await;
}
