//! A capture of a person's use (decision 0392, point 2; decision 0395;
//! package E2b): an agent asks for one with `capture.start`, and nothing
//! records until its person begins it on the engine's own page, past the
//! presence prompt; the person ends it there, the asker ends it with
//! `capture.stop`, or it ends at the bound its discoverer's manifest
//! declares. Its findings are stored as a discovery's are.
//!
//! The kernel and the local person surface are joined here as the product
//! joins them, with one pack of the test's own in place of the first-party
//! packs: its discoverer a person drives counts what it is asked, so the
//! test sees that no discoverer starts before the person begins. The page
//! is fetched and pressed over loopback as a browser would, and the
//! presence prompt is the test's: it passes only a prompt the person was
//! about to answer.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_actions::{
    BoxFuture, Caller, CallerCredential, CloseReason, ExitClass, FrontDoor, Kernel, KernelParts,
    Paths, PersonSurface, Request, Response, Settings, StorePlace,
};
use a2m_core::{
    Address, EvidenceLevel, Failure, Finding, FindingKind, ProducedBy, Source, Subject, Timestamp,
};
use a2m_files::{JsonSchema, SurfaceKindSpec};
use a2m_local::{
    Availability, FAILURE_CODE_HEADER, LocalSurface, PresencePrompt, PromptAnswer, PromptName,
    PromptText, SurfaceParts,
};
use a2m_provider::{
    CaptureEnd, CaptureSource, CapturedTraffic, CompilerRegistrations, Deadline, DescribeForm,
    Describer, Description, Discoverer, DiscovererName, DiscoveryReport, DiscoveryScope,
    FindingSink, Pack, PackContext, Registrar, RegistryBuilder, Roots, SandboxPort, Supervisor,
    SupervisorLimits, SurfaceDescription, SurfaceRef, TrafficCapture,
};
use a2m_telemetry::{Clock, SystemClock};
use a2m_vault::{MemoryKeyStore, OsRandom};
use common::{Folder, USER};
use serde_json::{Value as Json, json};
use tokio::sync::watch;

/// The kind of surface the test's pack serves.
const KIND: &str = "watched_app";

/// A made-up credential, stored in the vault, that a finding quotes.
const PLANTED: &str = "example-planted-credential-5e1d";

/// How long a capture may run, as the discoverer's manifest declares it.
const BOUND_SECONDS: u64 = 2;

const MANIFEST: &str = r#"manifest_format = 1
contract = 1

[provider]
name = "watching"
version = "0.1.0"
publisher = "Example"
kind = "pack"

[[surface_kinds]]
name = "watched_app"
archetype = "interface"

[points]
discoverer = ["watched"]
traffic_capture = ["watched_app"]

[[discoverers]]
name = "watched"
archetype = "interface"
surface_kinds = ["watched_app"]
kinds = ["exchange"]
sources = ["live_system"]
max_level = "discovered"
bounds = { seconds = 2, bytes = 1000000, depth = 4 }
driven_by = "person"

[[finding_kinds]]
name = "exchange"
surface_kinds = ["watched_app"]

[effects]
network = "surfaces"
"#;

/// A discoverer a person drives. It counts each time it is asked to
/// watch, offers two findings, one of which quotes the planted credential,
/// and returns when its capture ends.
struct Watched {
    asked: watch::Sender<u32>,
    returned: watch::Sender<u32>,
}

fn exchange(summary: &str) -> Finding {
    Finding::about(
        Subject::Address(Address::new("http://127.0.0.1:9", "/orders").unwrap()),
        FindingKind::new("exchange").unwrap(),
        summary,
        Source::LiveSystem,
        ProducedBy {
            provider: "watching".to_owned(),
            version: "0.1.0".to_owned(),
            component: None,
        },
        Timestamp::from_unix_millis(1_800_000_000_000),
    )
    .level(EvidenceLevel::Discovered)
    .build()
    .unwrap()
}

impl Discoverer for Watched {
    fn name(&self) -> DiscovererName {
        DiscovererName::new("watched").unwrap()
    }

    fn suits(&self, _surface: &SurfaceDescription) -> bool {
        true
    }

    fn discover<'a>(
        &'a self,
        _port: &'a dyn SandboxPort,
        _scope: &'a DiscoveryScope<'a>,
        _out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        unreachable!("a discoverer a person drives is never asked to discover")
    }

    fn watch<'a>(
        &'a self,
        _scope: &'a DiscoveryScope<'a>,
        end: &'a CaptureEnd,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        self.asked.send_modify(|asked| *asked += 1);
        Box::pin(async move {
            out.found(exchange("GET /orders"))?;
            out.found(exchange(&format!("GET /orders?token={PLANTED}")))?;
            end.ended().await;
            self.returned.send_modify(|returned| *returned += 1);
            Ok(DiscoveryReport::default())
        })
    }
}

struct Listed;

impl Describer for Listed {
    fn describe(&self, findings: &[Finding], _form: DescribeForm) -> Description {
        Description {
            text: format!("{} findings", findings.len()),
            ..Description::default()
        }
    }
}

/// A capture source the discoverer never opens here: the pack registers
/// one because its manifest declares a capture.
struct NoSource;

struct NoTraffic;

impl TrafficCapture for NoTraffic {
    fn start(&self) {}

    fn take_recorded<'a>(&'a self, _until: Deadline) -> BoxFuture<'a, CapturedTraffic> {
        Box::pin(async { CapturedTraffic::default() })
    }
}

impl CaptureSource for NoSource {
    fn open<'a>(
        &'a self,
        _surface: &'a SurfaceRef,
        _deadline: Deadline,
    ) -> BoxFuture<'a, Result<Box<dyn TrafficCapture>, Failure>> {
        Box::pin(async { Ok(Box::new(NoTraffic) as Box<dyn TrafficCapture>) })
    }
}

struct WatchingPack(Arc<Watched>);

impl Pack for WatchingPack {
    fn manifest(&self) -> &'static str {
        MANIFEST
    }

    fn register(&self, _cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        to.compiler(
            CompilerRegistrations::new().surface_kind(SurfaceKindSpec::new(
                a2m_core::SurfaceKind::new(KIND)?,
                "address",
                JsonSchema::object().required("address", JsonSchema::text()),
            )),
        )?;
        to.discoverer(Arc::clone(&self.0) as Arc<dyn Discoverer>, Arc::new(Listed))?;
        to.traffic_capture(Arc::new(NoSource))?;
        Ok(())
    }
}

/// The presence prompt with a person in front of it: they pass a prompt
/// only right after they pressed on the page that showed the act
/// (`expect`), and cancel any other, as the page tells them to.
struct AtPrompt {
    expecting: Mutex<Option<String>>,
    raised: AtomicU32,
}

impl PresencePrompt for AtPrompt {
    fn available(&self) -> BoxFuture<'_, Availability> {
        Box::pin(async {
            Availability::ready(PromptName::Polkit, "The prompt of a test, with a person.")
        })
    }

    fn ask(
        &self,
        text: PromptText,
        _deadline: tokio::time::Instant,
    ) -> BoxFuture<'_, PromptAnswer> {
        Box::pin(async move {
            self.raised.fetch_add(1, Ordering::SeqCst);
            let shown = text.detail.split_whitespace().next_back().unwrap_or("");
            let expected = self.expecting.lock().unwrap().take();
            if expected.as_deref() == Some(shown) {
                PromptAnswer::Present(PromptName::Polkit)
            } else {
                PromptAnswer::Dismissed
            }
        })
    }
}

fn project(folder: &Path) {
    let target = folder.join("targets").join("example");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(folder.join("anything2mcp.toml"), "").unwrap();
    std::fs::write(
        target.join("target.yaml"),
        format!(
            "schema_version: 1\ntarget: example\ntitle: Example orders\nsurfaces:\n  app:\n    kind: {KIND}\n    address: \"http://127.0.0.1:9/\"\nenvironments:\n  test:\n    kind: test\ndefault_environment: test\nenvironment: {{locale: en-US, timezone: UTC}}\ndrift_risk: low\n"
        ),
    )
    .unwrap();
}

struct Capturing {
    kernel: Kernel,
    agent: Caller,
    project: Folder,
    home: Folder,
    prompt: Arc<AtPrompt>,
    watched: Arc<Watched>,
    http: reqwest::Client,
}

async fn open() -> Capturing {
    let home = Folder::new("capture-home");
    let project_folder = Folder::new("capture");
    project(project_folder.path());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let random = Arc::new(OsRandom);
    let prepared = Kernel::prepare(KernelParts {
        settings: Settings::defaults(),
        paths: Paths::under(home.path()),
        place: StorePlace::Disk,
        key_store: Arc::new(MemoryKeyStore::new()),
        clock: Arc::clone(&clock),
        random: Arc::clone(&random) as Arc<dyn a2m_vault::RandomSource>,
        os_user: USER.to_owned(),
    })
    .await;
    let host = prepared.host_services(Supervisor::new(SupervisorLimits::default()), Roots::none());
    let mut builder = RegistryBuilder::new(prepared.settings().provider_settings(), host);
    let watched = Arc::new(Watched {
        asked: watch::channel(0).0,
        returned: watch::channel(0).0,
    });
    builder.add_pack(&WatchingPack(Arc::clone(&watched)));
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    let prompt = Arc::new(AtPrompt {
        expecting: Mutex::new(None),
        raised: AtomicU32::new(0),
    });
    let surface = LocalSurface::new(SurfaceParts {
        prompt: Arc::clone(&prompt) as Arc<dyn PresencePrompt>,
        clock,
        random,
        window: Duration::from_secs(15 * 60),
    });
    let kernel = Kernel::open(
        prepared,
        registry,
        problems,
        Arc::new(surface) as Arc<dyn PersonSurface>,
    )
    .await;
    let principal = kernel
        .verify_caller(CallerCredential::LocalUser(USER.to_owned()))
        .await
        .unwrap();
    Capturing {
        kernel,
        agent: Caller::new(principal, FrontDoor::Mcp, true),
        project: project_folder,
        home,
        prompt,
        watched,
        http: a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
            .no_proxy()
            .build()
            .unwrap(),
    }
}

fn between<'a>(text: &'a str, before: &str, after: char) -> &'a str {
    let from = text
        .find(before)
        .unwrap_or_else(|| panic!("{before}: {text}"))
        + before.len();
    &text[from..from + text[from..].find(after).unwrap()]
}

fn origin_of(link: &str) -> String {
    let rest = &link["http://".len()..];
    format!("http://{}", &rest[..rest.find('/').unwrap()])
}

fn code_of(answer: &reqwest::Response) -> String {
    answer
        .headers()
        .get(FAILURE_CODE_HEADER)
        .map(|value| value.to_str().unwrap().to_owned())
        .unwrap_or_default()
}

impl Capturing {
    async fn call(&self, action: &str, input: Json) -> Response {
        self.kernel
            .dispatch(
                Request::new(action, input, self.agent.clone())
                    .in_project(self.project.path().to_path_buf()),
            )
            .await
    }

    async fn ok(&self, action: &str, input: Json) -> Json {
        let response = self.call(action, input).await;
        assert!(response.ok, "{action}: {:?}", response.error);
        response.result.unwrap()
    }

    fn asked(&self) -> u32 {
        *self.watched.asked.borrow()
    }

    /// An agent asks for a capture of the target. It waits for its
    /// person, with the page's address, and nothing records.
    async fn ask(&self) -> (String, String) {
        let before = self.asked();
        let asked = self
            .call("capture.start", json!({"target": "example"}))
            .await;
        assert_eq!(asked.exit, ExitClass::PersonNeeded, "{asked:?}");
        let result = asked.result.unwrap();
        assert_eq!(result["state"], "asked", "{result}");
        let id = result["capture_id"].as_str().unwrap().to_owned();
        let link = result["link"].as_str().unwrap().to_owned();
        assert_eq!(
            self.asked(),
            before,
            "nothing records before the person begins"
        );
        (id, link)
    }

    /// The page as it is shown: what it says, its one-time value, and the
    /// short form of the act that a prompt for it names.
    async fn page(&self, link: &str) -> (String, String, String) {
        let page = self.http.get(link).send().await.unwrap();
        assert_eq!(page.status().as_u16(), 200);
        let text = page.text().await.unwrap();
        let once = between(&text, "name=\"once\" value=\"", '"').to_owned();
        let short = between(&text, "<span>Act <code>", '<').to_owned();
        (text, once, short)
    }

    async fn press(&self, at: &str, origin: &str, once: &str) -> (u16, String, String) {
        let answer = self
            .http
            .post(at)
            .header("content-type", "application/x-www-form-urlencoded")
            .header("origin", origin)
            .body(format!("once={once}"))
            .send()
            .await
            .unwrap();
        let status = answer.status().as_u16();
        let code = code_of(&answer);
        (status, code, answer.text().await.unwrap())
    }

    /// The person opens the page, presses Carry out, and passes the
    /// prompt. Gives the page the press answers with: the recording's own,
    /// which ends it.
    async fn person_begins(&self, link: &str) -> String {
        let (_, once, short) = self.page(link).await;
        *self.prompt.expecting.lock().unwrap() = Some(short);
        let (status, code, text) = self
            .press(&format!("{link}/confirm"), &origin_of(link), &once)
            .await;
        assert_eq!((status, code.as_str()), (200, ""), "{text}");
        text
    }

    async fn until_asked(&self, count: u32) {
        let mut seen = self.watched.asked.subscribe();
        tokio::time::timeout(Duration::from_secs(10), seen.wait_for(|n| *n >= count))
            .await
            .expect("the discoverer was asked to watch")
            .unwrap();
    }

    async fn until_returned(&self, count: u32) {
        let mut seen = self.watched.returned.subscribe();
        tokio::time::timeout(Duration::from_secs(10), seen.wait_for(|n| *n >= count))
            .await
            .expect("the discoverer returned")
            .unwrap();
    }
}

/// Every file under `folder` that holds `needle`.
fn files_holding(folder: &Path, needle: &[u8]) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut left = vec![folder.to_path_buf()];
    while let Some(at) = left.pop() {
        let Ok(entries) = std::fs::read_dir(&at) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                left.push(path);
            } else if let Ok(bytes) = std::fs::read(&path)
                && bytes.windows(needle.len()).any(|window| window == needle)
            {
                found.push(path);
            }
        }
    }
    found
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_capture_records_only_once_its_person_begins_it_on_the_page_and_ends_as_asked() {
    let world = open().await;
    let stored = world
        .call(
            "secret.set",
            json!({"name": "example_password", "value": PLANTED}),
        )
        .await;
    assert!(stored.ok, "{:?}", stored.error);

    // An agent asks: the capture waits for its person, with the page's
    // address, and no discoverer has been asked anything.
    let (id, link) = world.ask().await;
    let (text, _, _) = world.page(&link).await;
    assert!(text.contains("capture.start"), "{text}");
    assert!(text.contains("What is kept"), "{text}");

    // The agent cannot begin it itself by naming it.
    let named = world
        .call("capture.start", json!({"target": "example", "capture": id}))
        .await;
    let error = named.error.expect("refused");
    assert_eq!(error.code, "person_required", "{error:?}");
    assert_eq!(world.asked(), 0);

    // A script that presses the page's button, with the page's own origin
    // and one-time value, meets the prompt it cannot pass, and is refused
    // by name. Nothing records.
    let (_, once, _) = world.page(&link).await;
    let (status, code, _) = world
        .press(&format!("{link}/confirm"), &origin_of(&link), &once)
        .await;
    assert_eq!((status, code.as_str()), (403, "presence_failed"));
    assert_eq!(world.prompt.raised.load(Ordering::SeqCst), 1);
    assert_eq!(world.asked(), 0);

    // The person begins it, past the prompt: now the discoverer watches.
    let recording = world.person_begins(&link).await;
    assert!(recording.contains("Stop recording"), "{recording}");
    world.until_asked(1).await;

    // The asker ends it. Its findings are stored as a discovery's are,
    // and the one that quotes the credential is not kept.
    let stopped = world.ok("capture.stop", json!({"target": "example"})).await;
    assert_eq!(stopped["capture_id"], id.as_str(), "{stopped}");
    assert_eq!(stopped["state"], "ended", "{stopped}");
    assert_eq!(stopped["ended_by"], "asked", "{stopped}");
    assert_eq!(stopped["findings"], 1, "{stopped}");
    let listed = world.ok("finding.list", json!({"target": "example"})).await;
    assert_eq!(listed["total"], 1, "{listed}");
    assert_eq!(listed["findings"][0]["kind"], "exchange", "{listed}");
    world.until_returned(1).await;

    // A second capture, begun and left alone, ends at its bound and says
    // so.
    let (second, link) = world.ask().await;
    assert_ne!(second, id);
    world.person_begins(&link).await;
    world.until_asked(2).await;
    let began = std::time::Instant::now();
    world.until_returned(2).await;
    assert!(began.elapsed() < Duration::from_secs(BOUND_SECONDS + 5));
    let ended = world.ok("capture.stop", json!({"target": "example"})).await;
    assert_eq!(ended["capture_id"], second.as_str(), "{ended}");
    assert_eq!(ended["ended_by"], "bound", "{ended}");
    let bounds = ended["discoverers"][0]["report"]["bounds_hit"].clone();
    assert!(
        bounds
            .as_array()
            .unwrap()
            .iter()
            .any(|hit| hit == "seconds"),
        "{ended}"
    );

    // A third, ended by its person on the recording's page.
    let (third, link) = world.ask().await;
    let recording = world.person_begins(&link).await;
    world.until_asked(3).await;
    let stop = between(&recording, "<form method=\"post\" action=\"", '"').to_owned();
    let once = between(&recording, "name=\"once\" value=\"", '"').to_owned();
    let origin = origin_of(&link);
    let (status, code, text) = world
        .press(&format!("{origin}{stop}"), &origin, &once)
        .await;
    assert_eq!((status, code.as_str()), (200, ""), "{text}");
    world.until_returned(3).await;
    let ended = world.ok("capture.stop", json!({"target": "example"})).await;
    assert_eq!(ended["capture_id"], third.as_str(), "{ended}");
    assert_eq!(ended["ended_by"], "person", "{ended}");

    // The credential is in no file the engine stored.
    world.kernel.clone().close(CloseReason::Asked).await;
    for folder in [world.home.path(), world.project.path()] {
        let holding = files_holding(folder, PLANTED.as_bytes());
        assert!(holding.is_empty(), "{holding:?}");
    }
}
