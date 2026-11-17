//! What the tests share: the file stand, a registry that holds only the
//! file pack over the stand's root, the example target compiled through it,
//! a real store for real intent tokens, and the driver reached through the
//! handle, as the engine will reach it.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test helper: each test binary uses some of these, and a helper that cannot do its work stops the test"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use a2m_core::{
    ApprovalId, BindingPlan, Digest, EffectClass, Environment, Epoch, Failure, NoSecrets,
    OperationName, Plan, PrincipalId, RecordKey, RunState, Step, TargetName, Timestamp, Value,
    Values, Version, WorkspaceId,
};
use a2m_expr::{Bindings, Program};
use a2m_files::{CompileMode, CompilerSet, Diagnostics, TargetRoot};
use a2m_ledger::{
    Durability, Fence, IntentToken, NewRun, Opened, Owner, PlannedRecord, RecordIntent, RecordRef,
    RetentionClass, Store, StoreConfig, ToolKind, WriteClearance,
};
use a2m_provider::{
    Access, Authority, CheckedPaths, Deadline, Destinations, DriverHandle, HostServices, Posture,
    PrepareContext, Prepared, ProviderSettings, Registry, RegistryBuilder, RootName, Roots,
    StepFailure, StepInput, StepOutcome, Supervisor, SupervisorLimits, SurfaceRef,
};
use a2m_telemetry::{LogLevel, ManualClock, Telemetry};
use a2m_vault::{MemoryKeyStore, RandomSource, Vault};
use example_files::harness::FileStand;

/// The moment every test's clock starts at.
pub const NOW: i64 = 1_800_000_000_000;

/// The folder of the example target written for the file pack.
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("example")
}

/// A registry that holds only the file pack, over the roots given.
pub fn registry(roots: Roots) -> Registry {
    let clock = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(NOW)));
    let host = HostServices {
        supervisor: Supervisor::new(SupervisorLimits::default()),
        roots,
        redact: Arc::new(NoSecrets),
        telemetry: Telemetry::in_memory(LogLevel::Info, Arc::new(NoSecrets), clock),
    };
    let mut builder = RegistryBuilder::new(ProviderSettings::default(), host);
    builder.add_pack(&a2m_pack_file::pack());
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    registry
}

/// The roots of a test: the stand's root under the name `files`, which the
/// pack may write, or under the name `project`, which it may only read.
pub fn roots(stand: &FileStand, access: Access) -> Roots {
    let name = match access {
        Access::Write => "files",
        Access::Read => "project",
    };
    let mut roots = Roots::none();
    roots
        .add(RootName::new(name).unwrap(), stand.root(), access)
        .unwrap();
    roots
}

/// Compiles the target folder at `root` with the set given.
pub fn compile(root: &Path, set: &CompilerSet) -> Result<a2m_files::Compiled, Diagnostics> {
    let sources = a2m_files::load(&TargetRoot::new(root), set)?;
    a2m_files::compile(&sources, set, CompileMode::Draft, Version::new(1, 0, 0))
}

/// The stand, the registry over it, and the compiled example target.
pub struct Scene {
    pub stand: FileStand,
    pub registry: Registry,
    pub plan: Plan,
    /// The root the surfaces are in, when it is not the one they state.
    root: Option<&'static str>,
}

impl Scene {
    pub fn new() -> Self {
        Self::with_access(Access::Write)
    }

    pub fn with_access(access: Access) -> Self {
        let stand = FileStand::start().unwrap();
        let registry = registry(roots(&stand, access));
        let plan = compile(&fixtures(), registry.compiler_set())
            .unwrap_or_else(|problems| panic!("the fixtures do not compile: {problems:?}"))
            .plan;
        Self {
            stand,
            registry,
            plan,
            root: (access == Access::Read).then_some("project"),
        }
    }

    pub fn surface(&self, name: &str) -> SurfaceRef {
        let mut surface = self
            .plan
            .surfaces
            .iter()
            .find(|surface| surface.name.as_str() == name)
            .expect("the surface is declared")
            .clone();
        if let Some(root) = self.root {
            // As a target that states `root: project` on its surfaces.
            surface.payload =
                a2m_core::PackPayload::encode(&Values::new().with("root", root)).unwrap();
        }
        SurfaceRef {
            target: self.plan.header.target.clone(),
            environment: Environment::new("test").unwrap(),
            surface,
        }
    }

    /// The one binding of an operation, and the surface it acts on.
    pub fn binding(&self, operation: &str) -> (&BindingPlan, SurfaceRef) {
        let binding = self
            .plan
            .operations
            .iter()
            .find(|found| found.name.as_str() == operation)
            .unwrap_or_else(|| panic!("no operation `{operation}`"))
            .bindings
            .first()
            .unwrap();
        let surface = self.plan.surface(binding.surface).unwrap().name.clone();
        (binding, self.surface(surface.as_str()))
    }

    pub async fn prepared(
        &self,
        surface: &SurfaceRef,
        fence: Option<Fence>,
        posture: Posture,
    ) -> (DriverHandle, Prepared) {
        let handle = self
            .registry
            .driver(&a2m_core::Mode::new("file").unwrap())
            .unwrap();
        let fence = fence.unwrap_or(Fence {
            run: a2m_core::RunId::from_bytes([7; 16]),
            epoch: Epoch::new(1),
        });
        let supervisor = handle.supervisor(Vec::new());
        let cx = PrepareContext {
            run: fence.run,
            epoch: fence.epoch,
            surface,
            plan: &self.plan,
            posture,
            deadline: Deadline::after(Duration::from_secs(30)),
            redact: &NoSecrets,
            destinations: &Destinations::default(),
            paths: &CheckedPaths::none(),
            supervisor: &supervisor,
        };
        let prepared = handle.prepare(&cx, None).await.unwrap();
        (handle, prepared)
    }

    /// A step's arguments, worked out from the operation's inputs as the
    /// engine will work them out.
    pub fn args(&self, step: &Step, inputs: &Values) -> Values {
        let input = Value::Record(inputs.clone());
        let mut args = Values::new();
        for arg in &step.args {
            let program = Program::load(self.plan.expression(arg.value).unwrap()).unwrap();
            let value = program
                .evaluate(&Bindings::new().with("input", &input))
                .unwrap();
            args.insert(arg.name.clone(), value);
        }
        args
    }

    /// Runs steps that read, in order, and gives every outcome.
    pub async fn read(
        &self,
        handle: &DriverHandle,
        prepared: &mut Prepared,
        steps: &[Step],
        inputs: &Values,
    ) -> Result<Vec<StepOutcome>, StepFailure> {
        let mut outcomes = Vec::new();
        for step in steps {
            let args = self.args(step, inputs);
            let input = StepInput {
                args: &args,
                record: None,
                key: None,
                stamp: None,
                deadline: Deadline::after(Duration::from_secs(30)),
            };
            let outcome = handle.act(prepared, step, &input, Authority::Read).await?;
            let absent = matches!(outcome, StepOutcome::Absent(_));
            outcomes.push(outcome);
            if absent {
                break;
            }
        }
        Ok(outcomes)
    }

    /// Runs the steps of a write, in order, with the record's token.
    pub async fn write(
        &self,
        handle: &DriverHandle,
        prepared: &mut Prepared,
        steps: &[Step],
        inputs: &Values,
        granted: &Granted,
    ) -> Result<Vec<StepOutcome>, StepFailure> {
        let mut outcomes = Vec::new();
        for step in steps {
            let args = self.args(step, inputs);
            let input = StepInput {
                args: &args,
                record: Some(granted.record),
                key: Some(&granted.key),
                stamp: Some(&granted.stamp),
                deadline: Deadline::after(Duration::from_secs(30)),
            };
            let authority = if step.commits {
                Authority::Write(&granted.token)
            } else {
                Authority::Read
            };
            outcomes.push(handle.act(prepared, step, &input, authority).await?);
        }
        Ok(outcomes)
    }

    /// One whole read through a fresh driver: the values the last step gave.
    pub async fn read_operation(
        &self,
        operation: &str,
        inputs: &Values,
    ) -> Result<Vec<StepOutcome>, StepFailure> {
        let (binding, surface) = self.binding(operation);
        let (handle, mut prepared) = self.prepared(&surface, None, Posture::Live).await;
        let outcome = self
            .read(&handle, &mut prepared, &binding.act, inputs)
            .await;
        assert!(handle.release(prepared, outcome.is_err()).await.complete);
        outcome
    }
}

/// The values of the last outcome of a run of steps.
pub fn last_values(outcomes: &[StepOutcome]) -> &Values {
    match outcomes.last().expect("a step ran") {
        StepOutcome::Done(observed) | StepOutcome::Absent(observed) => &observed.values,
    }
}

/// Everything the steps returned for reconcile, by name.
pub fn for_reconcile(outcomes: &[StepOutcome]) -> Values {
    let mut all = Values::new();
    for outcome in outcomes {
        let (StepOutcome::Done(observed) | StepOutcome::Absent(observed)) = outcome;
        for (name, value) in observed.for_reconcile.iter() {
            all.insert(name.to_owned(), value.clone());
        }
    }
    all
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
            Timestamp::from_unix_millis(NOW),
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

/// What the ledger granted for one record of a write run: the run's fence,
/// the record, its key, its idempotency token, and its intent token.
pub struct Granted {
    pub fence: Fence,
    pub record: RecordRef,
    pub key: RecordKey,
    pub stamp: String,
    pub token: IntentToken,
}

/// A write run with one record planned, approved, running, and its intent
/// stored: everything the handle asks of a committing step.
pub async fn granted(store: &Store, caller: u8, name: &str) -> Granted {
    // One key per run: the ledger lets one run at a time hold a record.
    let key = RecordKey::new(vec![Value::from(format!("{name}-{caller}"))]).unwrap();
    let intended = Digest::of(format!("intended {name}").as_bytes());
    let workspace = WorkspaceId::from_bytes([1; 16]);
    let planned_key = key.clone();
    let input_hash = Digest::of(format!("{caller} {name}").as_bytes());
    let fence = store
        .write(Durability::Synced, move |tx| {
            let diff = Digest::of(b"diff");
            let fence = {
                let mut ledger = tx.ledger();
                let Opened::New(opened) = ledger.open_run(NewRun {
                    workspace,
                    principal: PrincipalId::from_bytes([caller; 16]),
                    client_label: "test".to_owned(),
                    tool_name: OperationName::new("example_stock_set").unwrap(),
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
                        key: planned_key,
                        operation: OperationName::new("example_stock_set").unwrap(),
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
    Granted {
        fence,
        record,
        key,
        stamp: format!("stamp-{caller}-{name}"),
        token,
    }
}
