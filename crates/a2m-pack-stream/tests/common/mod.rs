//! What the tests share: a folder of the test's own, a registry that holds
//! only the stream pack, the example target compiled through it, and a
//! redactor that knows one planted secret.

#![allow(
    dead_code,
    reason = "each test binary uses some of these helpers and not others"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use a2m_core::{NoSecrets, Plan, REDACTED, Redact, Version};
use a2m_files::{CompileMode, Compiled, Diagnostics, TargetRoot};
use a2m_provider::{
    HostServices, ProviderSettings, Registry, RegistryBuilder, Roots, Supervisor, SupervisorLimits,
};
use a2m_telemetry::TelemetrySettings;

/// A folder under the system's temporary folder, removed when dropped.
pub struct Folder(PathBuf);

impl Folder {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "a2m-pack-stream-{name}-{}-{}",
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

/// A secret a test plants in what the broker says.
pub const PLANTED: &str = "hunter2-planted-secret";

/// A redactor that knows [`PLANTED`], as the vault's would know a
/// registered secret.
pub struct Planted;

impl Redact for Planted {
    fn redact(&self, text: &str) -> String {
        text.replace(PLANTED, REDACTED)
    }
}

fn host(folder: &Folder, redact: Arc<dyn Redact>) -> HostServices {
    let logs = folder.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    HostServices {
        supervisor: Supervisor::new(SupervisorLimits::default()),
        roots: Roots::none(),
        redact: Arc::clone(&redact),
        telemetry: a2m_telemetry::start(&TelemetrySettings::default(), Arc::new(NoSecrets), &logs)
            .unwrap(),
    }
}

/// A registry that holds the stream pack and nothing else.
pub fn registry_with(
    folder: &Folder,
    settings: ProviderSettings,
    redact: Arc<dyn Redact>,
) -> Registry {
    let mut builder = RegistryBuilder::new(settings, host(folder, redact));
    a2m_pack_stream::register(&mut builder);
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    registry
}

pub fn registry(folder: &Folder) -> Registry {
    registry_with(folder, ProviderSettings::default(), Arc::new(Planted))
}

/// The example target of this crate's tests.
pub fn fixture_root() -> TargetRoot {
    TargetRoot::new(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("example"),
    )
}

pub fn compile(registry: &Registry, root: &TargetRoot) -> Result<Compiled, Diagnostics> {
    let set = registry.compiler_set();
    let sources = a2m_files::load(root, set)?;
    // As a draft: the rules about tests of each tool are warnings. The
    // example target of this crate holds no offline fixtures.
    a2m_files::compile(&sources, set, CompileMode::Draft, Version::new(1, 0, 0))
}

pub fn plan(registry: &Registry) -> Plan {
    match compile(registry, &fixture_root()) {
        Ok(compiled) => compiled.plan,
        Err(problems) => panic!("the example target does not compile:\n{problems}"),
    }
}

/// A copy of the example target in `folder`, with one file replaced or
/// added.
pub fn edited(folder: &Folder, file: &str, text: &str) -> TargetRoot {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.path().is_dir() {
                copy(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let root = folder.path().join("target");
    let _ = std::fs::remove_dir_all(&root);
    copy(fixture_root().path(), &root);
    let mut path = root.clone();
    for part in file.split('/') {
        path = path.join(part);
    }
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    TargetRoot::new(root)
}

// ---- a run against the test broker ----

use std::time::Duration;

use a2m_core::{
    ApprovalId, Digest, EffectClass, Environment, Epoch, Failure, OperationName, PrincipalId,
    RecordKey, RunId, RunState, Sensitivity, TargetName, Timestamp, Value, Values, WorkspaceId,
};
use a2m_ledger::{
    Durability, Fence, IntentToken, NewRun, Opened, Owner, PlannedRecord, RecordIntent, RecordRef,
    RetentionClass, Store, StoreConfig, ToolKind, WriteClearance,
};
use a2m_provider::{
    Authority, CheckedPaths, Deadline, Destinations, DriverHandle, Posture, PrepareContext,
    Prepared, StepFailure, StepInput, StepOutcome, SurfaceRef,
};
use a2m_telemetry::{
    CaptureLevel, ManualClock, Phase, StepRecord, TraceBuilder, TraceIdentity, TraceOutcome,
    TracedValue,
};
use a2m_vault::{MemoryKeyStore, RandomSource, Vault};
use example_broker::harness::TestBroker;

pub fn mode() -> a2m_core::Mode {
    a2m_core::Mode::new("stream").unwrap()
}

/// The registry, the compiled example target, and a test broker the
/// target's one surface points at.
pub struct Scene {
    pub folder: Folder,
    pub registry: Registry,
    pub plan: Plan,
    pub broker: TestBroker,
    pub surface: SurfaceRef,
    pub destinations: Destinations,
}

impl Scene {
    pub async fn start() -> Self {
        let folder = Folder::new("scene");
        let registry = registry(&folder);
        let plan = plan(&registry);
        let broker = TestBroker::start().await.unwrap();
        // The surface, at the address of the run's environment: here, the
        // test broker's own.
        let mut surface = plan.surfaces[0].clone();
        surface.address = broker.address().to_string();
        let destinations = Destinations::new([surface.address.clone()]);
        Self {
            surface: SurfaceRef {
                target: plan.header.target.clone(),
                environment: Environment::new("test").unwrap(),
                surface,
            },
            folder,
            registry,
            plan,
            broker,
            destinations,
        }
    }

    pub fn handle(&self) -> DriverHandle {
        self.registry.driver(&mode()).unwrap()
    }

    /// The one binding of an operation of the example target.
    pub fn binding(&self, operation: &str) -> &a2m_core::BindingPlan {
        &self
            .plan
            .operations
            .iter()
            .find(|found| found.name.as_str() == operation)
            .unwrap_or_else(|| panic!("no operation `{operation}`"))
            .bindings[0]
    }

    pub async fn try_prepare(
        &self,
        posture: Posture,
        fence: Option<Fence>,
    ) -> Result<(DriverHandle, Prepared), StepFailure> {
        let handle = self.handle();
        let fence = fence.unwrap_or(Fence {
            run: RunId::from_bytes([7; 16]),
            epoch: Epoch::new(1),
        });
        let supervisor = handle.supervisor(Vec::new());
        let cx = PrepareContext {
            run: fence.run,
            epoch: fence.epoch,
            surface: &self.surface,
            plan: &self.plan,
            posture,
            deadline: Deadline::after(Duration::from_secs(20)),
            redact: &Planted,
            destinations: &self.destinations,
            paths: &CheckedPaths::none(),
            supervisor: &supervisor,
        };
        let prepared = handle.prepare(&cx, None).await?;
        Ok((handle, prepared))
    }

    /// A prepared live run that reads.
    pub async fn reading(&self) -> (DriverHandle, Prepared) {
        self.try_prepare(Posture::Live, None).await.unwrap()
    }
}

pub fn input<'a>(args: &'a Values) -> StepInput<'a> {
    StepInput {
        args,
        record: None,
        key: None,
        stamp: None,
        deadline: Deadline::after(Duration::from_secs(20)),
    }
}

/// Carries out the steps of a list that reads, in order, each with the
/// same arguments, and gives the last outcome.
pub async fn read(
    handle: &DriverHandle,
    prepared: &mut Prepared,
    steps: &[a2m_core::Step],
    args: &Values,
) -> Result<StepOutcome, StepFailure> {
    let mut last = None;
    for step in steps {
        last = Some(
            handle
                .act(prepared, step, &input(args), Authority::Read)
                .await?,
        );
    }
    Ok(last.expect("a binding has a step"))
}

pub fn done(outcome: StepOutcome) -> Values {
    match outcome {
        StepOutcome::Done(observed) => observed.values,
        StepOutcome::Absent(_) => panic!("the record is absent"),
    }
}

pub fn text<'a>(values: &'a Values, name: &str) -> &'a str {
    values
        .get(name)
        .and_then(Value::as_text)
        .unwrap_or_else(|| panic!("no text `{name}` in {values:?}"))
}

pub fn record<'a>(values: &'a Values, name: &str) -> &'a Values {
    values
        .get(name)
        .and_then(Value::as_record)
        .unwrap_or_else(|| panic!("no record `{name}` in {values:?}"))
}

/// The class, the code, and whether anything was sent.
pub fn failed(result: Result<StepOutcome, StepFailure>) -> (String, String, a2m_provider::Sent) {
    let failure = result.expect_err("the step did not fail");
    (
        failure.failure.class().to_string(),
        failure.failure.code().to_string(),
        failure.sent,
    )
}

// ---- a real store, for real intent tokens ----

struct Counted(AtomicU64);

impl RandomSource for Counted {
    fn fill(&self, out: &mut [u8]) -> Result<(), Failure> {
        for chunk in out.chunks_mut(Digest::LENGTH) {
            let number = self.0.fetch_add(1, Ordering::SeqCst);
            let block = Digest::of(&number.to_le_bytes());
            chunk.copy_from_slice(&block.as_bytes()[..chunk.len()]);
        }
        Ok(())
    }
}

fn counted() -> Arc<Counted> {
    Arc::new(Counted(AtomicU64::new(0)))
}

/// A ledger in memory. What it gives is what the engine would hold when
/// it sends a write: a running run, and the intent token of one record.
pub struct Ledger {
    store: Store,
    callers: AtomicU64,
}

pub fn key(name: &str) -> RecordKey {
    RecordKey::new(vec![Value::from(name)]).unwrap()
}

/// One record a write is about to be sent for.
pub struct Write {
    pub fence: Fence,
    pub record: RecordRef,
    pub key: RecordKey,
    pub token: IntentToken,
}

impl Write {
    pub fn input<'a>(&'a self, args: &'a Values, stamp: Option<&'a str>) -> StepInput<'a> {
        StepInput {
            args,
            record: Some(self.record),
            key: Some(&self.key),
            stamp,
            deadline: Deadline::after(Duration::from_secs(20)),
        }
    }

    pub fn authority(&self) -> Authority<'_> {
        Authority::Write(&self.token)
    }
}

impl Ledger {
    pub async fn open() -> Self {
        let config = StoreConfig::in_memory()
            .with_clock(Arc::new(ManualClock::starting_at(
                Timestamp::from_unix_millis(1_800_000_000_000),
            )))
            .with_random(counted());
        let store = Store::open(config).await.unwrap();
        let vault = Arc::new(Vault::with_random(
            Arc::new(MemoryKeyStore::with_random(counted())),
            Arc::new(store.key_records()),
            counted(),
        ));
        store.attach(vault).await.unwrap();
        Self {
            store,
            callers: AtomicU64::new(1),
        }
    }

    /// A run that is planned, approved, and running, with the intent of
    /// its one record stored: the moment before a write is sent.
    pub async fn write(&self, name: &str) -> Write {
        let caller = u8::try_from(self.callers.fetch_add(1, Ordering::SeqCst)).unwrap();
        let record_key = key(name);
        let planned = record_key.clone();
        let intended = Digest::of(format!("intended {name}").as_bytes());
        let workspace = WorkspaceId::from_bytes([1; 16]);
        let input_hash = Digest::of(format!("{name} {caller}").as_bytes());
        let fence = self
            .store
            .write(Durability::Synced, move |tx| {
                let diff = Digest::of(b"diff");
                let fence = {
                    let mut ledger = tx.ledger();
                    let Opened::New(opened) = ledger.open_run(NewRun {
                        workspace,
                        principal: PrincipalId::from_bytes([caller; 16]),
                        client_label: "test".to_owned(),
                        tool_name: OperationName::new("example_order_status_set").unwrap(),
                        tool_kind: ToolKind::Operation,
                        target: TargetName::new("example").unwrap(),
                        target_version: Digest::of(b"plan"),
                        environment: Environment::new("test").unwrap(),
                        identity: None,
                        effect: EffectClass::Write,
                        input_hash,
                        input: None,
                        retention: RetentionClass::Long,
                    })?
                    else {
                        panic!("the run exists already");
                    };
                    let fence = ledger.new_epoch(opened.run)?;
                    ledger.set_state(&fence, RunState::Validating, None)?;
                    ledger.plan_records(
                        &fence,
                        &[PlannedRecord {
                            step_seq: 0,
                            record_seq: 0,
                            key: planned,
                            operation: OperationName::new("example_order_status_set").unwrap(),
                            intended,
                            intended_values: None,
                            before: None,
                            unchanged: false,
                        }],
                    )?;
                    ledger.store_diff(&fence, diff, None)?;
                    ledger.set_state(&fence, RunState::NeedsApproval, None)?;
                    fence
                };
                // The policy engine's stand-in: an approval, used once.
                let clearance = a2m_ledger::testing::approve(tx, &fence)?;
                tx.ledger().begin_writes(&fence, clearance)?;
                Ok(fence)
            })
            .await
            .unwrap();
        let record = RecordRef {
            run: fence.run,
            step_seq: 0,
            record_seq: 0,
        };
        self.store
            .write(Durability::Synced, move |tx| {
                tx.ledger().take_lock(&fence, record).map(|_| ())
            })
            .await
            .unwrap();
        let token = self
            .store
            .store_intents(
                &fence,
                vec![RecordIntent {
                    record,
                    intended,
                    mode: None,
                }],
            )
            .await
            .unwrap()
            .remove(0);
        Write {
            fence,
            record,
            key: record_key,
            token,
        }
    }
}

// ---- the trace signature of what a driver returned ----

struct TestKey;

impl a2m_core::KeyedDigest for TestKey {
    fn keyed_digest(
        &self,
        workspace: &WorkspaceId,
        purpose: a2m_core::DigestPurpose,
        bytes: &[u8],
    ) -> Digest {
        a2m_core::keyed_digest_under(b"a test key", workspace, purpose, bytes)
    }
}

/// The trace signature of one step that read `values`, as the engine's
/// trace would hold them.
pub fn signature_of(kind: &a2m_core::StepKind, values: &Values) -> Digest {
    let mut trace = TraceBuilder::new(
        TraceIdentity {
            workspace: WorkspaceId::from_bytes([1; 16]),
            run: RunId::from_bytes([7; 16]),
            attempt: 1,
            operation: OperationName::new("example_order_status_get").unwrap(),
            plan: Digest::of(b"plan"),
            engine: Version::new(0, 0, 1),
            registry: Digest::of(b"registry"),
        },
        CaptureLevel::Standard,
        Arc::new(Planted),
        Arc::new(TestKey),
    );
    trace.record(StepRecord {
        pass: 0,
        phase: Phase::Act,
        step: 0,
        kind: kind.clone(),
        record: None,
        begins: None,
        took: Duration::from_millis(3),
        locating: None,
        acting: None,
        values: values
            .iter()
            .map(|(name, value)| {
                TracedValue::read(name, Sensitivity::Internal, value.clone(), value.clone())
            })
            .collect(),
        outcome: TraceOutcome::Done,
        artifacts: Vec::new(),
    });
    trace.finish().unwrap().signature
}
