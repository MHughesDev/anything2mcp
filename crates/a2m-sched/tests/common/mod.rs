//! What the tests share: a store in a folder of its own with a clock that
//! moves only when told, fixed randomness, and a vault attached; a scheduler
//! over it; and the few steps of a write run that the engine and the policy
//! engine would take.

#![allow(
    dead_code,
    reason = "each test binary uses some of these helpers and not others"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::Duration;

use a2m_core::{
    Digest, EffectClass, Environment, Failure, Mode, OperationName, PrincipalId, RecordKey, RunId,
    RunState, RunnerId, TargetName, Timestamp, Value, WorkspaceId,
};
use a2m_ledger::{
    DATABASE_FILE_NAME, Durability, Fence, NewRun, PlannedRecord, RecordIntent, RecordRef,
    RetentionClass, Store, StoreConfig, ToolKind, Tx, WriteClearance,
};
use a2m_logs::{Clock, ManualClock};
use a2m_sched::{
    Admitted, HealthState, JobKind, JobLease, JobRequirements, Priority, RunnerOffer,
    SchedSettings, Scheduler, SessionLimits, Slots, Submission,
};
use a2m_vault::{MemoryKeyStore, RandomSource, Vault};

/// Random bytes that are the same on every run: a counter, hashed. A second
/// stream starts elsewhere, so a restarted store makes new identifiers.
pub struct Counted(AtomicU64);

impl Counted {
    pub fn starting_at(start: u64) -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(start)))
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

/// A clock that moves only when told: both its counts together, as time
/// passing does, or only its time of day, as a machine's clock that is set
/// or jumps does.
#[derive(Debug)]
pub struct TestClock {
    passing: ManualClock,
    jumped: AtomicI64,
}

impl TestClock {
    fn starting_at(now: Timestamp) -> Self {
        Self {
            passing: ManualClock::starting_at(now),
            jumped: AtomicI64::new(0),
        }
    }

    /// Time passes: the steady count and the time of day move together.
    pub fn advance(&self, by: Duration) {
        self.passing.advance(by);
    }

    /// The time of day jumps by `millis`, forward or back; no time passes.
    pub fn jump(&self, millis: i64) {
        self.jumped.fetch_add(millis, Ordering::SeqCst);
    }
}

impl Clock for TestClock {
    fn elapsed(&self) -> Duration {
        self.passing.elapsed()
    }

    fn now(&self) -> Timestamp {
        Timestamp::from_unix_millis(
            self.passing.now().as_unix_millis() + self.jumped.load(Ordering::SeqCst),
        )
    }
}

/// A folder under the system's temporary folder, removed when dropped.
pub struct Folder(PathBuf);

impl Folder {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "a2m-sched-{name}-{}-{}",
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

pub const START: i64 = 1_800_000_000_000;

/// A store, a scheduler over it, and the clock both read.
pub struct Stand {
    pub folder: Folder,
    pub keys: Arc<MemoryKeyStore>,
    pub clock: Arc<TestClock>,
    pub store: Store,
    pub sched: Scheduler,
    pub settings: SchedSettings,
    starts: u64,
}

async fn open(
    folder: &Folder,
    keys: &Arc<MemoryKeyStore>,
    now: Timestamp,
    starts: u64,
) -> (Arc<TestClock>, Store) {
    let clock = Arc::new(TestClock::starting_at(now));
    let config = StoreConfig::in_dir(folder.path())
        .with_clock(Arc::clone(&clock) as Arc<dyn Clock>)
        .with_random(Counted::starting_at(starts * 1_000_000));
    let store = Store::open(config).await.unwrap();
    let vault = Arc::new(Vault::with_random(
        Arc::clone(keys) as _,
        Arc::new(store.key_records()),
        Counted::starting_at(starts * 1_000_000 + 500_000),
    ));
    store.attach(vault).await.unwrap();
    (clock, store)
}

impl Stand {
    pub async fn new(name: &str) -> Self {
        Self::with(name, SchedSettings::default()).await
    }

    pub async fn with(name: &str, settings: SchedSettings) -> Self {
        let folder = Folder::new(name);
        let keys = Arc::new(MemoryKeyStore::with_random(Counted::starting_at(
            900_000_000,
        )));
        let (clock, store) = open(&folder, &keys, Timestamp::from_unix_millis(START), 0).await;
        let sched = Scheduler::new(store.clone(), settings, None);
        Self {
            folder,
            keys,
            clock,
            store,
            sched,
            settings,
            starts: 0,
        }
    }

    /// Closes the store and opens it again, as a restart of the process
    /// would: nothing of the scheduler's memory is kept. The clock carries
    /// on from where it was.
    pub async fn restart(self) -> Self {
        let Self {
            folder,
            keys,
            clock,
            store,
            sched,
            settings,
            starts,
        } = self;
        let now = clock.now();
        drop(sched);
        drop(store);
        let (clock, store) = open(&folder, &keys, now, starts + 1).await;
        let sched = Scheduler::new(store.clone(), settings, None);
        Self {
            folder,
            keys,
            clock,
            store,
            sched,
            settings,
            starts: starts + 1,
        }
    }

    pub fn advance(&self, seconds: u64) {
        self.clock.advance(Duration::from_secs(seconds));
    }

    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    pub fn after(&self, seconds: i64) -> Timestamp {
        Timestamp::from_unix_millis(self.now().as_unix_millis() + seconds * 1000)
    }

    /// A second connection to the store's database, as any other program
    /// that can read the file would have: for asking what was stored.
    pub fn raw(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.folder.path().join(DATABASE_FILE_NAME)).unwrap()
    }

    /// One text the database holds.
    pub fn stored(&self, sql: &str) -> String {
        self.raw().query_row(sql, [], |row| row.get(0)).unwrap()
    }

    /// One number the database holds.
    pub fn counted(&self, sql: &str) -> i64 {
        self.raw().query_row(sql, [], |row| row.get(0)).unwrap()
    }

    /// The state of a run, as the database holds it.
    pub fn run_state(&self, run: RunId) -> String {
        self.stored(&format!("SELECT state FROM runs WHERE id = '{run}'"))
    }

    /// Yields to the other tasks until the scheduler's earliest deadline is
    /// `at`: until whoever was to wait for that moment does.
    pub async fn waits_until(&self, at: Timestamp) {
        loop {
            if self.sched.next_due().await.unwrap() == Some(at) {
                return;
            }
            tokio::task::yield_now().await;
        }
    }

    pub async fn hello(&self, number: u8) -> RunnerId {
        self.sched.hello(offer(number)).await.unwrap();
        runner(number)
    }

    /// Submits and expects a new run.
    pub async fn admit(&self, submission: Submission) -> (RunId, a2m_core::JobId) {
        match self.sched.submit(submission).await.unwrap() {
            Admitted::New { run, job } => (run, job),
            Admitted::Existing { .. } => panic!("the run exists already"),
        }
    }

    /// Claims at once, without waiting.
    pub async fn claim(&self, runner: RunnerId, total: u32) -> Vec<JobLease> {
        self.sched
            .claim(runner, slots(total), Duration::ZERO)
            .await
            .unwrap()
    }

    pub async fn claim_one(&self, runner: RunnerId) -> JobLease {
        let mut leases = self.claim(runner, 1).await;
        assert_eq!(leases.len(), 1, "one job was to be handed out");
        leases.remove(0)
    }
}

pub fn workspace() -> WorkspaceId {
    WorkspaceId::from_bytes([1; 16])
}

pub fn principal(number: u8) -> PrincipalId {
    PrincipalId::from_bytes([number; 16])
}

pub fn runner(number: u8) -> RunnerId {
    RunnerId::from_bytes([number; 16])
}

pub fn target() -> TargetName {
    TargetName::new("example").unwrap()
}

pub fn environment() -> Environment {
    Environment::new("test").unwrap()
}

/// A mode, by a name no pack registers: the scheduler only compares names.
pub fn mode() -> Mode {
    Mode::new("alpha").unwrap()
}

pub fn offer(number: u8) -> RunnerOffer {
    RunnerOffer {
        runner: runner(number),
        workspace: workspace(),
        targets: vec![target()],
        modes: vec![mode()],
        os: None,
        owner: None,
        protocol: a2m_sched::PROTOCOL_VERSION,
    }
}

pub fn slots(total: u32) -> Slots {
    Slots {
        total,
        per_mode: BTreeMap::from([(mode(), total)]),
        keeper: 0,
    }
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
        environment: environment(),
        identity: None,
        effect,
        input_hash: Digest::of(input.as_bytes()),
        input: None,
        retention: RetentionClass::Long,
    }
}

pub fn requirements(writes: bool) -> JobRequirements {
    JobRequirements {
        kind: if writes {
            JobKind::Preview
        } else {
            JobKind::Execute
        },
        target: target(),
        environment: environment(),
        modes: vec![mode()],
        os: None,
        acts_as: None,
        session: None,
        session_limits: SessionLimits {
            reads: 4,
            writes: 1,
        },
        exclusive: Vec::new(),
        max_step: Duration::from_secs(10),
        writes,
        verify_after_loss: false,
    }
}

/// A read by `caller` with `input`, interactive, on a healthy kernel.
pub fn read(caller: u8, input: &str) -> Submission {
    Submission {
        run: new_run(EffectClass::Read, caller, input),
        job: requirements(false),
        priority: Priority::Interactive,
        health: HealthState::Ready,
        queue_wait: None,
        budgets: Vec::new(),
    }
}

/// A write by `caller` with `input`.
pub fn write(caller: u8, input: &str) -> Submission {
    Submission {
        run: new_run(EffectClass::Write, caller, input),
        job: requirements(true),
        ..read(caller, input)
    }
}

// ---- what the engine and the policy engine would do to a write run ----

pub fn key(name: &str) -> RecordKey {
    RecordKey::new(vec![Value::from(name)]).unwrap()
}

pub fn intended(name: &str) -> Digest {
    Digest::of(format!("intended {name}").as_bytes())
}

pub fn diff_hash() -> Digest {
    Digest::of(b"diff")
}

pub fn record(fence: &Fence) -> RecordRef {
    RecordRef {
        run: fence.run,
        step_seq: 0,
        record_seq: 0,
    }
}

/// The policy engine's stand-in: an approval of the run's diff, used once,
/// through the ledger's `testing` feature (decision 0145).
fn approve(tx: &mut Tx<'_>, fence: &Fence) -> Result<WriteClearance, Failure> {
    a2m_ledger::testing::approve(tx, fence)
}

/// Brings a write run, under `fence`, to `running` with the lock on the
/// record named `name` taken; with `intent`, the intent to write it is
/// stored too, so the record is in doubt if the lease is lost now.
pub async fn start_writing(store: &Store, fence: Fence, name: &str, intent: bool) {
    let planned = PlannedRecord {
        step_seq: 0,
        record_seq: 0,
        key: key(name),
        operation: OperationName::new("example_order_update").unwrap(),
        intended: intended(name),
        intended_values: None,
        before: None,
        unchanged: false,
    };
    store
        .write(Durability::Synced, move |tx| {
            let mut ledger = tx.ledger();
            ledger.set_state(&fence, RunState::Validating, None)?;
            ledger.plan_records(&fence, &[planned])?;
            ledger.store_diff(&fence, diff_hash(), None)?;
            ledger.set_state(&fence, RunState::NeedsApproval, None)?;
            let clearance = approve(tx, &fence)?;
            tx.ledger().begin_writes(&fence, clearance)?;
            tx.ledger().take_lock(&fence, record(&fence)).map(|_| ())
        })
        .await
        .unwrap();
    if intent {
        store
            .store_intents(
                &fence,
                vec![RecordIntent {
                    record: record(&fence),
                    intended: intended(name),
                    mode: None,
                }],
            )
            .await
            .unwrap();
    }
}
