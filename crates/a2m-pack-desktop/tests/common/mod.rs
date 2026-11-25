//! What the tests share: a registry that holds only the desktop pack, on a
//! session of the test's own; the fixture target compiled through it; the
//! driver prepared through the registry's handle; and discovery through
//! the read-only port. The same tests run against the stand on every
//! system that has one: on Linux a tree on a session bus of the test's
//! own, on Windows a window of plain controls and on macOS a window of
//! AppKit's controls, each in a process of its own, which is this test
//! binary started again (hence [`example_desk::stand_child`]).
//!
//! A test that needs the stand says it was skipped on a machine that
//! cannot give one (no session bus and accessibility bus on Linux, no
//! interactive desktop on Windows, no permission to use the accessibility
//! interface on macOS), unless `A2M_REQUIRE_DESKTOP=1` says the machine
//! must: then it fails, and says why.

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
    Archetype, CompiledBinding, CompiledTarget, Environment, Epoch, Finding, Mode, NoSecrets,
    RunId, Step, SurfaceKind, Values,
};
use a2m_files::{CompileMode, Compiled, Diagnostics, DraftReport, Drafter, TargetRoot};
use a2m_provider::{
    Authority, CheckedPaths, Deadline, Destinations, DiscoveryScope, DriverHandle, FindingSink,
    HostServices, PrepareContext, Prepared, ProviderSettings, ReadOnlyPort, Registry,
    RegistryBuilder, Roots, StepFailure, StepInput, StepOutcome, Supervisor, SupervisorLimits,
    SurfaceDescription, SurfaceRef,
};
use a2m_telemetry::TelemetrySettings;
use example_desk::{Desk, Order, Session};

pub mod typelib;

example_desk::stand_child!();

/// A folder under the system's temporary folder, removed when dropped.
pub struct Folder(PathBuf);

impl Folder {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "a2m-pack-desktop-{name}-{}-{}",
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

pub fn mode() -> Mode {
    Mode::new("desktop").unwrap()
}

/// A registry that holds only the desktop pack, reading the session at
/// `bus`, with its logs in `folder`.
pub fn registry(folder: &Path, bus: Option<&str>) -> Registry {
    let logs = folder.join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let redact: Arc<dyn a2m_core::Redact> = Arc::new(NoSecrets);
    let host = HostServices {
        supervisor: Supervisor::new(SupervisorLimits::default()),
        roots: Roots::none(),
        redact: Arc::clone(&redact),
        telemetry: a2m_telemetry::start(&TelemetrySettings::default(), redact, &logs).unwrap(),
    };
    let mut builder = RegistryBuilder::new(ProviderSettings::default(), host);
    match bus {
        Some(address) => builder.add_pack(&a2m_pack_desktop::DesktopPack::on_session_bus(address)),
        None => a2m_pack_desktop::add_to(&mut builder),
    }
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    registry
}

pub fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("example")
}

pub fn shown(problems: &Diagnostics) -> String {
    problems
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Compiles the target in `root`.
pub fn compile_at(registry: &Registry, root: &Path) -> Result<Compiled, Diagnostics> {
    let set = registry.compiler_set();
    let sources = a2m_files::load(&TargetRoot::new(root), set)?;
    a2m_files::compile(&sources, set, CompileMode::Draft, a2m_files::DRAFT_VERSION)
}

/// Copies the fixture into `into`, with `change` applied to the file at
/// `path`.
pub fn fixture_copy(into: &Path, path: &str, change: impl Fn(&str) -> String) {
    fn walk(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap().path();
            let name = entry.file_name().unwrap();
            if entry.is_dir() {
                walk(&entry, &to.join(name));
            } else {
                std::fs::copy(&entry, to.join(name)).unwrap();
            }
        }
    }
    walk(&fixture(), into);
    let mut file = into.to_path_buf();
    for part in path.split('/') {
        file = file.join(part);
    }
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    std::fs::write(&file, change(&text)).unwrap();
}

/// A session of the test's own with the stand's application on it, or
/// `None` when this machine cannot give one.
pub struct Scene {
    pub folder: Folder,
    pub desk: Option<Desk>,
    pub session: Session,
    pub registry: Registry,
    pub compiled_target: CompiledTarget,
    pub surface: SurfaceRef,
    /// The program that runs the stand, which the surface names.
    pub program: String,
}

impl Scene {
    /// The fixture compiled, the stand started on a session of its own.
    pub async fn start(name: &str) -> Option<Self> {
        Self::with(name, true).await
    }

    /// The same, with no application started on the session.
    pub async fn without_application(name: &str) -> Option<Self> {
        Self::with(name, false).await
    }

    /// The same, with the surface naming `service` as its service on the
    /// session bus instead of the fixture's own.
    pub async fn with_service(name: &str, service: &str) -> Option<Self> {
        let copy = Folder::new("service-target");
        let service = service.to_owned();
        fixture_copy(copy.path(), "target.yaml", move |text| {
            text.replace("service: org.example.Desk", &format!("service: {service}"))
        });
        Self::build(name, true, copy.path()).await
    }

    /// The same, with the surface stating the management class whose
    /// definition the scout is to read.
    pub async fn with_management_class(name: &str, class: &str) -> Option<Self> {
        let copy = Folder::new("management-target");
        let class = class.to_owned();
        fixture_copy(copy.path(), "target.yaml", move |text| {
            text.replace(
                "    service: org.example.Desk\n",
                &format!("    service: org.example.Desk\n    management_class: {class}\n"),
            )
        });
        Self::build(name, true, copy.path()).await
    }

    /// The same, with `lines` added to the surface after its service.
    pub async fn with_surface_lines(name: &str, lines: &str) -> Option<Self> {
        let copy = Folder::new("surface-target");
        let lines = lines.to_owned();
        fixture_copy(copy.path(), "target.yaml", move |text| {
            text.replace(
                "    service: org.example.Desk\n",
                &format!("    service: org.example.Desk\n{lines}"),
            )
        });
        Self::build(name, true, copy.path()).await
    }

    async fn with(name: &str, application: bool) -> Option<Self> {
        Self::build(name, application, &fixture()).await
    }

    async fn build(name: &str, application: bool, target: &Path) -> Option<Self> {
        let Some(session) = Session::start().unwrap() else {
            eprintln!(
                "skipped: this machine cannot give the desktop stand a session (on Linux a session bus and an accessibility bus, on Windows an interactive desktop, on macOS this program's permission to use the accessibility interface)"
            );
            return None;
        };
        let folder = Folder::new(name);
        let desk = if application {
            Some(Desk::start(&session, Order::example()).await.unwrap())
        } else {
            None
        };
        let registry = registry(folder.path(), session.bus());
        let compiled_target = match compile_at(&registry, target) {
            Ok(compiled) => compiled.compiled_target,
            Err(problems) => panic!("{}", shown(&problems)),
        };
        let program = session.program();
        let surface = surface_of(&compiled_target, &program);
        Some(Self {
            folder,
            desk,
            session,
            registry,
            compiled_target,
            surface,
            program,
        })
    }

    pub fn desk(&self) -> &Desk {
        self.desk.as_ref().unwrap()
    }

    pub fn handle(&self) -> DriverHandle {
        self.registry.driver(&mode()).unwrap()
    }

    pub async fn prepare(
        &self,
        posture: a2m_provider::Posture,
    ) -> Result<(DriverHandle, Prepared), StepFailure> {
        let handle = self.handle();
        let supervisor = handle.supervisor(Vec::new());
        let destinations = Destinations::new([self.surface.surface.address.clone()]);
        let cx = PrepareContext {
            run: RunId::from_bytes([7; 16]),
            epoch: Epoch::new(1),
            surface: &self.surface,
            compiled_target: &self.compiled_target,
            posture,
            deadline: Deadline::after(Duration::from_secs(20)),
            redact: &NoSecrets,
            destinations: &destinations,
            paths: &CheckedPaths::none(),
            supervisor: &supervisor,
        };
        let prepared = handle.prepare(&cx, None).await?;
        Ok((handle, prepared))
    }

    pub fn binding(&self, operation: &str) -> &CompiledBinding {
        binding_of(&self.compiled_target, operation)
    }

    /// Reads through the `act` steps of a read operation, each with the
    /// deadline given, through the registry's handle.
    pub async fn read(&self, operation: &str, within: Duration) -> Result<Values, StepFailure> {
        let (handle, mut prepared) = self.prepare(a2m_provider::Posture::Live).await?;
        let steps = self.binding(operation).act.clone();
        let result = run(&handle, &mut prepared, &steps, within).await;
        let cleanup = handle.release(prepared, result.is_err()).await;
        assert!(cleanup.complete);
        result
    }

    /// Reads through the `act` steps of a read operation that takes
    /// inputs, each step's arguments evaluated from them.
    pub async fn read_with(
        &self,
        operation: &str,
        inputs: &Values,
        within: Duration,
    ) -> Result<Values, StepFailure> {
        let (handle, mut prepared) = self.prepare(a2m_provider::Posture::Live).await?;
        let steps = self.binding(operation).act.clone();
        let result = run_with(
            &self.compiled_target,
            &handle,
            &mut prepared,
            &steps,
            inputs,
            None,
            within,
        )
        .await;
        let cleanup = handle.release(prepared, result.is_err()).await;
        assert!(cleanup.complete);
        result
    }

    /// The same read, which on failure says what the window holds: its
    /// shape, as the pack's discoverer gives it. A failing test's message
    /// is all a reader of a remote machine's run has.
    pub async fn read_or_explain(&self, operation: &str, within: Duration) -> Values {
        match self.read(operation, within).await {
            Ok(values) => values,
            Err(failure) => panic!(
                "`{operation}` stopped: {}\nThe window, as the pack sees it:\n{}",
                failure.failure,
                self.explain().await
            ),
        }
    }

    /// The shape of the application's windows, as the pack's discoverer
    /// gives it, cut to a few thousand characters; or why it could not be
    /// taken.
    pub async fn explain(&self) -> String {
        const KEPT: usize = 12_000;
        match self.discovered().await {
            Ok(findings) => findings
                .iter()
                .map(|finding| finding.found().to_owned())
                .collect::<Vec<_>>()
                .join("\n")
                .chars()
                .take(KEPT)
                .collect(),
            Err(why) => format!("(no shape: {why})"),
        }
    }

    /// The pack's discoverer, run against the session.
    pub async fn discover(&self) -> Vec<Finding> {
        self.discovered().await.unwrap()
    }

    async fn discovered(&self) -> Result<Vec<Finding>, String> {
        let description = SurfaceDescription {
            kind: SurfaceKind::new("desktop").unwrap(),
            archetype: Archetype::Interface,
            address: self.surface.surface.address.clone(),
            facts: Values::new(),
        };
        // The window's: the pack has another, of services, which a test of
        // its own runs.
        let discoverer = self
            .registry
            .discoverers_for(&description)
            .into_iter()
            .find(|found| found.declared().name.as_str() == "desktop_snapshot")
            .expect("the pack registers a discoverer of windows");
        let (handle, prepared) = self
            .prepare(a2m_provider::Posture::Discovery)
            .await
            .map_err(|failure| failure.failure.to_string())?;
        let port = ReadOnlyPort::new(handle, prepared);
        let focus = Values::new();
        let scope = DiscoveryScope {
            surface: &self.surface,
            bounds: discoverer.declared().bounds,
            deadline: Deadline::after(Duration::from_secs(20)),
            paths: &CheckedPaths::none(),
            redact: &NoSecrets,
            focus: &focus,
        };
        let mut kept = Kept(Vec::new());
        let said = discoverer.discover(&port, &scope, &mut kept).await;
        let (handle, prepared) = port.into_parts();
        handle.release(prepared, false).await;
        said.map_err(|failure| failure.to_string())?;
        Ok(kept.0)
    }

    /// A target folder with only a `target.yaml` whose surface is this
    /// test's program, and the drafts of `findings` written into it.
    pub fn draft(&self, findings: &[Finding]) -> (Folder, DraftReport) {
        let root = Folder::new("drafted");
        let target = std::fs::read_to_string(fixture().join("target.yaml"))
            .unwrap()
            .replace("/opt/example/bin/example-desk", &yaml_text(&self.program));
        std::fs::write(root.path().join("target.yaml"), target).unwrap();
        let report = Drafter::new(self.registry.compiler_set(), &TargetRoot::new(root.path()))
            .draft(findings)
            .unwrap();
        (root, report)
    }
}

/// A path as it stands inside the fixture's double-quoted string: a
/// Windows path's backslashes doubled.
fn yaml_text(path: &str) -> String {
    path.replace('\\', "\\\\")
}

/// The compiled target's surface, at the address of the program that runs the
/// stand.
pub fn surface_of(compiled_target: &CompiledTarget, program: &str) -> SurfaceRef {
    let mut surface = compiled_target.surfaces[0].clone();
    surface.address = program.to_owned();
    SurfaceRef {
        target: compiled_target.header.target.clone(),
        environment: Environment::new("test").unwrap(),
        surface,
    }
}

pub fn binding_of<'a>(compiled_target: &'a CompiledTarget, operation: &str) -> &'a CompiledBinding {
    compiled_target
        .operations
        .iter()
        .find(|known| known.name.as_str() == operation)
        .unwrap_or_else(|| panic!("no operation named `{operation}`"))
        .bindings
        .iter()
        .find(|binding| binding.mode == mode())
        .unwrap()
}

/// Carries out steps in order through the handle, as the engine would,
/// with no authority to write.
pub async fn run(
    handle: &DriverHandle,
    prepared: &mut Prepared,
    steps: &[Step],
    within: Duration,
) -> Result<Values, StepFailure> {
    let mut values = Values::new();
    let args = Values::new();
    for step in steps {
        let input = StepInput::new(&args, Deadline::after(within));
        let outcome = handle.act(prepared, step, &input, Authority::Read).await?;
        let (StepOutcome::Done(observed) | StepOutcome::Absent(observed)) = outcome;
        for (name, value) in observed.values {
            values.insert(name, value);
        }
    }
    Ok(values)
}

struct Kept(Vec<Finding>);

impl FindingSink for Kept {
    fn found(&mut self, finding: Finding) -> Result<(), a2m_core::Failure> {
        self.0.push(finding);
        Ok(())
    }
}

/// Every file under a folder, as text, for a test that looks for a value
/// in all of them.
pub fn every_file(root: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(folder) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
                out.push((path, text));
            }
        }
    }
    out
}

// ---- a write: arguments, a real store, real intent tokens ----

/// The arguments of a step, as the engine would evaluate them. Every
/// expression of the fixtures is `input.<name>`.
pub fn args(compiled_target: &CompiledTarget, step: &Step, inputs: &Values) -> Values {
    let mut args = Values::new();
    for arg in &step.args {
        let source = &compiled_target.expression(arg.value).unwrap().source;
        let name = source
            .strip_prefix("input.")
            .unwrap_or_else(|| panic!("the test helper cannot evaluate `{source}`"));
        if let Some(value) = inputs.get(name) {
            args.insert(arg.name.clone(), value.clone());
        }
    }
    args
}

/// Carries out steps in order through the handle, as the engine would:
/// each with its arguments evaluated, and, for a write, the committing
/// step with the ledger's token for the record.
pub async fn run_with(
    compiled_target: &CompiledTarget,
    handle: &DriverHandle,
    prepared: &mut Prepared,
    steps: &[Step],
    inputs: &Values,
    write: Option<&Write>,
    within: Duration,
) -> Result<Values, StepFailure> {
    let mut values = Values::new();
    for step in steps {
        let args = args(compiled_target, step, inputs);
        let input = StepInput::new(&args, Deadline::after(within));
        let input = match write {
            Some(write) => input.for_record(write.record(), Some(&write.key), None),
            None => input,
        };
        let authority = match write {
            Some(write) if step.commits => Authority::Write(&write.token),
            _ => Authority::Read,
        };
        let outcome = handle.act(prepared, step, &input, authority).await?;
        let (StepOutcome::Done(observed) | StepOutcome::Absent(observed)) = outcome;
        for (name, value) in observed.values {
            values.insert(name, value);
        }
    }
    Ok(values)
}

impl Scene {
    /// Prepares the driver for the write's run and lease number, in a
    /// posture.
    pub async fn prepare_for(
        &self,
        write: Option<&Write>,
        posture: a2m_provider::Posture,
    ) -> Result<(DriverHandle, Prepared), StepFailure> {
        let handle = self.handle();
        let supervisor = handle.supervisor(Vec::new());
        let destinations = Destinations::new([self.surface.surface.address.clone()]);
        let (run, epoch) = write.map_or((RunId::from_bytes([7; 16]), Epoch::new(1)), |write| {
            (write.fence.run, write.fence.epoch)
        });
        let cx = PrepareContext {
            run,
            epoch,
            surface: &self.surface,
            compiled_target: &self.compiled_target,
            posture,
            deadline: Deadline::after(Duration::from_secs(20)),
            redact: &NoSecrets,
            destinations: &destinations,
            paths: &CheckedPaths::none(),
            supervisor: &supervisor,
        };
        let prepared = handle.prepare(&cx, None).await?;
        Ok((handle, prepared))
    }

    /// Runs one list of a binding's steps, prepared afresh and released
    /// after, as the engine runs the act of a write or its read-back.
    pub async fn steps(
        &self,
        steps: &[Step],
        inputs: &Values,
        write: Option<&Write>,
        posture: a2m_provider::Posture,
        within: Duration,
    ) -> Result<Values, StepFailure> {
        let (handle, mut prepared) = self.prepare_for(write, posture).await?;
        let result = run_with(
            &self.compiled_target,
            &handle,
            &mut prepared,
            steps,
            inputs,
            write,
            within,
        )
        .await;
        handle.release(prepared, result.is_err()).await;
        result
    }
}

struct Counted(AtomicU64);

impl a2m_vault::RandomSource for Counted {
    fn fill(&self, out: &mut [u8]) -> Result<(), a2m_core::Failure> {
        for chunk in out.chunks_mut(a2m_core::Digest::LENGTH) {
            let number = self.0.fetch_add(1, Ordering::SeqCst);
            let block = a2m_core::Digest::of(&number.to_le_bytes());
            chunk.copy_from_slice(&block.as_bytes()[..chunk.len()]);
        }
        Ok(())
    }
}

fn counted() -> Arc<Counted> {
    Arc::new(Counted(AtomicU64::new(0)))
}

async fn store() -> a2m_ledger::Store {
    let clock = Arc::new(a2m_telemetry::ManualClock::starting_at(
        a2m_core::Timestamp::from_unix_millis(1_800_000_000_000),
    ));
    let config = a2m_ledger::StoreConfig::in_memory()
        .with_clock(clock)
        .with_random(counted());
    let store = a2m_ledger::Store::open(config).await.unwrap();
    let vault = Arc::new(a2m_vault::Vault::with_random(
        Arc::new(a2m_vault::MemoryKeyStore::with_random(counted())),
        Arc::new(store.key_records()),
        counted(),
    ));
    store.attach(vault).await.unwrap();
    store
}

/// A write run for one record, approved, with its intent stored: what the
/// engine holds when it reaches the commit point. The approval is the
/// ledger's test stand-in for the policy engine's, used once.
pub struct Write {
    pub fence: a2m_ledger::Fence,
    pub key: a2m_core::RecordKey,
    pub token: a2m_ledger::IntentToken,
}

impl Write {
    pub async fn new(operation: &str, key: &str) -> Self {
        use a2m_core::{Digest, EffectClass, OperationName, PrincipalId, RunState, TargetName};
        use a2m_ledger::{Durability, NewRun, Opened, PlannedRecord, RecordIntent, RetentionClass};

        let store = store().await;
        let key = a2m_core::RecordKey::new(vec![a2m_core::Value::from(key)]).unwrap();
        let intended = Digest::of(&key.canonical_bytes());
        let operation = OperationName::new(operation).unwrap();
        let planned = key.clone();
        let fence = store
            .write(Durability::Synced, move |tx| {
                let fence = {
                    let mut ledger = tx.ledger();
                    let Opened::New(opened) = ledger.open_run(NewRun {
                        workspace: a2m_core::WorkspaceId::from_bytes([1; 16]),
                        principal: PrincipalId::from_bytes([1; 16]),
                        client_label: "test".to_owned(),
                        tool_name: operation.clone(),
                        tool_kind: a2m_ledger::ToolKind::Operation,
                        target: TargetName::new("example").unwrap(),
                        target_version: Digest::of(b"plan"),
                        environment: Environment::new("test").unwrap(),
                        identity: None,
                        effect: EffectClass::Write,
                        input_hash: intended,
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
                            operation,
                            intended,
                            intended_values: None,
                            before: None,
                            unchanged: false,
                        }],
                    )?;
                    ledger.store_diff(&fence, Digest::of(b"diff"), None)?;
                    ledger.set_state(&fence, RunState::NeedsApproval, None)?;
                    fence
                };
                let clearance = a2m_ledger::testing::approve(tx, &fence)?;
                tx.ledger().begin_writes(&fence, clearance)?;
                Ok(fence)
            })
            .await
            .unwrap();
        let record = a2m_ledger::RecordRef {
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
                    intended,
                    mode: None,
                }],
            )
            .await
            .unwrap()
            .remove(0);
        Self { fence, key, token }
    }

    pub fn record(&self) -> a2m_ledger::RecordRef {
        a2m_ledger::RecordRef {
            run: self.fence.run,
            step_seq: 0,
            record_seq: 0,
        }
    }
}

/// The field type's own rule for an output, as the engine loads it.
pub fn normalizer(
    compiled_target: &CompiledTarget,
    operation: &str,
    output: &str,
) -> a2m_expr::Normalizer {
    let operation = compiled_target
        .operations
        .iter()
        .find(|known| known.name.as_str() == operation)
        .unwrap();
    let output = operation
        .outputs
        .iter()
        .find(|known| known.name == output)
        .unwrap();
    let field_type = &compiled_target.field_types[output.field_type.unwrap().get()];
    a2m_expr::Normalizer::load(
        compiled_target
            .expression(field_type.normalizer.unwrap())
            .unwrap(),
    )
    .unwrap()
}
