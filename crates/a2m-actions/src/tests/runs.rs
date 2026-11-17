//! A run that waits for a person: what a decline does to it, that one
//! resume after an approval completes the write every time, and that
//! reading a run asks nobody anything.

use a2m_actions::{ExitClass, PresenceProof, Response};
use a2m_core::{FailureClass, RunId, Timestamp};
use a2m_policy::{Channel, PresenceCheck};
use serde_json::{Value as Json, json};

use super::kernel::World;
use super::kernel::engine::{ITEM_UPSERT, ORDER_GET, START};

pub(super) fn item(code: &str) -> Json {
    json!({
        "order_id": "A-102",
        "item_code": code,
        "description": "New rail",
        "quantity": 3,
        "length_in": "150.5",
    })
}

pub(super) fn status(response: &Response) -> &str {
    response.result.as_ref().unwrap()["status"]
        .as_str()
        .unwrap()
}

impl World {
    /// Calls the writing tool and gives the run that rests for approval.
    pub(super) async fn rest_a_write(&self, code: &str) -> (String, RunId) {
        let rested = self
            .call(
                "tool.call",
                json!({"tool": ITEM_UPSERT, "input": item(code)}),
            )
            .await;
        assert_eq!(rested.exit, ExitClass::PersonNeeded, "{rested:?}");
        assert_eq!(status(&rested), "needs_approval");
        let run = rested.result.as_ref().unwrap()["run_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let id = run.parse().unwrap();
        (run, id)
    }

    /// The person approves the run at the desk, the diff the desk shows.
    pub(super) async fn approve(&self, run: RunId) {
        let desk = self.surface.desk();
        let waiting = desk.waiting(run).await.unwrap();
        let person = self.caller.principal.principal().clone();
        let proof = PresenceProof::new(
            PresenceCheck::OsPrompt,
            person.id,
            Timestamp::from_unix_millis(START),
        );
        desk.approve(run, waiting.diff_hash, person, Channel::LocalPrompt, proof)
            .await
            .unwrap();
    }

    fn asked(&self) -> usize {
        self.surface.approvals.lock().unwrap().len()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_decline_ends_the_run_as_expired_and_the_caller_is_told() {
    let world = World::ready().await;
    let desk = world.surface.desk();
    let person = world.caller.principal.principal().clone();

    // A person presses Decline.
    let (run, id) = world.rest_a_write("L-401").await;
    let asked = world.asked();
    desk.decline(id, Some(person), Channel::LocalPrompt)
        .await
        .unwrap();
    // The run has ended: it waits for nobody, and nobody is asked again.
    assert!(desk.waiting(id).await.is_err());
    for action in ["run.get", "run.resume", "run.get"] {
        let told = world.call(action, json!({"run_id": run})).await;
        assert_eq!(status(&told), "expired", "{action}: {told:?}");
        assert_eq!(told.exit, ExitClass::Failed, "{action}");
        let error = told.error.clone().unwrap();
        assert_eq!(
            (error.class, error.code.as_str()),
            (FailureClass::Policy, "approval_refused"),
            "{action}"
        );
        assert!(error.message.contains("declined"), "{}", error.message);
        // Nothing was sent, so the same call may be made again.
        assert!(error.retry_safe);
        let result = told.result.as_ref().unwrap();
        assert!(result.get("link").is_none());
        assert!(result["person"].as_str().unwrap().contains("declined"));
    }
    assert_eq!(world.asked(), asked);
    assert_eq!(world.app.write_count("A-102", "L-401").await.unwrap(), 0);
    // The ledger tells it: the last event is the move to `expired`.
    let trace = world.ok("trace.show", json!({"run_id": run})).await;
    let last = trace["events"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["kind"], "state_changed");
    assert_eq!(last["detail"]["to"], "expired");
    assert_eq!(last["detail"]["code"], "approval_refused");
    // A decline is final: an approval after it is refused, and a second
    // decline changes nothing.
    assert!(desk.decline(id, None, Channel::LocalPrompt).await.is_ok());

    // Silence is a decline by nobody, and says so.
    let (run, id) = world.rest_a_write("L-402").await;
    desk.decline(id, None, Channel::LocalPrompt).await.unwrap();
    let told = world.call("run.get", json!({"run_id": run})).await;
    assert_eq!(status(&told), "expired");
    assert!(
        told.error.unwrap().message.contains("no answer"),
        "{:?}",
        told.result
    );

    // The same call again is a new run with a diff of its own.
    let (again, _) = world.rest_a_write("L-401").await;
    assert_ne!(again, run);
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reading_a_run_asks_nobody_and_an_approved_run_says_so() {
    let world = World::ready().await;
    let (run, id) = world.rest_a_write("L-410").await;
    // The call that rested the run told its person, once.
    let asked = world.asked();
    assert_eq!(asked, 1);
    let rested = world.call("run.get", json!({"run_id": run})).await;
    assert_eq!(status(&rested), "needs_approval");
    assert_eq!(rested.exit, ExitClass::PersonNeeded);
    // The link the person was given is still shown; nobody was asked for it.
    assert!(
        rested.result.as_ref().unwrap()["link"]
            .as_str()
            .unwrap()
            .contains(&run)
    );
    assert_eq!(world.asked(), asked);
    // A resume that finds the run still waiting gives the same link, and
    // the person is not told a second time of the same diff.
    let still = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(status(&still), "needs_approval");
    assert!(
        still.result.as_ref().unwrap()["link"]
            .as_str()
            .unwrap()
            .contains(&run)
    );
    assert_eq!(world.asked(), asked);

    world.approve(id).await;
    for _ in 0..3 {
        let read = world.call("run.get", json!({"run_id": run})).await;
        assert_eq!(status(&read), "needs_approval");
        let result = read.result.as_ref().unwrap();
        assert_eq!(result["approved"], true, "{result}");
        assert!(result["person"].as_str().unwrap().contains("approved"));
        assert!(result.get("link").is_none(), "{result}");
        assert_eq!(read.next.as_ref().unwrap().action, "run.resume");
        // An approved run needs no person: the caller's next step is its own.
        assert_eq!(read.exit, ExitClass::Succeeded);
    }
    assert_eq!(world.asked(), asked);
    assert_eq!(world.app.write_count("A-102", "L-410").await.unwrap(), 0);

    let done = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(status(&done), "completed", "{done:?}");
    assert_eq!(world.asked(), asked);
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_desk_shows_the_change_a_waiting_run_would_make_and_no_other() {
    let world = World::ready().await;
    let desk = world.surface.desk();
    // A value the vault released as a secret is in the write.
    let stored = world
        .call(
            "secret.set",
            json!({"name": "example_password", "value": "New rail"}),
        )
        .await;
    assert!(stored.ok, "{:?}", stored.error);

    let (run, id) = world.rest_a_write("L-420").await;
    let waiting = desk.waiting(id).await.unwrap();
    let preview = desk.preview(id, 0).await.unwrap();
    // The page shows the diff whose hash it will approve.
    assert_eq!(preview.diff_hash, waiting.diff_hash);
    assert_eq!(preview.tool, ITEM_UPSERT);
    assert_eq!(preview.target, "example");
    assert_eq!(
        (preview.create, preview.update, preview.unchanged),
        (1, 0, 0)
    );
    assert_eq!((preview.total, preview.offset), (1, 0));
    assert!(preview.complete());
    let record = &preview.records[0];
    assert_eq!(record.action, "create");
    assert_eq!(record.key, "A-102 / L-420");
    let field = |name: &str| {
        record
            .fields
            .iter()
            .find(|field| field.name == name)
            .unwrap_or_else(|| panic!("no field {name} in {record:?}"))
    };
    // From nothing, to the exact values the write would leave.
    assert_eq!(field("quantity").current, None);
    assert_eq!(field("quantity").proposed, "3");
    assert_eq!(field("length_in").proposed, "150.5");
    assert!(field("quantity").changes && !field("quantity").cut);
    // Redacted like everything that leaves.
    assert_eq!(field("description").proposed, a2m_core::REDACTED);
    // Past the end there is nothing, and it says so.
    let past = desk.preview(id, 5).await.unwrap();
    assert!(past.records.is_empty() && past.complete());

    // Only a run that waits has a preview.
    world.approve(id).await;
    let done = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(status(&done), "completed");
    let gone = desk.preview(id, 0).await.unwrap_err();
    assert_eq!(
        (gone.class(), gone.code().as_str()),
        (FailureClass::Input, "not_found")
    );
    let read = world
        .call(
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
        .await;
    let read: RunId = read.result.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(desk.preview(read, 0).await.is_err());

    // A long value is cut and marked.
    let long = "x".repeat(a2m_actions::PREVIEW_VALUE_CHARS + 50);
    let mut input = item("L-421");
    input["description"] = json!(long);
    let rested = world
        .call("tool.call", json!({"tool": ITEM_UPSERT, "input": input}))
        .await;
    assert_eq!(status(&rested), "needs_approval", "{rested:?}");
    let id: RunId = rested.result.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let preview = desk.preview(id, 0).await.unwrap();
    let description = preview.records[0]
        .fields
        .iter()
        .find(|field| field.name == "description")
        .unwrap();
    assert!(description.cut);
    assert_eq!(
        description.proposed.chars().count(),
        a2m_actions::PREVIEW_VALUE_CHARS
    );
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}

/// How many approved writes the loop below resumes.
const RESUMES: usize = 300;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_resume_after_an_approval_completes_the_write_every_time_under_load() {
    let world = std::sync::Arc::new(World::ready().await);
    // Load: readers that keep the runner, the store, and the wake-ups busy
    // for as long as the loop runs.
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut readers = Vec::new();
    for reader in 0..3 {
        let world = std::sync::Arc::clone(&world);
        let stop = std::sync::Arc::clone(&stop);
        readers.push(tokio::spawn(async move {
            let mut read = 0_u32;
            while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                let order = if reader % 2 == 0 { "A-100" } else { "A-101" };
                let response = world
                    .call(
                        "tool.call",
                        json!({"tool": ORDER_GET, "input": {"order_id": order}}),
                    )
                    .await;
                assert!(response.ok, "{:?}", response.error);
                read += 1;
            }
            read
        }));
    }

    for round in 0..RESUMES {
        let code = format!("L-{}", 1000 + round);
        let (run, id) = world.rest_a_write(&code).await;
        world.approve(id).await;
        let done = world.call("run.resume", json!({"run_id": run})).await;
        assert_eq!(
            status(&done),
            "completed",
            "round {round}: {:?} {:?}",
            done.error,
            done.result
        );
        assert!(done.ok);
    }
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let mut read = 0;
    for reader in readers {
        read += reader.await.unwrap();
    }
    assert!(read > 0);
    // The test application holds each write exactly once.
    for round in [0, RESUMES / 2, RESUMES - 1] {
        let code = format!("L-{}", 1000 + round);
        assert_eq!(world.app.write_count("A-102", &code).await.unwrap(), 1);
    }
    std::sync::Arc::try_unwrap(world)
        .ok()
        .unwrap()
        .kernel
        .close(a2m_actions::CloseReason::Asked)
        .await;
}
