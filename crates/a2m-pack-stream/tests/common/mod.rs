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

use a2m_core::{CompiledTarget, NoSecrets, REDACTED, Redact, Version};
use a2m_files::{CompileMode, Compiled, Diagnostics, TargetRoot};
use a2m_logs::LogSettings;
use a2m_provider::{
    HostServices, ProviderSettings, Registry, RegistryBuilder, Roots, Supervisor, SupervisorLimits,
};

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
        logs_and_traces: a2m_logs::start(&LogSettings::default(), Arc::new(NoSecrets), &logs)
            .unwrap(),
    }
}

/// A registry that holds the stream pack and nothing else, registered as a
/// test registers it: with the test broker's protocol.
pub fn registry_with(
    folder: &Folder,
    settings: ProviderSettings,
    redact: Arc<dyn Redact>,
) -> Registry {
    let mut builder = RegistryBuilder::new(settings, host(folder, redact));
    a2m_pack_stream::register_with_test_broker(&mut builder);
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
    a2m_files::compile(&sources, set, CompileMode::Draft, a2m_files::DRAFT_VERSION)
}

pub fn compiled_target(registry: &Registry) -> CompiledTarget {
    match compile(registry, &fixture_root()) {
        Ok(compiled) => compiled.compiled_target,
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
    Digest, EffectClass, Environment, Epoch, Failure, OperationName, PrincipalId, RecordKey, RunId,
    RunState, Sensitivity, TargetName, Timestamp, Value, Values, WorkspaceId,
};
use a2m_ledger::{
    Durability, Fence, IntentToken, NewRun, Opened, PlannedRecord, RecordIntent, RecordRef,
    RetentionClass, Store, StoreConfig, ToolKind,
};
use a2m_logs::{
    CaptureLevel, ManualClock, Phase, StepRecord, TraceBuilder, TraceIdentity, TraceOutcome,
    TracedValue,
};
use a2m_provider::{
    Authority, CheckedPaths, Deadline, Destinations, DriverHandle, Posture, PrepareContext,
    Prepared, StepFailure, StepInput, StepOutcome, SurfaceRef,
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
    pub compiled_target: CompiledTarget,
    pub broker: TestBroker,
    pub surface: SurfaceRef,
    pub destinations: Destinations,
}

impl Scene {
    pub async fn start() -> Self {
        let folder = Folder::new("scene");
        let registry = registry(&folder);
        let compiled_target = compiled_target(&registry);
        let broker = TestBroker::start().await.unwrap();
        // The surface, at the address of the run's environment: here, the
        // test broker's own.
        let mut surface = compiled_target.surfaces[0].clone();
        surface.address = broker.address().to_string();
        let destinations = Destinations::new([surface.address.clone()]);
        Self {
            surface: SurfaceRef {
                target: compiled_target.header.target.clone(),
                environment: Environment::new("test").unwrap(),
                surface,
            },
            folder,
            registry,
            compiled_target,
            broker,
            destinations,
        }
    }

    pub fn handle(&self) -> DriverHandle {
        self.registry.driver(&mode()).unwrap()
    }

    /// The one binding of an operation of the example target.
    pub fn binding(&self, operation: &str) -> &a2m_core::CompiledBinding {
        &self
            .compiled_target
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
        self.try_prepare_in(&self.registry, posture, fence).await
    }

    /// Prepared by the driver of `registry`: another engine's, when it is
    /// not the scene's own.
    pub async fn try_prepare_in(
        &self,
        registry: &Registry,
        posture: Posture,
        fence: Option<Fence>,
    ) -> Result<(DriverHandle, Prepared), StepFailure> {
        let handle = registry.driver(&mode()).unwrap();
        let fence = fence.unwrap_or(Fence {
            run: RunId::from_bytes([7; 16]),
            epoch: Epoch::new(1),
        });
        let supervisor = handle.supervisor(Vec::new());
        let cx = PrepareContext {
            run: fence.run,
            epoch: fence.epoch,
            surface: &self.surface,
            compiled_target: &self.compiled_target,
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
    StepInput::new(args, Deadline::after(Duration::from_secs(20)))
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

/// What a read returns through the engine: each output the operation
/// declares, taken by its name from what the last step observed and
/// normalized by its field type, as the engine's own shaping does. A value
/// the step gave that no output declares is not returned; an output the
/// step gave no value for stops the test, as it stops a read in the engine.
pub fn outputs_of(compiled_target: &CompiledTarget, operation: &str, observed: &Values) -> Values {
    let operation = compiled_target
        .operations
        .iter()
        .find(|found| found.name.as_str() == operation)
        .unwrap();
    let mut out = Values::new();
    for output in &operation.outputs {
        let raw = observed
            .get(&output.name)
            .unwrap_or_else(|| panic!("no value for the output `{}`", output.name));
        let value = match output
            .field_type
            .and_then(|index| compiled_target.field_types[index.get()].normalizer)
        {
            Some(index) => {
                let normalizer =
                    a2m_expr::Normalizer::load(compiled_target.expression(index).unwrap()).unwrap();
                let one = |value: &Value| normalizer.normalize(value).unwrap().normalized;
                // A list output is normalized value by value; the engine
                // hands a normalizer the whole list (decision 0182).
                match raw {
                    Value::List(values) if matches!(output.ty, a2m_core::ValueType::List(_)) => {
                        Value::List(values.iter().map(one).collect())
                    }
                    other => one(other),
                }
            }
            None => raw.clone(),
        };
        assert!(
            value.is_null() || value.has_type(&output.ty),
            "the output `{}` is not a {}: {value:?}",
            output.name,
            output.ty.name()
        );
        out.insert(output.name.clone(), value);
    }
    out
}

/// A drafted target folder with the one judgment a drafted read can need
/// answered as a person who knows the example's messages would: a key the
/// broker does not name for a retained channel is the order's number. No
/// read needs the channel's `role`, and no drafted file asks it.
pub fn answer_keys(root: &Path) {
    let channels = root.join("channels");
    for entry in std::fs::read_dir(&channels).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("role:"), "{text}");
        let answered: Vec<String> = text
            .lines()
            .map(|line| {
                if line.starts_with("key: {open_question") {
                    "key:\n  field: \"order_id\"".to_owned()
                } else {
                    line.to_owned()
                }
            })
            .collect();
        std::fs::write(&path, answered.join("\n") + "\n").unwrap();
    }
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
        StepInput::new(args, Deadline::after(Duration::from_secs(20))).for_record(
            self.record,
            Some(&self.key),
            stamp,
        )
    }

    /// The input of a step of a record that has no key in the system, such
    /// as a take: the engine hands such a step no key (decision 0161).
    pub fn keyless<'a>(&'a self, args: &'a Values) -> StepInput<'a> {
        StepInput::new(args, Deadline::after(Duration::from_secs(20))).for_record(
            self.record,
            None,
            None,
        )
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
    signature_with(kind, values, None)
}

/// The same, with how the step was done, as the engine puts a step's
/// `acting` capture into its trace record.
pub fn signature_with(
    kind: &a2m_core::StepKind,
    values: &Values,
    acting: Option<a2m_logs::Acting>,
) -> Digest {
    let mut trace = TraceBuilder::new(
        TraceIdentity {
            workspace: WorkspaceId::from_bytes([1; 16]),
            run: RunId::from_bytes([7; 16]),
            attempt: 1,
            operation: OperationName::new("example_order_status_get").unwrap(),
            compiled_target: Digest::of(b"plan"),
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
        acting,
        values: values
            .iter()
            .map(|(name, value)| {
                TracedValue::read(name, Sensitivity::Internal, value.clone(), value.clone())
            })
            .collect(),
        outcome: TraceOutcome::Done,
        captures: Vec::new(),
    });
    trace.finish().unwrap().signature
}

// ---- the engine's answer about another record (decision 0193) ----

/// The pack's test control of the engine's answer to "does another record
/// hold this value": one fixed answer, and every question it was asked.
pub struct Answering {
    answer: a2m_provider::RecordedElsewhere,
    pub asked: std::sync::Mutex<Vec<(String, Digest)>>,
}

impl Answering {
    pub fn new(answer: a2m_provider::RecordedElsewhere) -> Arc<Self> {
        Arc::new(Self {
            answer,
            asked: std::sync::Mutex::new(Vec::new()),
        })
    }
}

impl a2m_provider::OtherRecords for Answering {
    fn holding<'a>(
        &'a self,
        name: &'a str,
        value: Digest,
    ) -> a2m_provider::BoxFuture<'a, a2m_provider::RecordedElsewhere> {
        self.asked.lock().unwrap().push((name.to_owned(), value));
        let answer = self.answer;
        Box::pin(async move { answer })
    }
}

/// The first two steps of a take, for `write`'s record, its wait through
/// the engine's answerer `others`, as the engine acts a step of a record.
pub async fn take_asking(
    handle: &DriverHandle,
    prepared: &mut Prepared,
    take: &[a2m_core::Step],
    write: &Write,
    others: Arc<Answering>,
) -> Result<StepOutcome, StepFailure> {
    let none = Values::new();
    handle
        .act(prepared, &take[0], &input(&none), Authority::Read)
        .await?;
    handle
        .act_asking(
            prepared,
            &take[1],
            &write.keyless(&none),
            Authority::Read,
            &Values::new(),
            others,
        )
        .await
}
