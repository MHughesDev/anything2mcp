//! Tests of the pack against its test stand, `targets/example-db`.
//!
//! The driver is reached only through the registry's handle, as the engine
//! reaches it. A committing step carries a real intent token from a real
//! store. Every write is proven by asking the stand what it stored.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: a helper that cannot do its work stops the test"
)]

mod compile;
mod discovery;
mod guards;
mod reads;
mod registration;
#[cfg(feature = "postgres")]
mod server;
mod writes;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use a2m_core::{
    BindingPlan, Digest, EffectClass, Environment, Epoch, Failure, FailureClass, FailureCode,
    NoSecrets, OperationName, Plan, PrincipalId, RecordKey, Redact, RunId, RunState, Step,
    TargetName, Timestamp, Value, Values, WorkspaceId,
};
use a2m_files::{CompileMode, Compiled, Diagnostics, TargetRoot};
use a2m_ledger::{
    Durability, Fence, IntentToken, NewRun, Opened, PlannedRecord, RecordIntent, RecordRef,
    RetentionClass, Store, StoreConfig, ToolKind,
};
use a2m_provider::{
    Access, Authority, CheckedPaths, Deadline, Destinations, DriverHandle, HostServices, Posture,
    PrepareContext, Prepared, ProviderSettings, Registry, RegistryBuilder, RootName, Roots, Sent,
    StepFailure, StepInput, StepOutcome, Supervisor, SupervisorLimits, SurfaceRef,
};
use a2m_telemetry::{ManualClock, TelemetrySettings};
use a2m_vault::{MemoryKeyStore, RandomSource, Vault};
use example_db::harness::TestDb;

use crate::DataPack;
use crate::names::Names;

/// A folder under the system's temporary folder, removed when dropped.
pub(crate) struct Folder(PathBuf);

impl Folder {
    pub(crate) fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "a2m-pack-data-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The folder of the pack's own `example` target.
pub(crate) fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("example")
}

/// A registry that holds only the pack given.
pub(crate) fn registry_of(pack: &DataPack, folder: &Folder, redact: Arc<dyn Redact>) -> Registry {
    let logs = folder.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let host = HostServices {
        supervisor: Supervisor::new(SupervisorLimits::default()),
        roots: Roots::none(),
        redact: Arc::clone(&redact),
        telemetry: a2m_telemetry::start(&TelemetrySettings::default(), redact, &logs).unwrap(),
    };
    let mut builder = RegistryBuilder::new(ProviderSettings::default(), host);
    builder.add_pack(pack);
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    registry
}

/// Compiles a target folder with the registry's compiler set.
pub(crate) fn compile_folder(registry: &Registry, root: &Path) -> Result<Compiled, Diagnostics> {
    let set = registry.compiler_set();
    let sources = a2m_files::load(&TargetRoot::new(root), set)?;
    a2m_files::compile(&sources, set, CompileMode::Draft, a2m_files::DRAFT_VERSION)
}

/// The pack's mode.
pub(crate) fn mode() -> a2m_core::Mode {
    Names::new().unwrap().mode
}

// ---- a real store, for real intent tokens ----

pub(crate) struct Counted(AtomicU64);

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

pub(crate) fn counted() -> Arc<Counted> {
    Arc::new(Counted(AtomicU64::new(0)))
}

pub(crate) async fn ledger() -> Store {
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

/// A write run with one record planned, approved, and running, and the
/// intent token of that record.
pub(crate) async fn write_run(store: &Store, key: RecordKey) -> (Fence, RecordRef, IntentToken) {
    let workspace = WorkspaceId::from_bytes([1; 16]);
    let intended = Digest::of(b"intended");
    let planned_key = key.clone();
    let fence = store
        .write(Durability::Synced, move |tx| {
            let diff = Digest::of(b"diff");
            let fence = {
                let mut ledger = tx.ledger();
                let Opened::New(opened) = ledger.open_run(NewRun {
                    workspace,
                    principal: PrincipalId::from_bytes([1; 16]),
                    client_label: "test".to_owned(),
                    tool_name: OperationName::new("example_line_item_update").unwrap(),
                    tool_kind: ToolKind::Operation,
                    target: TargetName::new("example").unwrap(),
                    target_version: Digest::of(b"plan"),
                    environment: Environment::new("test").unwrap(),
                    identity: None,
                    effect: EffectClass::Write,
                    input_hash: Digest::of(&planned_key.canonical_bytes()),
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
                        key: planned_key,
                        operation: OperationName::new("example_line_item_update").unwrap(),
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
    (fence, record, token)
}

// ---- the stand, the plan, and one prepared driver ----

/// The stand, the registry with only this pack, and the compiled fixtures.
pub(crate) struct Scene {
    // Order matters: the stand's file goes last, after every connection.
    pub(crate) registry: Registry,
    pub(crate) plan: Plan,
    pub(crate) surface: SurfaceRef,
    paths: CheckedPaths,
    _folder: Folder,
    pub(crate) db: TestDb,
}

impl Scene {
    pub(crate) fn new() -> Self {
        Self::with(&crate::pack(), Arc::new(NoSecrets))
    }

    pub(crate) fn with(pack: &DataPack, redact: Arc<dyn Redact>) -> Self {
        let db = TestDb::start().unwrap();
        let folder = Folder::new("scene");
        let registry = registry_of(pack, &folder, redact);
        let plan = match compile_folder(&registry, &fixtures()) {
            Ok(compiled) => compiled.plan,
            Err(problems) => panic!("the fixtures do not compile: {problems:#?}"),
        };
        let surface = SurfaceRef {
            target: plan.header.target.clone(),
            environment: Environment::new("test").unwrap(),
            surface: plan.surfaces[0].clone(),
        };
        // The stand's file, as the engine would hand it over: a checked path
        // in the root `project`, under the surface's name.
        let root = RootName::new("project").unwrap();
        let mut roots = Roots::none();
        roots
            .add(root.clone(), db.path().parent().unwrap(), Access::Write)
            .unwrap();
        let file = db.path().file_name().unwrap().to_str().unwrap();
        let mut paths = CheckedPaths::none();
        paths.insert(
            surface.surface.name.as_str(),
            roots.check(&root, file, Access::Write).unwrap(),
        );
        Self {
            registry,
            plan,
            surface,
            paths,
            _folder: folder,
            db,
        }
    }

    /// Changes one statement of the plan's section: a plan as the compiler
    /// would never make it, to show what the driver itself refuses.
    pub(crate) fn alter(&mut self, name: &str, change: impl FnOnce(&mut crate::model::Statement)) {
        let section = self
            .plan
            .packs
            .iter_mut()
            .find(|section| section.pack == crate::names::PACK)
            .unwrap();
        let mut held: crate::model::Section = section.payload.decode().unwrap();
        change(held.statements.get_mut(name).unwrap());
        section.payload = a2m_core::PackPayload::encode(&held).unwrap();
    }

    pub(crate) fn handle(&self) -> DriverHandle {
        self.registry.driver(&mode()).unwrap()
    }

    /// The binding of an operation of the fixtures.
    pub(crate) fn binding(&self, operation: &str) -> &BindingPlan {
        let operation = self
            .plan
            .operations
            .iter()
            .find(|found| found.name.as_str() == operation)
            .unwrap_or_else(|| panic!("the fixtures have no operation {operation}"));
        &operation.bindings[0]
    }

    /// A step's arguments, evaluated from the operation's inputs as the
    /// engine evaluates them.
    pub(crate) fn args(&self, step: &Step, inputs: &Values) -> Values {
        args_of(&self.plan, step, inputs)
    }

    pub(crate) async fn prepare(&self, posture: Posture) -> Run<'_> {
        self.prepare_as(
            posture,
            Fence {
                run: RunId::from_bytes([7; 16]),
                epoch: Epoch::new(1),
            },
        )
        .await
    }

    pub(crate) async fn prepare_as(&self, posture: Posture, fence: Fence) -> Run<'_> {
        let handle = self.handle();
        let prepared = self.try_prepare(&handle, posture, fence).await.unwrap();
        Run {
            plan: &self.plan,
            handle,
            prepared: Some(prepared),
            wait: Duration::from_secs(30),
            acted: Vec::new(),
            recorded: Values::new(),
        }
    }

    pub(crate) async fn try_prepare(
        &self,
        handle: &DriverHandle,
        posture: Posture,
        fence: Fence,
    ) -> Result<Prepared, StepFailure> {
        self.try_prepare_with(handle, posture, fence, &self.paths)
            .await
    }

    /// The same, with the checked paths the test gives: a run whose
    /// request came from another project folder.
    pub(crate) async fn try_prepare_with(
        &self,
        handle: &DriverHandle,
        posture: Posture,
        fence: Fence,
        paths: &CheckedPaths,
    ) -> Result<Prepared, StepFailure> {
        let supervisor = handle.supervisor(Vec::new());
        let cx = PrepareContext {
            run: fence.run,
            epoch: fence.epoch,
            surface: &self.surface,
            plan: &self.plan,
            posture,
            deadline: Deadline::after(Duration::from_secs(30)),
            redact: &NoSecrets,
            destinations: &Destinations::default(),
            paths,
            supervisor: &supervisor,
        };
        handle.prepare(&cx, None).await
    }

    /// A live run with a real intent token for the record `key`.
    pub(crate) async fn live(&self, key: &[&str]) -> (Run<'_>, Write) {
        let key = RecordKey::new(key.iter().map(|part| Value::from(*part)).collect()).unwrap();
        let store = ledger().await;
        let (fence, record, token) = write_run(&store, key.clone()).await;
        let run = self.prepare_as(Posture::Live, fence).await;
        (
            run,
            Write {
                _store: store,
                record,
                key,
                token,
            },
        )
    }
}

/// What a run gets from a request that came from this folder: the folder,
/// checked as the root `project`, and no path under a surface's name.
pub(crate) fn project_of(folder: &Path) -> CheckedPaths {
    let root = RootName::new("project").unwrap();
    let mut roots = Roots::none();
    roots.add(root.clone(), folder, Access::Write).unwrap();
    CheckedPaths::none().with_project(roots.folder(&root, Access::Write).unwrap())
}

/// A step's arguments, evaluated from the operation's inputs as the engine
/// evaluates them.
pub(crate) fn args_of(plan: &Plan, step: &Step, inputs: &Values) -> Values {
    let input = Value::Record(inputs.clone());
    step.args
        .iter()
        .map(|arg| {
            let expr = plan.expression(arg.value).unwrap();
            let program = a2m_expr::Program::load(expr).unwrap();
            let bindings = a2m_expr::Bindings::new().with("input", &input);
            (arg.name.clone(), program.evaluate(&bindings).unwrap())
        })
        .collect()
}

/// What a read returns through the engine: each output the operation
/// declares, taken by its name from what the last step observed and
/// normalized by its field type, as the engine's own shaping does. A value
/// the step gave that no output declares is not returned; an output the
/// step gave no value for stops the test, as it stops a read in the engine.
pub(crate) fn outputs_of(plan: &Plan, operation: &str, observed: &Values) -> Values {
    let operation = plan
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
            .and_then(|index| plan.field_types[index.get()].normalizer)
        {
            Some(index) => {
                let normalizer =
                    a2m_expr::Normalizer::load(plan.expression(index).unwrap()).unwrap();
                let one = |value: &Value| normalizer.normalize(value).unwrap().normalized;
                // A list output is normalized value by value. The engine's
                // own shaping hands the normalizer the whole list, which
                // it refuses (`value_unparsable`): reported to the engine.
                match raw {
                    Value::List(items) => Value::List(items.iter().map(one).collect()),
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
        // A `null`, whole or as an item, stops the engine in an output
        // that may not be absent (decision 0305).
        let holds_null = match &value {
            Value::List(items) => items.iter().any(Value::is_null),
            other => other.is_null(),
        };
        assert!(
            output.may_be_absent || !holds_null,
            "the output `{}` holds a null and may not be absent",
            output.name
        );
        out.insert(output.name.clone(), value);
    }
    out
}

/// What a committing step carries: the commit point, its key, and the
/// ledger's token for it.
pub(crate) struct Write {
    pub(crate) _store: Store,
    pub(crate) record: RecordRef,
    pub(crate) key: RecordKey,
    pub(crate) token: IntentToken,
}

/// One prepared driver, released when the test says so.
pub(crate) struct Run<'a> {
    pub(crate) plan: &'a Plan,
    pub(crate) handle: DriverHandle,
    pub(crate) prepared: Option<Prepared>,
    /// How long each step is given.
    pub(crate) wait: Duration,
    /// How each step was done, as the trace takes it: from the step's
    /// `acting` capture, in order.
    pub(crate) acted: Vec<a2m_telemetry::Acting>,
    /// What earlier steps of the record returned in `for_reconcile`, as the
    /// engine hands it on, within a pass and on a resume.
    pub(crate) recorded: Values,
}

impl Run<'_> {
    /// One step, with the right to read only.
    pub(crate) async fn step(
        &mut self,
        step: &Step,
        inputs: &Values,
    ) -> Result<StepOutcome, StepFailure> {
        let args = args_of(self.plan, step, inputs);
        self.step_with(step, &args, None).await
    }

    /// One step with the arguments given as they are, and the token when
    /// the test has one.
    pub(crate) async fn step_with(
        &mut self,
        step: &Step,
        args: &Values,
        write: Option<&Write>,
    ) -> Result<StepOutcome, StepFailure> {
        let mut input = StepInput::new(args, Deadline::after(self.wait));
        if let Some(write) = write {
            input = input.for_record(write.record, Some(&write.key), None);
        }
        let authority = write.map_or(Authority::Read, |write| Authority::Write(&write.token));
        let outcome = self
            .handle
            .act_recorded(
                self.prepared.as_mut().unwrap(),
                step,
                &input,
                authority,
                &self.recorded,
            )
            .await;
        if let Ok(StepOutcome::Done(observed) | StepOutcome::Absent(observed)) = &outcome {
            self.acted.extend(observed.acting());
        }
        outcome
    }

    /// Every step of a list, in order, stopping at the first failure. The
    /// last outcome is returned.
    pub(crate) async fn steps(
        &mut self,
        steps: &[Step],
        inputs: &Values,
        write: Option<&Write>,
    ) -> Result<StepOutcome, StepFailure> {
        let mut last = None;
        for step in steps {
            let args = args_of(self.plan, step, inputs);
            last = Some(
                self.step_with(step, &args, write.filter(|_| step.commits))
                    .await?,
            );
        }
        Ok(last.expect("a binding has steps"))
    }

    /// What the driver noted of each step, for the step trace.
    pub(crate) fn acting(&self) -> Vec<a2m_telemetry::Acting> {
        self.acted.clone()
    }

    pub(crate) async fn release(mut self) {
        let prepared = self.prepared.take().unwrap();
        assert!(self.handle.release(prepared, false).await.complete);
    }
}

/// The values a step observed.
pub(crate) fn values(outcome: StepOutcome) -> Values {
    match outcome {
        StepOutcome::Done(observed) => observed.values,
        StepOutcome::Absent(_) => panic!("the record is absent"),
    }
}

/// The class, the code, and whether anything was sent, of a failed step.
pub(crate) fn failed(
    result: Result<StepOutcome, StepFailure>,
) -> (FailureClass, FailureCode, Sent) {
    let failure = result.unwrap_err();
    (
        failure.failure.class(),
        failure.failure.code().clone(),
        failure.sent,
    )
}

/// The inputs of an operation, from names and texts.
pub(crate) fn inputs(pairs: &[(&str, &str)]) -> Values {
    pairs
        .iter()
        .map(|(name, text)| ((*name).to_owned(), Value::from(*text)))
        .collect()
}
