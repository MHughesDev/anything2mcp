//! Building tools through the dispatcher: a target's drafts are linted and
//! compiled with the compiler set the provider registry gathered, a person
//! publishes them, and the published tools are found by name.
//!
//! The pack is the compiler's own test pack, `stand`, registered whole
//! through the provider contract, over the `example` target of this
//! repository. Its driver is a stub, so nothing is called here; the calls
//! are in `tests/kernel.rs`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "this file holds only tests; the helpers the tests share do not carry the test attribute"
)]

#[path = "../../a2m-provider/tests/common/mod.rs"]
mod provider;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use a2m_actions::{
    ApprovalAsk, ApprovalDesk, BoxFuture, Caller, CallerCredential, CloseReason, ExitClass,
    FrontDoor, Kernel, KernelParts, Paths, PersonSurface, PresenceAsk, PresenceProof, Request,
    Response, SessionAsk, Settings, SignInAsk, StorePlace, SurfaceLink,
};
use a2m_auth::PersonAnswer;
use a2m_core::{Failure, Timestamp};
use a2m_files::testing;
use a2m_policy::PresenceCheck;
use a2m_provider::{
    CompilerRegistrations, Pack, PackContext, Registrar, RegistryBuilder, Roots, Supervisor,
    SupervisorLimits,
};
use a2m_telemetry::ManualClock;
use a2m_vault::MemoryKeyStore;
use provider::{Behaviour, Folder, StubDriver};
use serde_json::{Value as Json, json};

const MANIFEST: &str = r#"manifest_format = 1
contract = 1

[provider]
name = "stand"
version = "0.1.0"
publisher = "Example"
kind = "pack"

[[modes]]
name = "stand"
archetype = "request"
verbs = ["read", "upsert"]
steps = ["fetch", "send"]
rank = 40

[[surface_kinds]]
name = "stand"
archetype = "request"
session = true

[points]
driver = ["stand"]

[[file_kinds]]
name = "call"
directory = "calls/"
surface_kinds = ["stand"]

[[finding_kinds]]
name = "stand_call"
surface_kinds = ["stand"]

[[controls]]
name = "box"

[[lint_rules]]
code = "stand.path_not_rooted"
level = "error"
summary = "A call's path starts with a slash."

[[drafting_rules]]
finding_kind = "stand_call"
file_kind = "call"

[[settings]]
key = "page_budget"
type = "int"
default = 4
min = 1
max = 64
doc = "Open pages for all sessions together."

[effects]
network = "surfaces"
sessions = ["stand"]
"#;

struct StandPack;

impl Pack for StandPack {
    fn manifest(&self) -> &'static str {
        MANIFEST
    }

    fn register(&self, _cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        to.driver(
            testing::mode(),
            Arc::new(StubDriver {
                reached: Arc::new(Mutex::new(Vec::new())),
                behaviour: Arc::new(Mutex::new(Behaviour::Done)),
            }),
        )?;
        to.compiler(
            CompilerRegistrations::new()
                .surface_kind(testing::surface_kind_spec())
                .control(testing::control())
                .file_kind(testing::file_kind())
                .lowering(testing::lowering())
                .lint_rule(testing::lint_rule())
                .drafting_rule(testing::drafting_rule()),
        )
    }
}

/// A person surface whose person is always at the machine.
#[derive(Default)]
struct Present {
    desk: Mutex<Option<ApprovalDesk>>,
    asked: Mutex<Vec<String>>,
}

impl PersonSurface for Present {
    fn take_desk(&self, desk: ApprovalDesk) {
        *self.desk.lock().unwrap() = Some(desk);
    }

    fn ask_approval(&self, _ask: ApprovalAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async {
            Ok(SurfaceLink {
                url: None,
                text: String::new(),
            })
        })
    }

    fn ask_sign_in(&self, _ask: SignInAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async {
            Ok(SurfaceLink {
                url: None,
                text: String::new(),
            })
        })
    }

    fn confirm_presence(&self, ask: PresenceAsk) -> BoxFuture<'_, Result<PresenceProof, Failure>> {
        Box::pin(async move {
            self.asked.lock().unwrap().push(ask.action.to_string());
            Ok(PresenceProof::new(
                PresenceCheck::OsPrompt,
                ask.principal,
                Timestamp::from_unix_millis(1_800_000_000_000),
            ))
        })
    }

    fn ask_session(&self, _ask: SessionAsk) -> BoxFuture<'_, PersonAnswer> {
        Box::pin(async { PersonAnswer::NoAnswer })
    }
}

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

struct World {
    kernel: Kernel,
    surface: Arc<Present>,
    person: Caller,
    _folder: Folder,
}

impl World {
    async fn open() -> Self {
        let folder = Folder::new("actions-build");
        let settings = Settings::load(
            &a2m_actions::Sources::none()
                .with_pack_manifest(MANIFEST)
                .unwrap()
                .with_flag("packs.stand.page_budget", "8"),
        )
        .unwrap();
        let prepared = Kernel::prepare(KernelParts {
            settings,
            paths: Paths::under(&folder_path(&folder)),
            place: StorePlace::Memory,
            key_store: Arc::new(MemoryKeyStore::new()),
            clock: Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(
                1_800_000_000_000,
            ))),
            random: Arc::new(a2m_vault::OsRandom),
            os_user: "pat".to_owned(),
        })
        .await;
        let host =
            prepared.host_services(Supervisor::new(SupervisorLimits::default()), Roots::none());
        let mut builder = RegistryBuilder::new(prepared.settings().provider_settings(), host);
        builder.add_pack(&StandPack);
        let (registry, problems) = builder.build();
        assert!(problems.is_empty(), "{problems:?}");
        // The pack was handed its own group, as the loader read it.
        let group = registry
            .provider_settings(&a2m_provider::ProviderName::new("stand").unwrap())
            .unwrap();
        assert_eq!(group.get("page_budget"), Some(&a2m_core::Value::Int(8)));
        let surface = Arc::new(Present::default());
        let kernel = Kernel::open(
            prepared,
            registry,
            problems,
            Arc::clone(&surface) as Arc<dyn PersonSurface>,
        )
        .await;
        let principal = kernel
            .verify_caller(CallerCredential::LocalUser("pat".to_owned()))
            .await
            .unwrap();
        // Another user of the machine is not let in.
        assert!(
            kernel
                .verify_caller(CallerCredential::LocalUser("sam".to_owned()))
                .await
                .is_err()
        );
        Self {
            kernel,
            surface,
            person: Caller::new(principal, FrontDoor::Command, false),
            _folder: folder,
        }
    }

    async fn call(&self, caller: &Caller, action: &str, input: Json) -> Response {
        self.kernel
            .dispatch(Request::new(action, input, caller.clone()).in_project(repository()))
            .await
    }
}

fn folder_path(folder: &Folder) -> PathBuf {
    folder.path().to_path_buf()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn drafts_are_linted_a_person_publishes_them_and_the_tools_are_found() {
    let world = World::open().await;
    let agent = Caller::new(world.person.principal.clone(), FrontDoor::Mcp, true);

    // An agent lints the drafts: they compile, and nothing is stored.
    let linted = world
        .call(&agent, "draft.lint", json!({"target": "example"}))
        .await;
    assert!(linted.ok, "{:?}", linted.error);
    let result = linted.result.unwrap();
    assert_eq!(result["ok"], true);
    assert!(!result["tools"].as_array().unwrap().is_empty());
    let nothing = world.call(&agent, "tool.search", json!({})).await;
    assert_eq!(nothing.result.unwrap()["total"], 0);

    // A target that is not in the project, and a path that leaves it.
    let missing = world
        .call(&agent, "draft.lint", json!({"target": "nowhere"}))
        .await;
    assert_eq!(missing.result.unwrap()["ok"], false);
    let outside = world
        .kernel
        .dispatch(Request::new(
            "draft.lint",
            json!({"target": "example"}),
            agent.clone(),
        ))
        .await;
    assert_eq!(outside.error.unwrap().code, "path_not_allowed");

    // An agent cannot publish; the person can, after the presence prompt.
    let refused = world
        .call(&agent, "version.publish", json!({"target": "example"}))
        .await;
    assert_eq!(refused.exit, ExitClass::PersonNeeded);
    assert_eq!(refused.error.unwrap().code, "person_required");
    assert!(world.surface.asked.lock().unwrap().is_empty());

    let published = world
        .call(
            &world.person,
            "version.publish",
            json!({"target": "example"}),
        )
        .await;
    assert!(published.ok, "{:?}", published.error);
    let version = published.result.unwrap();
    assert_eq!(version["version"], "1.0.0");
    assert_eq!(version["current"], true);
    assert_eq!(
        world.surface.asked.lock().unwrap().as_slice(),
        ["version.publish"]
    );
    // The plan the store holds is the one the drafts compile to at that
    // version: the same hash the compiler gives on its own.
    let sources =
        a2m_files::load(&testing::example_root(), &testing::stand_set().unwrap()).unwrap();
    let alone = a2m_files::compile(
        &sources,
        &testing::stand_set().unwrap(),
        a2m_files::CompileMode::Publish,
        a2m_core::Version::new(1, 0, 0),
    )
    .unwrap();
    assert_eq!(version["plan_hash"], alone.encoded.hash.to_string());

    // A published version is never changed: the same label is refused.
    let again = world
        .call(
            &world.person,
            "version.publish",
            json!({"target": "example", "version": "1.0.0"}),
        )
        .await;
    assert_eq!(again.error.unwrap().code, "schema_invalid");

    // The tools are found, described, and shown, by anyone who may read.
    let found = world
        .call(&agent, "tool.search", json!({"target": "example"}))
        .await;
    let tools = found.result.unwrap();
    assert_eq!(tools["total"], version["tools"].as_array().unwrap().len());
    let name = tools["tools"][0]["name"].as_str().unwrap().to_owned();
    let described = world
        .call(&agent, "tool.describe", json!({"tool": name}))
        .await;
    let described = described.result.unwrap();
    assert_eq!(described["version"], "1.0.0");
    assert!(described["input_schema"].is_object());
    let shown = world
        .call(&agent, "plan.show", json!({"target": "example"}))
        .await;
    assert!(
        shown.result.unwrap()["text"]
            .as_str()
            .unwrap()
            .contains(&name)
    );

    // The pack's own group shows in `config.show`, with its layer.
    let config = world
        .call(
            &agent,
            "config.show",
            json!({"key": "packs.stand.page_budget"}),
        )
        .await;
    let row = &config.result.unwrap()["settings"][0];
    assert_eq!(
        (row["value"].as_i64(), row["layer"].as_str()),
        (Some(8), Some("flag"))
    );

    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_target_is_added_to_a_project_and_its_first_file_is_linted() {
    let world = World::open().await;
    let project = Folder::new("actions-project");
    let add = |name: &'static str| {
        world.kernel.dispatch(
            Request::new(
                "target.add",
                json!({"name": name, "address": "https://app.example.com"}),
                world.person.clone(),
            )
            .in_project(project.path()),
        )
    };
    let added = add("example").await;
    assert!(added.ok, "{:?}", added.error);
    let file = project
        .path()
        .join("targets")
        .join("example")
        .join("target.yaml");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("target: example"));
    assert_eq!(added.next.unwrap().action, "draft.lint");
    // The same name again is refused, and a name that would leave the
    // project is not a name.
    assert_eq!(add("example").await.error.unwrap().code, "schema_invalid");
    assert_eq!(add("../elsewhere").await.exit, ExitClass::Failed);
    // The first file is read by the compiler, which says what it lacks.
    let linted = world
        .kernel
        .dispatch(
            Request::new(
                "draft.lint",
                json!({"target": "example"}),
                world.person.clone(),
            )
            .in_project(project.path()),
        )
        .await;
    assert!(linted.ok, "{:?}", linted.error);
    world.kernel.close(CloseReason::Asked).await;
}
