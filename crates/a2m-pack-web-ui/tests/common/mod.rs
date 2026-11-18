//! What the tests share: a registry that holds only the web pack's
//! drivers, the fixture target compiled through it, a prepared driver
//! reached through its handle, and a real store for real intent tokens.
//!
//! A test that needs a browser fails with `browser_unavailable` when none
//! is found; it never passes by skipping. On a machine whose browser is
//! not in a known place, or whose tests run as the superuser, the test
//! environment says so through the two variables of the browser engine's
//! `test_support`.

#![allow(
    dead_code,
    reason = "each test binary uses some of these helpers and not others"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use a2m_core::{
    BindingPlan, Digest, EffectClass, Environment, Epoch, Failure, Mode, NoSecrets, OperationName,
    Plan, PrincipalId, RecordKey, Redact, RunId, RunState, Step, TargetName, Timestamp, Value,
    Values, WorkspaceId,
};
use a2m_files::{CompileMode, Compiled, Diagnostics, RelPath, TargetRoot};
use a2m_ledger::{
    Durability, Fence, IntentToken, NewRun, Opened, PlannedRecord, RecordIntent, RecordRef,
    RetentionClass, Store, StoreConfig, ToolKind,
};
use a2m_pack_web_browser::{PROFILE_ROOT, test_support};
use a2m_pack_web_ui::SharedBrowsers;
use a2m_provider::{
    Access, Authority, Capture, CheckedPaths, Deadline, Destinations, DriverHandle, HostServices,
    Pack, PackContext, Posture, PrepareContext, Prepared, ProviderSettings, Registrar, Registry,
    RegistryBuilder, RootName, Roots, StepFailure, StepInput, StepOutcome, Supervisor,
    SupervisorLimits, SurfaceRef,
};
use a2m_telemetry::{ManualClock, TelemetrySettings};
use a2m_vault::{MemoryKeyStore, RandomSource, Vault};

/// A folder under the system's temporary folder, removed when dropped.
pub struct Folder(PathBuf);

impl Folder {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "a2m-pack-web-ui-{name}-{}-{}",
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

/// The pack as the product registers it, with the two things a test
/// machine may need: the browser program the test environment names is
/// declared in the manifest, and the browsers are built with the test's
/// settings before the pack asks for them.
struct TestPack {
    manifest: &'static str,
    browsers: SharedBrowsers,
    /// Drafting rules a test adds to the pack's own, each declared in the
    /// manifest.
    rules: Vec<Arc<dyn a2m_files::DraftingRule>>,
}

impl Pack for TestPack {
    fn manifest(&self) -> &'static str {
        self.manifest
    }

    fn register(&self, cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        self.browsers.get_or_create(cx, test_support::settings())?;
        a2m_pack_web_ui::pack(self.browsers.clone()).register(cx, to)?;
        if self.rules.is_empty() {
            return Ok(());
        }
        let mut more = a2m_provider::CompilerRegistrations::new();
        for rule in &self.rules {
            more = more.drafting_rule(Arc::clone(rule));
        }
        to.compiler(more)
    }
}

/// The pack's own manifest, with its requirement replaced by the test
/// environment's, and `more` added before it.
fn test_manifest(more: &str) -> &'static str {
    let own = a2m_pack_web_ui::pack(SharedBrowsers::default()).manifest();
    let (before, _) = own
        .split_once("\n[[requirements]]")
        .expect("the manifest ends with its requirement");
    Box::leak(format!("{before}{more}{}", test_support::requirement()).into_boxed_str())
}

/// A registry that holds only the web pack's drivers, and what a test
/// needs to end it cleanly.
pub struct Stand {
    pub folder: Folder,
    pub registry: Registry,
    pub supervisor: Supervisor,
    pub browsers: SharedBrowsers,
}

impl Stand {
    pub fn new(name: &str, redact: Arc<dyn Redact>) -> Self {
        Self::with_rules(name, redact, Vec::new())
    }

    /// The same, with drafting rules of a test's own added to the pack's,
    /// each from a finding kind of its own to a file kind of the pack's.
    pub fn with_rules(
        name: &str,
        redact: Arc<dyn Redact>,
        rules: Vec<Arc<dyn a2m_files::DraftingRule>>,
    ) -> Self {
        let folder = Folder::new(name);
        let profiles = folder.path().join("profiles");
        let logs = folder.path().join("logs");
        std::fs::create_dir_all(&profiles).unwrap();
        std::fs::create_dir_all(&logs).unwrap();
        let mut roots = Roots::none();
        roots
            .add(
                RootName::new(PROFILE_ROOT).unwrap(),
                &profiles,
                Access::Write,
            )
            .unwrap();
        let supervisor = Supervisor::new(SupervisorLimits::default());
        let host = HostServices {
            supervisor: supervisor.clone(),
            roots,
            redact: Arc::clone(&redact),
            telemetry: a2m_telemetry::start(&TelemetrySettings::default(), redact, &logs).unwrap(),
        };
        let browsers = SharedBrowsers::default();
        let mut builder = RegistryBuilder::new(ProviderSettings::default(), host);
        let declared: String = rules
            .iter()
            .map(|rule| {
                format!(
                    "\n[[drafting_rules]]\nfinding_kind = \"{}\"\nfile_kind = \"{}\"\n",
                    rule.finding_kind().as_str(),
                    rule.file_kind()
                )
            })
            .collect();
        builder.add_pack(&TestPack {
            manifest: test_manifest(&declared),
            browsers: browsers.clone(),
            rules,
        });
        let (registry, problems) = builder.build();
        assert!(problems.is_empty(), "{problems:?}");
        Self {
            folder,
            registry,
            supervisor,
            browsers,
        }
    }

    /// Closes every browser and checks that nothing is left.
    pub async fn end(self) {
        if let Some(browsers) = self.browsers.get() {
            let cleanup = browsers.shutdown().await;
            assert!(cleanup.complete, "{cleanup:?}");
        }
        assert!(self.supervisor.list().is_empty());
    }
}

impl Drop for Stand {
    /// A test that failed before its end leaves no browser behind.
    fn drop(&mut self) {
        for left in self.supervisor.list() {
            kill(left.process);
        }
    }
}

/// Ends a process from outside, as a crash would.
pub fn kill(process: u32) {
    let _ = if cfg!(windows) {
        std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &process.to_string()])
            .output()
    } else {
        std::process::Command::new("kill")
            .args(["-9", &process.to_string()])
            .output()
    };
}

/// The folder of one of this crate's fixture targets.
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Compiles a fixture target as it is on disk.
pub fn compile(registry: &Registry, name: &str) -> Compiled {
    let set = registry.compiler_set();
    let sources = a2m_files::load(&TargetRoot::new(fixture(name)), set)
        .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
    a2m_files::compile(&sources, set, CompileMode::Draft, a2m_files::DRAFT_VERSION)
        .unwrap_or_else(|problems| panic!("{}", shown(&problems)))
}

/// Every file of a fixture target, with `change` applied to the text of
/// the one at `path`; a file that is not there is added.
pub fn compile_changed(
    registry: &Registry,
    name: &str,
    path: &str,
    change: impl Fn(&str) -> String,
) -> Result<Compiled, Diagnostics> {
    fn walk(root: &Path, folder: &Path, out: &mut Vec<(RelPath, Vec<u8>)>) {
        let mut entries: Vec<_> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for entry in entries {
            if entry.is_dir() {
                walk(root, &entry, out);
            } else {
                let relative: Vec<String> = entry
                    .strip_prefix(root)
                    .unwrap()
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.push((
                    RelPath::new(&relative.join("/")).unwrap(),
                    std::fs::read(&entry).unwrap(),
                ));
            }
        }
    }
    let root = fixture(name);
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    match files.iter_mut().find(|(known, _)| known.as_str() == path) {
        Some((_, bytes)) => {
            *bytes = change(std::str::from_utf8(bytes).unwrap()).into_bytes();
        }
        None => files.push((RelPath::new(path).unwrap(), change("").into_bytes())),
    }
    let set = registry.compiler_set();
    let sources = a2m_files::load_files(&TargetRoot::new(root), &files, set)?;
    a2m_files::compile(&sources, set, CompileMode::Draft, a2m_files::DRAFT_VERSION)
}

pub fn shown(problems: &Diagnostics) -> String {
    problems
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn ui_mode() -> Mode {
    Mode::new("ui").unwrap()
}

pub fn page_http_mode() -> Mode {
    Mode::new("page_http").unwrap()
}

/// What a list of steps gave when none of them failed.
#[derive(Debug, Default)]
pub struct Ran {
    pub values: Values,
    pub absent: bool,
    pub captures: Vec<Capture>,
}

impl Ran {
    /// Every capture of one name, as text, in the order of the steps.
    pub fn captures(&self, name: &str) -> Vec<String> {
        self.captures
            .iter()
            .filter(|capture| capture.name == name)
            .map(|capture| String::from_utf8(capture.bytes.clone()).unwrap())
            .collect()
    }

    /// What every step did, in order: the part of the trace two runs that
    /// did the same thing share.
    pub fn means(&self) -> Vec<Vec<a2m_telemetry::Detail>> {
        self.captures("acting")
            .iter()
            .map(|acting| {
                serde_json::from_str::<a2m_telemetry::Acting>(acting)
                    .unwrap()
                    .means
            })
            .collect()
    }

    /// The same, as text a test can search: one `name=value` for each
    /// detail. A detail's debug form shows only that it is there.
    pub fn means_text(&self) -> String {
        self.means()
            .iter()
            .flatten()
            .map(|detail| format!("{}={}", detail.name, detail.value))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// One compiled fixture target, with its surface at the address of the
/// system a test started.
pub struct Scene {
    pub stand: Stand,
    pub plan: Plan,
    pub surface: SurfaceRef,
    pub destinations: Destinations,
}

impl Scene {
    /// The `example` fixture as it is on disk.
    pub fn new(name: &str, address: &str) -> Self {
        let stand = Stand::new(name, Arc::new(NoSecrets));
        let plan = compile(&stand.registry, "example").plan;
        Self::over(stand, plan, address)
    }

    pub fn over(stand: Stand, plan: Plan, address: &str) -> Self {
        let mut surface = plan.surfaces[0].clone();
        surface.address = address.to_owned();
        Self {
            surface: SurfaceRef {
                target: plan.header.target.clone(),
                environment: Environment::new("test").unwrap(),
                surface,
            },
            destinations: Destinations::new([address.to_owned()]),
            stand,
            plan,
        }
    }

    pub fn handle(&self) -> DriverHandle {
        self.handle_of(&ui_mode())
    }

    /// The registry's handle on the driver of a mode.
    pub fn handle_of(&self, mode: &Mode) -> DriverHandle {
        self.stand.registry.driver(mode).unwrap()
    }

    pub async fn try_prepare(
        &self,
        fence: Option<Fence>,
        posture: Posture,
    ) -> Result<(DriverHandle, Prepared), StepFailure> {
        self.try_prepare_in(&ui_mode(), fence, posture).await
    }

    /// The same, for the driver of a mode.
    pub async fn try_prepare_in(
        &self,
        mode: &Mode,
        fence: Option<Fence>,
        posture: Posture,
    ) -> Result<(DriverHandle, Prepared), StepFailure> {
        let handle = self.handle_of(mode);
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
            deadline: Deadline::after(Duration::from_secs(60)),
            redact: &NoSecrets,
            destinations: &self.destinations,
            paths: &CheckedPaths::none(),
            supervisor: &supervisor,
        };
        let prepared = handle.prepare(&cx, None).await?;
        Ok((handle, prepared))
    }

    /// A driver prepared for reading. With no browser on the machine this
    /// is where a test fails, by the named failure.
    pub async fn reader(&self) -> (DriverHandle, Prepared) {
        match self.try_prepare(None, Posture::Live).await {
            Ok(prepared) => prepared,
            Err(failure) => panic!(
                "{}/{}: {failure}",
                failure.failure.class(),
                failure.failure.code()
            ),
        }
    }

    /// The first `ui` binding of an operation.
    pub fn binding(&self, operation: &str) -> &BindingPlan {
        self.binding_in(operation, &ui_mode())
    }

    /// The first binding of an operation in a mode.
    pub fn binding_in(&self, operation: &str, mode: &Mode) -> &BindingPlan {
        self.plan
            .operations
            .iter()
            .find(|known| known.name.as_str() == operation)
            .unwrap_or_else(|| panic!("no operation named `{operation}`"))
            .bindings
            .iter()
            .find(|binding| binding.mode == *mode)
            .unwrap()
    }

    /// The arguments of a step, as the engine would evaluate them. Every
    /// expression of the fixtures is `input.<name>`.
    pub fn args(&self, step: &Step, inputs: &Values) -> Values {
        let mut args = Values::new();
        for arg in &step.args {
            let source = &self.plan.expression(arg.value).unwrap().source;
            let name = source
                .strip_prefix("input.")
                .unwrap_or_else(|| panic!("the test helper cannot evaluate `{source}`"));
            if let Some(value) = inputs.get(name) {
                args.insert(arg.name.clone(), value.clone());
            }
        }
        args
    }

    /// Carries out a list of steps in order through the handle, as the
    /// engine would: it stops at the first failure and at the first step
    /// that finds its record absent.
    pub async fn run(
        &self,
        handle: &DriverHandle,
        prepared: &mut Prepared,
        steps: &[Step],
        inputs: &Values,
        write: Option<(&IntentToken, RecordRef, &RecordKey)>,
    ) -> Result<Ran, StepFailure> {
        let mut ran = Ran::default();
        for step in steps {
            let args = self.args(step, inputs);
            let input = StepInput::new(&args, Deadline::after(Duration::from_secs(60)));
            let input = match write {
                Some((_, record, key)) => input.for_record(record, Some(key), None),
                None => input,
            };
            let authority = match write {
                Some((token, _, _)) if step.commits => Authority::Write(token),
                _ => Authority::Read,
            };
            let outcome = handle.act(prepared, step, &input, authority).await?;
            let (observed, absent) = match outcome {
                StepOutcome::Done(observed) => (observed, false),
                StepOutcome::Absent(observed) => (observed, true),
            };
            for (name, value) in observed.values {
                ran.values.insert(name, value);
            }
            ran.captures.extend(observed.captures);
            if absent {
                ran.absent = true;
                break;
            }
        }
        Ok(ran)
    }

    /// Reads through the `act` steps of a read operation.
    pub async fn read(
        &self,
        handle: &DriverHandle,
        prepared: &mut Prepared,
        operation: &str,
        inputs: &Values,
    ) -> Result<Ran, StepFailure> {
        let steps = &self.binding(operation).act;
        self.run(handle, prepared, steps, inputs, None).await
    }

    /// Reads through the `act` steps of a read operation's binding in a
    /// mode.
    pub async fn read_in(
        &self,
        mode: &Mode,
        handle: &DriverHandle,
        prepared: &mut Prepared,
        operation: &str,
        inputs: &Values,
    ) -> Result<Ran, StepFailure> {
        let steps = &self.binding_in(operation, mode).act;
        self.run(handle, prepared, steps, inputs, None).await
    }
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

fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(
        1_800_000_000_000,
    )))
}

pub async fn store() -> (Store, Arc<Vault>) {
    let config = StoreConfig::in_memory()
        .with_clock(clock())
        .with_random(counted());
    let store = Store::open(config).await.unwrap();
    let vault = Arc::new(Vault::with_random(
        Arc::new(MemoryKeyStore::with_random(counted())),
        Arc::new(store.key_records()),
        counted(),
    ));
    store.attach(Arc::clone(&vault)).await.unwrap();
    (store, vault)
}

pub fn key(parts: &[&str]) -> RecordKey {
    RecordKey::new(parts.iter().map(|part| Value::from(*part)).collect()).unwrap()
}

fn intended(key: &RecordKey) -> Digest {
    Digest::of(&key.canonical_bytes())
}

/// A write run with the records planned, approved, and running.
pub async fn running(store: &Store, keys: &[RecordKey]) -> Fence {
    let keys = keys.to_vec();
    let workspace = WorkspaceId::from_bytes([1; 16]);
    store
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
                    input_hash: Digest::of(&keys[0].canonical_bytes()),
                    input: None,
                    retention: RetentionClass::Long,
                })?
                else {
                    panic!("the run exists already");
                };
                let fence = ledger.new_epoch(opened.run)?;
                ledger.set_state(&fence, RunState::Validating, None)?;
                let records: Vec<_> = keys
                    .iter()
                    .enumerate()
                    .map(|(at, key)| PlannedRecord {
                        step_seq: 0,
                        record_seq: u32::try_from(at).unwrap(),
                        key: key.clone(),
                        operation: OperationName::new("example_line_item_update").unwrap(),
                        intended: intended(key),
                        intended_values: None,
                        before: None,
                        unchanged: false,
                    })
                    .collect();
                ledger.plan_records(&fence, &records)?;
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
        .unwrap()
}

pub fn record(fence: &Fence, record_seq: u32) -> RecordRef {
    RecordRef {
        run: fence.run,
        step_seq: 0,
        record_seq,
    }
}

/// The ledger's intent token for one planned record: the lock is taken and
/// the intent is stored, as the engine does before a write is sent.
pub async fn token(store: &Store, fence: Fence, record_seq: u32, key: &RecordKey) -> IntentToken {
    store
        .write(Durability::Synced, move |tx| {
            tx.ledger()
                .take_lock(&fence, record(&fence, record_seq))
                .map(|_| ())
        })
        .await
        .unwrap();
    let intents = vec![RecordIntent {
        record: record(&fence, record_seq),
        intended: intended(key),
        mode: None,
    }];
    store
        .store_intents(&fence, intents)
        .await
        .unwrap()
        .remove(0)
}

/// A write run for one record, with its intent stored: what the engine has
/// in hand when it reaches a commit point.
pub struct Write {
    pub fence: Fence,
    pub key: RecordKey,
    pub token: IntentToken,
}

impl Write {
    pub async fn new(parts: &[&str]) -> Self {
        let (store, _vault) = store().await;
        let key = key(parts);
        let fence = running(&store, std::slice::from_ref(&key)).await;
        let token = token(&store, fence, 0, &key).await;
        Self { fence, key, token }
    }

    pub fn authority(&self) -> (&IntentToken, RecordRef, &RecordKey) {
        (&self.token, record(&self.fence, 0), &self.key)
    }
}
