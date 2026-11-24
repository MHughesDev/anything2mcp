//! The kernel through its dispatcher, against the built-in test
//! application: `Kernel::open` and its health states, a read end to end, a
//! write that rests until a person approves it at the desk, the decision
//! recorded for every dispatch, and the redactor on the way out.
//!
//! The test pack, its driver over the test application's JSON interface, and
//! the hand-built compiled target of the imaginary `example` target are the
//! engine's own test helpers, used here as they are. The tests are part of
//! the library's own test build, because they publish that compiled target
//! through `Kernel::publish_compiled_target`, which exists only for tests.

#[path = "../../../a2m-engine/tests/common/mod.rs"]
pub(super) mod engine;

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use a2m_actions::{
    ApprovalAsk, ApprovalDesk, BoxFuture, Caller, CallerCredential, CloseReason, ExitClass,
    FrontDoor, HealthState, Kernel, KernelParts, McpResultFace, Paths, PersonSurface, PresenceAsk,
    PresenceProof, Request, Response, SessionAsk, Settings, SignInAsk, StorePlace, SurfaceLink,
};
use a2m_auth::PersonAnswer;
use a2m_core::{Failure, FailureClass, Timestamp};
use a2m_policy::{Channel, PresenceCheck, PrincipalRef};
use a2m_provider::{RegistryBuilder, Roots, Supervisor, SupervisorLimits};
use a2m_telemetry::{Clock, ManualClock};
use a2m_vault::{BoxFuture as VaultFuture, KeyStore, MemoryKeyStore};
use engine::{AppPack, Counted, Folder, ITEM_UPSERT, ORDER_GET, START};
use example_app::harness::TestApp;
use serde_json::{Value as Json, json};

pub(super) const USER: &str = "pat";

/// The person surface of a test: it keeps the desk, notes what it was
/// asked, and answers the presence prompt as the test set it.
pub(super) struct Surface {
    desk: Mutex<Option<Arc<ApprovalDesk>>>,
    pub(super) approvals: Mutex<Vec<ApprovalAsk>>,
    pub(super) presence_asked: AtomicU32,
    pub(super) person_present: Mutex<bool>,
    /// When set, the proof handed back is one kept from this long ago, in
    /// milliseconds, as a surface that reuses an earlier prompt's would be.
    pub(super) proof_age_ms: Mutex<i64>,
    /// When set, the proof handed back is this person's and not the
    /// caller's.
    pub(super) proof_of: Mutex<Option<a2m_core::PrincipalId>>,
    /// Whether it has a page for a person's act an agent asked for, and the
    /// acts shown there.
    pub(super) pages: Mutex<bool>,
    pub(super) acts: Mutex<Vec<a2m_actions::PersonActAsk>>,
    clock: Arc<dyn Clock>,
}

impl Surface {
    pub(super) fn desk(&self) -> Arc<ApprovalDesk> {
        self.desk
            .lock()
            .unwrap()
            .clone()
            .expect("the kernel handed the desk over")
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
                let age = *self.proof_age_ms.lock().unwrap();
                let at = Timestamp::from_unix_millis(self.clock.now().as_unix_millis() - age);
                let of = self.proof_of.lock().unwrap().unwrap_or(ask.principal);
                Ok(PresenceProof::new(PresenceCheck::OsPrompt, of, at))
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

    fn shows_person_acts(&self) -> bool {
        *self.pages.lock().unwrap()
    }

    fn ask_person_act(
        &self,
        ask: a2m_actions::PersonActAsk,
    ) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            let link = format!("http://127.0.0.1/act/{}", ask.act);
            self.acts.lock().unwrap().push(ask);
            Ok(SurfaceLink {
                url: Some(link),
                text: "Open the page to carry it out.".to_owned(),
            })
        })
    }
}

/// A key store that does not answer, as a locked or missing one.
struct NoKeyStore;

impl KeyStore for NoKeyStore {
    fn describe(&self) -> a2m_vault::StoreDescription {
        MemoryKeyStore::new().describe()
    }

    fn wrap<'a>(
        &'a self,
        _key: &'a a2m_vault::KeyMaterial,
        _bound_to: &'a a2m_vault::Binding,
    ) -> VaultFuture<'a, Result<a2m_vault::Wrapped, Failure>> {
        Box::pin(async { Err(unreachable_store()) })
    }

    fn unwrap<'a>(
        &'a self,
        _wrapped: &'a a2m_vault::Wrapped,
        _bound_to: &'a a2m_vault::Binding,
    ) -> VaultFuture<'a, Result<a2m_vault::KeyMaterial, Failure>> {
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

pub(super) struct World {
    pub(super) app: TestApp,
    /// The clock the kernel was opened with, to move by hand.
    pub(super) clock: Arc<ManualClock>,
    pub(super) kernel: Kernel,
    pub(super) surface: Arc<Surface>,
    pub(super) caller: Caller,
    pub(super) folder: Folder,
    /// What the test pack's driver saw, and the faults it can be told to
    /// make.
    pub(super) hooks: Arc<engine::Hooks>,
    /// The test pack's discoverer, and what each discovery handed it.
    pub(super) discoverer: Arc<engine::WritingDiscoverer>,
    /// How many times the test provider `example_login` signed in, when
    /// the options registered it.
    pub(super) logins: Arc<AtomicU32>,
}

pub(super) struct Options {
    place: StorePlace,
    key_store: Arc<dyn KeyStore>,
    block_store: bool,
    /// The settings the kernel is opened with.
    pub(super) settings: Settings,
    /// What is done to the folders before the kernel opens.
    pub(super) before_open: Option<fn(&Paths)>,
    /// The test application's pack also registers its surface kind with
    /// the compiler, with a notice for `profile: open`.
    pub(super) notices: bool,
    /// The auth provider `example_login`, which the hand-built compiled
    /// target names, is registered: a provider that needs the person
    /// (`super::probe`).
    pub(super) login: bool,
    /// One more pack a test brings, registered beside the others.
    pub(super) extra_pack: Option<Arc<dyn a2m_provider::Pack>>,
}

impl Options {
    /// The defaults, with the test application's surface kind registered
    /// with the compiler and a notice for `profile: open`.
    pub(super) fn with_notices() -> Self {
        Self {
            notices: true,
            ..Self::default()
        }
    }

    /// The defaults, with `before` done to the folders before the kernel
    /// opens.
    pub(super) fn before_open(before: fn(&Paths)) -> Self {
        Self {
            before_open: Some(before),
            ..Self::default()
        }
    }

    /// The defaults, with a store on disk that cannot open: the kernel is
    /// stopped.
    pub(super) fn with_blocked_store() -> Self {
        Self {
            place: StorePlace::Disk,
            block_store: true,
            ..Self::default()
        }
    }
}

impl Default for Options {
    fn default() -> Self {
        Self {
            place: StorePlace::Memory,
            key_store: Arc::new(MemoryKeyStore::with_random(Counted::new())),
            block_store: false,
            settings: Settings::defaults(),
            before_open: None,
            notices: false,
            login: false,
            extra_pack: None,
        }
    }
}

impl Options {
    /// The defaults, with these settings.
    pub(super) fn with_settings(settings: Settings) -> Self {
        Self {
            settings,
            ..Self::default()
        }
    }

    /// The defaults, with a user's settings file holding `text`.
    pub(super) fn with_user_settings(text: &str) -> Self {
        Self {
            settings: Settings::load(&a2m_actions::Sources {
                user: Some(a2m_actions::settings::SourceFile::new(
                    "settings.toml",
                    text,
                )),
                ..a2m_actions::Sources::none()
            })
            .unwrap(),
            ..Self::default()
        }
    }
}

pub(super) async fn world(options: Options) -> World {
    let app = TestApp::start().await.unwrap();
    let folder = Folder::new("actions");
    let paths = Paths::under(folder.path());
    if options.block_store {
        // A file where the store's folder should be: the store cannot open.
        std::fs::create_dir_all(&paths.data).unwrap();
        std::fs::write(&paths.store, b"not a folder").unwrap();
    }
    if let Some(before) = options.before_open {
        before(&paths);
    }
    let manual = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(START)));
    let clock: Arc<dyn Clock> = Arc::clone(&manual) as Arc<dyn Clock>;
    let prepared = Kernel::prepare(KernelParts {
        settings: options.settings,
        paths,
        place: options.place,
        key_store: options.key_store,
        clock: Arc::clone(&clock),
        random: Counted::new(),
        os_user: USER.to_owned(),
    })
    .await;
    let host = prepared.host_services(Supervisor::new(SupervisorLimits::default()), Roots::none());
    let mut builder = RegistryBuilder::new(prepared.settings().provider_settings(), host);
    let hooks = Arc::new(engine::Hooks::default());
    let discoverer = Arc::new(engine::WritingDiscoverer::default());
    let app_pack = AppPack {
        hooks: Arc::clone(&hooks),
        discoverer: Arc::clone(&discoverer),
    };
    if options.notices {
        builder.add_pack(&NoticedAppPack(app_pack));
    } else {
        builder.add_pack(&app_pack);
    }
    builder.add_pack(&ProjectPack);
    let logins = Arc::new(AtomicU32::new(0));
    let login_pack = super::probe::LoginPack(Arc::clone(&logins));
    if options.login {
        builder.add_pack(&login_pack);
    }
    if let Some(extra) = &options.extra_pack {
        builder.add_pack(extra.as_ref());
    }
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    let surface = Arc::new(Surface {
        desk: Mutex::new(None),
        approvals: Mutex::new(Vec::new()),
        presence_asked: AtomicU32::new(0),
        person_present: Mutex::new(true),
        proof_age_ms: Mutex::new(0),
        proof_of: Mutex::new(None),
        pages: Mutex::new(false),
        acts: Mutex::new(Vec::new()),
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
        clock: manual,
        kernel,
        surface,
        caller,
        folder,
        hooks,
        discoverer,
        logins,
    }
}

impl World {
    /// A healthy kernel with the runner loop started and the `example`
    /// target published.
    pub(super) async fn ready() -> Self {
        let world = world(Options::default()).await;
        assert_eq!(
            world.kernel.health().state,
            HealthState::Ok,
            "{:?}",
            world.kernel.health()
        );
        world.kernel.start_runner_loop(4).await.unwrap();
        world
            .kernel
            .publish_compiled_target(&engine::compiled_target(&world.app.url("")))
            .await
            .unwrap();
        world
    }

    /// The time by the kernel's clock.
    pub(super) fn now(&self) -> Timestamp {
        Clock::now(self.clock.as_ref())
    }

    /// A folder under the test's own that is a project: it holds a
    /// project file.
    pub(super) fn project(&self, name: &str) -> std::path::PathBuf {
        let project = self.folder.path().join(name);
        std::fs::create_dir_all(project.join("sources")).unwrap();
        std::fs::write(project.join("anything2mcp.toml"), "").unwrap();
        project
    }

    /// A healthy kernel opened with `options`, with the runner loop
    /// started and the `example` target published.
    pub(super) async fn ready_with(options: Options) -> Self {
        let world = world(options).await;
        assert_eq!(
            world.kernel.health().state,
            HealthState::Ok,
            "{:?}",
            world.kernel.health()
        );
        world.kernel.start_runner_loop(4).await.unwrap();
        world
            .kernel
            .publish_compiled_target(&engine::compiled_target(&world.app.url("")))
            .await
            .unwrap();
        world
    }

    pub(super) async fn call(&self, action: &str, input: Json) -> Response {
        self.kernel
            .dispatch(Request::new(action, input, self.caller.clone()))
            .await
    }

    pub(super) async fn ok(&self, action: &str, input: Json) -> Json {
        let response = self.call(action, input).await;
        assert!(response.ok, "{action}: {:?}", response.error);
        response.result.unwrap()
    }

    /// How many decisions the audit of policy holds, through the one
    /// action that reads them back.
    pub(super) async fn explained(&self, response: &Response) -> Json {
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
        .call(
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
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
    let asked = world
        .surface
        .approvals
        .lock()
        .unwrap()
        .last()
        .cloned()
        .unwrap();
    assert_eq!(asked.run, run_id);
    let person = world.caller.principal.principal().clone();
    let proof = PresenceProof::new(
        PresenceCheck::OsPrompt,
        person.id,
        Timestamp::from_unix_millis(START),
    );
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
    let stored = world
        .app
        .stored_item("A-102", "L-300")
        .await
        .unwrap()
        .unwrap();
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
    let at = |kind: &str| {
        kinds
            .iter()
            .position(|found| *found == kind)
            .unwrap_or_else(|| panic!("no {kind} in {kinds:?}"))
    };
    assert!(at("approval_bound") < at("intent"));
    assert!(at("intent") < at("acted"));
    // The read-back is compared by the ledger and recorded as what verified
    // the record.
    assert!(at("acted") < at("verified"));
    assert_eq!(kinds.last(), Some(&"state_changed"));

    // An approval is used once: resuming again writes nothing more.
    let again = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(again.result.as_ref().unwrap()["status"], "completed");
    assert_eq!(world.app.write_count("A-102", "L-300").await.unwrap(), 1);

    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_with_no_environment_runs_in_the_targets_default_and_the_help_says_so() {
    let world = World::ready().await;
    let described = world
        .ok("action.describe", json!({"action": "tool.call"}))
        .await;
    let help = described["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "environment")
        .unwrap()["help"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        help.contains("no environment runs in the target's default environment"),
        "{help}"
    );
    // The same key with the default named is the same call: a call with no
    // environment ran in the example target's default, `test`.
    let input = json!({"order_id": "A-100"});
    let first = world
        .ok(
            "tool.call",
            json!({"tool": ORDER_GET, "input": input, "idempotency_key": "default-env-1"}),
        )
        .await;
    let again = world
        .ok(
            "tool.call",
            json!({"tool": ORDER_GET, "input": input, "environment": "test", "idempotency_key": "default-env-1"}),
        )
        .await;
    assert_eq!(first["run_id"], again["run_id"]);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_calls_at_once_are_two_runs_and_both_read() {
    let world = World::ready().await;
    let (first, second) = tokio::join!(
        world.call(
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}})
        ),
        world.call(
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-101"}})
        ),
    );
    for response in [&first, &second] {
        assert!(response.ok, "{:?}", response.error);
    }
    let run = |response: &Response| {
        response.result.as_ref().unwrap()["run_id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_ne!(run(&first), run(&second));
    // The same call again, while nothing changed, is a new run of its own:
    // the first has ended.
    let third = world
        .call(
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
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
    assert_eq!(
        (error.class, error.code.as_str()),
        (FailureClass::Input, "schema_invalid")
    );
    assert!(error.fix.contains("tool.call"), "{}", error.fix);
    assert!(unknown.decision.is_none());

    // Input that breaks the schema: an unknown field, a missing one, a
    // wrong type. Each names the field; none reaches policy.
    for (input, says) in [
        (json!({"tool": ORDER_GET, "inptu": {}}), "input"),
        (json!({}), "tool"),
        (json!({"tool": 7}), "tool"),
        (
            json!({"tool": ORDER_GET, "wait_seconds": 100_000}),
            "wait_seconds",
        ),
    ] {
        let response = world.call("tool.call", input).await;
        assert_eq!(response.exit, ExitClass::Malformed);
        let error = response.error.unwrap();
        assert_eq!(error.code, "schema_invalid");
        assert!(
            error.message.contains(says) || error.fix.contains(says),
            "{error:?}"
        );
        assert_eq!(response.next.unwrap().action, "action.describe");
    }

    // A valid input that names nothing is a different failure with a
    // different next step.
    let missing = world
        .call("tool.call", json!({"tool": "example_nothing_here"}))
        .await;
    assert_eq!(missing.exit, ExitClass::Failed);
    let error = missing.error.clone().unwrap();
    assert_eq!(
        (error.class, error.code.as_str()),
        (FailureClass::Input, "not_found")
    );
    assert!(error.fix.contains("tool.search"));
    assert!(error.retry_safe);
    let decided = world.explained(&missing).await;
    assert_eq!(decided["answer"], "allow");

    // An action in the catalog that is not built answers with one named
    // failure, after its decision was recorded.
    let not_built = world
        .call("capture.start", json!({"target": "example"}))
        .await;
    let error = not_built.error.clone().unwrap();
    assert_eq!(
        (error.class, error.code.as_str()),
        (FailureClass::Input, "action_not_built")
    );
    assert!(not_built.decision.is_some());

    // An action the client end carries out says so.
    let client = world.call("engine.stop", json!({})).await;
    assert_eq!(client.error.unwrap().code, "client_action");

    // A person's act asked for by an agent is refused before the handler,
    // and the surface is never asked.
    let publish = world
        .kernel
        .dispatch(
            Request::new(
                "version.publish",
                json!({"target": "example"}),
                world.caller.clone(),
            )
            .in_project(repository()),
        )
        .await;
    assert_eq!(publish.exit, ExitClass::PersonNeeded);
    assert_eq!(publish.error.unwrap().code, "person_required");
    assert_eq!(world.surface.presence_asked.load(Ordering::SeqCst), 0);

    // `action.run` reaches any action, and the action's own rules apply.
    let through = world
        .ok(
            "action.run",
            json!({"action": "action.describe", "input": {"action": "run.get"}}),
        )
        .await;
    assert_eq!(through["tool"], "a2m_run_get");
    let refused = world
        .call(
            "action.run",
            json!({"action": "run.get", "input": {"nope": 1}}),
        )
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
    // A guide is served from the files in `guides/`, with what the
    // registry says of each action it names.
    let guide = world.ok("docs.get", json!({"topic": "approvals"})).await;
    let text = guide["text"].as_str().unwrap();
    assert!(text.starts_with("# Approvals"), "{text}");
    assert!(text.contains("- `run.resume`: built."), "{text}");

    // Every action that needs the store refuses, with the reason.
    for (action, input) in [
        ("tool.call", json!({"tool": ORDER_GET})),
        ("tool.search", json!({})),
        ("run.get", json!({"run_id": "r_0000000000000000000000000"})),
        (
            "secret.set",
            json!({"name": "example_password", "value": "x"}),
        ),
        ("target.list", json!({})),
    ] {
        let response = world.call(action, input).await;
        let error = response.error.unwrap_or_else(|| panic!("{action} ran"));
        assert_eq!(
            (error.class, error.code.as_str()),
            (FailureClass::Busy, "engine_degraded"),
            "{action}"
        );
        assert!(
            error.message.contains("stopped"),
            "{action}: {}",
            error.message
        );
        assert_eq!(response.next.unwrap().action, "doctor.run");
    }
    assert!(world.kernel.start_runner_loop(1).await.is_err());
    world.kernel.close(CloseReason::Asked).await;
}

/// A write the person has not approved, as an agent would send it.
fn an_upsert() -> Json {
    json!({"tool": ITEM_UPSERT, "input": {
        "order_id": "A-102",
        "item_code": "L-300",
        "description": "New rail",
        "quantity": 3,
        "length_in": "150.5",
    }})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_store_that_stops_committing_while_the_engine_runs_stops_the_kernel_and_sends_nothing() {
    let world = World::ready_with(Options {
        place: StorePlace::Disk,
        ..Options::default()
    })
    .await;
    assert_eq!(world.kernel.health().state, HealthState::Ok);

    // Another program holds the database's write lock, as a failing disk or
    // a stuck process would leave it: the store's own commits fail.
    let file = Paths::under(world.folder.path())
        .store
        .join(a2m_ledger::DATABASE_FILE_NAME);
    let other = rusqlite::Connection::open(&file).unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    let health = world.kernel.recheck().await;
    assert_eq!(health.state, HealthState::Stopped, "{health:?}");
    assert!(health.cause().contains("store"), "{}", health.cause());
    assert_eq!(world.kernel.health().state, HealthState::Stopped);

    // A write answers `busy`, `engine_degraded`, naming the store and the
    // stop, and the test application is asked what it stored: nothing.
    let refused = world.call("tool.call", an_upsert()).await;
    let error = refused.error.unwrap_or_else(|| panic!("the write ran"));
    assert_eq!(
        (error.class, error.code.as_str()),
        (FailureClass::Busy, "engine_degraded")
    );
    assert!(error.message.contains("stopped"), "{}", error.message);
    assert!(error.message.contains("store"), "{}", error.message);
    assert_eq!(world.app.write_count("A-102", "L-300").await.unwrap(), 0);
    assert!(world.app.writes().await.unwrap().is_empty());

    // The status says why.
    let status = world.ok("engine.status", json!({})).await;
    assert_eq!(status["state"], "stopped");
    assert_eq!(status["reasons"][0]["part"], "store");
    assert_eq!(status["reasons"][0]["code"], "store_unavailable");
    assert!(world.app.writes().await.unwrap().is_empty());
    drop(other);
    world.kernel.close(CloseReason::Asked).await;
}

/// A file that is a database whose pages after the first are overwritten,
/// as a failing disk would leave it.
fn a_damaged_database(paths: &Paths) {
    std::fs::create_dir_all(&paths.store).unwrap();
    let file = paths.store.join(a2m_ledger::DATABASE_FILE_NAME);
    let raw = rusqlite::Connection::open(&file).unwrap();
    raw.query_row("PRAGMA journal_mode = DELETE", [], |_| Ok(()))
        .unwrap();
    raw.execute_batch("CREATE TABLE filler (x TEXT)").unwrap();
    for _ in 0..40 {
        raw.execute("INSERT INTO filler VALUES (?1)", ["x".repeat(900)])
            .unwrap();
    }
    drop(raw);
    let mut bytes = std::fs::read(&file).unwrap();
    let page = usize::from(u16::from_be_bytes([bytes[16], bytes[17]]));
    assert!(bytes.len() > 3 * page, "{} bytes", bytes.len());
    for byte in &mut bytes[page..] {
        *byte = 0x5a;
    }
    std::fs::write(&file, &bytes).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_database_file_damaged_before_the_start_leaves_the_kernel_stopped_and_sends_nothing() {
    let world = world(Options {
        place: StorePlace::Disk,
        before_open: Some(a_damaged_database),
        ..Options::default()
    })
    .await;
    let health = world.kernel.health();
    assert_eq!(health.state, HealthState::Stopped, "{health:?}");
    assert!(health.cause().contains("store"), "{}", health.cause());

    // Status names the cause by the store's own code.
    let status = world.ok("engine.status", json!({})).await;
    assert_eq!(status["state"], "stopped");
    assert_eq!(status["reasons"][0]["part"], "store");
    assert_eq!(status["reasons"][0]["code"], "store_corrupt");

    // A write is refused, and the test application stored nothing.
    let refused = world.call("tool.call", an_upsert()).await;
    let error = refused.error.unwrap_or_else(|| panic!("the write ran"));
    assert_eq!(
        (error.class, error.code.as_str()),
        (FailureClass::Busy, "engine_degraded")
    );
    assert!(error.message.contains("stopped"), "{}", error.message);
    assert!(world.app.writes().await.unwrap().is_empty());

    // The damaged file was kept as it was left, for its restore.
    let file = Paths::under(world.folder.path())
        .store
        .join(a2m_ledger::DATABASE_FILE_NAME);
    let bytes = std::fs::read(&file).unwrap();
    assert!(bytes.ends_with(&[0x5a; 64]));
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
        (
            "secret.set",
            json!({"name": "example_password", "value": "x"}),
        ),
        (
            "run.cancel",
            json!({"run_id": "r_0000000000000000000000000"}),
        ),
        ("killswitch.set", json!({"scope": "everything", "on": true})),
    ] {
        let response = world.call(action, input).await;
        let error = response.error.unwrap_or_else(|| panic!("{action} ran"));
        assert_eq!(error.code, "engine_degraded", "{action}");
        assert!(error.message.contains("degraded"));
    }
    world.kernel.close(CloseReason::Asked).await;
}

/// A key store that answers until it is switched off, as one that is
/// locked or stopped after the engine started would.
struct Switchable {
    inner: MemoryKeyStore,
    down: std::sync::atomic::AtomicBool,
}

impl Switchable {
    fn set_down(&self, down: bool) {
        self.down.store(down, Ordering::SeqCst);
    }

    fn is_down(&self) -> bool {
        self.down.load(Ordering::SeqCst)
    }
}

impl KeyStore for Switchable {
    fn describe(&self) -> a2m_vault::StoreDescription {
        self.inner.describe()
    }

    fn wrap<'a>(
        &'a self,
        key: &'a a2m_vault::KeyMaterial,
        bound_to: &'a a2m_vault::Binding,
    ) -> VaultFuture<'a, Result<a2m_vault::Wrapped, Failure>> {
        Box::pin(async move {
            if self.is_down() {
                Err(unreachable_store())
            } else {
                self.inner.wrap(key, bound_to).await
            }
        })
    }

    fn unwrap<'a>(
        &'a self,
        wrapped: &'a a2m_vault::Wrapped,
        bound_to: &'a a2m_vault::Binding,
    ) -> VaultFuture<'a, Result<a2m_vault::KeyMaterial, Failure>> {
        Box::pin(async move {
            if self.is_down() {
                Err(unreachable_store())
            } else {
                self.inner.unwrap(wrapped, bound_to).await
            }
        })
    }

    fn health(&self) -> VaultFuture<'_, a2m_vault::StoreHealth> {
        Box::pin(async move {
            if self.is_down() {
                a2m_vault::StoreHealth::Unreachable
            } else {
                self.inner.health().await
            }
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_key_store_that_stops_after_the_start_degrades_the_kernel_until_it_answers_again() {
    let store = Arc::new(Switchable {
        inner: MemoryKeyStore::with_random(Counted::new()),
        down: std::sync::atomic::AtomicBool::new(false),
    });
    let world = World::ready_with(Options {
        key_store: Arc::clone(&store) as Arc<dyn KeyStore>,
        ..Options::default()
    })
    .await;
    // One read while the key store answers: the workspace's key that seals
    // a run's trace is opened and held in memory, as it is in an engine
    // that has run anything since its start.
    let warm = world
        .call(
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
        .await;
    assert!(warm.ok, "{:?}", warm.error);

    // The key store stops answering after the start.
    store.set_down(true);
    let health = world.kernel.recheck().await;
    assert_eq!(health.state, HealthState::Degraded, "{health:?}");
    assert!(health.cause().contains("vault"), "{}", health.cause());
    let status = world.ok("engine.status", json!({})).await;
    assert_eq!(status["state"], "degraded");
    assert_eq!(status["reasons"][0]["part"], "vault");
    assert_eq!(status["reasons"][0]["code"], "key_store_unreachable");

    // A write is refused before anything runs, and the test application
    // is asked what it stored: nothing.
    let refused = world.call("tool.call", an_upsert()).await;
    let error = refused.error.unwrap_or_else(|| panic!("the write ran"));
    assert_eq!(
        (error.class, error.code.as_str()),
        (FailureClass::Busy, "engine_degraded")
    );
    assert!(error.message.contains("degraded"), "{}", error.message);
    assert!(world.app.writes().await.unwrap().is_empty());

    // A read that needs no secret runs.
    let read = world
        .call(
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
        .await;
    assert!(read.ok, "{:?}", read.error);
    assert_eq!(read.result.unwrap()["result"]["order_id"], "A-100");

    // Switched on again, the next look finds the kernel healthy, with no
    // restart.
    store.set_down(false);
    let health = world.kernel.recheck().await;
    assert_eq!(health.state, HealthState::Ok, "{health:?}");
    assert_eq!(world.ok("engine.status", json!({})).await["state"], "ok");
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "a read's trace is sealed under the workspace's key, and a kernel that has opened no key since its start needs the key store to seal it: the first read after the key store stops is refused `key_store_unreachable`, where section 224 says a degraded kernel takes reads (package 9.3h)"]
async fn a_key_store_that_stops_before_any_read_still_lets_a_read_run() {
    let store = Arc::new(Switchable {
        inner: MemoryKeyStore::with_random(Counted::new()),
        down: std::sync::atomic::AtomicBool::new(false),
    });
    let world = World::ready_with(Options {
        key_store: Arc::clone(&store) as Arc<dyn KeyStore>,
        ..Options::default()
    })
    .await;
    store.set_down(true);
    assert_eq!(world.kernel.recheck().await.state, HealthState::Degraded);
    let read = world
        .call(
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
        .await;
    assert!(read.ok, "{:?}", read.error);
    world.kernel.close(CloseReason::Asked).await;
}

/// The room a test says the evidence store's disk has.
struct Room(std::sync::atomic::AtomicU64);

impl a2m_actions::FreeSpace for Room {
    fn free_bytes(&self, _folder: &Path) -> Option<u64> {
        Some(self.0.load(Ordering::SeqCst))
    }
}

/// The captures a run kept: its events that tie a capture to it.
async fn kept_captures(world: &World, response: &Response) -> Vec<Json> {
    let run: a2m_core::RunId = response.result.as_ref().unwrap()["run_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    world
        .kernel
        .store()
        .unwrap()
        .read(move |tx| tx.ledger().events(run, 0, u32::MAX))
        .await
        .unwrap()
        .iter()
        .filter(|event| event.kind == a2m_ledger::EventKind::Need)
        .map(|event| serde_json::from_str::<Json>(&event.payload).unwrap_or(Json::Null))
        .filter(|payload| payload.get("capture").is_some())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_evidence_store_with_no_room_degrades_the_kernel_to_minimal_reads_until_there_is_room() {
    // Reads keep every exchange, and a read of an order captures the
    // application's answer.
    let world = World::ready_with(Options::with_user_settings(
        "[evidence]\ncapture = \"full\"\n",
    ))
    .await;
    world.hooks.tells_acting.store(true, Ordering::SeqCst);
    let room = Arc::new(Room(std::sync::atomic::AtomicU64::new(u64::MAX)));
    world
        .kernel
        .measure_free_space_with(Arc::clone(&room) as Arc<dyn a2m_actions::FreeSpace>);
    let read = || {
        world.call(
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
    };
    assert_eq!(world.kernel.recheck().await.state, HealthState::Ok);
    let roomy = read().await;
    assert!(roomy.ok, "{:?}", roomy.error);
    assert!(
        kept_captures(&world, &roomy)
            .await
            .iter()
            .any(|capture| capture["capture"] == "response"),
        "with room, the read keeps its capture"
    );

    // Less free than the floor, the default of one gibibyte.
    room.0.store(1_000, Ordering::SeqCst);
    let health = world.kernel.recheck().await;
    assert_eq!(health.state, HealthState::Degraded, "{health:?}");
    let status = world.ok("engine.status", json!({})).await;
    assert_eq!(status["state"], "degraded");
    assert_eq!(status["reasons"][0]["part"], "evidence_space");
    assert_eq!(status["reasons"][0]["code"], "storage_full");

    // A read runs, and keeps nothing above the lowest capture level.
    let short = read().await;
    assert!(short.ok, "{:?}", short.error);
    assert_eq!(
        short.result.as_ref().unwrap()["result"]["order_id"],
        "A-100"
    );
    let kept = kept_captures(&world, &short).await;
    assert!(kept.is_empty(), "{kept:?}");

    // A write is refused before a run is made, and the test application
    // is asked what it stored: nothing.
    let refused = world.call("tool.call", an_upsert()).await;
    let error = refused.error.unwrap_or_else(|| panic!("the write ran"));
    assert_eq!(
        (error.class, error.code.as_str()),
        (FailureClass::Busy, "engine_degraded")
    );
    assert!(error.message.contains("degraded"), "{}", error.message);
    assert!(refused.result.is_none());
    assert!(world.app.writes().await.unwrap().is_empty());

    // With room again, the next look finds the kernel healthy.
    room.0.store(u64::MAX, Ordering::SeqCst);
    assert_eq!(world.kernel.recheck().await.state, HealthState::Ok);
    assert_eq!(world.ok("engine.status", json!({})).await["state"], "ok");
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_planted_secret_appears_in_no_result() {
    let world = World::ready().await;
    // The test application is made to answer with a value the vault has
    // released as a secret: the customer's name of one order.
    let secret = "Example Customer One";
    let stored = world
        .call(
            "secret.set",
            json!({"name": "example_password", "value": secret}),
        )
        .await;
    assert!(stored.ok, "{:?}", stored.error);
    // Storing a secret gives nothing back.
    assert!(!stored.to_json().to_string().contains(secret));

    let response = world
        .call(
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
        .await;
    assert!(response.ok, "{:?}", response.error);
    let text = response.to_json().to_string();
    assert!(!text.contains(secret), "{text}");
    assert!(text.contains(a2m_core::REDACTED));
    // Nor does a later read of the same run, its events, or a failure that
    // quotes the caller's own input.
    let run = response.result.as_ref().unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    for (action, input) in [
        ("run.get", json!({"run_id": run})),
        ("trace.show", json!({"run_id": run})),
        ("docs.get", json!({"topic": secret})),
        (secret, json!({})),
    ] {
        let text = world.call(action, input).await.to_json().to_string();
        assert!(!text.contains(secret), "{action}: {text}");
    }
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_first_actions_of_building_answer_through_the_dispatcher() {
    let world = World::ready().await;
    let listed = world.ok("provider.list", json!({})).await;
    // Listed in order of name: the pack of a provider action first.
    assert_eq!(listed["providers"][0]["name"], "example_files");
    assert_eq!(listed["providers"][1]["name"], "example_json");
    assert_eq!(listed["providers"][1]["availability"], "ready");
    let described = world
        .ok("provider.describe", json!({"provider": "example_json"}))
        .await;
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

    let shown = world
        .ok("config.show", json!({"key": "policy.approve_writes"}))
        .await;
    assert_eq!(shown["settings"][0]["value"], "person_each_run");
    assert_eq!(shown["settings"][0]["layer"], "default");

    let status = world.ok("engine.status", json!({})).await;
    assert_eq!(status["state"], "ok");
    assert_eq!(status["runner"]["running"], true);
    let doctor = world.ok("doctor.run", json!({})).await;
    assert_eq!(doctor["state"], "ok");
    // The doctor says whether a person can be proved present here: this
    // test's surface does not say, which is reported as unknown and never
    // as a prompt that is there.
    let presence = doctor["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["check"] == "presence")
        .expect("the doctor reports on the presence prompt");
    assert_eq!(presence["state"], "unknown", "{presence}");
    assert!(
        presence["detail"]
            .as_str()
            .unwrap()
            .contains("not_available")
    );

    let docs = world.ok("docs.get", json!({"topic": "tool.call"})).await;
    assert!(
        docs["text"]
            .as_str()
            .unwrap()
            .contains("anything2mcp tool call <tool>")
    );
    let found = world.ok("action.search", json!({"query": "publish"})).await;
    assert!(
        found["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["name"] == "version.publish")
    );

    let targets = world.ok("target.list", json!({})).await;
    assert_eq!(targets["targets"][0]["target"], "example");
    assert_eq!(targets["targets"][0]["versions"][0]["current"], true);
    let compiled_target = world.ok("target.show", json!({"target": "example"})).await;
    assert!(
        compiled_target["text"]
            .as_str()
            .unwrap()
            .contains(ORDER_GET)
    );

    let sessions = world.ok("session.status", json!({})).await;
    assert_eq!(sessions["sessions"], json!([]));

    // A kill switch stops work at once, and only a person releases it.
    let on = world
        .ok("killswitch.set", json!({"scope": "everything", "on": true}))
        .await;
    assert_eq!(on["on"], true);
    let held = world
        .call(
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
        .await;
    assert!(!held.ok, "{held:?}");
    let release = world
        .call(
            "killswitch.set",
            json!({"scope": "everything", "on": false}),
        )
        .await;
    assert_eq!(release.error.unwrap().code, "person_required");
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
    assert_eq!(world.surface.presence_asked.load(Ordering::SeqCst), 1);
    let after = world
        .call(
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
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
    assert!(
        actions.contains(&"version.publish") && actions.contains(&"killswitch.set"),
        "{actions:?}"
    );

    world.kernel.close(CloseReason::Asked).await;
}

// ---- a surface kind that carries a notice ----

/// The test application's pack, which also registers its surface kind with
/// the compiler, as a pack does whose surfaces a person must be told of:
/// `profile: open` carries the notice of credentials over an unencrypted
/// link (decision 0193).
struct NoticedAppPack(AppPack);

impl a2m_provider::Pack for NoticedAppPack {
    fn manifest(&self) -> &'static str {
        self.0.manifest()
    }

    fn register(
        &self,
        cx: &a2m_provider::PackContext,
        to: &mut a2m_provider::Registrar<'_>,
    ) -> Result<(), a2m_core::Failure> {
        self.0.register(cx, to)?;
        to.compiler(
            a2m_provider::CompilerRegistrations::new().surface_kind(
                a2m_files::SurfaceKindSpec::new(
                    a2m_core::SurfaceKind::new("json_app")?,
                    "address",
                    a2m_files::JsonSchema::object()
                        .required("address", a2m_files::JsonSchema::text())
                        .optional("profile", a2m_files::JsonSchema::name()),
                )
                .with_notice(
                    "profile",
                    "open",
                    a2m_core::SurfaceNotice::UnencryptedCredentials,
                ),
            ),
        )
    }
}

// ---- a provider action that reads its project root ----

const PROJECT_MANIFEST: &str = r#"manifest_format = 1
contract = 1

[provider]
name = "example_files"
version = "0.1.0"
publisher = "Example"
kind = "pack"

[[actions]]
name = "example_files.folder"
effect = "read"
"#;

/// A pack whose one action names the folder its project root resolves to
/// for the call (decision 0196): a root bound to no folder, resolved in the
/// call's own checked project folder.
struct ProjectPack;

impl a2m_provider::Pack for ProjectPack {
    fn manifest(&self) -> &'static str {
        PROJECT_MANIFEST
    }

    fn register(
        &self,
        _cx: &a2m_provider::PackContext,
        to: &mut a2m_provider::Registrar<'_>,
    ) -> Result<(), a2m_core::Failure> {
        to.action(Arc::new(FolderAction {
            schema: a2m_files::JsonSchema::any(),
        }))
    }
}

struct FolderAction {
    schema: a2m_files::JsonSchema,
}

impl a2m_provider::ProviderAction for FolderAction {
    fn name(&self) -> a2m_provider::ActionName {
        a2m_provider::ActionName::new("example_files.folder").unwrap()
    }

    fn input_schema(&self) -> &a2m_files::JsonSchema {
        &self.schema
    }

    fn output_schema(&self) -> &a2m_files::JsonSchema {
        &self.schema
    }

    fn handle<'a>(
        &'a self,
        _input: &'a a2m_core::Values,
        cx: &'a a2m_provider::ActionContext<'a>,
    ) -> a2m_provider::BoxFuture<'a, Result<a2m_core::Values, a2m_core::Failure>> {
        Box::pin(async move {
            let mut roots = Roots::none();
            let name = a2m_provider::RootName::new("project").unwrap();
            roots.add_project(name.clone(), a2m_provider::Access::Read)?;
            let folder = roots.folder_in(&name, a2m_provider::Access::Read, cx.paths)?;
            Ok(a2m_core::Values::new().with(
                "folder",
                folder.as_path().to_string_lossy().into_owned().as_str(),
            ))
        })
    }
}
