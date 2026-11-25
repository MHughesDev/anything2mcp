//! The publish page as this crate's surface serves it, over the compiler's
//! test pack and the imaginary `example` target: it lists the version's
//! surfaces first, and marks there each address no person registered for
//! the target (spec section 502, rule 2; decision 0413).

#[path = "../../../a2m-provider/tests/common/mod.rs"]
mod provider;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_actions::{
    Caller, CallerCredential, CloseReason, ExitClass, FrontDoor, Kernel, KernelParts, NEW_PLACE,
    PersonSurface, Request, Response, Settings, StorePlace,
};
use a2m_core::{Failure, Timestamp};
use a2m_files::testing;
use a2m_provider::{
    CompilerRegistrations, Pack, PackContext, Registrar, RegistryBuilder, Roots, Supervisor,
    SupervisorLimits,
};
use a2m_telemetry::{Clock, ManualClock};
use a2m_vault::MemoryKeyStore;
use provider::{Behaviour, StubDriver};
use reqwest::StatusCode;
use reqwest::header::{CONTENT_TYPE, ORIGIN};
use serde_json::json;

use super::world::engine::{Folder, START};
use super::world::{PASSED, TestPrompt, home, within};
use crate::{LocalSurface, OsUser, SurfaceParts};

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

[[controls]]
name = "box"

[[lint_rules]]
code = "stand.path_not_rooted"
level = "error"
summary = "A call's path starts with a slash."

[effects]
network = "surfaces"
sessions = ["stand"]
"#;

/// The compiler's test pack `stand`, whole, with a driver that is a stub:
/// nothing here is called, only compiled and published.
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
                .lint_rule(testing::lint_rule()),
        )
    }
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

struct Publishing {
    kernel: Kernel,
    person: Caller,
    agent: Caller,
    project: Folder,
    _home: Folder,
}

impl Publishing {
    async fn open() -> Self {
        let (folder, paths) = home("p");
        let clock: Arc<dyn Clock> =
            Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(START)));
        let user = OsUser::current().unwrap().to_string();
        let prepared = Kernel::prepare(KernelParts {
            settings: Settings::defaults(),
            paths,
            place: StorePlace::Memory,
            key_store: Arc::new(MemoryKeyStore::new()),
            clock: Arc::clone(&clock),
            random: Arc::new(a2m_vault::OsRandom),
            os_user: user.clone(),
        })
        .await;
        let host =
            prepared.host_services(Supervisor::new(SupervisorLimits::default()), Roots::none());
        let mut builder = RegistryBuilder::new(prepared.settings().provider_settings(), host);
        builder.add_pack(&StandPack);
        let (registry, problems) = builder.build();
        assert!(problems.is_empty(), "{problems:?}");
        let surface = LocalSurface::new(SurfaceParts {
            prompt: TestPrompt::answering(PASSED) as _,
            clock,
            random: Arc::new(a2m_vault::OsRandom),
            window: Duration::from_secs(900),
        });
        let kernel = Kernel::open(
            prepared,
            registry,
            problems,
            Arc::new(surface) as Arc<dyn PersonSurface>,
        )
        .await;
        let principal = kernel
            .verify_caller(CallerCredential::LocalUser(user))
            .await
            .unwrap();
        let project = Folder::new("pp");
        std::fs::write(project.path().join("anything2mcp.toml"), "").unwrap();
        let target = Path::new("targets").join("example");
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        copy_folder(&repository.join(&target), &project.path().join(&target));
        Self {
            kernel,
            person: Caller::new(principal.clone(), FrontDoor::Command, false),
            agent: Caller::new(principal, FrontDoor::Mcp, true),
            project,
            _home: folder,
        }
    }

    /// A publish of the `example` target, at the next version or at
    /// `version`: an act asked for again with the same input is given back
    /// what became of it, so each ask names its own.
    async fn publish(&self, caller: &Caller, version: Option<&str>) -> Response {
        let mut input = json!({"target": "example"});
        if let Some(version) = version {
            input["version"] = json!(version);
        }
        within(
            "a publish",
            self.kernel.dispatch(
                Request::new("version.publish", input, caller.clone())
                    .in_project(self.project.path()),
            ),
        )
        .await
    }

    fn target_file(&self) -> std::path::PathBuf {
        self.project
            .path()
            .join("targets")
            .join("example")
            .join("target.yaml")
    }
}

/// The page an agent's publish at `version` waits on, as the surface
/// serves it, and that page declined.
async fn page_of(publishing: &Publishing, version: &str) -> String {
    let asked = publishing.publish(&publishing.agent, Some(version)).await;
    assert_eq!(asked.exit, ExitClass::PersonNeeded, "{asked:?}");
    let link = asked.result.unwrap()["link"].as_str().unwrap().to_owned();
    let http = a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
        .no_proxy()
        .build()
        .unwrap();
    let page = http.get(&link).send().await.unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let text = page.text().await.unwrap();
    let marker = "name=\"once\" value=\"";
    let from = text.find(marker).unwrap() + marker.len();
    let once = &text[from..from + text[from..].find('"').unwrap()];
    let declined = http
        .post(format!("{link}/decline"))
        .header(ORIGIN, &link[..link.find("/act/").unwrap()])
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(format!("once={once}"))
        .send()
        .await
        .unwrap();
    assert_eq!(declined.status(), StatusCode::OK);
    text
}

/// Where the page shows `heading`; it must show it.
fn at(text: &str, heading: &str) -> usize {
    text.find(heading)
        .unwrap_or_else(|| panic!("no {heading} on the page: {text}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_publish_page_lists_the_surfaces_first_and_marks_a_new_address_there() {
    let publishing = Publishing::open().await;
    // A person publishes the target as it is: its places are registered.
    let first = publishing.publish(&publishing.person, None).await;
    assert!(first.ok, "{first:?}");

    // A draft that reaches no new place: the surfaces come first, and
    // nothing is marked.
    let file = publishing.target_file();
    let written = std::fs::read_to_string(&file).unwrap();
    std::fs::write(
        &file,
        written.replace("title: Example order entry", "title: Example orders"),
    )
    .unwrap();
    let text = page_of(&publishing, "2.0.0").await;
    let leads = at(&text, "<h2>Where it leads</h2>");
    assert!(
        leads < at(&text, "<h2>What each tool may do</h2>"),
        "{text}"
    );
    assert!(leads < at(&text, "<h2>Before you publish</h2>"), "{text}");
    assert!(
        text.contains("<code>desk</code>, a surface of the kind <code>stand</code>, leads to <code>https://test.example.com</code> in the environment <code>test</code>;"),
        "{text}"
    );
    assert!(!text.contains("new-place"), "{text}");
    assert!(!text.contains("New address"), "{text}");

    // A draft that adds a surface at another place: that surface's address
    // is marked where the surfaces are listed, first, and the desk's is not.
    std::fs::write(
        &file,
        written.replace(
            "environments:",
            "  spare:\n    kind: stand\n    address: \"https://elsewhere.example.com\"\nenvironments:",
        ),
    )
    .unwrap();
    let text = page_of(&publishing, "3.0.0").await;
    let mark = format!("<strong class=\"new-place\">({NEW_PLACE})</strong>");
    let leads = at(&text, "<h2>Where it leads</h2>");
    let marked = at(&text, &mark);
    assert!(leads < marked, "{text}");
    assert!(
        marked < at(&text, "<h2>What each tool may do</h2>"),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "<code>spare</code>, a surface of the kind <code>stand</code>, leads to <code>https://elsewhere.example.com</code> in the environment <code>test</code> {mark}"
        )),
        "{text}"
    );
    assert!(
        text.contains(
            "leads to <code>https://test.example.com</code> in the environment <code>test</code>;"
        ),
        "{text}"
    );
    assert!(
        !text.contains(&format!(
            "<code>https://test.example.com</code> in the environment <code>test</code> {mark}"
        )),
        "{text}"
    );
    // The kernel's own line still names it, after the surfaces.
    assert!(
        at(
            &text,
            "New address: the drafts reach `https://elsewhere.example.com`"
        ) > marked,
        "{text}"
    );
    publishing.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn what_breaks_comes_right_after_where_it_leads_and_before_the_sign_ins_and_the_tools() {
    let publishing = Publishing::open().await;
    let first = publishing.publish(&publishing.person, None).await;
    assert!(first.ok, "{first:?}");

    // A draft that drops an output of the order's read: a break, named
    // after the surfaces and before the sign-ins and the tools (decisions
    // 0418 and 0454).
    let folder = publishing.target_file().parent().unwrap().to_path_buf();
    let edit = |path: &Path, from: &str, to: &str| {
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.contains(from), "{from} in {}", path.display());
        std::fs::write(path, text.replacen(from, to, 1)).unwrap();
    };
    edit(
        &folder.join("operations").join("order_get.yaml"),
        "  note: {type: note}\n",
        "",
    );
    edit(
        &folder.join("tests").join("order_get.yaml"),
        ", note: \"Call before delivery\"",
        "",
    );
    let text = page_of(&publishing, "2.0.0").await;
    let leads = at(&text, "<h2>Where it leads</h2>");
    let breaks = at(&text, "<h2>What breaks</h2>");
    let signs_in = at(&text, "<h2>How it signs in</h2>");
    let tools = at(&text, "<h2>What each tool may do</h2>");
    assert!(
        leads < breaks && breaks < signs_in && signs_in < tools,
        "{text}"
    );
    let section = &text[breaks..signs_in];
    assert!(
        section.contains("<code>example_order_get</code>"),
        "{section}"
    );
    assert!(
        section.contains("the output <code>note</code> is gone"),
        "{section}"
    );
    publishing.kernel.close(CloseReason::Asked).await;
}
