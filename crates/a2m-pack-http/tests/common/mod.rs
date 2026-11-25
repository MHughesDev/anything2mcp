//! What the tests share: a registry that holds only the HTTP pack, the
//! fixture targets compiled through it, a prepared driver reached through
//! its handle, a real store for real intent tokens, and a real session
//! keeper for a real lease.

#![allow(
    dead_code,
    reason = "each test binary uses some of these helpers and not others"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use a2m_auth::{
    AskPerson, AuthEntry, CredentialRef, InProcessWork, Keeper, KeeperWorker, LeaseRequest,
    PersonAnswer, PersonAsk, ProviderSet, Purpose, RunSessionCheck, SessionCheckOutcome,
    SessionKey, SessionLease, SessionRef, SessionStore, StoredCredentials, Wait,
};
use a2m_core::{
    AccountName, CompiledBinding, CompiledTarget, DecisionId, Digest, EffectClass, Environment,
    Epoch, Failure, Identity, Mode, NoSecrets, OperationName, PrincipalId, RecordKey, Redact,
    RunId, RunState, Secret, Step, TargetName, Timestamp, Value, Values, WorkspaceId,
};
use a2m_files::{CompileMode, Compiled, Diagnostics, RelPath, TargetRoot};
use a2m_ledger::{
    Durability, Fence, IntentToken, NewRun, Opened, PlannedRecord, RecordIntent, RecordRef,
    RetentionClass, Store, StoreConfig, ToolKind,
};
use a2m_logs::{LogSettings, ManualClock};
use a2m_provider::{
    Authority, BoxFuture, Capture, CheckedPaths, Deadline, Destinations, DriverHandle,
    HostServices, Posture, PrepareContext, Prepared, ProviderSettings, Registry, RegistryBuilder,
    Roots, StepFailure, StepInput, StepOutcome, Supervisor, SupervisorLimits, SurfaceRef,
};
use a2m_vault::{MemoryKeyStore, RandomSource, Vault};

/// A folder under the system's temporary folder, removed when dropped.
pub struct Folder(PathBuf);

impl Folder {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "a2m-pack-http-{name}-{}-{}",
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

/// A registry that holds only the HTTP pack.
pub fn registry(folder: &Folder, redact: Arc<dyn Redact>) -> Registry {
    let logs = folder.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let host = HostServices {
        supervisor: Supervisor::new(SupervisorLimits::default()),
        roots: Roots::none(),
        redact: Arc::clone(&redact),
        logs_and_traces: a2m_logs::start(&LogSettings::default(), redact, &logs).unwrap(),
    };
    let mut builder = RegistryBuilder::new(ProviderSettings::default(), host);
    a2m_pack_http::add_to(&mut builder);
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    registry
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
    compile_changed_files(registry, name, &[(path, &change)])
}

/// A change to one file's text.
pub type Change<'a> = (&'a str, &'a dyn Fn(&str) -> String);

/// The same, with a change to each of several files.
pub fn compile_changed_files(
    registry: &Registry,
    name: &str,
    changes: &[Change<'_>],
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
    for (path, change) in changes {
        match files.iter_mut().find(|(known, _)| known.as_str() == *path) {
            Some((_, bytes)) => {
                *bytes = change(std::str::from_utf8(bytes).unwrap()).into_bytes();
            }
            None => files.push((RelPath::new(path).unwrap(), change("").into_bytes())),
        }
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

pub fn http_mode() -> Mode {
    Mode::new("http").unwrap()
}

/// What a list of steps gave when none of them failed.
#[derive(Debug, Default)]
pub struct Ran {
    pub values: Values,
    pub absent: bool,
    pub captures: Vec<Capture>,
}

impl Ran {
    pub fn capture(&self, name: &str) -> String {
        let capture = self
            .captures
            .iter()
            .find(|capture| capture.name == name)
            .unwrap_or_else(|| panic!("no capture named `{name}`"));
        String::from_utf8(capture.bytes.clone()).unwrap()
    }
}

/// One compiled fixture target, with its surface at the address of the
/// system a test started.
pub struct Scene {
    pub folder: Folder,
    pub registry: Registry,
    pub compiled_target: CompiledTarget,
    pub surface: SurfaceRef,
    pub destinations: Destinations,
}

impl Scene {
    pub fn new(fixture_name: &str, address: &str) -> Self {
        Self::with_redactor(fixture_name, address, Arc::new(NoSecrets))
    }

    pub fn with_redactor(fixture_name: &str, address: &str, redact: Arc<dyn Redact>) -> Self {
        let folder = Folder::new(fixture_name);
        let registry = registry(&folder, redact);
        let compiled_target = compile(&registry, fixture_name).compiled_target;
        Self::over(folder, registry, compiled_target, address)
    }

    pub fn over(
        folder: Folder,
        registry: Registry,
        compiled_target: CompiledTarget,
        address: &str,
    ) -> Self {
        let mut surface = compiled_target.surfaces[0].clone();
        surface.address = address.to_owned();
        Self {
            surface: SurfaceRef {
                target: compiled_target.header.target.clone(),
                environment: Environment::new("test").unwrap(),
                surface,
            },
            destinations: Destinations::new([address.to_owned()]),
            folder,
            registry,
            compiled_target,
        }
    }

    pub fn handle(&self) -> DriverHandle {
        self.handle_of(&http_mode())
    }

    /// The handle of the driver of one mode of the pack.
    pub fn handle_of(&self, mode: &Mode) -> DriverHandle {
        self.registry.driver(mode).unwrap()
    }

    pub async fn try_prepare(
        &self,
        lease: Option<&SessionLease>,
        fence: Option<Fence>,
        posture: Posture,
    ) -> Result<(DriverHandle, Prepared), StepFailure> {
        self.try_prepare_mode(&http_mode(), lease, fence, posture)
            .await
    }

    /// As [`Self::try_prepare`], for the driver of one mode.
    pub async fn try_prepare_mode(
        &self,
        mode: &Mode,
        lease: Option<&SessionLease>,
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
            compiled_target: &self.compiled_target,
            posture,
            deadline: Deadline::after(Duration::from_secs(30)),
            redact: &NoSecrets,
            destinations: &self.destinations,
            paths: &CheckedPaths::none(),
            supervisor: &supervisor,
            confirmed: a2m_provider::ConfirmedReads::none(),
        };
        let prepared = handle.prepare(&cx, lease).await?;
        Ok((handle, prepared))
    }

    /// A driver prepared for reading, with no session.
    pub async fn reader(&self) -> (DriverHandle, Prepared) {
        self.try_prepare(None, None, Posture::Live).await.unwrap()
    }

    /// A driver prepared for reading in one mode, with no session.
    pub async fn reader_of(&self, mode: &str) -> (DriverHandle, Prepared) {
        self.try_prepare_mode(&Mode::new(mode).unwrap(), None, None, Posture::Live)
            .await
            .unwrap()
    }

    /// The first `http` or `api` binding of an operation.
    pub fn binding(&self, operation: &str) -> &CompiledBinding {
        self.compiled_target
            .operations
            .iter()
            .find(|known| known.name.as_str() == operation)
            .unwrap_or_else(|| panic!("no operation named `{operation}`"))
            .bindings
            .iter()
            .find(|binding| binding.mode == http_mode() || binding.mode.as_str() == "api")
            .unwrap()
    }

    /// The arguments of a step, as the engine would evaluate them. Every
    /// expression of the fixtures is `input.<name>` or a quoted text.
    pub fn args(&self, step: &Step, inputs: &Values) -> Values {
        let mut args = Values::new();
        for arg in &step.args {
            let source = &self.compiled_target.expression(arg.value).unwrap().source;
            let quoted = |mark: char| {
                source
                    .strip_prefix(mark)
                    .and_then(|rest| rest.strip_suffix(mark))
            };
            if let Some(text) = quoted('\'').or_else(|| quoted('"')) {
                args.insert(arg.name.clone(), a2m_core::Value::Text(text.to_owned()));
                continue;
            }
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
        within: Duration,
    ) -> Result<Ran, StepFailure> {
        let mut ran = Ran::default();
        for step in steps {
            let args = self.args(step, inputs);
            let input = StepInput::new(&args, Deadline::after(within));
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
        self.run(
            handle,
            prepared,
            steps,
            inputs,
            None,
            Duration::from_secs(30),
        )
        .await
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
                    tool_name: OperationName::new("example_line_item_upsert").unwrap(),
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
                        operation: OperationName::new("example_line_item_upsert").unwrap(),
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

// ---- a real keeper, for a real lease ----

struct NoPerson;

impl AskPerson for NoPerson {
    fn ask<'a>(&'a self, _session: &'a SessionRef, _ask: PersonAsk) -> BoxFuture<'a, PersonAnswer> {
        Box::pin(async { PersonAnswer::NoAnswer })
    }
}

struct SignedInCheck;

impl RunSessionCheck for SignedInCheck {
    fn check_session<'a>(&'a self, _session: &'a SessionRef) -> BoxFuture<'a, SessionCheckOutcome> {
        Box::pin(async { SessionCheckOutcome::SignedIn })
    }
}

/// A session keeper over memory, holding one stored credential.
pub struct Signed {
    pub vault: Arc<Vault>,
    pub keeper: Keeper,
    key: SessionKey,
    entry: Arc<AuthEntry>,
}

impl Signed {
    /// A keeper whose one session signs in with `provider`, the settings
    /// given, and one stored credential under `role`.
    pub async fn new(provider: &str, settings: &[(&str, &str)], role: &str, secret: &str) -> Self {
        let (ledger, vault) = store().await;
        let clock = clock();
        let sessions = SessionStore::new(ledger);
        let credentials = StoredCredentials::new(sessions.clone(), Arc::clone(&vault));
        let providers = ProviderSet::neutral();
        let worker = Arc::new(KeeperWorker::new(
            providers.clone(),
            Arc::clone(&vault),
            Arc::new(credentials.clone()),
            clock.clone(),
        ));
        let work = Arc::new(InProcessWork::new(
            Arc::clone(&worker),
            Arc::new(NoPerson),
            Arc::new(SignedInCheck),
        ));
        let keeper = Keeper::builder(sessions, providers, work, clock)
            .local_worker(worker)
            .build();
        let key = SessionKey {
            workspace: WorkspaceId::from_bytes([7; 16]),
            target: TargetName::new("example").unwrap(),
            environment: Environment::new("test").unwrap(),
            identity: Identity::Account(AccountName::new("service").unwrap()),
        };
        let reference = CredentialRef::new(&format!("vault://example/service/{role}")).unwrap();
        credentials
            .put(
                &key.workspace,
                &key.identity,
                &reference,
                &Secret::new(secret),
            )
            .await
            .unwrap();
        let mut entry = AuthEntry::new(a2m_auth::ProviderName::new(provider).unwrap());
        for (name, value) in settings {
            entry = entry.setting(name, value);
        }
        Self {
            vault,
            keeper,
            key,
            // A run's sign-in is a published version's (decision 0248).
            entry: Arc::new(entry.credential(role, reference).published()),
        }
    }

    pub async fn lease(&self) -> SessionLease {
        self.keeper
            .lease(LeaseRequest {
                key: self.key.clone(),
                entry: Arc::clone(&self.entry),
                run: RunId::from_bytes([3; 16]),
                decided: DecisionId::from_bytes([4; 16]),
                acting_for: None,
                wait: Wait::DEFAULT,
                headroom: None,
                purpose: Purpose::Run,
            })
            .await
            .unwrap()
    }
}

// ---- a check of a result against a tool's output schema ----

/// Whether `value` fits `schema`, for the words of JSON Schema that the
/// compiler writes in an output schema. A word it does not know is a panic,
/// so that the schema cannot grow a promise this check does not hold.
pub fn schema_fits(schema: &serde_json::Value, value: &serde_json::Value) -> Result<(), String> {
    use serde_json::Value as Json;
    let Json::Object(words) = schema else {
        return Err(format!("{schema} is not a schema"));
    };
    for (word, rule) in words {
        match word.as_str() {
            // Words that say what a value is, and hold it to nothing: the
            // untrusted mark tells an agent the text is data (decision 0460).
            "description" | "title" | "x-anything2mcp-untrusted" => {}
            "type" => {
                let kind = rule.as_str().unwrap_or_default();
                let fits = match (kind, value) {
                    ("object", Json::Object(_))
                    | ("array", Json::Array(_))
                    | ("string", Json::String(_))
                    | ("boolean", Json::Bool(_))
                    | ("null", Json::Null) => true,
                    ("integer", Json::Number(number)) => number.is_i64() || number.is_u64(),
                    _ => false,
                };
                if !fits {
                    return Err(format!("{value} is not of the type {kind}"));
                }
            }
            "const" => {
                if rule != value {
                    return Err(format!("{value} is not the constant {rule}"));
                }
            }
            "enum" => {
                if !rule.as_array().is_some_and(|words| words.contains(value)) {
                    return Err(format!("{value} is not one of {rule}"));
                }
            }
            "anyOf" => {
                let arms = rule.as_array().cloned().unwrap_or_default();
                if !arms.iter().any(|arm| schema_fits(arm, value).is_ok()) {
                    return Err(format!("{value} fits none of the arms of {rule}"));
                }
            }
            "required" => {
                for name in rule.as_array().into_iter().flatten() {
                    let name = name.as_str().unwrap_or_default();
                    if value.get(name).is_none() {
                        return Err(format!("`{name}` is missing"));
                    }
                }
            }
            "properties" => {
                if let (Json::Object(held), Json::Object(declared)) = (value, rule) {
                    for (name, inner) in held {
                        if let Some(property) = declared.get(name) {
                            schema_fits(property, inner).map_err(|why| format!("{name}: {why}"))?;
                        }
                    }
                }
            }
            "items" => {
                for item in value.as_array().into_iter().flatten() {
                    schema_fits(rule, item)?;
                }
            }
            other => panic!("the check does not know the schema word `{other}`"),
        }
    }
    Ok(())
}
