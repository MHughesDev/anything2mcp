//! A discoverer a person drives (decision 0392, point 2, as decision 0395
//! builds it): its manifest says so, only a pack that captures traffic may
//! declare one, the registry gives it to a capture job and never to a
//! discovery, and its handle calls it by `watch` alone, until the
//! capture's end or its bound.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_core::{
    AccessKind, CompiledSurface, Environment, EvidenceLevel, Failure, FailureClass, Finding,
    NoSecrets, PackPayload, Step, SurfaceKind, SurfaceName, TargetName, Values,
};
use a2m_provider::{
    BoxFuture, CaptureEnd, CaptureSource, CapturedTraffic, CheckedPaths, Deadline, Discoverer,
    DiscovererName, DiscoveryReport, DiscoveryScope, DrivenBy, FindingSink, Manifest, Pack,
    PackContext, ProviderSettings, Registrar, Registry, RegistryBuilder, Roots, SandboxPort,
    StepFailure, StepInput, StepOutcome, SurfaceDescription, SurfaceRef, TrafficCapture, codes,
};
use common::{Folder, StubDescriber, finding, host};

/// The manifest of a pack with two discoverers of one surface kind: `look`,
/// driven by the engine, and `watched`, driven as `driven_by` says.
fn manifest(name: &str, driven_by: &str, captures: &str) -> String {
    format!(
        r#"manifest_format = 1
contract = 1

[provider]
name = "{name}"
version = "0.1.0"
publisher = "Example"
kind = "pack"

[[surface_kinds]]
name = "web"
archetype = "interface"

[points]
discoverer = ["look", "watched"]
traffic_capture = [{captures}]

[[discoverers]]
name = "look"
archetype = "interface"
kinds = ["call"]
sources = ["live_system"]
max_level = "discovered"
bounds = {{ seconds = 60, bytes = 1000000, depth = 4 }}
files = ["engine_file"]

[[discoverers]]
name = "watched"
archetype = "interface"
kinds = ["call"]
sources = ["live_system"]
max_level = "inferred"
bounds = {{ seconds = 1, bytes = 1000000, depth = 4 }}
files = ["person_file"]
{driven_by}

[[finding_kinds]]
name = "call"
surface_kinds = ["web"]

[effects]
network = "surfaces"
"#
    )
}

/// A discoverer that counts how often each method is asked, offers the
/// findings it is given, and, when watching, waits for the capture's end.
struct Counting {
    name: &'static str,
    /// Whether it implements `watch`, or keeps the contract's default.
    watches: bool,
    discovered: AtomicUsize,
    watched: AtomicUsize,
    findings: Mutex<Vec<Finding>>,
}

impl Counting {
    fn new(name: &'static str, watches: bool) -> Arc<Self> {
        Arc::new(Self {
            name,
            watches,
            discovered: AtomicUsize::new(0),
            watched: AtomicUsize::new(0),
            findings: Mutex::new(Vec::new()),
        })
    }

    fn offer(&self, out: &mut dyn FindingSink) -> Result<(), Failure> {
        let findings: Vec<Finding> = self.findings.lock().unwrap().drain(..).collect();
        for one in findings {
            out.found(one)?;
        }
        Ok(())
    }
}

impl Discoverer for Counting {
    fn name(&self) -> DiscovererName {
        DiscovererName::new(self.name).unwrap()
    }

    fn suits(&self, _surface: &SurfaceDescription) -> bool {
        true
    }

    fn discover<'a>(
        &'a self,
        _port: &'a dyn SandboxPort,
        _scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        self.discovered.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            self.offer(out)?;
            Ok(DiscoveryReport::default())
        })
    }

    fn watch<'a>(
        &'a self,
        scope: &'a DiscoveryScope<'a>,
        end: &'a CaptureEnd,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        if !self.watches {
            return Box::pin(async move {
                // The contract's default, reached through a type that keeps it.
                Defaulted.watch(scope, end, out).await
            });
        }
        self.watched.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            self.offer(out)?;
            end.ended().await;
            Ok(DiscoveryReport::default())
        })
    }
}

/// A discoverer that implements nothing but what `Discoverer` requires.
struct Defaulted;

impl Discoverer for Defaulted {
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
        Box::pin(async { Ok(DiscoveryReport::default()) })
    }
}

/// A capture source that is never opened here: the pack must register one
/// because its manifest declares a capture.
struct NoTraffic;

impl TrafficCapture for NoTraffic {
    fn start(&self) {}

    fn take_recorded<'a>(&'a self, _until: Deadline) -> BoxFuture<'a, CapturedTraffic> {
        Box::pin(async { CapturedTraffic::default() })
    }
}

struct NoSource;

impl CaptureSource for NoSource {
    fn open<'a>(
        &'a self,
        _surface: &'a SurfaceRef,
        _deadline: Deadline,
    ) -> BoxFuture<'a, Result<Box<dyn TrafficCapture>, Failure>> {
        Box::pin(async { Ok(Box::new(NoTraffic) as Box<dyn TrafficCapture>) })
    }
}

struct Watching {
    manifest: &'static str,
    captures: bool,
    look: Arc<Counting>,
    watched: Arc<Counting>,
}

impl Watching {
    fn new(driven_by: &str, captures: bool, watches: bool) -> Self {
        let text = manifest("watching", driven_by, if captures { "\"web\"" } else { "" });
        Self {
            manifest: Box::leak(text.into_boxed_str()),
            captures,
            look: Counting::new("look", watches),
            watched: Counting::new("watched", watches),
        }
    }
}

impl Pack for Watching {
    fn manifest(&self) -> &'static str {
        self.manifest
    }

    fn register(&self, _cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        to.discoverer(self.look.clone(), Arc::new(StubDescriber))?;
        to.discoverer(self.watched.clone(), Arc::new(StubDescriber))?;
        if self.captures {
            to.traffic_capture(Arc::new(NoSource))?;
        }
        Ok(())
    }
}

fn registry_of(pack: &Watching) -> Registry {
    let folder = Folder::new("watch");
    let mut builder =
        RegistryBuilder::new(ProviderSettings::default(), host(&folder, Roots::none()));
    builder.add_pack(pack);
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    registry
}

fn description() -> SurfaceDescription {
    SurfaceDescription {
        kind: SurfaceKind::new("web").unwrap(),
        access_kind: AccessKind::Interface,
        address: "http://127.0.0.1:8080/".to_owned(),
        facts: Values::new(),
    }
}

fn surface() -> SurfaceRef {
    SurfaceRef {
        target: TargetName::new("example").unwrap(),
        environment: Environment::new("test").unwrap(),
        surface: CompiledSurface {
            name: SurfaceName::new("app").unwrap(),
            kind: SurfaceKind::new("web").unwrap(),
            address: "http://127.0.0.1:8080/".to_owned(),
            same_session_as: None,
            stands: None,
            rate: None,
            programs: Vec::new(),
            payload: PackPayload::none(),
        },
    }
}

struct Kept(Vec<Finding>);

impl FindingSink for Kept {
    fn found(&mut self, finding: Finding) -> Result<(), Failure> {
        self.0.push(finding);
        Ok(())
    }
}

/// A port that is never reached: a discoverer of this file sends no step.
struct Unreached;

impl SandboxPort for Unreached {
    fn act<'a>(
        &'a self,
        _step: &'a Step,
        _input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        unreachable!("no step is sent in these tests")
    }
}

const PERSON: &str = r#"driven_by = "person""#;

#[test]
fn a_discoverer_a_person_drives_belongs_to_a_pack_that_captures_traffic() {
    // Left out, a discoverer is driven by the engine.
    let left_out = Manifest::parse(&manifest("plain", "", "")).unwrap();
    assert!(
        left_out
            .discoverers
            .iter()
            .all(|d| d.driven_by == DrivenBy::Engine)
    );
    let said = Manifest::parse(&manifest("said", r#"driven_by = "engine""#, "")).unwrap();
    assert_eq!(said.discoverers[1].driven_by, DrivenBy::Engine);

    // Driven by a person with a capture declared, it is read.
    let read = Manifest::parse(&manifest("read", PERSON, "\"web\"")).unwrap();
    assert_eq!(read.discoverers[0].driven_by, DrivenBy::Engine);
    assert_eq!(read.discoverers[1].driven_by, DrivenBy::Person);

    // Driven by a person with no capture declared, the manifest is refused.
    let refused = Manifest::parse(&manifest("refused", PERSON, "")).unwrap_err();
    assert_eq!(
        (refused.class(), refused.code()),
        (FailureClass::Policy, &codes::EFFECT_NOT_DECLARED)
    );

    // A word that is neither is refused as any unknown word of a manifest.
    let unknown =
        Manifest::parse(&manifest("unknown", r#"driven_by = "agent""#, "\"web\"")).unwrap_err();
    assert_eq!(unknown.code(), &codes::MANIFEST_INVALID);
}

#[test]
fn the_registry_gives_a_person_driven_discoverer_to_a_capture_and_never_to_a_discovery() {
    let pack = Watching::new(PERSON, true, true);
    let registry = registry_of(&pack);
    let surface = description();

    let discovery: Vec<String> = registry
        .discoverers_for(&surface)
        .iter()
        .map(|handle| handle.declared().name.to_string())
        .collect();
    assert_eq!(discovery, ["look"]);
    let capture: Vec<String> = registry
        .capture_discoverers_for(&surface)
        .iter()
        .map(|handle| handle.declared().name.to_string())
        .collect();
    assert_eq!(capture, ["watched"]);
    // A discovery may be handed only the files of the discoverers it runs.
    assert_eq!(
        registry
            .files_taken(&surface)
            .into_iter()
            .collect::<Vec<_>>(),
        ["engine_file"]
    );

    // Driven by the engine, the same discoverer is a discovery's.
    let engine = Watching::new("", true, true);
    let registry = registry_of(&engine);
    assert_eq!(registry.discoverers_for(&surface).len(), 2);
    assert!(registry.capture_discoverers_for(&surface).is_empty());
}

#[tokio::test]
async fn each_handle_refuses_the_call_its_manifest_does_not_name() {
    let pack = Watching::new(PERSON, true, true);
    let registry = registry_of(&pack);
    let surface = description();
    let target = surface_ref_and_scope_parts();
    let scope = target.scope();
    let mut kept = Kept(Vec::new());

    let person = registry.capture_discoverers_for(&surface).remove(0);
    let refused = person
        .discover(&Unreached, &scope, &mut kept)
        .await
        .unwrap_err();
    assert_eq!(
        (refused.class(), refused.code()),
        (FailureClass::Policy, &codes::EFFECT_NOT_DECLARED)
    );
    assert_eq!(pack.watched.discovered.load(Ordering::SeqCst), 0);

    let engine = registry.discoverers_for(&surface).remove(0);
    let stop = registry.capture_stop();
    let refused = engine
        .watch(&scope, stop.end(), &mut kept)
        .await
        .unwrap_err();
    assert_eq!(
        (refused.class(), refused.code()),
        (FailureClass::Policy, &codes::EFFECT_NOT_DECLARED)
    );
    assert_eq!(pack.look.watched.load(Ordering::SeqCst), 0);

    // Each is still called by its own method.
    engine
        .discover(&Unreached, &scope, &mut kept)
        .await
        .unwrap();
    assert_eq!(pack.look.discovered.load(Ordering::SeqCst), 1);
    stop.stop();
    person.watch(&scope, stop.end(), &mut kept).await.unwrap();
    assert_eq!(pack.watched.watched.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn a_watch_returns_at_its_end_and_at_its_bound_when_the_end_is_never_set() {
    let pack = Watching::new(PERSON, true, true);
    let registry = registry_of(&pack);
    let person = registry.capture_discoverers_for(&description()).remove(0);
    let parts = surface_ref_and_scope_parts();
    let scope = parts.scope();
    let mut kept = Kept(Vec::new());

    // Handed an end already set, it returns at once, and names no bound.
    let stop = registry.capture_stop();
    stop.stop();
    let started = tokio::time::Instant::now();
    let report = person.watch(&scope, stop.end(), &mut kept).await.unwrap();
    assert_eq!(started.elapsed(), Duration::ZERO);
    assert!(report.bounds_hit.is_empty(), "{report:?}");

    // An end never set: it returns at the manifest's bound of one second,
    // although the scope allows thirty, and the report says so. The end is
    // then set, so the kernel sees the capture is over.
    let stop = registry.capture_stop();
    let started = tokio::time::Instant::now();
    let report = person.watch(&scope, stop.end(), &mut kept).await.unwrap();
    assert_eq!(started.elapsed(), Duration::from_secs(1));
    assert_eq!(report.bounds_hit, ["seconds"]);
    assert!(stop.end().is_set());

    // Set while it watches, it returns then.
    let stop = registry.capture_stop();
    let later = stop.clone();
    let started = tokio::time::Instant::now();
    let (report, ()) = tokio::join!(person.watch(&scope, stop.end(), &mut kept), async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        later.stop();
    });
    assert_eq!(started.elapsed(), Duration::from_millis(300));
    assert!(report.unwrap().bounds_hit.is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_discoverer_that_does_not_return_after_its_end_is_given_up_as_a_timeout() {
    struct Deaf;
    impl Discoverer for Deaf {
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
            Box::pin(async { Ok(DiscoveryReport::default()) })
        }
        fn watch<'a>(
            &'a self,
            _scope: &'a DiscoveryScope<'a>,
            _end: &'a CaptureEnd,
            _out: &'a mut dyn FindingSink,
        ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
            Box::pin(std::future::pending())
        }
    }
    struct DeafPack(&'static str);
    impl Pack for DeafPack {
        fn manifest(&self) -> &'static str {
            self.0
        }
        fn register(&self, _cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
            to.discoverer(Counting::new("look", true), Arc::new(StubDescriber))?;
            to.discoverer(Arc::new(Deaf), Arc::new(StubDescriber))?;
            to.traffic_capture(Arc::new(NoSource))
        }
    }
    let text = manifest("deaf", PERSON, "\"web\"");
    let pack = DeafPack(Box::leak(text.into_boxed_str()));
    let folder = Folder::new("watch-deaf");
    let mut builder =
        RegistryBuilder::new(ProviderSettings::default(), host(&folder, Roots::none()));
    builder.add_pack(&pack);
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    let person = registry.capture_discoverers_for(&description()).remove(0);
    let parts = surface_ref_and_scope_parts();
    let scope = parts.scope();
    let stop = registry.capture_stop();
    let started = tokio::time::Instant::now();
    let failure = person
        .watch(&scope, stop.end(), &mut Kept(Vec::new()))
        .await
        .unwrap_err();
    assert_eq!(
        (failure.class(), failure.code()),
        (FailureClass::Transient, &codes::TIMEOUT)
    );
    assert_eq!(
        started.elapsed(),
        Duration::from_secs(1) + a2m_provider::CAPTURE_END_GRACE
    );
}

#[tokio::test]
async fn a_finding_its_manifest_does_not_own_is_refused_from_watch_as_from_discover() {
    let pack = Watching::new(PERSON, true, true);
    let registry = registry_of(&pack);
    let person = registry.capture_discoverers_for(&description()).remove(0);
    let parts = surface_ref_and_scope_parts();
    let scope = parts.scope();
    let mut kept = Kept(Vec::new());
    let stop = registry.capture_stop();
    stop.stop();

    pack.watched
        .findings
        .lock()
        .unwrap()
        .push(finding("call", EvidenceLevel::Inferred));
    person.watch(&scope, stop.end(), &mut kept).await.unwrap();
    assert_eq!(kept.0.len(), 1);

    for refused in [
        finding("screen", EvidenceLevel::Inferred),
        finding("call", EvidenceLevel::Discovered),
    ] {
        pack.watched.findings.lock().unwrap().push(refused);
        let failure = person
            .watch(&scope, stop.end(), &mut kept)
            .await
            .unwrap_err();
        assert_eq!(failure.code(), &codes::FINDING_NOT_DECLARED);
        assert_eq!(kept.0.len(), 1);
    }
}

#[tokio::test]
async fn a_discoverer_a_person_drives_that_does_not_watch_fails_by_the_contracts_default() {
    let pack = Watching::new(PERSON, true, false);
    let registry = registry_of(&pack);
    let person = registry.capture_discoverers_for(&description()).remove(0);
    let parts = surface_ref_and_scope_parts();
    let scope = parts.scope();
    let stop = registry.capture_stop();
    let failure = person
        .watch(&scope, stop.end(), &mut Kept(Vec::new()))
        .await
        .unwrap_err();
    assert_eq!(
        (failure.class(), failure.code()),
        (
            FailureClass::UnknownVariant,
            &codes::DISCOVERER_CANNOT_WATCH
        )
    );
}

/// What a scope borrows, held for one test.
struct ScopeParts {
    surface: SurfaceRef,
    paths: CheckedPaths,
    focus: Values,
}

impl ScopeParts {
    fn scope(&self) -> DiscoveryScope<'_> {
        DiscoveryScope {
            surface: &self.surface,
            bounds: a2m_provider::Bounds {
                seconds: 30,
                bytes: 1_000_000,
                depth: 4,
            },
            deadline: Deadline::after(Duration::from_secs(30)),
            paths: &self.paths,
            redact: &NoSecrets,
            focus: &self.focus,
        }
    }
}

fn surface_ref_and_scope_parts() -> ScopeParts {
    ScopeParts {
        surface: surface(),
        paths: CheckedPaths::none(),
        focus: Values::new(),
    }
}
