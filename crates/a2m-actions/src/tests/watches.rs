//! Watches through the dispatcher (decision 0283), against the built-in
//! test application: an agent's ask runs nothing until its person confirms
//! it on the page; an agent that calls the confirmation is sent to the page;
//! a watch, and every change it recorded, is its own person's alone; and
//! two watches of one read share its run, which ending one never cancels
//! while the other rests on it (decisions 0285 and 0287).

use std::sync::atomic::Ordering;
use std::time::Duration;

use a2m_actions::{
    Caller, CloseReason, ExitClass, FrontDoor, PersonActAsk, PresenceProof, Response,
};
use a2m_core::{RunId, RunState};
use a2m_ledger::{ChangeKind, CheckOutcome, CheckRow, WatchId, WatchReason, WatchRow, WatchState};
use a2m_policy::{PresenceCheck, Role, RoleBinding, Subject};
use serde_json::{Value as Json, json};

use super::kernel::engine::ORDER_GET;
use super::kernel::{Options, World, engine, world};

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

/// A world whose runner is enrolled and claims nothing: a check's run stays
/// queued, and so unfinished, until the test starts the runner's loop. What the watches stored is read
/// from the ledger itself.
impl World {
    async fn held() -> Self {
        let world = world(Options::default()).await;
        world
            .kernel
            .publish_compiled_target(&engine::compiled_target(&world.app.url("")))
            .await
            .unwrap();
        *world.surface.pages.lock().unwrap() = true;
        // The runner is enrolled, so that a check is admitted, and claims
        // nothing until the test starts its loop again.
        world
            .kernel
            .start_runner_loop(4)
            .await
            .unwrap()
            .stop()
            .await;
        world
    }

    /// A watch of the one read, asked and confirmed on the page, that ends
    /// `ends_in` after now.
    async fn confirmed_watch(&self, ends_in: Duration) -> WatchId {
        let mut input = self.watch_input();
        input["ends_at"] =
            json!(self.now().as_unix_millis() + i64::try_from(ends_in.as_millis()).unwrap());
        let asked = self.call("watch.create", input).await;
        let id = asked.result.unwrap()["watch_id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let ask = self.confirmation();
        assert!(self.confirmed_on_the_page(&ask).await.ok);
        id
    }

    async fn stored_watch(&self, id: WatchId) -> WatchRow {
        let store = self.kernel.store().unwrap();
        store.read(move |tx| tx.watches().get(id)).await.unwrap()
    }

    async fn stored_checks(&self, id: WatchId) -> Vec<CheckRow> {
        let store = self.kernel.store().unwrap();
        store
            .read(move |tx| tx.watches().checks(id, None, 100))
            .await
            .unwrap()
    }

    async fn stored_kinds(&self, id: WatchId) -> Vec<ChangeKind> {
        let store = self.kernel.store().unwrap();
        store
            .read(move |tx| tx.watches().changes(id, 0, 100))
            .await
            .unwrap()
            .iter()
            .map(|change| change.kind)
            .collect()
    }

    async fn run_state(&self, run: RunId) -> RunState {
        let store = self.kernel.store().unwrap();
        store
            .read(move |tx| tx.ledger().run(run))
            .await
            .unwrap()
            .state
    }

    async fn end_watch(&self, id: WatchId) {
        let ended = self
            .ok("watch.end", json!({"watch_id": id.to_string()}))
            .await;
        assert_eq!(ended["state"], "ended", "{ended}");
    }

    /// Two watches of the one read, both due in one pass: the first's check
    /// opens a run and the second's is handed the same one. Gives them as
    /// (the watch whose check opened the run, the other) and the run.
    async fn two_watches_on_one_run(&self) -> (WatchId, WatchId, RunId) {
        let first = self.confirmed_watch(Duration::from_secs(3_600)).await;
        // Another end: a watch that differs in what it was asked for, and
        // reads the same thing.
        let second = self.confirmed_watch(Duration::from_secs(7_200)).await;
        self.clock.advance(Duration::from_secs(10));
        self.kernel.tend().await;
        let (one, two) = (
            self.stored_checks(first).await,
            self.stored_checks(second).await,
        );
        assert_eq!((one.len(), two.len()), (1, 1), "{one:?} {two:?}");
        assert_eq!(one[0].run, two[0].run, "the two checks share one run");
        assert_ne!(one[0].opened_run, two[0].opened_run, "{one:?} {two:?}");
        let run = one[0].run;
        if one[0].opened_run {
            (first, second, run)
        } else {
            (second, first, run)
        }
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
    // from the compiled target.
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ending_one_watch_leaves_the_run_another_rests_on_and_it_ends_with_its_result() {
    let world = World::held().await;
    let (opener, other, run) = world.two_watches_on_one_run().await;
    assert_eq!(world.run_state(run).await, RunState::Queued);

    // The watch whose check opened the run ends. Its check closes as a gap,
    // and the run the other check rests on is not cancelled.
    world.end_watch(opener).await;
    assert_eq!(world.run_state(run).await, RunState::Queued);
    let closed = &world.stored_checks(opener).await[0];
    assert_eq!(closed.outcome, CheckOutcome::Failed);
    assert_eq!(
        world.stored_kinds(opener).await,
        [ChangeKind::Confirmed, ChangeKind::Gap, ChangeKind::Ended]
    );
    assert_eq!(world.stored_watch(other).await.state, WatchState::Running);

    // The run is carried out once; the other watch's check ends with its
    // result, and records no gap. It is cut short, as every check is while
    // the engine records no completeness (decision 0262).
    world.kernel.start_runner_loop(4).await.unwrap();
    for _ in 0..PASSES {
        world.kernel.tend().await;
        if world.stored_watch(other).await.state == WatchState::Confirmed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let check = &world.stored_checks(other).await[0];
    assert_eq!(check.outcome, CheckOutcome::CutShort, "{check:?}");
    assert_eq!(check.records_seen, Some(1), "{check:?}");
    assert_eq!(check.run, run);
    assert_eq!(world.run_state(run).await, RunState::Completed);
    assert_eq!(world.stored_kinds(other).await, [ChangeKind::Confirmed]);
    assert_eq!(world.order_reads().await, 1);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ending_the_last_watch_on_a_run_its_check_opened_cancels_the_run() {
    let world = World::held().await;
    let id = world.confirmed_watch(Duration::from_secs(3_600)).await;
    world.clock.advance(Duration::from_secs(10));
    world.kernel.tend().await;
    let check = world.stored_checks(id).await[0].clone();
    assert!(check.opened_run, "{check:?}");
    world.end_watch(id).await;
    assert_eq!(world.run_state(check.run).await, RunState::Cancelled);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shared_run_that_waits_for_a_sign_in_pauses_every_watch_resting_on_it() {
    let world = World::held().await;
    let (opener, other, run) = world.two_watches_on_one_run().await;
    // The person's session has ended: the run rests at `needs_login`.
    world.hooks.session_ended.store(true, Ordering::SeqCst);
    world.kernel.start_runner_loop(4).await.unwrap();
    for _ in 0..PASSES {
        world.kernel.tend().await;
        let (one, two) = (
            world.stored_watch(opener).await,
            world.stored_watch(other).await,
        );
        if one.state != WatchState::Running && two.state != WatchState::Running {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    for id in [opener, other] {
        let watch = world.stored_watch(id).await;
        assert_eq!(watch.state, WatchState::Paused, "{watch:?}");
        assert_eq!(watch.reason, Some(WatchReason::NeedsLogin), "{watch:?}");
        let check = &world.stored_checks(id).await[0];
        assert_eq!(
            check.why.as_ref().map(|(_, code)| code.as_str()),
            Some("needs_login"),
            "{check:?}"
        );
        assert_eq!(
            world.stored_kinds(id).await,
            [ChangeKind::Confirmed, ChangeKind::Gap, ChangeKind::Paused]
        );
        // What each watch tells its agent names the one run.
        let told = world
            .ok("watch.changes", json!({"watch_id": id.to_string()}))
            .await;
        assert!(
            told["changes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|change| change["run_id"] == run.to_string()),
            "{told}"
        );
    }
    // The run waits for no watch once the last check on it has ended.
    assert_eq!(world.run_state(run).await, RunState::Cancelled);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_check_that_joined_a_run_it_did_not_open_never_cancels_it() {
    let world = World::held().await;
    let id = world.confirmed_watch(Duration::from_secs(3_600)).await;
    // The person's own call of the same read and input is under way, as a
    // run the scheduler admitted for them, not for a watch.
    let row = world.stored_watch(id).await;
    let sched = world.kernel.sched().unwrap();
    let theirs = match sched
        .submit(world.kernel.check_submission(&row))
        .await
        .unwrap()
    {
        a2m_sched::Admitted::New { run, .. } => run,
        other => panic!("the person's call opened no run: {other:?}"),
    };
    world.clock.advance(Duration::from_secs(10));
    world.kernel.tend().await;
    let check = world.stored_checks(id).await[0].clone();
    assert_eq!(check.run, theirs);
    assert!(!check.opened_run, "{check:?}");
    // The watch ends: its check closes, and the person's run goes on.
    world.end_watch(id).await;
    assert_eq!(world.run_state(theirs).await, RunState::Queued);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_condition_is_checked_against_the_reads_outputs_and_ends_the_watch_when_it_holds() {
    let world = World::ready().await;
    *world.surface.pages.lock().unwrap() = true;
    // A condition is an expression over the read's outputs that gives true
    // or false; anything else is refused at the ask, and no watch is kept.
    for (condition, code) in [
        ("result.no_such_output == 1", "expression_invalid"),
        ("result.status", "expression_type_error"),
        ("input.order_id == 'A-100'", "expression_invalid"),
    ] {
        let mut input = world.watch_input();
        input["condition"] = json!(condition);
        let refused = world.call("watch.create", input).await;
        assert_eq!(code_of(&refused), code, "{condition}: {refused:?}");
    }
    let listed = world.ok("watch.list", json!({})).await;
    assert_eq!(listed["watches"], json!([]), "{listed}");

    // One that always holds of what the read returns: the watch ends at its
    // first check, which made the baseline and compared nothing.
    let mut input = world.watch_input();
    input["condition"] = json!("result.lines >= 0");
    let asked = world.call("watch.create", input).await;
    let watch = asked.result.clone().unwrap();
    assert_eq!(watch["condition"], "result.lines >= 0", "{watch}");
    let id: WatchId = watch["watch_id"].as_str().unwrap().parse().unwrap();
    // The person is shown the condition before confirming.
    let ask = world.confirmation();
    let read = ask.details.join("\n");
    assert!(read.contains("result.lines >= 0"), "{read}");
    assert!(world.confirmed_on_the_page(&ask).await.ok);

    world.clock.advance(Duration::from_secs(10));
    let ended = world
        .tended_until(&id.to_string(), |watch| watch["state"] == "ended")
        .await;
    assert_eq!(ended["reason"], "condition_met", "{ended}");
    // The ledger holds one check, completed, and the condition's change
    // between the confirmation and the end.
    let stored = world.stored_watch(id).await;
    assert_eq!(stored.reason, Some(WatchReason::ConditionMet));
    let checks = world.stored_checks(id).await;
    assert_eq!(checks.len(), 1, "{checks:?}");
    assert_ne!(checks[0].outcome, CheckOutcome::Failed, "{checks:?}");
    assert_eq!(
        world.stored_kinds(id).await,
        [
            ChangeKind::Confirmed,
            ChangeKind::ConditionMet,
            ChangeKind::Ended
        ]
    );
    let told = world
        .ok("watch.changes", json!({"watch_id": id.to_string()}))
        .await;
    let met = &told["changes"][1];
    assert_eq!(met["kind"], "condition_met", "{told}");
    assert_eq!(met["run_id"], checks[0].run.to_string(), "{told}");
    // Nothing more is read: the order was read once.
    world.clock.advance(Duration::from_secs(600));
    world.kernel.tend().await;
    assert_eq!(world.order_reads().await, 1);
    world.kernel.close(CloseReason::Asked).await;
}
