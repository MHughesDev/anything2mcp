//! What the tests share: a registry that holds only the MCP pack, the
//! pack's fixture target compiled for a stand, a real store for real intent
//! tokens, and a keeper for real leases.

#![allow(
    dead_code,
    reason = "each test binary uses some of these helpers and not others"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use a2m_core::{
    CompiledTarget, Digest, EffectClass, Environment, Epoch, Failure, NoSecrets, OperationName,
    PrincipalId, RecordKey, Redact, RunId, RunState, Step, TargetName, Timestamp, Value, Values,
    WorkspaceId,
};
use a2m_files::{CompileMode, Compiled, Diagnostics, TargetRoot};
use a2m_ledger::{
    Durability, Fence, IntentToken, NewRun, Opened, PlannedRecord, RecordIntent, RecordRef,
    RetentionClass, Store, StoreConfig, ToolKind,
};
use a2m_logs::{LogSettings, ManualClock};
use a2m_pack_mcp::{ToolDefinition, definition_digest};
use a2m_provider::{
    CheckedPaths, Deadline, Destinations, DriverHandle, HostServices, Posture, PrepareContext,
    Prepared, ProgramRef, ProviderSettings, Registry, RegistryBuilder, Roots, StepFailure,
    StepInput, Supervisor, SupervisorLimits, SurfaceRef,
};
use a2m_vault::{MemoryKeyStore, RandomSource, Vault};

/// A folder under the system's temporary folder, removed when dropped.
pub struct Folder(PathBuf);

impl Folder {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "a2m-pack-mcp-{name}-{}-{}",
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

/// A registry that holds only the MCP pack, with the redactor given.
pub fn registry_with(folder: &Folder, redact: Arc<dyn Redact>) -> Registry {
    registry_set(folder, redact, Values::new())
}

/// A registry that holds only the MCP pack, with the redactor and the
/// pack's own settings given.
pub fn registry_set(folder: &Folder, redact: Arc<dyn Redact>, settings: Values) -> Registry {
    let logs = folder.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let host = HostServices {
        supervisor: Supervisor::new(SupervisorLimits::default()),
        roots: Roots::none(),
        redact: Arc::clone(&redact),
        logs_and_traces: a2m_logs::start(&LogSettings::default(), redact, &logs).unwrap(),
    };
    let mut provider_settings = ProviderSettings::default();
    provider_settings.packs.insert(
        a2m_provider::ProviderName::new(a2m_pack_mcp::PACK).unwrap(),
        settings,
    );
    let mut builder = RegistryBuilder::new(provider_settings, host);
    builder.add_pack(&a2m_pack_mcp::pack());
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    registry
}

/// A registry that holds only the MCP pack.
pub fn registry(folder: &Folder) -> Registry {
    registry_with(folder, Arc::new(NoSecrets))
}

/// The pack's fixture target, as it is in the repository.
pub fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("example")
}

/// The digest that pins each tool a server lists, by name: read over the
/// wire with a plain HTTP client, so that it does not pass through the code
/// under test.
pub async fn listed_pins(url: &str) -> Vec<(String, String)> {
    let client = a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
        .no_proxy()
        .build()
        .unwrap();
    let mut pins = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let params = match &cursor {
            Some(cursor) => serde_json::json!({"cursor": cursor}),
            None => serde_json::json!({}),
        };
        let body = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": params});
        let answer: serde_json::Value = client
            .post(url)
            .header("MCP-Protocol-Version", "2026-07-28")
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        for tool in answer["result"]["tools"].as_array().unwrap() {
            let definition = ToolDefinition::from_listed(tool).unwrap();
            pins.push((
                definition.name.clone(),
                format!("sha256:{}", definition_digest(&definition)),
            ));
        }
        match answer["result"]["nextCursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => return pins,
        }
    }
}

/// Copies the fixture target into `to`, with `edit` applied to every file's
/// text. The edit is given the file's path within the target.
pub fn write_target(to: &Path, edit: &dyn Fn(&str, String) -> String) {
    let root = fixture_root();
    for folder in ["", "mappings", "operations"] {
        let from = root.join(folder);
        std::fs::create_dir_all(to.join(folder)).unwrap();
        for entry in std::fs::read_dir(&from).unwrap() {
            let entry = entry.unwrap();
            if !entry.file_type().unwrap().is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(entry.path()).unwrap();
            let relative = if folder.is_empty() {
                name.clone()
            } else {
                format!("{folder}/{name}")
            };
            std::fs::write(to.join(folder).join(&name), edit(&relative, text)).unwrap();
        }
    }
}

/// Compiles the target in `root` with the registry's compiler set.
pub fn compile(registry: &Registry, root: &Path) -> Result<Compiled, Diagnostics> {
    let set = registry.compiler_set();
    let sources = a2m_files::load(&TargetRoot::new(root), set)?;
    a2m_files::compile(&sources, set, CompileMode::Draft, a2m_files::DRAFT_VERSION)
}

/// The fixture target, compiled as it is in the repository.
pub fn compiled_fixture(registry: &Registry) -> CompiledTarget {
    match compile(registry, &fixture_root()) {
        Ok(compiled) => compiled.compiled_target,
        Err(problems) => panic!("the fixture target does not compile:\n{}", shown(&problems)),
    }
}

pub fn shown(problems: &Diagnostics) -> String {
    problems
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The surface of the compiled target with this name, at `address`, as the
/// engine hands it to a driver for the environment `test`.
pub fn surface(compiled_target: &CompiledTarget, name: &str, address: Option<&str>) -> SurfaceRef {
    let mut surface = compiled_target
        .surfaces
        .iter()
        .find(|surface| surface.name.as_str() == name)
        .expect("the compiled target has the surface")
        .clone();
    if let Some(address) = address {
        address.clone_into(&mut surface.address);
    }
    SurfaceRef {
        target: compiled_target.header.target.clone(),
        environment: Environment::new("test").unwrap(),
        surface,
    }
}

/// The act, read-back, and lookup steps of one operation's first binding.
pub struct Steps {
    pub act: Step,
    pub read_back: Option<Step>,
    pub lookup: Option<Step>,
}

pub fn steps(compiled_target: &CompiledTarget, operation: &str) -> Steps {
    let operation = compiled_target
        .operations
        .iter()
        .find(|found| found.name.as_str() == operation)
        .expect("the compiled target has the operation");
    let binding = &operation.bindings[0];
    Steps {
        act: binding.act[0].clone(),
        read_back: binding.read_back.first().cloned(),
        lookup: binding.lookup.first().cloned(),
    }
}

pub fn fence_of(run: u8) -> Fence {
    Fence {
        run: RunId::from_bytes([run; 16]),
        epoch: Epoch::new(1),
    }
}

/// The compiled target with one mapped tool's entry changed, as a compiled
/// target compiled by something other than this build's compiler could hold
/// it. The driver is the second line behind the compiler, so it is tested
/// with compiled targets the compiler would refuse.
pub fn remapped(
    compiled_target: &CompiledTarget,
    surface: &str,
    tool: &str,
    change: impl FnOnce(&mut serde_json::Value),
) -> CompiledTarget {
    let mut compiled_target = compiled_target.clone();
    let section = compiled_target
        .packs
        .iter_mut()
        .find(|section| section.pack == a2m_pack_mcp::PACK)
        .expect("the compiled target has the pack's section");
    let mut body: serde_json::Value = section.payload.decode().unwrap();
    let entry = &mut body["surfaces"][surface]["tools"][tool];
    assert!(entry.is_object(), "the mapping names the tool `{tool}`");
    change(entry);
    section.payload = a2m_core::PackPayload::encode(&body).unwrap();
    compiled_target
}

/// Prepares the driver for a surface through the handle. The run may
/// reach the surface's own address.
pub async fn prepare(
    registry: &Registry,
    compiled_target: &CompiledTarget,
    surface: &SurfaceRef,
    fence: Fence,
    posture: Posture,
    lease: Option<&a2m_auth::SessionLease>,
) -> Result<(DriverHandle, Prepared), StepFailure> {
    let destinations = Destinations::new([surface.surface.address.clone()]);
    prepare_reaching(
        registry,
        compiled_target,
        surface,
        fence,
        posture,
        lease,
        &destinations,
    )
    .await
}

/// Prepares the driver for a surface through the handle, for a run that
/// may reach `destinations` and nothing else.
pub async fn prepare_reaching(
    registry: &Registry,
    compiled_target: &CompiledTarget,
    surface: &SurfaceRef,
    fence: Fence,
    posture: Posture,
    lease: Option<&a2m_auth::SessionLease>,
    destinations: &Destinations,
) -> Result<(DriverHandle, Prepared), StepFailure> {
    prepare_within(
        registry,
        compiled_target,
        surface,
        fence,
        posture,
        lease,
        destinations,
        Duration::from_secs(30),
    )
    .await
}

/// As [`prepare_reaching`], with a deadline of `within` for the prepare.
#[allow(
    clippy::too_many_arguments,
    reason = "a test helper that names every input of a prepare"
)]
pub async fn prepare_within(
    registry: &Registry,
    compiled_target: &CompiledTarget,
    surface: &SurfaceRef,
    fence: Fence,
    posture: Posture,
    lease: Option<&a2m_auth::SessionLease>,
    destinations: &Destinations,
    within: Duration,
) -> Result<(DriverHandle, Prepared), StepFailure> {
    let mode = a2m_pack_mcp::Names::new().unwrap().mode;
    let handle = registry.driver(&mode).unwrap();
    let programs = surface
        .surface
        .programs
        .iter()
        .map(ProgramRef::from)
        .collect();
    let supervisor = handle.supervisor(programs);
    let cx = PrepareContext {
        run: fence.run,
        epoch: fence.epoch,
        surface,
        compiled_target,
        posture,
        deadline: Deadline::after(within),
        redact: &NoSecrets,
        destinations,
        paths: &CheckedPaths::none(),
        supervisor: &supervisor,
    };
    let prepared = handle.prepare(&cx, lease).await?;
    Ok((handle, prepared))
}

pub fn input<'a>(
    args: &'a Values,
    record: Option<RecordRef>,
    key: Option<&'a RecordKey>,
    stamp: Option<&'a str>,
) -> StepInput<'a> {
    let mut input = StepInput::new(args, Deadline::after(Duration::from_secs(30)));
    match record {
        Some(record) => input = input.for_record(record, key, stamp),
        // A step of no record may still be given a key or a stamp.
        None => {
            input.key = key;
            input.stamp = stamp;
        }
    }
    input
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

pub fn clock() -> Arc<ManualClock> {
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

pub fn key(name: &str) -> RecordKey {
    RecordKey::new(vec![Value::from(name)]).unwrap()
}

fn intended(name: &str) -> Digest {
    Digest::of(format!("intended {name}").as_bytes())
}

/// A write run with the named records planned, approved, and running.
pub async fn running(store: &Store, caller: u8, names: &[&str]) -> Fence {
    let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
    let workspace = WorkspaceId::from_bytes([1; 16]);
    store
        .write(Durability::Synced, move |tx| {
            let diff = Digest::of(b"diff");
            let fence = {
                let mut ledger = tx.ledger();
                let Opened::New(opened) = ledger.open_run(NewRun {
                    workspace,
                    principal: PrincipalId::from_bytes([caller; 16]),
                    client_label: "test".to_owned(),
                    tool_name: OperationName::new("example_order_note_update").unwrap(),
                    tool_kind: ToolKind::Operation,
                    target: TargetName::new("example").unwrap(),
                    target_version: Digest::of(b"plan"),
                    environment: Environment::new("test").unwrap(),
                    identity: None,
                    effect: EffectClass::Write,
                    input_hash: Digest::of(format!("{caller} {}", names.join(",")).as_bytes()),
                    input: None,
                    retention: RetentionClass::Long,
                })?
                else {
                    panic!("the run exists already");
                };
                let fence = ledger.new_epoch(opened.run)?;
                ledger.set_state(&fence, RunState::Validating, None)?;
                let records: Vec<_> = names
                    .iter()
                    .enumerate()
                    .map(|(at, name)| PlannedRecord {
                        step_seq: 0,
                        record_seq: u32::try_from(at).unwrap(),
                        key: key(name),
                        operation: OperationName::new("example_order_note_update").unwrap(),
                        intended: intended(name),
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

/// The intent token of one planned record, from the ledger.
pub async fn token(store: &Store, fence: Fence, record_seq: u32, name: &str) -> IntentToken {
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
        intended: intended(name),
        mode: None,
    }];
    store
        .store_intents(&fence, intents)
        .await
        .unwrap()
        .remove(0)
}

// ---- a real keeper, for real leases ----

use a2m_auth::{
    AskPerson, AuthEntry, AuthProvider, CredentialRef, InProcessWork, Keeper, KeeperWorker,
    LeaseRequest, PersonAnswer, PersonAsk, ProviderSet, Purpose, RunSessionCheck,
    SessionCheckOutcome, SessionKey, SessionLease, SessionRef, SessionStore, StoredCredentials,
    Wait,
};
use a2m_core::{AccountName, DecisionId, Identity, Secret};

struct NoPerson;

impl AskPerson for NoPerson {
    fn ask<'a>(
        &'a self,
        _session: &'a SessionRef,
        _ask: PersonAsk,
    ) -> a2m_auth::BoxFuture<'a, PersonAnswer> {
        Box::pin(async { PersonAnswer::NoAnswer })
    }
}

struct AlwaysSignedIn;

impl RunSessionCheck for AlwaysSignedIn {
    fn check_session<'a>(
        &'a self,
        _session: &'a SessionRef,
    ) -> a2m_auth::BoxFuture<'a, SessionCheckOutcome> {
        Box::pin(async { SessionCheckOutcome::SignedIn })
    }
}

/// A session keeper over a store in memory, with the pack's auth provider
/// beside the neutral ones.
pub struct Sessions {
    pub keeper: Keeper,
    pub vault: Arc<Vault>,
    credentials: StoredCredentials,
}

impl Sessions {
    pub async fn new(providers: Vec<Arc<dyn AuthProvider>>) -> Self {
        let (ledger, vault) = store().await;
        let store = SessionStore::new(ledger);
        let credentials = StoredCredentials::new(store.clone(), Arc::clone(&vault));
        let mut set = ProviderSet::neutral();
        for provider in providers {
            set = set.with(provider).unwrap();
        }
        let clock = clock();
        let worker = Arc::new(KeeperWorker::new(
            set.clone(),
            Arc::clone(&vault),
            Arc::new(credentials.clone()),
            clock.clone(),
        ));
        let work = Arc::new(InProcessWork::new(
            Arc::clone(&worker),
            Arc::new(NoPerson),
            Arc::new(AlwaysSignedIn),
        ));
        let keeper = Keeper::builder(store, set, work, clock)
            .local_worker(worker)
            .build();
        Self {
            keeper,
            vault,
            credentials,
        }
    }

    /// The redactor that knows every secret the keeper placed.
    pub fn redact(&self) -> Arc<dyn Redact> {
        self.vault.redactor()
    }

    /// Stores `token` for a dedicated account, signs the account in through
    /// `upstream_mcp`, and leases the session to a run.
    pub async fn lease(&self, account: &str, token: &str) -> SessionLease {
        let entry = AuthEntry::new(a2m_pack_mcp::auth_provider());
        self.lease_through(entry, "token", account, token).await
    }

    /// The same through any provider's entry, whose one credential has the
    /// role `role`.
    pub async fn lease_through(
        &self,
        entry: AuthEntry,
        role: &str,
        account: &str,
        token: &str,
    ) -> SessionLease {
        let key = SessionKey {
            workspace: WorkspaceId::from_bytes([7; 16]),
            target: TargetName::new("example").unwrap(),
            environment: Environment::new("test").unwrap(),
            identity: Identity::Account(AccountName::new(account).unwrap()),
        };
        let reference = CredentialRef::new(&format!("vault://example/{account}/{role}")).unwrap();
        self.credentials
            .put(
                &key.workspace,
                &key.identity,
                &reference,
                &Secret::new(token),
            )
            .await
            .unwrap();
        // As the entry of a published version's compiled target: only such an
        // entry signs in without its person (decision 0248).
        let entry = Arc::new(entry.credential(role, reference).published());
        self.keeper
            .sign_in(&key, Arc::clone(&entry), None)
            .await
            .unwrap();
        self.keeper.idle().await;
        self.keeper
            .lease(LeaseRequest {
                key,
                entry,
                run: RunId::from_bytes([3; 16]),
                decided: DecisionId::from_bytes([4; 16]),
                acting_for: None,
                wait: Wait::Interactive(Duration::from_secs(30)),
                headroom: None,
                purpose: Purpose::Run,
            })
            .await
            .unwrap()
    }
}
