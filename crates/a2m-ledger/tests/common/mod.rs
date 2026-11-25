//! What the tests share: a store on a folder of its own or in memory, with a
//! fixed clock, fixed randomness, and a vault attached; and a stand-in for
//! the policy engine, which takes the key to the policy engine's tables from
//! each store it opens and writes them through the same interface the real
//! one uses.

#![allow(
    dead_code,
    reason = "each test binary uses some of these helpers and not others"
)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use a2m_core::{
    ApprovalId, DecisionId, Digest, EffectClass, Environment, Failure, OperationName, PrincipalId,
    RecordKey, RunId, RunState, TargetName, Timestamp, Value, WorkspaceId,
};
use a2m_ledger::{
    DATABASE_FILE_NAME, Durability, Fence, NewRun, Opened, PlannedRecord, PolicyKey, RecordRef,
    RetentionClass, Store, StoreConfig, Tables, ToolKind, Tx, WriteClearance,
};
use a2m_logs::ManualClock;
use a2m_vault::{KeyStore, MemoryKeyStore, RandomSource, Vault};

/// Random bytes that are the same on every run: a counter, hashed.
pub struct Counted(AtomicU64);

impl Counted {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(0)))
    }
}

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

/// A folder under the system's temporary folder, removed when dropped.
pub struct Folder(PathBuf);

impl Folder {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "a2m-ledger-{name}-{}-{}",
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

    /// A second connection to the store's database, as any other program
    /// that can read the file would have. For asking the store what it holds
    /// and for trying what the rules forbid.
    pub fn raw(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.0.join(DATABASE_FILE_NAME)).unwrap()
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub const START: i64 = 1_800_000_000_000;

/// The root key of a test installation. Keep it across a simulated restart.
pub fn key_store() -> Arc<MemoryKeyStore> {
    Arc::new(MemoryKeyStore::with_random(Counted::new()))
}

pub async fn open_with(folder: Option<&Path>, keys: Arc<MemoryKeyStore>) -> (Store, Arc<Vault>) {
    let clock = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(START)));
    open_clocked(folder, keys, clock).await
}

/// A store in memory whose clock the test moves.
pub async fn memory_clocked() -> (Store, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(START)));
    let store = open_clocked(None, key_store(), Arc::clone(&clock)).await.0;
    (store, clock)
}

pub async fn open_clocked(
    folder: Option<&Path>,
    keys: Arc<MemoryKeyStore>,
    clock: Arc<ManualClock>,
) -> (Store, Arc<Vault>) {
    open_keyed(folder, keys, clock).await
}

/// The same, with the vault's root in any key store, such as one derived
/// from a passphrase, which opens the same root in every process.
pub async fn open_keyed(
    folder: Option<&Path>,
    keys: Arc<dyn KeyStore>,
    clock: Arc<ManualClock>,
) -> (Store, Arc<Vault>) {
    let config = match folder {
        Some(folder) => StoreConfig::in_dir(folder),
        None => StoreConfig::in_memory(),
    }
    .with_clock(clock)
    .with_random(Counted::new());
    let store = Store::open(config).await.unwrap();
    let vault = Arc::new(Vault::with_random(
        keys,
        Arc::new(store.key_records()),
        Counted::new(),
    ));
    store.attach(Arc::clone(&vault)).await.unwrap();
    // These tests stand in for the policy engine: they take its key from
    // every store they open, as `Policy::open` does.
    POLICY_KEYS
        .lock()
        .unwrap()
        .push(store.policy_key().unwrap());
    (store, vault)
}

/// The keys to the policy engine's tables of every store these tests opened.
static POLICY_KEYS: Mutex<Vec<PolicyKey>> = Mutex::new(Vec::new());

/// The policy engine's tables in `tx`, with the key of the store `tx` is of.
pub fn policy_tables<'t, 'a>(tx: &'t mut Tx<'a>) -> Tables<'t, 'a> {
    let keys = POLICY_KEYS.lock().unwrap();
    let at = keys
        .iter()
        .position(|key| tx.policy_tables(key).is_ok())
        .unwrap();
    tx.policy_tables(&keys[at]).unwrap()
}

pub async fn memory() -> Store {
    open_with(None, key_store()).await.0
}

pub async fn on_disk(folder: &Folder) -> Store {
    open_with(Some(folder.path()), key_store()).await.0
}

pub fn workspace() -> WorkspaceId {
    WorkspaceId::from_bytes([1; 16])
}

pub fn principal(number: u8) -> PrincipalId {
    PrincipalId::from_bytes([number; 16])
}

pub fn target() -> TargetName {
    TargetName::new("example").unwrap()
}

pub fn new_run(effect: EffectClass, caller: u8, input: &str) -> NewRun {
    NewRun {
        workspace: workspace(),
        principal: principal(caller),
        client_label: "test".to_owned(),
        tool_name: OperationName::new("example_order_update").unwrap(),
        tool_kind: ToolKind::Operation,
        target: target(),
        target_version: Digest::of(b"plan"),
        environment: Environment::new("test").unwrap(),
        identity: None,
        effect,
        input_hash: Digest::of(input.as_bytes()),
        input: None,
        retention: RetentionClass::Long,
    }
}

pub fn key(name: &str) -> RecordKey {
    RecordKey::new(vec![Value::from(name)]).unwrap()
}

pub fn intended(name: &str) -> Digest {
    Digest::of(format!("intended {name}").as_bytes())
}

pub fn planned(record_seq: u32, name: &str) -> PlannedRecord {
    PlannedRecord {
        step_seq: 0,
        record_seq,
        key: key(name),
        operation: OperationName::new("example_order_update").unwrap(),
        intended: intended(name),
        intended_values: None,
        before: None,
        unchanged: false,
    }
}

pub fn diff_hash() -> Digest {
    Digest::of(b"diff")
}

// ---- the policy engine's stand-in: its own tables, with the test key ----

/// An approval row as the policy engine writes it, with each field a test may
/// change to break rule 3.
#[derive(Clone)]
pub struct ApprovalRow {
    pub workspace: WorkspaceId,
    pub diff: Digest,
    pub tool_version: Option<Digest>,
    pub channel: &'static str,
    pub presence: &'static str,
    /// Milliseconds after the transaction's moment; `None` for no expiry.
    pub expires_in: Option<i64>,
}

impl ApprovalRow {
    pub fn good() -> Self {
        Self {
            workspace: workspace(),
            diff: diff_hash(),
            tool_version: Some(Digest::of(b"plan")),
            channel: "local_prompt",
            presence: "os_prompt",
            expires_in: Some(600_000),
        }
    }
}

pub fn record_approval(tx: &mut Tx<'_>, run: RunId, diff: &Digest) -> Result<ApprovalId, Failure> {
    let row = ApprovalRow {
        diff: *diff,
        ..ApprovalRow::good()
    };
    record_approval_as(tx, run, &row)
}

pub fn record_approval_as(
    tx: &mut Tx<'_>,
    run: RunId,
    row: &ApprovalRow,
) -> Result<ApprovalId, Failure> {
    let id = ApprovalId::from_parts(tx.now(), tx.random()?)?;
    let now = tx.now();
    let expires = row
        .expires_in
        .map(|after| Timestamp::from_unix_millis(now.as_unix_millis() + after));
    policy_tables(tx).execute(
        "INSERT INTO approvals
            (id, workspace_id, scope, run_id, diff_hash, tool_version_hash, approver_id, channel,
             presence_check, granted_at, expires_at)
         VALUES (?1, ?2, 'run', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        &[
            id.to_string().into(),
            row.workspace.to_string().into(),
            run.to_string().into(),
            row.diff.to_string().into(),
            row.tool_version.map(|version| version.to_string()).into(),
            principal(9).to_string().into(),
            row.channel.into(),
            row.presence.into(),
            now.into(),
            expires.into(),
        ],
    )?;
    Ok(id)
}

/// Marks an approval used, once. What `consume` of the policy engine does.
pub fn consume(tx: &mut Tx<'_>, approval: ApprovalId) -> Result<WriteClearance, Failure> {
    let now = tx.now();
    let used = policy_tables(tx).execute(
        "UPDATE approvals SET used_at = ?2 WHERE id = ?1 AND used_at IS NULL AND revoked_at IS NULL",
        &[approval.to_string().into(), now.into()],
    )?;
    assert_eq!(used, 1, "the approval was used or revoked before");
    Ok(WriteClearance::Approval(approval))
}

pub fn record_decision(
    tx: &mut Tx<'_>,
    run: RunId,
    diff: &Digest,
    allowed: bool,
) -> Result<DecisionId, Failure> {
    let id = DecisionId::from_parts(tx.now(), tx.random()?)?;
    let now = tx.now();
    policy_tables(tx).execute(
        "INSERT INTO decisions
            (id, workspace_id, run_id, principal_id, question, allowed, diff_hash, reasons,
             policy_version, decided_at)
         VALUES (?1, ?2, ?3, ?4, 'write_without_person', ?5, ?6, '[]', 'v', ?7)",
        &[
            id.to_string().into(),
            workspace().to_string().into(),
            run.to_string().into(),
            principal(1).to_string().into(),
            allowed.into(),
            diff.to_string().into(),
            now.into(),
        ],
    )?;
    Ok(id)
}

// ---- runs brought to a known point ----

/// A write run with the named records planned, waiting at `needs_approval`
/// under its first lease.
pub async fn previewed(store: &Store, caller: u8, names: &[&str]) -> Fence {
    let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
    let input = names.join(",");
    store
        .write(Durability::Synced, move |tx| {
            let mut ledger = tx.ledger();
            let Opened::New(opened) =
                ledger.open_run(new_run(EffectClass::Write, caller, &input))?
            else {
                panic!("the run exists already");
            };
            let fence = ledger.new_epoch(opened.run)?;
            ledger.set_state(&fence, RunState::Validating, None)?;
            let records: Vec<_> = names
                .iter()
                .enumerate()
                .map(|(at, name)| planned(u32::try_from(at).unwrap(), name))
                .collect();
            ledger.plan_records(&fence, &records)?;
            ledger.store_diff(&fence, diff_hash(), None)?;
            ledger.set_state(&fence, RunState::NeedsApproval, None)?;
            Ok(fence)
        })
        .await
        .unwrap()
}

/// The same run, approved and `running`.
pub async fn running(store: &Store, caller: u8, names: &[&str]) -> Fence {
    let fence = previewed(store, caller, names).await;
    store
        .write(Durability::Synced, move |tx| {
            let approval = record_approval(tx, fence.run, &diff_hash())?;
            let clearance = consume(tx, approval)?;
            tx.ledger().begin_writes(&fence, clearance)
        })
        .await
        .unwrap();
    fence
}

pub fn record(fence: &Fence, record_seq: u32) -> RecordRef {
    RecordRef {
        run: fence.run,
        step_seq: 0,
        record_seq,
    }
}

pub async fn lock(store: &Store, fence: Fence, record_seq: u32) -> Result<(), Failure> {
    store
        .write(Durability::Synced, move |tx| {
            tx.ledger()
                .take_lock(&fence, record(&fence, record_seq))
                .map(|_| ())
        })
        .await
}

pub async fn evidence(store: &Store, text: &str) -> a2m_ledger::BlobRef {
    store
        .blobs()
        .put(a2m_ledger::NewBlob {
            workspace: workspace(),
            owner: a2m_vault::OwnerRef::service("engine").unwrap(),
            kind: a2m_ledger::BlobKind::Values,
            class: RetentionClass::Long,
            plain: text.as_bytes().to_vec(),
        })
        .await
        .unwrap()
}

pub async fn intent(
    store: &Store,
    fence: Fence,
    record_seq: u32,
    name: &str,
) -> Result<a2m_ledger::IntentToken, Failure> {
    let intents = vec![a2m_ledger::RecordIntent {
        record: record(&fence, record_seq),
        intended: intended(name),
        mode: None,
    }];
    Ok(store.store_intents(&fence, intents).await?.remove(0))
}

pub async fn state_of(store: &Store, fence: Fence, record_seq: u32) -> a2m_core::RecordState {
    store
        .read(move |tx| tx.ledger().record(record(&fence, record_seq)))
        .await
        .unwrap()
        .state
}

pub fn stopped_by(class: a2m_core::FailureClass, code: &'static str) -> Failure {
    Failure::new(
        class,
        a2m_core::FailureCode::fixed(code),
        "It stopped.",
        "Look.",
    )
}
