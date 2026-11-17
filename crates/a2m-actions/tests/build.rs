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
    FrontDoor, Kernel, KernelParts, Paths, PersonActAsk, PersonActId, PersonSurface, PresenceAsk,
    PresenceProof, Request, Response, SessionAsk, Settings, SignInAsk, StorePlace, SurfaceLink,
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

struct StandPack {
    reached: Arc<Mutex<Vec<String>>>,
}

impl Pack for StandPack {
    fn manifest(&self) -> &'static str {
        MANIFEST
    }

    fn register(&self, _cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        to.driver(
            testing::mode(),
            Arc::new(StubDriver {
                reached: Arc::clone(&self.reached),
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

/// A person surface whose person is always at the machine. It shows a
/// person's act an agent asked for only when `pages` is set, as a surface
/// with a page for it does.
#[derive(Default)]
struct Present {
    desk: Mutex<Option<Arc<ApprovalDesk>>>,
    asked: Mutex<Vec<String>>,
    pages: Mutex<bool>,
    acts: Mutex<Vec<PersonActAsk>>,
}

impl Present {
    fn desk(&self) -> Arc<ApprovalDesk> {
        self.desk.lock().unwrap().clone().unwrap()
    }
}

impl PersonSurface for Present {
    fn take_desk(&self, desk: ApprovalDesk) {
        *self.desk.lock().unwrap() = Some(Arc::new(desk));
    }

    fn shows_person_acts(&self) -> bool {
        *self.pages.lock().unwrap()
    }

    fn ask_person_act(&self, ask: PersonActAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            if !*self.pages.lock().unwrap() {
                return Err(Failure::new(
                    a2m_core::FailureClass::Policy,
                    a2m_actions::codes::PERSON_REQUIRED,
                    "No page here.",
                    "Run it at a terminal.",
                ));
            }
            let link = format!("http://127.0.0.1/act/{}", ask.act);
            self.acts.lock().unwrap().push(ask);
            Ok(SurfaceLink {
                url: Some(link),
                text: "Open the page to carry it out.".to_owned(),
            })
        })
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
    /// Every step that reached the stand's driver.
    reached: Arc<Mutex<Vec<String>>>,
    clock: Arc<ManualClock>,
    kernel: Kernel,
    surface: Arc<Present>,
    person: Caller,
    /// A project folder: it holds a project file and the files of the
    /// imaginary `example` target, copied from the repository.
    project: Folder,
    _folder: Folder,
}

/// Copies a folder and everything in it.
fn copy_folder(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_folder(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A project folder with the `example` target's files in it.
fn example_project() -> Folder {
    let project = Folder::new("actions-example-project");
    std::fs::write(project.path().join("anything2mcp.toml"), "").unwrap();
    let target = Path::new("targets").join("example");
    copy_folder(&repository().join(&target), &project.path().join(&target));
    project
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
        let clock = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(
            1_800_000_000_000,
        )));
        let prepared = Kernel::prepare(KernelParts {
            settings,
            paths: Paths::under(&folder_path(&folder)),
            place: StorePlace::Memory,
            key_store: Arc::new(MemoryKeyStore::new()),
            clock: Arc::clone(&clock) as _,
            random: Arc::new(a2m_vault::OsRandom),
            os_user: "pat".to_owned(),
        })
        .await;
        let host =
            prepared.host_services(Supervisor::new(SupervisorLimits::default()), Roots::none());
        let mut builder = RegistryBuilder::new(prepared.settings().provider_settings(), host);
        let reached = Arc::new(Mutex::new(Vec::new()));
        builder.add_pack(&StandPack {
            reached: Arc::clone(&reached),
        });
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
            reached,
            clock,
            kernel,
            surface,
            person: Caller::new(principal, FrontDoor::Command, false),
            project: example_project(),
            _folder: folder,
        }
    }

    async fn call(&self, caller: &Caller, action: &str, input: Json) -> Response {
        self.kernel
            .dispatch(Request::new(action, input, caller.clone()).in_project(self.project.path()))
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
    // A folder is a project once a person has put a project file in it.
    let no_file = world
        .kernel
        .dispatch(
            Request::new(
                "target.add",
                json!({"name": "example"}),
                world.person.clone(),
            )
            .in_project(project.path()),
        )
        .await;
    assert_eq!(no_file.error.unwrap().code, "path_not_allowed");
    std::fs::write(project.path().join("anything2mcp.toml"), "").unwrap();
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

/// A proof of presence from the page of `act`, of now by the engine's clock.
fn proof_for(world: &World, act: PersonActId) -> PresenceProof {
    PresenceProof::new(
        PresenceCheck::OsPrompt,
        world.person.principal.principal().id,
        a2m_telemetry::Clock::now(world.clock.as_ref()),
    )
    .for_act(act)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_publish_an_agent_asks_for_is_carried_out_once_on_its_page_by_the_persons_proof() {
    let world = World::open().await;
    *world.surface.pages.lock().unwrap() = true;
    let agent = Caller::new(world.person.principal.clone(), FrontDoor::Mcp, true);

    // The agent asks; nothing is published, and the person is shown the
    // act on a page, with what it will store pinned.
    let asked = world
        .call(&agent, "version.publish", json!({"target": "example"}))
        .await;
    assert_eq!(asked.exit, ExitClass::PersonNeeded, "{:?}", asked.error);
    assert!(asked.error.is_none());
    let result = asked.result.clone().unwrap();
    assert_eq!(result["state"], "needs_person");
    assert!(result["link"].as_str().unwrap().contains("/act/"));
    let next = asked.next.clone().unwrap();
    assert_eq!(next.action, "version.publish");
    // The result fits the action's published output schema.
    let def = world.kernel.actions().find("version.publish").unwrap();
    let mut shown = result.clone();
    shown.as_object_mut().unwrap().insert("next".to_owned(), json!({"action": "version.publish", "tool": "a2m_version_publish", "args": {}, "when": "later"}));
    def.output.check_json(&shown).unwrap();
    assert!(
        world.surface.asked.lock().unwrap().is_empty(),
        "no prompt at the caller's end"
    );
    let ask = world.surface.acts.lock().unwrap()[0].clone();
    let fields: Vec<(&str, &str)> = ask
        .input
        .iter()
        .map(|field| (field.name.as_str(), field.value.as_str()))
        .collect();
    assert_eq!(fields[0], ("plan_hash", fields[0].1));
    assert!(
        fields.contains(&("target", "example")) && fields.contains(&("version", "1.0.0")),
        "{fields:?}"
    );
    assert!(!ask.details.is_empty(), "the review summary is shown");
    let nothing = world.call(&agent, "target.list", json!({})).await;
    assert_eq!(
        nothing.result.unwrap()["targets"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| !row["versions"].as_array().unwrap().is_empty())
            .count(),
        0
    );

    // The same call while it waits gives back the same page.
    let again = world
        .call(&agent, "version.publish", json!({"target": "example"}))
        .await;
    assert_eq!(again.result.unwrap()["act"], result["act"]);
    assert_eq!(world.surface.acts.lock().unwrap().len(), 1);

    let desk = world.surface.desk();
    let act = ask.act;
    // A proof for no act, for another, of another person, or old does
    // nothing, and the act goes on waiting.
    let bare = PresenceProof::new(
        PresenceCheck::OsPrompt,
        world.person.principal.principal().id,
        a2m_telemetry::Clock::now(world.clock.as_ref()),
    );
    let other = PersonActId::from_bytes([7; 16]);
    let someone = a2m_core::PrincipalId::from_bytes([9; 16]);
    let refusals = [
        desk.carry_out(act, bare).await,
        desk.carry_out(act, proof_for(&world, other)).await,
        desk.carry_out(
            act,
            PresenceProof::new(
                PresenceCheck::OsPrompt,
                someone,
                a2m_telemetry::Clock::now(world.clock.as_ref()),
            )
            .for_act(act),
        )
        .await,
        desk.carry_out(
            act,
            PresenceProof::new(
                PresenceCheck::OsPrompt,
                world.person.principal.principal().id,
                Timestamp::from_unix_millis(1_799_999_000_000),
            )
            .for_act(act),
        )
        .await,
    ];
    for refused in refusals {
        assert_eq!(refused.unwrap_err().code().as_str(), "presence_required");
    }
    assert!(desk.person_act(act).is_ok());

    // The person's proof from the act's page carries it out, as them.
    let done = desk.carry_out(act, proof_for(&world, act)).await.unwrap();
    assert!(done.ok, "{:?}", done.error);
    let version = done.result.unwrap();
    assert_eq!(version["version"], "1.0.0");
    assert_eq!(version["plan_hash"], fields[0].1);
    // Once: a second press does nothing.
    let second = desk.carry_out(act, proof_for(&world, act)).await;
    assert_eq!(second.unwrap_err().code().as_str(), "not_found");
    assert!(desk.person_act(act).is_err());

    // The store holds exactly one version, published by the person.
    let listed = world.call(&agent, "target.list", json!({})).await;
    let targets = listed.result.unwrap();
    let example: Vec<&Json> = targets["targets"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["target"] == "example")
        .collect();
    assert_eq!(
        example[0]["versions"].as_array().unwrap().len(),
        1,
        "{targets}"
    );
    let audit = world.call(&world.person, "audit.query", json!({})).await;
    let audit = audit.result.unwrap().to_string();
    assert!(audit.contains("version.publish"), "{audit}");

    // The agent's same call gives back what was done, and does nothing more.
    let repeated = world
        .call(&agent, "version.publish", json!({"target": "example"}))
        .await;
    assert!(repeated.ok, "{:?}", repeated.error);
    assert_eq!(repeated.result.unwrap()["version"], "1.0.0");
    assert!(!repeated.warnings.is_empty());
    let listed = world.call(&agent, "target.list", json!({})).await;
    assert!(
        listed
            .result
            .unwrap()
            .to_string()
            .matches("\"version\":\"")
            .count()
            == 1
    );

    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_requested_publish_rests_on_the_drafts_shown_and_ends_unanswered_with_its_window() {
    let world = World::open().await;
    *world.surface.pages.lock().unwrap() = true;
    let agent = Caller::new(world.person.principal.clone(), FrontDoor::Mcp, true);

    // `publish.request` asks; the person decides on the page.
    let asked = world
        .call(&agent, "publish.request", json!({"target": "example"}))
        .await;
    assert_eq!(asked.exit, ExitClass::PersonNeeded, "{:?}", asked.error);
    let ask = world.surface.acts.lock().unwrap()[0].clone();
    assert_eq!(ask.action.as_str(), "version.publish");

    // The drafts change after the person was shown them: the press
    // publishes nothing.
    let file = world
        .project
        .path()
        .join("targets")
        .join("example")
        .join("target.yaml");
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(
        &file,
        text.replacen(
            "title: Example order entry",
            "title: Example order entry, changed",
            1,
        ),
    )
    .unwrap();
    let changed = world
        .surface
        .desk()
        .carry_out(ask.act, proof_for(&world, ask.act))
        .await
        .unwrap();
    assert_eq!(changed.error.unwrap().code, "drafts_changed");
    let listed = world.call(&agent, "target.list", json!({})).await;
    assert!(!listed.result.unwrap().to_string().contains("plan_hash"));

    // A new ask, declined: nothing is published, and the same call is told.
    let asked = world
        .call(
            &agent,
            "publish.request",
            json!({"target": "example", "version": "2.0.0"}),
        )
        .await;
    assert_eq!(asked.exit, ExitClass::PersonNeeded);
    let ask = world.surface.acts.lock().unwrap().last().unwrap().clone();
    world.surface.desk().decline_act(ask.act, true);
    let told = world
        .call(
            &agent,
            "publish.request",
            json!({"target": "example", "version": "2.0.0"}),
        )
        .await;
    assert_eq!(told.error.unwrap().code, "approval_refused");

    // An ask nobody answers ends with the window: a press after it does
    // nothing.
    let asked = world
        .call(
            &agent,
            "publish.request",
            json!({"target": "example", "version": "3.0.0"}),
        )
        .await;
    assert_eq!(asked.exit, ExitClass::PersonNeeded);
    let ask = world.surface.acts.lock().unwrap().last().unwrap().clone();
    world.clock.advance(std::time::Duration::from_secs(16 * 60));
    let late = world
        .surface
        .desk()
        .carry_out(ask.act, proof_for(&world, ask.act))
        .await;
    assert_eq!(late.unwrap_err().code().as_str(), "not_found");
    let listed = world.call(&agent, "target.list", json!({})).await;
    assert!(!listed.result.unwrap().to_string().contains("plan_hash"));

    // A caller with no page to go to is refused by name, as before.
    *world.surface.pages.lock().unwrap() = false;
    let refused = world
        .call(
            &agent,
            "version.publish",
            json!({"target": "example", "version": "4.0.0"}),
        )
        .await;
    assert_eq!(refused.error.unwrap().code, "person_required");
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agent_writes_reads_tries_and_tests_drafts_inside_the_project_alone() {
    let world = World::open().await;
    world.kernel.start_runner_loop(2).await.unwrap();
    let agent = Caller::new(world.person.principal.clone(), FrontDoor::Mcp, true);
    let target_dir = world.project.path().join("targets").join("example");

    // The project's targets are listed, and the target's state.
    let listed = world.call(&agent, "target.list", json!({})).await;
    let listed = listed.result.unwrap();
    assert_eq!(listed["targets"][0]["target"], "example", "{listed}");
    assert_eq!(listed["targets"][0]["in_project"], true);
    let status = world
        .call(&agent, "target.status", json!({"target": "example"}))
        .await;
    assert!(status.ok, "{:?}", status.error);
    let status = status.result.unwrap();
    assert_eq!(
        (status["state"].as_str(), status["compiles"].as_bool()),
        (Some("drafted"), Some(true)),
        "{status}"
    );

    // A file is read as text, by its path in the target's folder.
    let read = world
        .call(
            &agent,
            "draft.read",
            json!({"target": "example", "file": "target.yaml"}),
        )
        .await;
    let read = read.result.unwrap();
    assert!(
        read["content"]
            .as_str()
            .unwrap()
            .contains("target: example")
    );
    // No path leaves the target's folder, and a hidden name is not read.
    for file in [
        "../../anything2mcp.toml",
        "/etc/hosts",
        ".hidden.yaml",
        "calls/../../x.yaml",
    ] {
        let refused = world
            .call(
                &agent,
                "draft.read",
                json!({"target": "example", "file": file}),
            )
            .await;
        assert_eq!(refused.error.unwrap().code, "path_not_allowed", "{file}");
    }
    let missing = world
        .call(
            &agent,
            "draft.read",
            json!({"target": "example", "file": "calls/none.yaml"}),
        )
        .await;
    assert_eq!(missing.error.unwrap().code, "not_found");

    // A file that does not read as its kind is not written: the folder
    // holds nothing new.
    let broken = world
        .call(
            &agent,
            "draft.write",
            json!({"target": "example", "file": "calls/broken.yaml", "content": "a: [\n"}),
        )
        .await;
    let broken = broken.result.unwrap();
    assert_eq!(broken["written"], false, "{broken}");
    assert!(!target_dir.join("calls").join("broken.yaml").exists());
    // One that reads is written, and the file system holds what was sent.
    let text = std::fs::read_to_string(target_dir.join("target.yaml")).unwrap();
    let changed = text.replacen("title: Example order entry", "title: Example orders", 1);
    let written = world
        .call(
            &agent,
            "draft.write",
            json!({"target": "example", "file": "target.yaml", "content": changed}),
        )
        .await;
    let written = written.result.unwrap();
    assert_eq!(
        (written["written"].as_bool(), written["ok"].as_bool()),
        (Some(true), Some(true)),
        "{written}"
    );
    assert_eq!(
        std::fs::read_to_string(target_dir.join("target.yaml")).unwrap(),
        changed
    );
    // A link in the target's folder that leads out of the project is never
    // written through.
    #[cfg(unix)]
    {
        let outside = Folder::new("actions-outside");
        let victim = outside.path().join("victim.yaml");
        std::fs::write(&victim, "kept\n").unwrap();
        std::os::unix::fs::symlink(&victim, target_dir.join("calls").join("linked.yaml")).unwrap();
        let through = world
            .call(
                &agent,
                "draft.write",
                json!({"target": "example", "file": "calls/linked.yaml", "content": "calls: []\n"}),
            )
            .await;
        assert!(
            !through.ok || through.result.as_ref().unwrap()["written"] == false,
            "{through:?}"
        );
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "kept\n");
        std::fs::remove_file(target_dir.join("calls").join("linked.yaml")).unwrap();
    }

    // The gates: the lint gate runs, the offline tests wait for a replay,
    // and the result is never a pass.
    let tested = world
        .call(&agent, "test.run", json!({"target": "example"}))
        .await;
    assert!(tested.ok, "{:?}", tested.error);
    let tested = tested.result.unwrap();
    assert_eq!(
        (tested["passed"].as_bool(), tested["complete"].as_bool()),
        (Some(false), Some(false))
    );
    assert_eq!(tested["gates"][0]["state"], "passed", "{tested}");
    let offline = tested["gates"][1]["tests"].as_array().unwrap();
    assert_eq!(offline.len(), 3, "{tested}");
    assert!(offline.iter().all(|test| test["state"] == "not_run"));

    // A drafted read is tried in the sandbox as a run of its own, and
    // nothing becomes a version.
    let tried = world
        .call(&agent, "draft.try", json!({"target": "example", "tool": "example_order_get", "input": {"order_id": "SO-1001"}}))
        .await;
    let result = tried.result.clone().unwrap_or_default();
    assert!(result["run_id"].is_string(), "{tried:?}");
    assert_eq!(result["tool"], "example_order_get");
    let got = world
        .call(&agent, "run.get", json!({"run_id": result["run_id"]}))
        .await;
    assert!(got.result.is_some(), "{got:?}");
    // The trial is a run the engine carried out: here it stops where the
    // stand's sign-in is not registered, as a call of a published tool
    // would, and nothing reached the stand's driver.
    assert_eq!(result["mode"], "stand", "{result}");
    assert_eq!(
        result["failure"]["code"], "auth_provider_unknown",
        "{result}"
    );
    assert!(world.reached.lock().unwrap().is_empty());
    let nothing = world.call(&agent, "tool.search", json!({})).await;
    assert_eq!(
        nothing.result.unwrap()["total"],
        0,
        "a trial publishes nothing"
    );
    // A drafted write is not tried.
    let write = world
        .call(
            &agent,
            "draft.try",
            json!({"target": "example", "tool": "example_line_item_upsert", "input": {}}),
        )
        .await;
    assert_eq!(write.error.unwrap().code, "action_not_built");
    world.kernel.close(CloseReason::Asked).await;
}
