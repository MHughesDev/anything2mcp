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
    CompiledBinding, CompiledTarget, Digest, EffectClass, Environment, Epoch, Failure, NoSecrets,
    OperationName, PrincipalId, RecordKey, RunState, Step, TargetName, Timestamp, Value, Values,
    WorkspaceId,
};
use a2m_expr::{Bindings, Program};
use a2m_files::{CompileMode, CompilerSet, Diagnostics, TargetRoot};
use a2m_ledger::{
    Durability, Fence, IntentToken, NewRun, Opened, PlannedRecord, RecordIntent, RecordRef,
    RetentionClass, Store, StoreConfig, ToolKind,
};
use a2m_logs::{LogLevel, LogsAndTraces, ManualClock};
use a2m_provider::{
    Access, Authority, CheckedPaths, Deadline, Destinations, DriverHandle, HostServices, Posture,
    PrepareContext, Prepared, ProviderSettings, Registry, RegistryBuilder, RootName, Roots,
    StepFailure, StepInput, StepOutcome, Supervisor, SupervisorLimits, SurfaceRef,
};
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
        logs_and_traces: LogsAndTraces::in_memory(LogLevel::Info, Arc::new(NoSecrets), clock),
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

/// The run's paths with `folder` as the project folder of the request that
/// started it, checked as the engine checks it: as a root of its own, for
/// writing.
pub fn project_paths(folder: &Path) -> CheckedPaths {
    let name = RootName::new("project").unwrap();
    let mut roots = Roots::none();
    roots.add(name.clone(), folder, Access::Write).unwrap();
    CheckedPaths::none().with_project(roots.folder(&name, Access::Write).unwrap())
}

/// Compiles the target folder at `root` with the set given.
pub fn compile(root: &Path, set: &CompilerSet) -> Result<a2m_files::Compiled, Diagnostics> {
    let sources = a2m_files::load(&TargetRoot::new(root), set)?;
    a2m_files::compile(&sources, set, CompileMode::Draft, a2m_files::DRAFT_VERSION)
}

/// The stand, the registry over it, and the compiled example target.
pub struct Scene {
    pub stand: FileStand,
    pub registry: Registry,
    pub compiled_target: CompiledTarget,
    /// The root the surfaces are in, when it is not the one they state.
    root: Option<&'static str>,
    /// The run's checked paths: with a project folder, for a scene whose
    /// surfaces are in the root `project`.
    pub paths: CheckedPaths,
}

impl Scene {
    pub fn new() -> Self {
        Self::with_access(Access::Write)
    }

    /// With `Access::Read`, the surfaces are in the root `project`: their
    /// files are the stand's, as the project folder of the request that
    /// started the run, and the pack's own root of that name, the folder the
    /// engine was started in, is the stand's folder outside its root, where
    /// no document's file is.
    pub fn with_access(access: Access) -> Self {
        let stand = FileStand::start().unwrap();
        let (roots, paths) = match access {
            Access::Write => (roots(&stand, access), CheckedPaths::none()),
            Access::Read => {
                let mut started_in = Roots::none();
                started_in
                    .add(
                        RootName::new("project").unwrap(),
                        stand.outside_folder(),
                        Access::Read,
                    )
                    .unwrap();
                (started_in, project_paths(stand.root()))
            }
        };
        let registry = registry(roots);
        let compiled_target = compile(&fixtures(), registry.compiler_set())
            .unwrap_or_else(|problems| panic!("the fixtures do not compile: {problems:?}"))
            .compiled_target;
        Self {
            stand,
            registry,
            compiled_target,
            root: (access == Access::Read).then_some("project"),
            paths,
        }
    }

    /// The surfaces in the root `files`, which here stands for the project
    /// folder of each request (`Roots::add_project`), as a served engine
    /// holds it: the run's project folder is the stand's root, and a run
    /// given another project's paths resolves in that project.
    pub fn with_project_files() -> Self {
        let stand = FileStand::start().unwrap();
        let mut roots = Roots::none();
        roots
            .add_project(RootName::new("files").unwrap(), Access::Write)
            .unwrap();
        let registry = registry(roots);
        let compiled_target = compile(&fixtures(), registry.compiler_set())
            .unwrap_or_else(|problems| panic!("the fixtures do not compile: {problems:?}"))
            .compiled_target;
        let paths = project_paths(stand.root());
        Self {
            stand,
            registry,
            compiled_target,
            root: None,
            paths,
        }
    }

    pub fn surface(&self, name: &str) -> SurfaceRef {
        let mut surface = self
            .compiled_target
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
            target: self.compiled_target.header.target.clone(),
            environment: Environment::new("test").unwrap(),
            surface,
        }
    }

    /// The one binding of an operation, and the surface it acts on.
    pub fn binding(&self, operation: &str) -> (&CompiledBinding, SurfaceRef) {
        let binding = self
            .compiled_target
            .operations
            .iter()
            .find(|found| found.name.as_str() == operation)
            .unwrap_or_else(|| panic!("no operation `{operation}`"))
            .bindings
            .first()
            .unwrap();
        let surface = self
            .compiled_target
            .surface(binding.surface)
            .unwrap()
            .name
            .clone();
        (binding, self.surface(surface.as_str()))
    }

    pub async fn prepared(
        &self,
        surface: &SurfaceRef,
        fence: Option<Fence>,
        posture: Posture,
    ) -> (DriverHandle, Prepared) {
        self.prepared_in(surface, fence, posture, &self.paths)
            .await
            .unwrap()
    }

    /// The same, with the run's checked paths given.
    pub async fn prepared_in(
        &self,
        surface: &SurfaceRef,
        fence: Option<Fence>,
        posture: Posture,
        paths: &CheckedPaths,
    ) -> Result<(DriverHandle, Prepared), StepFailure> {
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
            compiled_target: &self.compiled_target,
            posture,
            deadline: Deadline::after(Duration::from_secs(30)),
            redact: &NoSecrets,
            destinations: &Destinations::default(),
            paths,
            supervisor: &supervisor,
        };
        let prepared = handle.prepare(&cx, None).await?;
        Ok((handle, prepared))
    }

    /// A step's arguments, worked out from the operation's inputs as the
    /// engine will work them out.
    pub fn args(&self, step: &Step, inputs: &Values) -> Values {
        let input = Value::Record(inputs.clone());
        let mut args = Values::new();
        for arg in &step.args {
            let program =
                Program::load(self.compiled_target.expression(arg.value).unwrap()).unwrap();
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
            let input = StepInput::new(&args, Deadline::after(Duration::from_secs(30)));
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
            let input = StepInput::new(&args, Deadline::after(Duration::from_secs(30))).for_record(
                granted.record,
                Some(&granted.key),
                Some(&granted.stamp),
            );
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

/// What a read returns through the engine: each output the operation
/// declares, taken by its name from what the last step observed and
/// normalized by its field type. The engine's own shaping (`outputs_of` in
/// `a2m-engine`) does the same, which a pack cannot call: it may not depend
/// on the engine. A value the step gave that no output declares is not
/// returned; an output the step gave no value for stops the test, as it
/// stops a read in the engine. A list output is normalized item by item,
/// and a `null` item stays `null`, as the engine does (decision 0180).
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
                let one = |value: &Value| {
                    if value.is_null() {
                        Value::Null
                    } else {
                        normalizer.normalize(value).unwrap().normalized
                    }
                };
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
        out.insert(output.name.clone(), value);
    }
    out
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

/// The checksum a package keeps of each part.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}

/// A package of parts, each stored as it is, built byte by byte: the
/// container a workbook is kept in, with no office suite to make it.
pub fn stored_package(parts: &[(&str, &str)]) -> Vec<u8> {
    let put16 = |out: &mut Vec<u8>, value: usize| {
        out.extend_from_slice(&u16::try_from(value).unwrap().to_le_bytes());
    };
    let put32 = |out: &mut Vec<u8>, value: usize| {
        out.extend_from_slice(&u32::try_from(value).unwrap().to_le_bytes());
    };
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (name, content) in parts {
        let content = content.as_bytes();
        let offset = out.len();
        let checksum = crc32(content);
        for (bytes, local) in [(&mut out, true), (&mut directory, false)] {
            if local {
                put32(bytes, 0x0403_4b50);
            } else {
                put32(bytes, 0x0201_4b50);
                put16(bytes, 20); // version made by
            }
            put16(bytes, 20); // version needed
            put16(bytes, 0); // flags
            put16(bytes, 0); // method: stored
            put16(bytes, 0); // time
            put16(bytes, 0x0021); // date: 1 January 1980
            bytes.extend_from_slice(&checksum.to_le_bytes());
            put32(bytes, content.len());
            put32(bytes, content.len());
            put16(bytes, name.len());
            put16(bytes, 0); // extra field
            if local {
                bytes.extend_from_slice(name.as_bytes());
                bytes.extend_from_slice(content);
            } else {
                put16(bytes, 0); // comment
                put16(bytes, 0); // disk
                put16(bytes, 0); // internal attributes
                put32(bytes, 0); // external attributes
                put32(bytes, offset);
                bytes.extend_from_slice(name.as_bytes());
            }
        }
    }
    let directory_offset = out.len();
    out.extend_from_slice(&directory);
    put32(&mut out, 0x0605_4b50);
    put16(&mut out, 0);
    put16(&mut out, 0);
    put16(&mut out, parts.len());
    put16(&mut out, parts.len());
    put32(&mut out, directory.len());
    put32(&mut out, directory_offset);
    put16(&mut out, 0);
    out
}

/// A workbook as an open-source office suite saves a spreadsheet a person
/// keeps: one sheet with its rows under a header row, no table object, no
/// defined name, strings shared, and the sheet's rows spanning only the
/// cells that hold something. The parts are those the suite writes, cut
/// to what a reader needs; the data is invented.
pub fn office_suite_workbook() -> Vec<u8> {
    const HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
    let content_types = format!(
        "{HEAD}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/><Override PartName=\"/xl/sharedStrings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml\"/></Types>"
    );
    let root_rels = format!(
        "{HEAD}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>"
    );
    let workbook = format!(
        "{HEAD}<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><fileVersion appName=\"Calc\"/><workbookPr backupFile=\"false\" showObjects=\"all\" dateCompatibility=\"false\"/><workbookProtection/><bookViews><workbookView showHorizontalScroll=\"true\" showVerticalScroll=\"true\" showSheetTabs=\"true\" xWindow=\"0\" yWindow=\"0\" windowWidth=\"16384\" windowHeight=\"8192\" tabRatio=\"500\" firstSheet=\"0\" activeTab=\"0\"/></bookViews><sheets><sheet name=\"Parts\" sheetId=\"1\" state=\"visible\" r:id=\"rId3\"/></sheets><calcPr iterateCount=\"100\" refMode=\"A1\" iterate=\"false\" iterateDelta=\"0.001\"/></workbook>"
    );
    let workbook_rels = format!(
        "{HEAD}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/><Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/><Relationship Id=\"rId4\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings\" Target=\"sharedStrings.xml\"/></Relationships>"
    );
    let styles = format!(
        "{HEAD}<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><fonts count=\"1\"><font><sz val=\"10\"/><name val=\"Arial\"/></font></fonts><fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills><borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs><cellXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/></cellXfs></styleSheet>"
    );
    // The headers, then the text of the rows, in the order the suite
    // writes its shared strings: as it meets them.
    let strings = [
        "part",
        "stock",
        "price",
        "note",
        "washer",
        "bolt",
        "nut",
        "back order",
    ];
    let shared = strings
        .iter()
        .map(|text| format!("<si><t xml:space=\"preserve\">{text}</t></si>"))
        .collect::<String>();
    let shared = format!(
        "{HEAD}<sst xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" count=\"{n}\" uniqueCount=\"{n}\">{shared}</sst>",
        n = strings.len()
    );
    let row = |number: u32, cells: &str| {
        format!(
            "<row r=\"{number}\" customFormat=\"false\" ht=\"12.8\" hidden=\"false\" customHeight=\"false\" outlineLevel=\"0\" collapsed=\"false\">{cells}</row>"
        )
    };
    let text = |at: &str, string: u32| format!("<c r=\"{at}\" s=\"0\" t=\"s\"><v>{string}</v></c>");
    let number =
        |at: &str, value: &str| format!("<c r=\"{at}\" s=\"0\" t=\"n\"><v>{value}</v></c>");
    let rows = [
        row(
            1,
            &(text("A1", 0) + &text("B1", 1) + &text("C1", 2) + &text("D1", 3)),
        ),
        // A cell with nothing in it is not written at all.
        row(
            2,
            &(text("A2", 4) + &number("B2", "12") + &number("C2", "1.5")),
        ),
        row(3, &(text("A3", 5) + &number("C3", "0.25") + &text("D3", 7))),
        row(4, &(text("A4", 6) + &number("B4", "7"))),
    ]
    .concat();
    let sheet = format!(
        "{HEAD}<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheetPr filterMode=\"false\"><pageSetUpPr fitToPage=\"false\"/></sheetPr><dimension ref=\"A1:D4\"/><sheetFormatPr defaultColWidth=\"11.53515625\" defaultRowHeight=\"12.8\" zeroHeight=\"false\" outlineLevelRow=\"0\" outlineLevelCol=\"0\"></sheetFormatPr><sheetData>{rows}</sheetData><pageMargins left=\"0.7875\" right=\"0.7875\" top=\"1.05277777777778\" bottom=\"1.05277777777778\" header=\"0.7875\" footer=\"0.7875\"/></worksheet>"
    );
    stored_package(&[
        ("[Content_Types].xml", &content_types),
        ("_rels/.rels", &root_rels),
        ("xl/workbook.xml", &workbook),
        ("xl/_rels/workbook.xml.rels", &workbook_rels),
        ("xl/styles.xml", &styles),
        ("xl/sharedStrings.xml", &shared),
        ("xl/worksheets/sheet1.xml", &sheet),
    ])
}
