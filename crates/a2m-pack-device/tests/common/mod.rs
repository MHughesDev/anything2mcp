//! What the tests share: a registry that holds only the device pack, the
//! example target compiled with it, a simulated device, a real store for
//! real intent tokens, and a helper that carries a list of steps out
//! through the driver handle as the engine would.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "each test binary uses some of these helpers and not others; a helper that cannot do its work stops the test"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use a2m_core::{
    CompiledBinding, CompiledTarget, Digest, EffectClass, Environment, Epoch, Failure,
    OperationName, PackPayload, PrincipalId, RecordKey, Redact, RunId, RunState, Step, TargetName,
    Timestamp, Value, Values, Version, WorkspaceId,
};
use a2m_expr::{Bindings, Normalizer, Program};
use a2m_files::{CompileMode, Compiled, CompilerSet, Diagnostics, RelPath, TargetRoot};
use a2m_ledger::{
    Durability, Fence, IntentToken, NewRun, Opened, PlannedRecord, RecordIntent, RecordRef,
    RetentionClass, Store, StoreConfig, ToolKind,
};
use a2m_logs::{LogSettings, ManualClock};
use a2m_provider::{
    Authority, Capture, CheckedPaths, Deadline, Destinations, DriverHandle, HostServices, Posture,
    PrepareContext, Prepared, ProviderName, ProviderSettings, Registry, RegistryBuilder, Roots,
    StepFailure, StepInput, StepOutcome, Supervisor, SupervisorLimits, SurfaceRef,
};
use a2m_vault::{MemoryKeyStore, RandomSource, Vault};
use example_device::TestDevice;

pub const VERSION: Version = Version::new(1, 0, 0);

/// A folder under the system's temporary folder, removed when dropped.
pub struct Folder(PathBuf);

impl Folder {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "a2m-pack-device-{name}-{}-{}",
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

/// A redactor with one planted secret.
pub struct Planted(pub &'static str);

impl Redact for Planted {
    fn redact(&self, text: &str) -> String {
        text.replace(self.0, a2m_core::REDACTED)
    }
}

/// A registry that holds the device pack and nothing else.
pub fn registry_with(folder: &Folder, redact: Arc<dyn Redact>) -> Registry {
    registry_set(folder, redact, None)
}

/// The same, with the pack's own settings group as a person wrote it.
pub fn registry_set(
    folder: &Folder,
    redact: Arc<dyn Redact>,
    settings: Option<Values>,
) -> Registry {
    let logs = folder.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let host = HostServices {
        supervisor: Supervisor::new(SupervisorLimits::default()),
        roots: Roots::none(),
        redact: Arc::clone(&redact),
        logs_and_traces: a2m_logs::start(&LogSettings::default(), redact, &logs).unwrap(),
    };
    let mut provider = ProviderSettings::default();
    if let Some(settings) = settings {
        provider
            .packs
            .insert(ProviderName::new("device").unwrap(), settings);
    }
    let mut builder = RegistryBuilder::new(provider, host);
    a2m_pack_device::add_to(&mut builder);
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    registry
}

/// A registry that holds the device pack and the optional provider that
/// inspects firmware images.
pub fn registry_inspecting(folder: &Folder) -> Registry {
    let redact: Arc<dyn Redact> = Arc::new(a2m_core::NoSecrets);
    let logs = folder.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let host = HostServices {
        supervisor: Supervisor::new(SupervisorLimits::default()),
        roots: Roots::none(),
        redact: Arc::clone(&redact),
        logs_and_traces: a2m_logs::start(&LogSettings::default(), redact, &logs).unwrap(),
    };
    let mut builder = RegistryBuilder::new(ProviderSettings::default(), host);
    a2m_pack_device::add_to(&mut builder);
    a2m_pack_device::add_firmware_inspection_to(&mut builder);
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    registry
}

pub fn registry(folder: &Folder) -> Registry {
    registry_with(folder, Arc::new(a2m_core::NoSecrets))
}

/// The folder of the example target written for this pack.
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("example")
}

/// Every file of the example target, as bytes, with line feeds.
pub fn files() -> Vec<(RelPath, Vec<u8>)> {
    fn walk(folder: &Path, prefix: &str, out: &mut Vec<(RelPath, Vec<u8>)>) {
        let mut entries: Vec<_> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap())
            .collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if entry.path().is_dir() {
                walk(&entry.path(), &path, out);
            } else {
                let text = String::from_utf8(std::fs::read(entry.path()).unwrap()).unwrap();
                out.push((
                    RelPath::new(&path).unwrap(),
                    text.replace("\r\n", "\n").into_bytes(),
                ));
            }
        }
    }
    let mut files = Vec::new();
    walk(&fixtures(), "", &mut files);
    files
}

/// The example target with one edit: `find` replaced by `replace` in `path`.
pub fn edited(path: &str, find: &str, replace: &str) -> Vec<(RelPath, Vec<u8>)> {
    let mut files = files();
    let file = files
        .iter_mut()
        .find(|(known, _)| known.as_str() == path)
        .unwrap();
    let text = String::from_utf8(file.1.clone()).unwrap();
    assert!(text.contains(find), "`{find}` is not in {path}");
    file.1 = text.replacen(find, replace, 1).into_bytes();
    files
}

pub fn compile_files(
    files: &[(RelPath, Vec<u8>)],
    set: &CompilerSet,
    mode: CompileMode,
) -> Result<Compiled, Diagnostics> {
    let root = TargetRoot::new("example");
    let sources = a2m_files::load_files(&root, files, set)?;
    // A draft is compiled under the draft version: under a release version
    // it would have a published compiled target's bytes.
    let version = if matches!(mode, CompileMode::Draft) {
        a2m_files::DRAFT_VERSION
    } else {
        VERSION
    };
    a2m_files::compile(&sources, set, mode, version)
}

/// A target folder on disk, compiled as a draft; a test stops if it does
/// not compile.
pub fn compile_folder(root: &Path, set: &CompilerSet) -> CompiledTarget {
    let sources = a2m_files::load(&TargetRoot::new(root), set)
        .unwrap_or_else(|problems| panic!("{problems}"));
    a2m_files::compile(&sources, set, CompileMode::Draft, a2m_files::DRAFT_VERSION)
        .unwrap_or_else(|problems| panic!("{problems}"))
        .compiled_target
}

/// The example target, compiled as a draft: publishing also asks for an
/// offline test of every operation, and a fixture of recorded frames is
/// not built.
pub fn compiled(set: &CompilerSet) -> Compiled {
    compile_files(&files(), set, CompileMode::Draft).unwrap_or_else(|problems| panic!("{problems}"))
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

pub async fn store() -> Store {
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
    store
}

pub fn key(name: &str) -> RecordKey {
    RecordKey::new(vec![Value::from(name)]).unwrap()
}

fn intended(name: &str) -> Digest {
    Digest::of(format!("intended {name}").as_bytes())
}

/// A write run with one record planned, approved, and running, and the
/// intent token the ledger issued for that record.
pub async fn write_run(store: &Store, caller: u8, name: &str) -> (Fence, RecordRef, IntentToken) {
    let record_name = name.to_owned();
    let workspace = WorkspaceId::from_bytes([1; 16]);
    let fence = store
        .write(Durability::Synced, move |tx| {
            let diff = Digest::of(b"diff");
            let fence = {
                let mut ledger = tx.ledger();
                let Opened::New(opened) = ledger.open_run(NewRun {
                    workspace,
                    principal: PrincipalId::from_bytes([caller; 16]),
                    client_label: "test".to_owned(),
                    tool_name: OperationName::new("example_dosing_rate_set").unwrap(),
                    tool_kind: ToolKind::Operation,
                    target: TargetName::new("example").unwrap(),
                    target_version: Digest::of(b"plan"),
                    environment: Environment::new("test").unwrap(),
                    identity: None,
                    effect: EffectClass::Write,
                    input_hash: Digest::of(record_name.as_bytes()),
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
                        key: key(&record_name),
                        operation: OperationName::new("example_dosing_rate_set").unwrap(),
                        intended: intended(&record_name),
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
    store
        .write(Durability::Synced, move |tx| {
            tx.ledger().take_lock(&fence, record).map(|_| ())
        })
        .await
        .unwrap();
    let token = store
        .store_intents(
            &fence,
            vec![RecordIntent {
                record,
                intended: intended(name),
                mode: None,
            }],
        )
        .await
        .unwrap()
        .remove(0);
    (fence, record, token)
}

// ---- the compiled target, the registry, and one simulated device ----

pub struct Scene {
    _folder: Folder,
    pub registry: Registry,
    pub compiled_target: CompiledTarget,
    pub device: TestDevice,
    pub surface: SurfaceRef,
    pub destinations: Destinations,
}

/// What a list of steps gave, taken together.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Ran {
    pub values: Values,
    pub for_reconcile: Values,
    pub captures: Vec<Capture>,
    pub kinds: Vec<String>,
    /// How each step was done, from its `acting` capture, in order.
    pub acting: Vec<a2m_logs::Acting>,
}

/// The right to write one record, as the engine holds it.
pub struct Writing<'a> {
    pub token: &'a IntentToken,
    pub record: RecordRef,
}

impl Scene {
    pub async fn start() -> Self {
        Self::start_with(Arc::new(a2m_core::NoSecrets)).await
    }

    pub async fn start_with(redact: Arc<dyn Redact>) -> Self {
        Self::start_set(redact, None).await
    }

    /// A scene whose registry was built with the pack's settings given.
    pub async fn start_set(redact: Arc<dyn Redact>, settings: Option<Values>) -> Self {
        let folder = Folder::new("scene");
        let registry = registry_set(&folder, redact, settings);
        let compiled_target = compiled(registry.compiler_set()).compiled_target;
        let device = TestDevice::start().await.unwrap();
        // The compiled target's surface, at the address of the run's
        // environment: here the address the simulated device chose.
        let mut surface = compiled_target.surfaces[0].clone();
        surface.address = device.address().to_string();
        let destinations = Destinations::new([surface.address.clone()]);
        Self {
            _folder: folder,
            registry,
            surface: SurfaceRef {
                target: compiled_target.header.target.clone(),
                environment: Environment::new("test").unwrap(),
                surface,
            },
            compiled_target,
            device,
            destinations,
        }
    }

    /// Declares another serial for the device of the environment `test`
    /// than the target's files do, or none: what the surface's fields
    /// would compile to.
    pub fn declare_serial(&mut self, serial: Option<&str>) {
        let mut fields = serde_json::json!({"protocol": "modbus_tcp", "unit": 1});
        if let Some(serial) = serial {
            fields["serials"] = serde_json::json!({"test": serial});
        }
        self.surface.surface.payload = PackPayload::encode(&fields).unwrap();
    }

    pub fn handle(&self) -> DriverHandle {
        self.registry
            .driver(&a2m_core::Mode::new("device").unwrap())
            .unwrap()
    }

    pub async fn try_prepare(
        &self,
        fence: Option<Fence>,
        posture: Posture,
        destinations: &Destinations,
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
            compiled_target: &self.compiled_target,
            posture,
            deadline: Deadline::after(Duration::from_secs(30)),
            redact: &a2m_core::NoSecrets,
            destinations,
            paths: &CheckedPaths::none(),
            supervisor: &supervisor,
        };
        let prepared = handle.prepare(&cx, None).await?;
        Ok((handle, prepared))
    }

    pub async fn prepare(
        &self,
        fence: Option<Fence>,
        posture: Posture,
    ) -> (DriverHandle, Prepared) {
        self.try_prepare(fence, posture, &self.destinations)
            .await
            .unwrap()
    }

    /// The first binding of an operation.
    pub fn binding(&self, operation: &str) -> &CompiledBinding {
        &self
            .compiled_target
            .operations
            .iter()
            .find(|found| found.name.as_str() == operation)
            .unwrap_or_else(|| panic!("no operation `{operation}`"))
            .bindings[0]
    }

    /// The values of a step's arguments, from the compiled target's
    /// expressions and the call's inputs, as the engine evaluates them.
    fn args(&self, step: &Step, inputs: &Values) -> Values {
        let input = Value::Record(inputs.clone());
        let mut args = Values::new();
        for arg in &step.args {
            let program =
                Program::load(&self.compiled_target.expressions[arg.value.get()]).unwrap();
            let bindings = Bindings::new().with("input", &input);
            args.insert(arg.name.clone(), program.evaluate(&bindings).unwrap());
        }
        args
    }

    /// Carries out a list of steps in order, stopping at the first failure.
    pub async fn run(
        &self,
        handle: &DriverHandle,
        prepared: &mut Prepared,
        steps: &[Step],
        inputs: &Values,
        writing: Option<&Writing<'_>>,
    ) -> Result<Ran, StepFailure> {
        self.run_recorded(handle, prepared, steps, inputs, writing, &Values::new())
            .await
    }

    /// The same, with what the record's earlier steps gave for reconcile, as
    /// the engine hands it on, within a pass and on a resume.
    pub async fn run_recorded(
        &self,
        handle: &DriverHandle,
        prepared: &mut Prepared,
        steps: &[Step],
        inputs: &Values,
        writing: Option<&Writing<'_>>,
        recorded: &Values,
    ) -> Result<Ran, StepFailure> {
        let mut ran = Ran::default();
        for step in steps {
            let args = self.args(step, inputs);
            // A register has no key from an input: the record has none.
            let mut input = StepInput::new(
                &args,
                Deadline::after(Duration::from_millis(u64::from(step.timeout_ms))),
            );
            if let Some(writing) = writing {
                input = input.for_record(writing.record, None, None);
            }
            let authority = match writing {
                Some(writing) if step.commits => Authority::Write(writing.token),
                _ => Authority::Read,
            };
            let outcome = handle
                .act_recorded(prepared, step, &input, authority, recorded)
                .await?;
            let (StepOutcome::Done(observed) | StepOutcome::Absent(observed)) = outcome;
            ran.kinds.push(step.kind.to_string());
            for (name, value) in observed.values.iter() {
                ran.values.insert(name, value.clone());
            }
            for (name, value) in observed.for_reconcile.iter() {
                ran.for_reconcile.insert(name, value.clone());
            }
            ran.acting.extend(observed.acting());
            ran.captures.extend(
                observed
                    .captures
                    .into_iter()
                    .filter(|capture| !capture.is_acting()),
            );
        }
        Ok(ran)
    }

    /// A value as its field type normalizes it: the one rule by which an
    /// intended value and a value read back are compared.
    pub fn normalized(&self, field_type: &str, value: &Value) -> Value {
        let field_type = self
            .compiled_target
            .field_types
            .iter()
            .find(|found| found.name == field_type)
            .unwrap();
        let normalizer = Normalizer::load(
            &self.compiled_target.expressions[field_type.normalizer.unwrap().get()],
        )
        .unwrap();
        normalizer.normalize(value).unwrap().normalized
    }

    /// Waits until the device has seen its last link close.
    pub async fn links_closed(&self) {
        assert!(
            self.device
                .control()
                .wait_until(Duration::from_secs(30), |now| now.open_links == 0)
                .await,
            "the device still has an open link"
        );
    }
}

pub fn decimal(text: &str) -> Value {
    Value::Decimal(a2m_core::Decimal::parse(text).unwrap())
}
