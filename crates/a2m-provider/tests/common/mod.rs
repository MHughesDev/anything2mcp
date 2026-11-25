//! What the tests share: a test pack that registers a mode with a driver, a
//! file kind with a lowering, a discoverer with its describer, and an auth
//! provider; the host services a registry is built with; and a folder of
//! the test's own.

#![allow(
    dead_code,
    reason = "each test binary uses some of these helpers and not others"
)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use a2m_auth::{
    AuthEntry, AuthProvider, CredentialSink, ExpirySign, Material, ProviderNeeds, RawSign,
    SignInContext, SignedIn,
};
use a2m_core::{
    Failure, FailureClass, FailureCode, FileKindName, Finding, IdentityStrategy, Mode, NoSecrets,
    PackPayload, Step, StepKind, SurfaceKind, SurfaceName, Timestamp, Values, Verb,
};
use a2m_files::{
    CompiledSection, DefinedName, Diagnostics, Doc, ExprSite, FileKind, JsonSchema, KindContext,
    Located, LowerContext, Lowered, Lowering, Possible, ResolvedBinding, SurfaceKindSpec,
};
use a2m_logs::LogSettings;
use a2m_provider::{
    BoxFuture, Cleanup, CompilerRegistrations, DescribeForm, Describer, Description, Discoverer,
    DiscovererName, DiscoveryReport, DiscoveryScope, Driver, DriverHealth, FindingSink,
    HostServices, Observed, Pack, PackContext, PrepareContext, Prepared, Registrar, Roots,
    SandboxPort, StepFailure, StepInput, StepOutcome, Supervisor, SupervisorLimits,
    SurfaceDescription, SurfaceRef,
};

/// A folder under the system's temporary folder, removed when dropped.
pub struct Folder(PathBuf);

impl Folder {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "a2m-provider-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The host services of a test: a supervisor, the roots given, no secrets.
pub fn host(folder: &Folder, roots: Roots) -> HostServices {
    host_with(folder, roots, Arc::new(NoSecrets))
}

/// A redactor that knows one secret.
pub struct OneSecret(pub &'static str);

impl a2m_core::Redact for OneSecret {
    fn redact(&self, text: &str) -> String {
        text.replace(self.0, a2m_core::REDACTED)
    }
}

/// The host services of a test, with the redactor given.
pub fn host_with(folder: &Folder, roots: Roots, redact: Arc<dyn a2m_core::Redact>) -> HostServices {
    let logs = folder.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    HostServices {
        supervisor: Supervisor::new(SupervisorLimits::default()),
        roots,
        redact,
        logs_and_traces: a2m_logs::start(&LogSettings::default(), Arc::new(NoSecrets), &logs)
            .unwrap(),
    }
}

/// The manifest of the test pack. `name` is the pack; `access kind` that of
/// its mode; `extra` is added at the end.
pub fn manifest(name: &str, access_kind: &str, extra: &str) -> String {
    format!(
        r#"manifest_format = 1
contract = 1

[provider]
name = "{name}"
version = "0.1.0"
publisher = "Example"
kind = "pack"

[[modes]]
name = "stand"
archetype = "{access_kind}"
verbs = ["read", "upsert"]
steps = ["fetch", "send"]
rank = 40
session_reads = 4

[[surface_kinds]]
name = "{name}_surface"
archetype = "{access_kind}"
session = true

[points]
driver = ["stand"]
discoverer = ["look"]
auth_provider = ["{name}_login"]

[[discoverers]]
name = "look"
archetype = "{access_kind}"
kinds = ["call"]
sources = ["live_system"]
max_level = "discovered"
bounds = {{ seconds = 60, bytes = 1000000, depth = 4 }}

[[file_kinds]]
name = "{name}_call"
directory = "{name}_calls/"
surface_kinds = ["{name}_surface"]

[[finding_kinds]]
name = "call"
surface_kinds = ["{name}_surface"]

[[failure_codes]]
class = "rejected"
code = "example_refused"

[effects]
read_roots = ["project"]
network = "surfaces"
sessions = ["{name}_surface"]

[[settings]]
key = "page_size"
type = "int"
default = 50
min = 1
max = 500
doc = "How many records one fetch asks for."
{extra}"#
    )
}

pub fn mode() -> Mode {
    Mode::new("stand").unwrap()
}

pub fn step_kind(name: &str) -> StepKind {
    StepKind::new(name).unwrap()
}

struct CallKind {
    name: FileKindName,
    folder: String,
    kinds: Vec<SurfaceKind>,
    schema: JsonSchema,
}

impl FileKind for CallKind {
    fn name(&self) -> FileKindName {
        self.name.clone()
    }
    fn surface_kinds(&self) -> &[SurfaceKind] {
        &self.kinds
    }
    fn folder(&self) -> &str {
        &self.folder
    }
    fn schema(&self) -> &JsonSchema {
        &self.schema
    }
    fn names(&self, _doc: &Located<Doc>) -> Vec<Located<DefinedName>> {
        Vec::new()
    }
    fn expressions(&self, _doc: &Located<Doc>) -> Vec<ExprSite> {
        Vec::new()
    }
    fn compile(
        &self,
        _docs: &[Located<Doc>],
        _cx: &KindContext<'_>,
    ) -> Result<CompiledSection, Diagnostics> {
        Ok(CompiledSection {
            format: 1,
            payload: PackPayload::none(),
        })
    }
}

struct StandLowering {
    verbs: Vec<Verb>,
    steps: Vec<StepKind>,
    schema: JsonSchema,
}

impl Lowering for StandLowering {
    fn mode(&self) -> Mode {
        mode()
    }
    fn verbs(&self) -> &[Verb] {
        &self.verbs
    }
    fn reads(&self) -> &[(FileKindName, SurfaceKind)] {
        &[]
    }
    fn step_kinds(&self) -> &[StepKind] {
        &self.steps
    }
    fn binding_schema(&self) -> &JsonSchema {
        &self.schema
    }
    fn identity(&self, _binding: &ResolvedBinding<'_>, _strategy: IdentityStrategy) -> Possible {
        Possible::Yes
    }
    fn lower(
        &self,
        _binding: &ResolvedBinding<'_>,
        _cx: &LowerContext<'_>,
    ) -> Result<Lowered, Diagnostics> {
        Ok(Lowered {
            surface: SurfaceName::new("main").unwrap(),
            act: Vec::new(),
            confirm: Vec::new(),
            read_back: Vec::new(),
            lookup: Vec::new(),
            settle_ms: 0,
            prefer_response: false,
        })
    }
}

/// What the stub driver does when a step reaches it.
#[derive(Clone)]
pub enum Behaviour {
    Done,
    /// Done, with what was observed.
    Saw(Observed),
    Absent,
    DoneWithReconcile,
    /// Done, giving back as its values what the handle set as recorded.
    EchoRecorded,
    /// Done, giving back as its value `answer` what the engine answered
    /// when asked whether another record holds this value under this name.
    AskElsewhere(String, a2m_core::Value),
    Fail(StepFailure),
}

/// A driver that records every step that reaches it.
pub struct StubDriver {
    pub reached: Arc<Mutex<Vec<String>>>,
    pub behaviour: Arc<Mutex<Behaviour>>,
}

impl Driver for StubDriver {
    fn prepare<'a>(
        &'a self,
        _cx: &'a PrepareContext<'a>,
        _lease: Option<&'a a2m_auth::SessionLease>,
    ) -> BoxFuture<'a, Result<Prepared, StepFailure>> {
        Box::pin(async move { Ok(Prepared::new(0_u32)) })
    }

    fn act<'a>(
        &'a self,
        prepared: &'a mut Prepared,
        step: &'a Step,
        input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async move {
            if let Some(count) = prepared.state::<u32>() {
                *count += 1;
            }
            self.reached.lock().unwrap().push(step.kind.to_string());
            let behaviour = self.behaviour.lock().unwrap().clone();
            match behaviour {
                Behaviour::Done => Ok(StepOutcome::Done(Observed::default())),
                Behaviour::Saw(observed) => Ok(StepOutcome::Done(observed)),
                Behaviour::Absent => Ok(StepOutcome::Absent(Observed::default())),
                Behaviour::DoneWithReconcile => Ok(StepOutcome::Done(Observed {
                    for_reconcile: Values::new().with("assigned", "K-1"),
                    ..Observed::default()
                })),
                Behaviour::EchoRecorded => Ok(StepOutcome::Done(Observed {
                    values: prepared.recorded().clone(),
                    ..Observed::default()
                })),
                Behaviour::AskElsewhere(name, value) => {
                    let answer = match prepared
                        .recorded_elsewhere(&name, &value, input.deadline)
                        .await
                    {
                        a2m_provider::RecordedElsewhere::Held(record) => {
                            format!("held:{}:{}", record.step_seq, record.record_seq)
                        }
                        a2m_provider::RecordedElsewhere::NotHeld => "not_held".to_owned(),
                        a2m_provider::RecordedElsewhere::CannotTell => "cannot_tell".to_owned(),
                    };
                    Ok(StepOutcome::Done(Observed {
                        values: Values::new().with("answer", answer),
                        ..Observed::default()
                    }))
                }
                Behaviour::Fail(failure) => Err(failure),
            }
        })
    }

    fn health(&self, _surface: &SurfaceRef) -> DriverHealth {
        DriverHealth::Ready
    }

    fn release<'a>(&'a self, _prepared: Prepared, _poisoned: bool) -> BoxFuture<'a, Cleanup> {
        Box::pin(async move { Cleanup::complete() })
    }
}

/// A discoverer that sends the step it is given through the port, and
/// offers the findings it is given to the sink.
pub struct StubDiscoverer {
    pub step: Mutex<Option<Step>>,
    pub findings: Mutex<Vec<Finding>>,
    pub port_result: Mutex<Option<Result<StepOutcome, StepFailure>>>,
}

impl StubDiscoverer {
    pub fn new() -> Self {
        Self {
            step: Mutex::new(None),
            findings: Mutex::new(Vec::new()),
            port_result: Mutex::new(None),
        }
    }
}

impl Discoverer for StubDiscoverer {
    fn name(&self) -> DiscovererName {
        DiscovererName::new("look").unwrap()
    }

    fn suits(&self, _surface: &SurfaceDescription) -> bool {
        true
    }

    fn discover<'a>(
        &'a self,
        port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(async move {
            let step = self.step.lock().unwrap().clone();
            if let Some(step) = step {
                let args = Values::new();
                let input = StepInput {
                    args: &args,
                    record: None,
                    key: None,
                    stamp: None,
                    deadline: scope.deadline,
                };
                let result = port.act(&step, &input).await;
                *self.port_result.lock().unwrap() = Some(result);
            }
            let findings: Vec<Finding> = self.findings.lock().unwrap().drain(..).collect();
            for finding in findings {
                out.found(finding)?;
            }
            Ok(DiscoveryReport::default())
        })
    }
}

pub struct StubDescriber;

impl Describer for StubDescriber {
    fn describe(&self, findings: &[Finding], _form: DescribeForm) -> Description {
        Description {
            text: format!("{} findings", findings.len()),
            ..Description::default()
        }
    }
}

struct StubAuth(String);

impl AuthProvider for StubAuth {
    fn name(&self) -> a2m_auth::ProviderName {
        a2m_auth::ProviderName::new(&self.0).unwrap()
    }
    fn needs(&self) -> ProviderNeeds {
        ProviderNeeds {
            person: false,
            silent_renewal: false,
            light_prompt: false,
            movable: true,
            confirms: false,
        }
    }
    fn check(&self, _entry: &AuthEntry) -> Result<(), Failure> {
        Ok(())
    }
    fn sign_in<'a>(
        &'a self,
        _cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<Material, Failure>> {
        Box::pin(async move { Ok(Material::new(self.name())) })
    }
    fn is_signed_in<'a>(
        &'a self,
        _material: &'a Material,
        _now: Timestamp,
    ) -> BoxFuture<'a, SignedIn> {
        Box::pin(async move { SignedIn::Yes { until: None } })
    }
    fn detect_expiry(&self, _sign: &RawSign) -> Option<ExpirySign> {
        None
    }
    fn refresh<'a>(
        &'a self,
        _material: &'a Material,
        _cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<Material, Failure>> {
        Box::pin(async move { Ok(Material::new(self.name())) })
    }
    fn sign_out<'a>(
        &'a self,
        _material: &'a Material,
        _cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move { Ok(()) })
    }
    fn inject(&self, _material: &Material, _sink: &mut dyn CredentialSink) -> Result<(), Failure> {
        Ok(())
    }
}

/// What a test pack leaves out or adds when it registers.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Twist {
    None,
    /// Registers a driver for a mode the manifest does not list.
    UndeclaredDriver,
    /// Registers no driver.
    NoDriver,
}

/// The test pack. Every part of it is a stub; the registry and the handle
/// are what is under test.
pub struct TestPack {
    pub name: String,
    pub manifest: &'static str,
    pub twist: Twist,
    pub reached: Arc<Mutex<Vec<String>>>,
    pub behaviour: Arc<Mutex<Behaviour>>,
    pub discoverer: Arc<StubDiscoverer>,
    pub seen_settings: Arc<Mutex<Option<Values>>>,
    pub context: Arc<Mutex<Option<PackContext>>>,
}

impl TestPack {
    pub fn new(name: &str) -> Self {
        Self::with_manifest(name, manifest(name, "request", ""))
    }

    pub fn with_manifest(name: &str, manifest: String) -> Self {
        Self {
            name: name.to_owned(),
            manifest: Box::leak(manifest.into_boxed_str()),
            twist: Twist::None,
            reached: Arc::new(Mutex::new(Vec::new())),
            behaviour: Arc::new(Mutex::new(Behaviour::Done)),
            discoverer: Arc::new(StubDiscoverer::new()),
            seen_settings: Arc::new(Mutex::new(None)),
            context: Arc::new(Mutex::new(None)),
        }
    }

    pub fn twisted(mut self, twist: Twist) -> Self {
        self.twist = twist;
        self
    }
}

impl Pack for TestPack {
    fn manifest(&self) -> &'static str {
        self.manifest
    }

    fn register(&self, cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        *self.seen_settings.lock().unwrap() = Some(cx.settings().clone());
        *self.context.lock().unwrap() = Some(cx.clone());
        let surface_kind = SurfaceKind::new(&format!("{}_surface", self.name)).unwrap();
        let driver = Arc::new(StubDriver {
            reached: Arc::clone(&self.reached),
            behaviour: Arc::clone(&self.behaviour),
        });
        match self.twist {
            Twist::None => to.driver(mode(), driver)?,
            Twist::UndeclaredDriver => to.driver(Mode::new("other").unwrap(), driver)?,
            Twist::NoDriver => {}
        }
        to.compiler(
            CompilerRegistrations::new()
                .surface_kind(SurfaceKindSpec {
                    kind: surface_kind.clone(),
                    address_key: "address".to_owned(),
                    fields: JsonSchema::object().required("address", JsonSchema::text()),
                    programs: Vec::new(),
                    notices: Vec::new(),
                })
                .file_kind(Arc::new(CallKind {
                    name: FileKindName::new(&format!("{}_call", self.name)).unwrap(),
                    folder: format!("{}_calls", self.name),
                    kinds: vec![surface_kind],
                    schema: JsonSchema::object().required("call", JsonSchema::name()),
                })),
        )?;
        if self.twist != Twist::NoDriver {
            to.compiler(
                CompilerRegistrations::new().lowering(Arc::new(StandLowering {
                    verbs: vec![Verb::new("read").unwrap(), Verb::new("upsert").unwrap()],
                    steps: vec![step_kind("fetch"), step_kind("send")],
                    schema: JsonSchema::object().required("call", JsonSchema::name()),
                })),
            )?;
        }
        to.discoverer(self.discoverer.clone(), Arc::new(StubDescriber))?;
        to.auth_provider(Arc::new(StubAuth(format!("{}_login", self.name))))?;
        Ok(())
    }
}

pub fn failure(class: FailureClass, code: &'static str) -> Failure {
    Failure::new(class, FailureCode::fixed(code), "It stopped.", "Look.")
}

/// A finding of the kind and level given, observed on the live system.
pub fn finding(kind: &str, level: a2m_core::EvidenceLevel) -> Finding {
    Finding::about(
        a2m_core::Subject::Address(
            a2m_core::Address::new("https://example.test", "/orders").unwrap(),
        ),
        a2m_core::FindingKind::new(kind).unwrap(),
        "GET /orders",
        a2m_core::Source::LiveSystem,
        a2m_core::ProducedBy {
            provider: "alpha".to_owned(),
            version: "0.1.0".to_owned(),
            component: None,
        },
        Timestamp::from_unix_millis(1_800_000_000_000),
    )
    .level(level)
    .build()
    .unwrap()
}
