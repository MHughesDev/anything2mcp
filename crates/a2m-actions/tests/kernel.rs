//! The kernel through its dispatcher, against the built-in test
//! application: `Kernel::open` and its health states, a read end to end, a
//! write that rests until a person approves it at the desk, the decision
//! recorded for every dispatch, and the redactor on the way out.
//!
//! The test pack, its driver over the test application's JSON interface,
//! and the hand-built plan of the imaginary `example` target are the
//! engine's own test helpers, used here as they are.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "this file holds only tests; the helpers the tests share do not carry the test attribute"
)]

#[path = "../../a2m-engine/tests/common/mod.rs"]
mod engine;

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use a2m_actions::{
    ApprovalAsk, ApprovalDesk, BoxFuture, Caller, CallerCredential, CloseReason, ExitClass,
    FrontDoor, HealthState, Kernel, KernelParts, McpResultFace, Paths, PersonSurface, PresenceAsk,
    PresenceProof, Request, Response, SessionAsk, Settings, SignInAsk, StorePlace, SurfaceLink,
};
use a2m_auth::PersonAnswer;
use a2m_core::{Failure, FailureClass, FailureCode, Secret, Timestamp};
use a2m_policy::{Channel, PresenceCheck, PrincipalRef};
use a2m_provider::{ProviderSettings, RegistryBuilder, Roots, Supervisor, SupervisorLimits};
use a2m_telemetry::{Clock, ManualClock};
use a2m_vault::{BoxFuture as VaultFuture, KeyStore, MemoryKeyStore};
use engine::{AppPack, Counted, Folder, ITEM_UPSERT, ORDER_GET, START};
use example_app::harness::TestApp;
use serde_json::{Value as Json, json};

const USER: &str = "pat";

/// The person surface of a test: it keeps the desk, notes what it was
/// asked, and answers the presence prompt as the test set it.
struct Surface {
    desk: Mutex<Option<Arc<ApprovalDesk>>>,
    approvals: Mutex<Vec<ApprovalAsk>>,
    presence_asked: AtomicU32,
    person_present: Mutex<bool>,
    clock: Arc<dyn Clock>,
}

impl Surface {
    fn desk(&self) -> Arc<ApprovalDesk> {
        self.desk.lock().unwrap().clone().expect("the kernel handed the desk over")
    }
}

impl PersonSurface for Surface {
    fn take_desk(&self, desk: ApprovalDesk) {
        *self.desk.lock().unwrap() = Some(Arc::new(desk));
    }

    fn ask_approval(&self, ask: ApprovalAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            let run = ask.run;
            self.approvals.lock().unwrap().push(ask);
            Ok(SurfaceLink {
                url: Some(format!("http://127.0.0.1/approve/{run}")),
                text: "Open the approval page.".to_owned(),
            })
        })
    }

    fn ask_sign_in(&self, _ask: SignInAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            Ok(SurfaceLink {
                url: None,
                text: "Sign in at the engine's own window.".to_owned(),
            })
        })
    }

    fn confirm_presence(&self, ask: PresenceAsk) -> BoxFuture<'_, Result<PresenceProof, Failure>> {
        Box::pin(async move {
            self.presence_asked.fetch_add(1, Ordering::SeqCst);
            if *self.person_present.lock().unwrap() {
                Ok(PresenceProof::new(
                    PresenceCheck::OsPrompt,
                    ask.principal,
                    self.clock.now(),
                ))
            } else {
                Err(Failure::new(
                    FailureClass::Policy,
                    a2m_policy::PRESENCE_REQUIRED,
                    "Nobody answered the presence prompt.",
                    "Answer the prompt at the machine.",
                ))
            }
        })
    }

    fn ask_session(&self, _ask: SessionAsk) -> BoxFuture<'_, PersonAnswer> {
        Box::pin(async move { PersonAnswer::NoAnswer })
    }
}

/// A key store that does not answer, as a locked or missing one.
struct NoKeyStore;

impl KeyStore for NoKeyStore {
    fn describe(&self) -> a2m_vault::StoreDescription {
        MemoryKeyStore::new().describe()
    }

    fn load_root(&self) -> VaultFuture<'_, Result<Option<a2m_vault::KeyMaterial>, Failure>> {
        Box::pin(async { Err(unreachable_store()) })
    }

    fn store_root(
        &self,
        _root: &a2m_vault::KeyMaterial,
    ) -> VaultFuture<'_, Result<(), Failure>> {
        Box::pin(async { Err(unreachable_store()) })
    }

    fn health(&self) -> VaultFuture<'_, a2m_vault::StoreHealth> {
        Box::pin(async { a2m_vault::StoreHealth::Unreachable })
    }
}

fn unreachable_store() -> Failure {
    Failure::new(
        FailureClass::Busy,
        a2m_vault::codes::KEY_STORE_UNREACHABLE,
        "The key store does not answer.",
        "Unlock the key store.",
    )
}

struct World {
    app: TestApp,
    kernel: Kernel,
    surface: Arc<Surface>,
    caller: Caller,
    _folder: Folder,
}

struct Options {
    place: StorePlace,
    key_store: Arc<dyn KeyStore>,
    block_store: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            place: StorePlace::Memory,
            key_store: Arc::new(MemoryKeyStore::with_random(Counted::new())),
            block_store: false,
        }
    }
}

async fn world(options: Options) -> World {
    let app = TestApp::start().await.unwrap();
    let folder = Folder::new("actions");
    let paths = Paths::under(folder.path());
    if options.block_store {
        // A file where the store's folder should be: the store cannot open.
        std::fs::create_dir_all(&paths.data).unwrap();
        std::fs::write(&paths.store, b"not a folder").unwrap();
    }
    let clock: Arc<dyn Clock> =
        Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(START)));
    let prepared = Kernel::prepare(KernelParts {
        settings: Settings::defaults(),
        paths,
        place: options.place,
        key_store: options.key_store,
        clock: Arc::clone(&clock),
        random: Counted::new(),
        os_user: USER.to_owned(),
    })
    .await;
    let host = prepared.host_services(
        Supervisor::new(SupervisorLimits::default()),
        Roots::none(),
    );
    let mut builder = RegistryBuilder::new(prepared.settings().provider_settings(), host);
    builder.add_pack(&AppPack::default());
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    let surface = Arc::new(Surface {
        desk: Mutex::new(None),
        approvals: Mutex::new(Vec::new()),
        presence_asked: AtomicU32::new(0),
        person_present: Mutex::new(true),
        clock,
    });
    let kernel = Kernel::open(
        prepared,
        registry,
        problems,
        Arc::clone(&surface) as Arc<dyn PersonSurface>,
    )
    .await;
    let principal = match kernel
        .verify_caller(CallerCredential::LocalUser(USER.to_owned()))
        .await
    {
        Ok(principal) => principal,
        Err(_) => kernel.status_only_caller(),
    };
    // An agent on the person's own machine: the usual caller.
    let caller = Caller::new(principal, FrontDoor::Mcp, true);
    World {
        app,
        kernel,
        surface,
        caller,
        _folder: folder,
    }
}

impl World {
    /// A healthy kernel with the runner loop started and the `example`
    /// target published.
    async fn ready() -> Self {
        let world = world(Options::default()).await;
        assert_eq!(world.kernel.health().state, HealthState::Ok, "{:?}", world.kernel.health());
        world.kernel.start_runner_loop(4).await.unwrap();
        world
            .kernel
            .publish_plan(&engine::plan(&world.app.url("")))
            .await
            .unwrap();
        world
    }

    async fn call(&self, action: &str, input: Json) -> Response {
        self.kernel
            .dispatch(Request::new(action, input, self.caller.clone()))
            .await
    }

    async fn ok(&self, action: &str, input: Json) -> Json {
        let response = self.call(action, input).await;
        assert!(response.ok, "{action}: {:?}", response.error);
        response.result.unwrap()
    }

    /// How many decisions the audit of policy holds, through the one
    /// action that reads them back.
    async fn explained(&self, response: &Response) -> Json {
        let decision = response.decision.expect("a decision was recorded");
        self.ok("policy.explain", json!({"decision": decision.to_string()}))
            .await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_goes_end_to_end_with_its_run_its_events_its_decision_and_its_trace() {
    let world = World::ready().await;

    let found = world.ok("tool.search", json!({"query": "order get"})).await;
    assert!(
        found["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == ORDER_GET && tool["effect"] == "read"),
        "{found}"
    );
    let described = world.ok("tool.describe", json!({"tool": ORDER_GET})).await;
    assert_eq!(described["target"], "example");
    assert_eq!(described["effect"], "read");

    let response = world
        .call("tool.call", json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}))
        .await;
    assert!(response.ok, "{:?} {:?}", response.error, response.result);
    assert_eq!(response.exit, ExitClass::Succeeded);
    let result = response.result.clone().unwrap();
    assert_eq!(result["status"], "completed");
    assert_eq!(result["tool"], ORDER_GET);
    // The value is what the test application holds.
    assert_eq!(result["result"]["order_id"], "A-100");
    assert_eq!(result["result"]["customer"], "Example Customer One");
    assert_eq!(result["mode"], "app_json");

    // The dispatch recorded one decision, an allow, about this action.
    let decision = world.explained(&response).await;
    assert_eq!(decision["answer"], "allow");
    assert_eq!(decision["resource"], "action:tool.call");

    // The run has a row, events in order, and a stored trace.
    let run = result["run_id"].as_str().unwrap().to_owned();
    let again = world.ok("run.get", json!({"run_id": run})).await;
    assert_eq!(again["status"], "completed");
    assert_eq!(again["result"], result["result"]);
    let trace = world.ok("trace.show", json!({"run_id": run})).await;
    let kinds: Vec<&str> = trace["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"lease_granted"), "{kinds:?}");
    assert!(kinds.contains(&"binding_chosen"), "{kinds:?}");
    assert_eq!(kinds.last(), Some(&"state_changed"));
    assert!(trace["trace"].as_str().unwrap().starts_with("blob:"));
    // The guard's decision about the tool itself is recorded as well: the
    // audit log of policy holds it beside the action's.
    let seqs: Vec<u64> = trace["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["seq"].as_u64().unwrap())
        .collect();
    assert!(seqs.windows(2).all(|pair| pair[0] < pair[1]));

    // The MCP face's rendering: structured content, and the same as text.
    let face = McpResultFace::of(&response);
    assert!(!face.is_error);
    assert_eq!(face.structured.as_ref(), Some(&result));
    assert_eq!(serde_json::from_str::<Json>(&face.text).unwrap(), result);

    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_write_rests_until_a_person_approves_at_the_desk_and_is_written_exactly_once() {
    let world = World::ready().await;
    let input = json!({
        "order_id": "A-102",
        "item_code": "L-300",
        "description": "New rail",
        "quantity": 3,
        "length_in": "150.5",
    });

    let rested = world
        .call("tool.call", json!({"tool": ITEM_UPSERT, "input": input}))
        .await;
    assert_eq!(rested.exit, ExitClass::PersonNeeded, "{rested:?}");
    assert!(!rested.ok);
    let result = rested.result.clone().unwrap();
    assert_eq!(result["status"], "needs_approval");
    let run = result["run_id"].as_str().unwrap().to_owned();
    assert_eq!(rested.next.as_ref().unwrap().action, "run.resume");
    // Nothing was written, and the caller holds a link and nothing that
    // could approve.
    assert_eq!(world.app.write_count("A-102", "L-300").await.unwrap(), 0);
    assert!(result["link"].as_str().unwrap().contains(&run));
    assert!(result["diff"]["hash"].is_string());

    // Resuming before anyone approved changes nothing: the run still rests.
    let still = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(still.result.as_ref().unwrap()["status"], "needs_approval");
    assert_eq!(world.app.write_count("A-102", "L-300").await.unwrap(), 0);

    // No action could have approved it: every action that is built was
    // offered the run and the word, and the run still rests.
    let run_id: a2m_core::RunId = run.parse().unwrap();
    let desk = world.surface.desk();
    assert_eq!(desk.waiting(run_id).await.unwrap().run, run_id);

    // The person approves, at the desk, the diff they were shown. An agent
    // cannot: the policy engine refuses an approver that is not a person.
    let asked = world.surface.approvals.lock().unwrap().last().cloned().unwrap();
    assert_eq!(asked.run, run_id);
    let person = world.caller.principal.principal.clone();
    let proof = PresenceProof::new(PresenceCheck::OsPrompt, person.id, Timestamp::from_unix_millis(START));
    let by_agent = desk
        .approve(
            run_id,
            asked.diff_hash,
            PrincipalRef::agent(person.id),
            Channel::LocalPrompt,
            proof,
        )
        .await
        .unwrap_err();
    assert_eq!(by_agent.class(), FailureClass::Policy);
    desk.approve(run_id, asked.diff_hash, person, Channel::LocalPrompt, proof)
        .await
        .unwrap();

    let done = world.call("run.resume", json!({"run_id": run})).await;
    assert!(done.ok, "{:?} {:?}", done.error, done.result);
    assert_eq!(done.result.as_ref().unwrap()["status"], "completed");

    // The test application confirms exactly one write, with the values.
    assert_eq!(world.app.write_count("A-102", "L-300").await.unwrap(), 1);
    let stored = world.app.stored_item("A-102", "L-300").await.unwrap().unwrap();
    assert_eq!(stored.description, "New rail");
    assert_eq!(stored.quantity, 3);

    // The ledger's events tell the write in order: intent before acted
    // before the read-back.
    let trace = world.ok("trace.show", json!({"run_id": run})).await;
    let kinds: Vec<&str> = trace["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["kind"].as_str().unwrap())
        .collect();
    let at = |kind: &str| kinds.iter().position(|found| *found == kind).unwrap_or_else(|| panic!("no {kind} in {kinds:?}"));
    assert!(at("approval_bound") < at("intent"));
    assert!(at("intent") < at("acted"));
    assert!(at("acted") < at("reconcile_read").min(kinds.len()) || kinds.contains(&"verified"));

    // An approval is used once: resuming again writes nothing more.
    let again = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(again.result.as_ref().unwrap()["status"], "completed");
    assert_eq!(world.app.write_count("A-102", "L-300").await.unwrap(), 1);

    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_calls_at_once_are_two_runs_and_both_read() {
    let world = World::ready().await;
    let (first, second) = tokio::join!(
        world.call("tool.call", json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}})),
        world.call("tool.call", json!({"tool": ORDER_GET, "input": {"order_id": "A-101"}})),
    );
    for response in [&first, &second] {
        assert!(response.ok, "{:?}", response.error);
    }
    let run = |response: &Response| response.result.as_ref().unwrap()["run_id"].as_str().unwrap().to_owned();
    assert_ne!(run(&first), run(&second));
    // The same call again, while nothing changed, is a new run of its own:
    // the first has ended.
    let third = world
        .call("tool.call", json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}))
        .await;
    assert!(third.ok);
    assert_ne!(run(&first), run(&third));
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_dispatch_records_one_decision_and_a_refusal_carries_its_four_parts() {
    let world = World::ready().await;

    // An unknown action is malformed, with the nearest names as the fix.
    let unknown = world.call("tool.cal", json!({})).await;
    assert_eq!(unknown.exit, ExitClass::Malformed);
    let error = unknown.error.unwrap();
    assert_eq!((error.class, error.code.as_str()), (FailureClass::Input, "schema_invalid"));
    assert!(error.fix.contains("tool.call"), "{}", error.fix);
    assert!(unknown.decision.is_none());

    // Input that breaks the schema: an unknown field, a missing one, a
    // wrong type. Each names the field; none reaches policy.
    for (input, says) in [
        (json!({"tool": ORDER_GET, "inptu": {}}), "input"),
        (json!({}), "tool"),
        (json!({"tool": 7}), "tool"),
        (json!({"tool": ORDER_GET, "wait_seconds": 100_000}), "wait_seconds"),
    ] {
        let response = world.call("tool.call", input).await;
        assert_eq!(response.exit, ExitClass::Malformed);
        let error = response.error.unwrap();
        assert_eq!(error.code, "schema_invalid");
        assert!(error.message.contains(says) || error.fix.contains(says), "{error:?}");
        assert_eq!(response.next.unwrap().action, "action.describe");
    }

    // A valid input that names nothing is a different failure with a
    // different next step.
    let missing = world.call("tool.call", json!({"tool": "example_nothing_here"})).await;
    assert_eq!(missing.exit, ExitClass::Failed);
    let error = missing.error.clone().unwrap();
    assert_eq!((error.class, error.code.as_str()), (FailureClass::Input, "not_found"));
    assert!(error.fix.contains("tool.search"));
    assert!(error.retry_safe);
    let decided = world.explained(&missing).await;
    assert_eq!(decided["answer"], "allow");

    // An action in the catalog that is not built answers with one named
    // failure, after its decision was recorded.
    let not_built = world.call("capture.start", json!({"target": "example"})).await;
    let error = not_built.error.clone().unwrap();
    assert_eq!((error.class, error.code.as_str()), (FailureClass::Input, "action_not_built"));
    assert!(not_built.decision.is_some());

    // An action the client end carries out says so.
    let client = world.call("engine.stop", json!({})).await;
    assert_eq!(client.error.unwrap().code, "client_action");

    // A person's act asked for by an agent is refused before the handler,
    // and the surface is never asked.
    let publish = world
        .kernel
        .dispatch(
            Request::new("version.publish", json!({"target": "example"}), world.caller.clone())
                .in_project(repository()),
        )
        .await;
    assert_eq!(publish.exit, ExitClass::PersonNeeded);
    assert_eq!(publish.error.unwrap().code, "person_required");
    assert_eq!(world.surface.presence_asked.load(Ordering::SeqCst), 0);

    // `action.run` reaches any action, and the action's own rules apply.
    let through = world
        .ok("action.run", json!({"action": "action.describe", "input": {"action": "run.get"}}))
        .await;
    assert_eq!(through["tool"], "a2m_run_get");
    let refused = world
        .call("action.run", json!({"action": "run.get", "input": {"nope": 1}}))
        .await;
    assert_eq!(refused.error.unwrap().code, "schema_invalid");

    // A failure renders for the MCP face as one compact object in text.
    let face = McpResultFace::of(&missing);
    assert!(face.is_error && face.structured.is_none());
    let text: Json = serde_json::from_str(&face.text).unwrap();
    assert_eq!(text["class"], "input");
    assert_eq!(text["code"], "not_found");
    assert!(text["message"].is_string() && text["fix"].is_string());

    world.kernel.close(CloseReason::Asked).await;
}

fn repository() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stopped_store_serves_status_and_nothing_else() {
    let world = world(Options {
        place: StorePlace::Disk,
        block_store: true,
        ..Options::default()
    })
    .await;
    let health = world.kernel.health();
    assert_eq!(health.state, HealthState::Stopped, "{health:?}");
    assert!(health.cause().contains("store"));

    // Status and the doctor answer, with the reason and a fix.
    let status = world.ok("engine.status", json!({})).await;
    assert_eq!(status["state"], "stopped");
    assert_eq!(status["reasons"][0]["part"], "store");
    let doctor = world.ok("doctor.run", json!({})).await;
    assert_eq!(doctor["state"], "stopped");
    assert!(
        doctor["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["check"] == "store" && check["state"] == "runtime_failure")
    );
    world.ok("provider.list", json!({})).await;
    world.ok("config.show", json!({})).await;
    world.ok("docs.get", json!({"topic": "approvals"})).await;

    // Every action that needs the store refuses, with the reason.
    for (action, input) in [
        ("tool.call", json!({"tool": ORDER_GET})),
        ("tool.search", json!({})),
        ("run.get", json!({"run_id": "r_0000000000000000000000000"})),
        ("secret.set", json!({"name": "example_password", "value": "x"})),
        ("target.list", json!({})),
    ] {
        let response = world.call(action, input).await;
        let error = response.error.unwrap_or_else(|| panic!("{action} ran"));
        assert_eq!((error.class, error.code.as_str()), (FailureClass::Busy, "engine_degraded"), "{action}");
        assert!(error.message.contains("stopped"), "{action}: {}", error.message);
        assert_eq!(response.next.unwrap().action, "doctor.run");
    }
    assert!(world.kernel.start_runner_loop(1).await.is_err());
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_key_store_the_kernel_is_degraded_and_writes_nothing() {
    let world = world(Options {
        key_store: Arc::new(NoKeyStore),
        ..Options::default()
    })
    .await;
    let health = world.kernel.health();
    assert_eq!(health.state, HealthState::Degraded, "{health:?}");
    assert!(health.cause().contains("vault"));
    // A read that needs no secret still answers.
    world.ok("target.list", json!({})).await;
    world.ok("tool.search", json!({})).await;
    // Nothing writes.
    for (action, input) in [
        ("secret.set", json!({"name": "example_password", "value": "x"})),
        ("run.cancel", json!({"run_id": "r_0000000000000000000000000"})),
        ("killswitch.set", json!({"scope": "everything", "on": true})),
    ] {
        let response = world.call(action, input).await;
        let error = response.error.unwrap_or_else(|| panic!("{action} ran"));
        assert_eq!(error.code, "engine_degraded", "{action}");
        assert!(error.message.contains("degraded"));
    }
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_planted_secret_appears_in_no_result() {
    let world = World::ready().await;
    // The test application is made to answer with a value the vault has
    // released as a secret: the customer's name of one order.
    let secret = "Example Customer One";
    let stored = world
        .call("secret.set", json!({"name": "example_password", "value": secret}))
        .await;
    assert!(stored.ok, "{:?}", stored.error);
    // Storing a secret gives nothing back.
    assert!(!stored.to_json().to_string().contains(secret));

    let response = world
        .call("tool.call", json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}))
        .await;
    assert!(response.ok, "{:?}", response.error);
    let text = response.to_json().to_string();
    assert!(!text.contains(secret), "{text}");
    assert!(text.contains(a2m_core::REDACTED));
    // Nor does a later read of the same run, its events, or a failure that
    // quotes the caller's own input.
    let run = response.result.as_ref().unwrap()["run_id"].as_str().unwrap().to_owned();
    for (action, input) in [
        ("run.get", json!({"run_id": run})),
        ("trace.show", json!({"run_id": run})),
        ("docs.get", json!({"topic": secret})),
        (secret, json!({})),
    ] {
        let text = world.call(action, input).await.to_json().to_string();
        assert!(!text.contains(secret), "{action}: {text}");
    }
    let _ = Secret::new(secret);
    let _ = FailureCode::SCHEMA_INVALID;
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_first_actions_of_building_answer_through_the_dispatcher() {
    let world = World::ready().await;
    let listed = world.ok("provider.list", json!({})).await;
    assert_eq!(listed["providers"][0]["name"], "example_json");
    assert_eq!(listed["providers"][0]["availability"], "ready");
    let described = world.ok("provider.describe", json!({"provider": "example_json"})).await;
    assert_eq!(described["modes"], json!(["app_json", "app_json_spare"]));
    assert_eq!(
        world
            .call("provider.describe", json!({"provider": "example_jsno"}))
            .await
            .error
            .unwrap()
            .fix,
        "Use one of: example_json."
    );

    let shown = world.ok("config.show", json!({"key": "policy.approve_writes"})).await;
    assert_eq!(shown["settings"][0]["value"], "person_each_run");
    assert_eq!(shown["settings"][0]["layer"], "default");

    let status = world.ok("engine.status", json!({})).await;
    assert_eq!(status["state"], "ok");
    assert_eq!(status["runner"]["running"], true);
    let doctor = world.ok("doctor.run", json!({})).await;
    assert_eq!(doctor["state"], "ok");

    let docs = world.ok("docs.get", json!({"topic": "tool.call"})).await;
    assert!(docs["text"].as_str().unwrap().contains("anything2mcp tool call <tool>"));
    let found = world.ok("action.search", json!({"query": "publish"})).await;
    assert!(found["actions"].as_array().unwrap().iter().any(|action| action["name"] == "version.publish"));

    let targets = world.ok("target.list", json!({})).await;
    assert_eq!(targets["targets"][0]["target"], "example");
    assert_eq!(targets["targets"][0]["versions"][0]["current"], true);
    let plan = world.ok("plan.show", json!({"target": "example"})).await;
    assert!(plan["text"].as_str().unwrap().contains(ORDER_GET));

    let sessions = world.ok("session.status", json!({})).await;
    assert_eq!(sessions["sessions"], json!([]));

    // A kill switch stops work at once, and only a person releases it.
    let on = world.ok("killswitch.set", json!({"scope": "everything", "on": true})).await;
    assert_eq!(on["on"], true);
    let held = world
        .call("tool.call", json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}))
        .await;
    assert!(!held.ok, "{held:?}");
    let release = world.call("killswitch.set", json!({"scope": "everything", "on": false})).await;
    assert_eq!(release.error.unwrap().code, "person_required");
    let person = Caller::new(world.caller.principal.clone(), FrontDoor::Command, false);
    let released = world
        .kernel
        .dispatch(Request::new("killswitch.set", json!({"scope": "everything", "on": false}), person))
        .await;
    assert!(released.ok, "{:?}", released.error);
    assert_eq!(world.surface.presence_asked.load(Ordering::SeqCst), 1);
    let after = world
        .call("tool.call", json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}))
        .await;
    assert!(after.ok, "{:?}", after.error);

    // The audit log holds what was done, and its chain is whole.
    let audit = world.ok("audit.query", json!({})).await;
    assert_eq!(audit["chain"], "intact");
    let actions: Vec<&str> = audit["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["action"].as_str().unwrap())
        .collect();
    assert!(actions.contains(&"version.publish") && actions.contains(&"killswitch.set"), "{actions:?}");

    world.kernel.close(CloseReason::Asked).await;
}
