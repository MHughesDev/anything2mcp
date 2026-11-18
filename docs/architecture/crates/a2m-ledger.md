# `a2m-ledger`

The store: one schema on SQLite and Postgres, its migrations and transactions, and the tables that say what happened. It holds runs, the ledger of commit points and its nine rules, run events, evidence blobs, findings and open questions, published versions, and the audit chain. Everything in the kernel that persists goes through it.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 3, Stores |
| May depend on | Layers 1 and 2 |
| Depends on | `a2m-core`; `a2m-vault` to seal blobs, for its `RandomSource`, and to implement its `KeyRecords`; `a2m-telemetry` for its `Clock` and for spans. Outside: `rusqlite` with SQLite built in ([decision 0067](../../decisions/0067-the-kernel-is-built-layer-by-layer-in-parallel-and-the-first-driver-is-the-http-packs.md)), `serde_json`, and `tokio` for the channel an answer comes back on. Identifiers and SHA-256 come from `a2m-core` |
| Built in | Phase 1, group H: runs, events, blobs, migrations, on SQLite. Phase 3, group A and slice 3.1: record rows, locks, the nine rules. Stage E, package E1: findings and open questions. Phase 6: Postgres |
| Carries out | Spec sections [208](../../spec/2-kernel/208-runs-the-ledger-and-resume.md), [208](../../spec/2-kernel/208-runs-the-ledger-and-resume.md), [209](../../spec/2-kernel/209-record-identity-and-dependent-records.md) (assigned keys and stored outputs), [217](../../spec/2-kernel/217-traces-evidence-and-repair.md) (where evidence is kept), [224](../../spec/2-kernel/224-when-the-engine-itself-fails.md) (no ledger, no write), [218](../../spec/2-kernel/218-discovery-and-findings.md) (the findings store and the unknowns ledger); the [ledger schema](../../reference/ledger-schema.md) |

## Owns

- Opening the store, checking its integrity, and running migrations: numbered, forward-only, one sequence for the whole schema, tested on both databases.
- Transactions. A `Tx` is the only way to read or write a table. It lives inside one closure on the store's own thread, and every crate that persists is handed one.
- The ledger proper: run rows, pipeline step rows, one record row per commit point, and run events, with the nine rules of section 208 enforced where the data is, not by callers' good behavior.
- The intent token: proof, as a type, that an intent was durably stored.
- Evidence blobs, named by the keyed digest of their plain content and sealed through the vault; their retention and legal hold.
- The vault's wrapped keys, kept through the vault's `KeyRecords`, which this crate implements.
- Findings and the unknowns ledger, as stored data.
- Published versions and their plan bytes, with the hash checked on every load.
- The audit chain, one per workspace.
- The outbox, and callers' idempotency keys: the table `caller_idempotency` (migration `0013`), which keeps the workspace's keyed digest of a key, never the key, made under the purpose `caller_key` of `a2m-core`, which no other digest uses ([decision 0172](../../decisions/0172-the-ledger-ends-a-record-on-an-acknowledgement-and-keeps-what-the-system-said.md)).

It must never: read or write a target system; decide whether something may happen; normalize or compare business values itself (it compares digests it is given); decode a plan; hold a secret in the clear; let a row be written outside a transaction.

## How a service owns its tables

[Decision 0061](../../decisions/0061-the-layers-are-redrawn-so-that-every-needed-dependency-is-allowed.md) says a service owns the meaning of its tables and the store owns the connection, the transactions, and the migrations. [Decision 0080](../../decisions/0080-the-store-is-one-sqlite-file-and-a-crate-reaches-its-own-tables-through-the-transaction.md) settles how:

| Concern | Who | How |
| --- | --- | --- |
| The connection, the single SQLite writer, the readers, durability per commit | `a2m-ledger` | `Store::write` and `Store::read`, which run a closure on the store's own thread and hand it a `Tx` |
| The migration that creates a table | `a2m-ledger` holds the file, in `crates/a2m-ledger/migrations/`; the owner writes it | A file named `NNNN-<crate>-<what>.sql` whose first line is `-- owner: a2m-<crate>`. The build script lists the folder, so adding a migration is adding one file. One numbered sequence, each file applied once, in its own transaction, with its checksum kept |
| The queries on a table, its row types, its state rules | The owning crate | Plain functions in that crate that take `&mut Tx` and run fixed statements through `tx.tables(Owner::X)` |
| Holding a crate to its own tables | `a2m-ledger` | The database's authorizer. A statement run through `tx.tables(Owner::X)` is refused, when it is prepared, if it names a table another crate's migration created, the database's own tables, or anything but reading and writing rows: `unknown_variant`, `table_not_owned` |
| Atomicity across owners | The caller that began the transaction | Two crates' functions called in one closure commit together or not at all |

Table owners. The ledger's and the policy engine's tables exist; the others are provisional until their owner's first migration:

| Owner | Tables |
| --- | --- |
| `a2m-ledger` | Built: `runs`, `run_records`, `run_events`, `record_locks`, `blobs`, `target_versions`, `findings`, `unknowns`, `audit`, `vault_keys`, `schema_migrations`, `caller_idempotency`. Not built yet: `workspaces`, `run_steps`, `outbox` |
| `a2m-policy` | `decisions`, `approvals`, and the rest of what its own migrations create: `principals`, `grants`, `policy_versions`, `consents`, `delegations`, `kill_switches`, `quota_counters` |
| `a2m-auth` | `sessions`, `session_events`, `session_state` |
| `a2m-sched` | Built: `jobs`, with its `session_key` column; `runners`; `resource_holds`; `caller_turns`; `scheduler_holds`; `budgets`, which holds the rate buckets |
| `a2m-engine` | `bindings_health`, written through `Control` |
| `a2m-actions` | `toolsets`, `toolset_items`; `scout_stages` (run, stage, input hash, result reference); `outbox_deliveries`, the outcomes of delivering the outbox's webhooks |

A test reads every migration and fails if a table has no owner or two, or if the store holds a table no migration declares. The one declared exception to "a crate's SQL names only its own tables" is the ledger's check for rule 3, which reads `approvals` and `decisions` (below); the authorizer allows the ledger those two reads and nothing else outside its own tables.

A crate passes its own name to `tx.tables` and no other. The store cannot tell which crate is calling, so for most owners that one line is held by a check over the source, which the documentation audit still owes. The policy engine's tables are what rule 3 trusts, so they are held by a capability instead (decision 0145): `Store::policy_key` hands out a `PolicyKey` once for the life of the store, `Policy::open` takes it, and `Tx::policy_tables(&key)` is the only way to those tables; `tx.tables(Owner::Policy)` refuses every statement, and a key of one store opens no other. What remains: a crate that depends on the ledger and is handed the key could still forge, and so could a program that opens the database file itself; the kernel hands the key to the policy engine alone, and a test holds that no other crate's source names it. Tests stand in for the policy engine through the `testing` feature, which a crate turns on only as a feature of its dev-dependency: `testing::approve` and `testing::pre_approve` write a whole approval or decision, and `Tx::policy_tables_for_tests` reaches the tables. The ledger's own tests take the key from each store they open, as the policy engine does. The ledger's own tables carry the nine rules and are not reachable through `tx.tables` at all, only through the ledger's calls.

## Interface

The database crate blocks, so no database call is made on an async thread. One thread owns the writing connection and runs one transaction at a time. A store on disk has two reading threads beside it, each with its own connection, so a read never waits for a write. A caller hands a closure to `Store::write` or `Store::read` and awaits the answer. Inside the closure everything is synchronous, and nothing can wait on anything else, so the single writer is never held by a task that went elsewhere.

```rust
// Opening. Called once by Kernel::open in a2m-actions.
pub struct StoreConfig { /* data directory or memory; clock; random source; telemetry */ }
impl StoreConfig {
    pub fn in_dir(data_dir: impl Into<PathBuf>) -> Self;   // store.sqlite3 and blobs/ inside it
    pub fn in_memory() -> Self;                            // for tests
    pub fn with_clock(self, clock: Arc<dyn Clock>) -> Self;
    pub fn with_random(self, random: Arc<dyn RandomSource>) -> Self;
    pub fn with_telemetry(self, telemetry: Telemetry) -> Self;   // a span per commit
}
#[derive(Clone)] pub struct Store { /* the writing thread, the reading threads, what is attached */ }
impl Store {
    /// Opens, checks integrity, and migrates. Refuses a store a newer engine wrote.
    pub async fn open(config: StoreConfig) -> Result<Store, Failure>;
    /// What the vault is opened with. Reads and writes only `vault_keys`.
    pub fn key_records(&self) -> StoreKeyRecords;
    /// Joins the opened vault: sealing, the keyed digest, the redactor. Until then runs, events,
    /// records, blobs, questions, and the audit chain refuse, and health says so.
    pub async fn attach(&self, vault: Arc<Vault>) -> Result<(), Failure>;
    /// One writing transaction around `work`, on the writing thread. Ok commits; a failure or a panic takes all of it back.
    pub async fn write<T, F>(&self, durability: Durability, work: F) -> Result<T, Failure>
        where T: Send + 'static, F: FnOnce(&mut Tx<'_>) -> Result<T, Failure> + Send + 'static;
    /// One reading transaction: one unchanging state of the store, and nothing can be changed.
    pub async fn read<T, F>(&self, work: F) -> Result<T, Failure>
        where T: Send + 'static, F: FnOnce(&mut Tx<'_>) -> Result<T, Failure> + Send + 'static;
    /// Rule 1. Commits with Durability::Synced, then gives the tokens. Up to the intent window at once.
    pub async fn store_intents(&self, fence: &Fence, intents: Vec<RecordIntent>) -> Result<Vec<IntentToken>, Failure>;
    /// `mark_verified` in a transaction of its own, for a caller that holds the proof of `acted` by reference.
    pub async fn store_verified(&self, token: &ActedToken, read_back: ReadBack) -> Result<(), Failure>;
    /// Opens the run's own stored diff and answers when it says the record's write is `acknowledged` (decision 0172).
    pub async fn acknowledged_in_diff(&self, record: RecordRef) -> Result<AcknowledgedInDiff, Failure>;
    /// `acknowledged_in_diff`, then `mark_acknowledged` in a transaction of its own, synced.
    pub async fn store_acknowledged(&self, token: &ActedToken, evidence: BlobRef) -> Result<(), Failure>;
    pub fn blobs(&self) -> Blobs<'_>;
    pub async fn health(&self) -> StoreHealth;          // can commit; vault attached
    pub async fn tables(&self) -> Result<Vec<(String, Option<Owner>)>, Failure>;
    pub fn clock(&self) -> Arc<dyn Clock>;
    pub fn blob_dir(&self) -> Option<&Path>;
}
pub enum Durability { Synced, Relaxed }   // Synced for intents, approvals, keys, audit; Relaxed for read runs

// The transaction, and a crate's own tables through it.
pub struct Tx<'a> { /* one database transaction, inside one closure */ }
impl Tx<'_> {
    pub fn now(&self) -> Timestamp;                    // the store's clock, read once when the transaction began
    pub fn random(&mut self) -> Result<[u8; 10], Failure>;   // for a new identifier: XId::from_parts(tx.now(), tx.random()?)
    pub fn writes(&self) -> bool;
    pub fn tables(&mut self, owner: Owner) -> Tables<'_, '_>;     // Owner::Policy: every statement refused
    pub fn policy_tables(&mut self, key: &PolicyKey) -> Result<Tables<'_, '_>, Failure>;
    pub fn ledger(&mut self) -> Ledger<'_, '_>;
    pub fn findings(&mut self) -> Findings<'_, '_>;
    pub fn versions(&mut self) -> Versions<'_, '_>;
    pub fn audit(&mut self) -> Audit<'_, '_>;
}
pub enum Owner { Ledger, Policy, Auth, Sched, Engine, Actions }
pub struct PolicyKey { /* this store's number; not Clone */ }   // Store::policy_key(&self) -> Result<PolicyKey, Failure>, once
impl Tables<'_, '_> {
    // Fixed statements with numbered parameters (?1, ?2); values are bound, never written into the text.
    pub fn execute(&mut self, sql: &'static str, params: &[SqlValue]) -> Result<u64, Failure>;
    pub fn query(&mut self, sql: &'static str, params: &[SqlValue]) -> Result<Vec<Row>, Failure>;
    pub fn query_first(&mut self, sql: &'static str, params: &[SqlValue]) -> Result<Option<Row>, Failure>;
    pub fn now(&self) -> Timestamp;
}
pub enum SqlValue { Null, Int(i64), Text(String), Bytes(Vec<u8>) }   // From<&str>, String, i64, u32, bool, Vec<u8>, Timestamp, Option<T>
impl Row {   // a column by its position, as the type the caller expects
    pub fn text(&self, column: usize) -> Result<&str, Failure>;      pub fn opt_text(..) -> Result<Option<&str>, Failure>;
    pub fn int(&self, column: usize) -> Result<i64, Failure>;        pub fn opt_int(..) -> Result<Option<i64>, Failure>;
    pub fn bytes(&self, column: usize) -> Result<&[u8], Failure>;    pub fn opt_bytes(..) -> Result<Option<&[u8]>, Failure>;
}
pub fn migrations() -> Result<&'static [Migration], Failure>;   // number, name, owner, tables, checksum
pub fn table_owner(table: &str) -> Option<Owner>;

// Defined elsewhere, used here: `Vault`, `KeyRecords`, `RandomSource`, `Opened` in a2m-vault; `KeyedDigest` and
// `Redact` in a2m-core; `Clock` in a2m-telemetry.
pub struct StoreKeyRecords { /* the store, held weakly */ }
impl KeyRecords for StoreKeyRecords { /* get, newest, insert, replace */ }

// The run store and the ledger: `tx.ledger()`. Every mutating call carries a Fence and appends
// an event in the same transaction as the current-state change.
pub struct Fence { pub run: RunId, pub epoch: Epoch }
impl Ledger<'_, '_> {
    pub fn open_run(&mut self, new: NewRun) -> Result<Opened, Failure>;   // Opened::New(Fence) | Opened::Existing(RunId) (rule 6)
    pub fn new_epoch(&mut self, run: RunId) -> Result<Fence, Failure>;    // lease granted; sorts the locks of the lease that ended
    pub fn set_state(&mut self, f: &Fence, to: RunState, why: Option<&Failure>) -> Result<(), Failure>;
    pub fn store_diff(&mut self, f: &Fence, hash: Digest, diff: Option<BlobRef>) -> Result<(), Failure>;
    /// Rule 3. The first entry of a write run into `running`.
    pub fn begin_writes(&mut self, f: &Fence, c: WriteClearance) -> Result<(), Failure>;
    pub fn plan_records(&mut self, f: &Fence, records: &[PlannedRecord]) -> Result<(), Failure>;
    pub fn take_lock(&mut self, f: &Fence, record: RecordRef) -> Result<LockHeld, Failure>;
    pub fn mark_acted(&mut self, token: IntentToken, acted: Acted) -> Result<ActedToken, Failure>;
    /// Rule 2. Refuses unless the read-back digest equals the intended digest and evidence is given.
    pub fn mark_verified(&mut self, token: &ActedToken, read_back: ReadBack) -> Result<(), Failure>;
    /// Ends an `acted` record on the system's acknowledgement: stays `acted`, never verified, settled,
    /// lock released. Only for a cleared irreversible run whose stored diff says so (decision 0172).
    pub fn mark_acknowledged(&mut self, token: &ActedToken, evidence: BlobRef, diff: &AcknowledgedInDiff) -> Result<(), Failure>;
    pub fn mark_failed(&mut self, f: &Fence, record: RecordRef, why: &Failure, evidence: Option<BlobRef>) -> Result<(), Failure>;
    /// Rule 5. Records in `intent`, `acted`, or a retryable `failed`, in key order.
    pub fn in_doubt(&mut self, run: RunId) -> Result<Vec<RecordRow>, Failure>;
    pub fn settle(&mut self, f: &Fence, record: RecordRef, seen: Reconciled) -> Result<Settled, Failure>;
    pub fn quarantine_locks(&mut self, run: RunId) -> Result<(), Failure>;
    pub fn store_result(&mut self, f: &Fence, result: BlobRef) -> Result<(), Failure>;
    pub fn store_trace(&mut self, run: RunId, trace: BlobRef) -> Result<(), Failure>;
    pub fn append_event(&mut self, f: &Fence, kind: EventKind, payload: &serde_json::Value, evidence: Option<BlobRef>) -> Result<u64, Failure>;
    pub fn late_evidence(&mut self, run: RunId, evidence: BlobRef) -> Result<u64, Failure>;
    // Reading, for `run.get`, `run.trace`, and reconcile.
    pub fn run(&mut self, run: RunId) -> Result<RunRow, Failure>;
    pub fn events(&mut self, run: RunId, after: u64, limit: u32) -> Result<Vec<RunEvent>, Failure>;
    pub fn records(&mut self, run: RunId) -> Result<Vec<RecordRow>, Failure>;
    pub fn record(&mut self, record: RecordRef) -> Result<RecordRow, Failure>;
    pub fn locks(&mut self, run: RunId) -> Result<Vec<LockRow>, Failure>;
    pub fn tally(&mut self, run: RunId) -> Result<RecordTally, Failure>;   // verified, unchanged, acknowledged, open
    /// A stored failure, whole: class and code from the row; message, next step, and `said` from its event.
    pub fn run_failure(&mut self, run: RunId) -> Result<Option<Failure>, Failure>;
    pub fn record_failure(&mut self, record: RecordRef) -> Result<Option<Failure>, Failure>;
    // A caller's own runs and idempotency keys (decision 0160).
    pub fn runs(&mut self, query: &RunQuery) -> Result<Vec<RunRow>, Failure>;  // newest first, at most RUN_PAGE_MOST (200)
    pub fn caller_key(&mut self, ws: &WorkspaceId, who: &PrincipalId, key: &CallerKey) -> Result<Option<CallerKeyRow>, Failure>;
    pub fn hold_caller_key(&mut self, ws, who, key, request: &Digest) -> Result<(), Failure>;   // before the run is admitted
    pub fn bind_caller_key(&mut self, ws, who, key, run: RunId) -> Result<(), Failure>;        // once; a run of the same caller
    pub fn release_caller_key(&mut self, ws, who, key) -> Result<(), Failure>;                 // an unbound key only
    pub fn run_called_since(&mut self, ws, who, call: (&OperationName, &Environment, &Digest), since: Timestamp)
        -> Result<Option<RunId>, Failure>;   // the run a held key's call was admitted as
}
pub struct RunQuery { pub workspace, pub principal, pub states: Vec<RunState>, pub tool, pub target,
                      pub since: Option<Timestamp>, pub caller_key: Option<CallerKey>, pub after: Option<RunId>, pub limit: u32 }
pub struct CallerKey(/* 1 to 200 printable ASCII, no space; debug prints nothing of it */);
impl CallerKey { pub fn new(text: &str) -> Result<CallerKey, Failure>; pub fn as_str(&self) -> &str; }
pub struct CallerKeyRow { pub request: Digest, pub run: Option<RunId>, pub held_at: Timestamp }
pub struct RecordRef { pub run: RunId, pub step_seq: u32, pub record_seq: u32 }
pub enum WriteClearance { Approval(ApprovalId), PreApproved(DecisionId) }
pub struct ReadBack { pub digest: Digest, pub after: Option<BlobRef>, pub evidence: BlobRef }
pub enum Reconciled {
    Landed { read: ReadBack, acted: Acted },  // the outputs and assigned key the read found, kept with the record
    NotLanded { evidence: BlobRef },          // a record goes back to `planned` only on a read that was made
    Neither { evidence: BlobRef },
    NeverSent,                                // a `failed` record with no intent since it was planned, checked from its row
}
// RecordRow also carries `mode` (the binding its intent was stored for), `acted_at`, `reconciled_at`,
// `acknowledged_at`, `intent_seq` (the event that stored its current intent: what was recorded after it belongs
// to this attempt; decision 0185) and `intent_at`, and `in_doubt()`, rule 5's question for one row,
// `acknowledged()`, and `ended_on()`.
pub struct AcknowledgedInDiff { /* record, diff blob; made only by Store::acknowledged_in_diff */ }
pub struct RecordTally { pub verified: u32, pub unchanged: u32, pub acknowledged: u32, pub open: u32 }   // settled()
pub enum Settled { Verified, Planned, Stale }
pub fn replay(events: &[RunEvent]) -> Result<Replayed, Failure>;   // the state the events alone give

/// Proof that one record's intent is durably stored under the current lease.
/// No public constructor, not Clone, not Serialize, not Default.
#[must_use]
pub struct IntentToken { /* run, record, key, epoch, intended digest, idempotency token, intent event seq */ }
impl IntentToken {
    pub fn run(&self) -> RunId;                 pub fn record(&self) -> RecordRef;
    pub fn record_key(&self) -> &RecordKey;     pub fn epoch(&self) -> Epoch;
    pub fn intended(&self) -> &Digest;
    pub fn idempotency_token(&self) -> &str;   // stable per run and commit point; the stamp for `stamped` identity
}
#[must_use] pub struct ActedToken { /* record, epoch */ }

// Evidence, findings, versions, audit.
impl Blobs<'_> {
    pub async fn put(&self, new: NewBlob) -> Result<BlobRef, Failure>;   // workspace, owner, kind, retention class, plain bytes
    pub async fn get(&self, blob: &BlobRef) -> Result<Opened, Failure>;
    pub async fn exists(&self, blob: &BlobRef) -> Result<bool, Failure>;
}
impl Findings<'_, '_> {
    pub fn put(&mut self, workspace: &WorkspaceId, target: &TargetName, f: &Finding) -> Result<(), Failure>; // idempotent by id
    pub fn get(..) -> Result<Option<Finding>, Failure>;   pub fn list(..) -> Result<Vec<Finding>, Failure>;
    pub fn open_question(&mut self, q: NewUnknown) -> Result<UnknownId, Failure>;
    pub fn set_status(&mut self, id: UnknownId, status: UnknownStatus) -> Result<(), Failure>;
    pub fn settle(&mut self, id: UnknownId, s: &Settlement) -> Result<(), Failure>;   // evidence required
    pub fn unknown(..) -> Result<Option<UnknownRow>, Failure>;   pub fn unknowns(..) -> Result<Vec<UnknownRow>, Failure>;
}
impl Versions<'_, '_> {
    pub fn put(&mut self, v: &PublishedVersion) -> Result<(), Failure>;     // version.publish only
    pub fn load(&mut self, workspace: &WorkspaceId, target: &TargetName, hash: &Digest) -> Result<EncodedPlan, Failure>; // hash checked
    pub fn make_current(..) -> Result<(), Failure>;   pub fn current(..) -> Result<Option<Digest>, Failure>;
    pub fn list(..) -> Result<Vec<VersionRow>, Failure>;
}
impl Audit<'_, '_> {
    pub fn append(&mut self, e: &AuditEntry) -> Result<AuditSeq, Failure>;
    pub fn entries(&mut self, workspace: &WorkspaceId, after: AuditSeq, limit: u32) -> Result<Vec<AuditRow>, Failure>;
    pub fn verify(&mut self, workspace: &WorkspaceId) -> Result<ChainCheck, Failure>;   // Intact { entries, head } | Broken { at }
}
```

Contracts that the signatures do not show:

- **A transaction lives inside one closure.** The closure is `Send + 'static`, so it owns what it uses; it returns a value, which reaches the caller only if the commit succeeded. Code that needs to wait, such as sealing a blob, does so before or after the closure, never inside.
- **The intent token is the write gate.** The engine receives it through `Control`, and the driver's handle in `a2m-provider` refuses a step that commits without one, so a driver cannot be called for a write without it. A token is made only by `Store::store_intents`, and only after the transaction that stored the intent has committed with a sync to disk. `mark_acted` consumes it, and refuses a record whose row is not in `intent` under the token's lease and intent event. A caller may read the token's fields and pass it by reference.
- **Rule 5 is enforced by the data.** `store_intents` refuses, and so does entering `running`, while any record of the run is in doubt with an intent from a lease below the current one. In doubt means `intent`, `acted` and not ended on an acknowledgement, or `failed` and not read back since, either after its write was sent (`acted_at` is set), whatever class it failed with, or after an intent with a class other than `policy`, `stale`, or `input`. `mark_failed` keeps the lock of a record that reached `acted`, quarantined, whatever the class: the class is the caller's word, and only a read settles a sent write (decision 0147). There is no flag to forget to set.
- **Rule 3 reads two policy tables.** `begin_writes` reads the approval it is given and refuses unless it names this run and its workspace, carries the run's diff hash and tool version, names a principal, is unexpired by the store's clock, is not revoked, proves presence wherever its channel is not the engine's own prompt, is marked used, and was changed by this very transaction, which the store knows from the database's update hook. For a pre-approval it refuses unless the decision row is an allowed `write_without_person` of this run and workspace, with this diff hash, made under a policy version, and recorded in this very transaction, so no kill switch or newer set came between the decision and the writes. Each refusal is a test (decision 0145). The ledger never marks an approval used. A write run with records to write cannot enter `running` any other way, and no intent is stored for a run that was never cleared.
- **The rules are held twice.** Each call checks the rule and answers with a failure that says what was refused. The triggers of the migrations refuse the same things whoever sends the statement: a record acted without an intent, verified without a matching read-back and evidence, an intent under another lease, a change to a final run, an event out of order or from another lease, a second lock on a record, an update or delete of history; and, from migration 0012, a write run entering `running` uncleared or with a clearance that is not its own, an intent for a run not cleared, an intent while a record of an earlier lease is in doubt, an intent without the record's lock under its lease, and a return to `planned` without a read; and, from migration 0015, an acknowledgement on a record that is not `acted` under the lease of a cleared irreversible run or that carries no evidence, any change to a record that ended on one, and a run entering `completed` while a record is neither verified, unchanged, nor acknowledged. Three things no trigger holds, and the code alone does: whether an approval has expired, which needs the store's clock, and whether it was marked used in this very transaction, which only the connection knows, and whether the run's diff says a record's write is `acknowledged`, because the diff is sealed. Migration 0002's opening sentence, that its triggers hold all nine rules, was not true of rules 3 and 5; migrations are not edited once applied, so 0012 says so.
- **A record that ends on an acknowledgement** ([decision 0172](../../decisions/0172-the-ledger-ends-a-record-on-an-acknowledgement-and-keeps-what-the-system-said.md)). A binding whose only evidence is the system's answer (`acknowledged`, decision 0161) ends its record through `mark_acknowledged`. The ledger takes that evidence from the run's own stored diff, a sealed blob it opens outside the transaction (`Store::acknowledged_in_diff`, reading the diff's `records` by `step_seq`, `record_seq`, `operation`, and `evidence`), never from the caller's word, and checks inside the transaction that the run's diff is still that one. It refuses a run that is not `running`, not cleared, or not irreversible; a record not `acted` under the lease, ended before, or without its lock; and evidence that names no blob. The record stays `acted`, keeps the answer in `evidence_ref`, has no read-back, and is settled: its lock is released, rule 5 and reconcile pass over it, and its run may complete. A second `acted` event says `"evidence": "acknowledged"`, so `replay` gives the same. Migration 0015 holds it in triggers.
- **A stored failure keeps what the system said.** `set_state` and `mark_failed` write the failure's class, code, sentence, next step, and `said` into the event, redacted; `run_failure` and `record_failure` hand the failure back whole, with `said` apart, untrusted, never joined into the sentence.
- **Events and state together.** Every mutating call appends a `run_events` row with no gap in `seq` and updates the current row in the same transaction. `replay` rebuilds the state from the events, and a test compares.
- **Locks follow the record.** A lock is released in the transaction that marks its record `verified`, or `failed` with nothing in doubt. It is quarantined when a record fails after an intent with the outcome in doubt, when the run stops or needs a sign-in, and when a new lease is granted, for every record then in doubt; the run's other locks are released at the same moments. A quarantined lock is taken back only by its own run, under a new lease, on the way to `settle`.
- **Append-only by trigger.** Triggers refuse an update or delete on `run_events` and `audit`, except deletes of a run whose retention state is `purging`, which only the retention job will set.
- **Redaction on the way in.** Event payloads, audit entries, questions, and failure messages pass the redactor before they are written.
- **A blob is named by a keyed digest.** `BlobRef` is the workspace's keyed digest of the plain content, never its plain hash, so someone who can read the store cannot confirm a guessed value by hashing it. Identical evidence within a workspace is still stored once. A digest that must be the same on every machine, such as a diff hash or a plan hash, is never a blob name. A record key is named in rows and locks by the workspace's keyed digest of its canonical bytes, for the purpose `record_key`: the same construction a trace uses, so the two agree.
- **Wrapped keys are rows, nothing more.** `vault_keys` holds what the vault hands over, already wrapped. The ledger never unwraps, chooses, or rotates a key; the vault does, through `KeyRecords`.
- **Blobs are sealed before they are written,** with the workspace, the owner, the kind `blob`, and the blob's name as the sealing context, and synced to disk before the row that names them commits. A blob whose row never committed is collected by retention. A row that takes a blob reference refuses one that names no blob.
- **An `Opened::Existing` is not an error.** A second identical call gets the first run's identifier.
- **A finding cannot be stored above `discovered`.** The type refuses to make one or to read one back, and the table refuses the row.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-policy` | `Store::write`, `Store::read`, `Tx`, `Store::policy_key` once, and its own tables through `Tx::policy_tables` |
| `a2m-auth`, `a2m-sched` | `Store::write`, `Store::read`, `Tx`, and their own tables through `Tx::tables` |
| `a2m-actions` | `Store::open`, `key_records`, `attach`; versions, audit, findings and questions, retention. As the in-process `Control`: the run-path calls (opening a run, intents, acted, verified, reconcile, locks, events, evidence) on the engine's behalf |
| `a2m-runner`, control-plane side of the runner link | The same run-path calls, answering a runner's `need` and `event` messages |
| `a2m-runner`, runner side | A token rebuilt from the control plane's acknowledgement. Not built: until the runner link exists the ledger offers no way to make a token but storing an intent |
| `a2m-engine` | The `IntentToken` type only. It never opens the store |
| `a2m-provider` | `IntentToken`, which the driver's handle checks on every step that commits |
| `a2m-vault` | `StoreKeyRecords`, through its own `KeyRecords` trait |

It uses `a2m-vault` through `Vault::seal` and `Vault::open`, the `Redact` and `KeyedDigest` of `a2m-core` as implemented by the vault, and `a2m-telemetry` for its clock and for a span on each commit.

## State

The tables in the owner table above that are its own, the migration files, the database file `store.sqlite3`, and the blob directory `blobs/` in the data directory (object storage or the database on Postgres). A store in memory, for tests, keeps its blobs sealed in the database. In memory: one thread that owns the writing connection and serves a queue of closures, and on a store on disk two reading threads with a connection each, so that no database call is made on an async thread.

## Failures

The codes are constants in the crate's `codes` module.

| Class | Code | When |
| --- | --- | --- |
| `busy` | `storage_full` | A blob or row cannot be written for lack of space |
| `busy` | `store_unavailable` | The database cannot commit, the store's threads have stopped, the code inside a transaction panicked, or no vault is attached yet |
| `busy` | `store_corrupt` | The integrity check, a stored row that no longer reads, a stored plan's hash, a stored finding that no longer holds, or a blob's missing file |
| `busy` | `store_version_unsupported` | The store holds a migration this engine does not carry, or carries differently |
| `busy` | `record_locked` | Another run holds the lock |
| `possible_commit` | `runner_lost` | A call carries a lease number other than the run's |
| `possible_commit` | `reconcile_required` | Rule 5: an intent or an entry into `running` while a record is in doubt from an earlier lease |
| `policy` | `approval_missing`, `approval_stale` | Rule 3 finds no clearance, or a clearance for another diff |
| `mismatch` | `read_back_differs` | `mark_verified`, or `settle` with `Landed`, is given a digest other than the intended one |
| `stale` | `changed_since_preview` | The class and code a record is left with by `settle` with `Neither`. The call itself succeeds and answers `Settled::Stale`, so that what it recorded is not taken back with the caller's transaction |
| `unknown_variant` | `ledger_state_unexpected` | A transition no rule allows: a final run changed, a record skipping a state, a constraint or a trigger of the tables refusing a row |
| `unknown_variant` | `table_not_owned` | A statement through `Tx::tables` names a table of another crate, or does what only the store may |
| `input` | `not_found` | No run, record, blob, version, or question with that identifier |
| `input` | `schema_invalid`, `plan_hash_mismatch` | Something the store cannot keep; a plan whose hash is not the digest of its bytes |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Each of the nine rules of section 208 | One test per rule in `tests/rules.rs`, with no application, that breaks the rule through the calls and, where a statement can express it, through a second connection to the database file, and sees the refusal |
| A record ends on an acknowledgement only where the run's stored diff says so, stays `acted`, is never verified, and is settled | `tests/acknowledged.rs`: each acceptance and each refusal, through the calls and through a second connection |
| A stored failure hands back what the system said apart from its sentence | `tests/acknowledged.rs` |
| Current state equals the state rebuilt from events | One scripted run that passes through every record state, a lost lease, and a reconcile, replayed and compared. A property test over random sequences of calls is not written |
| No event or audit row can be updated or deleted outside retention | Tests that try, through a second connection. On SQLite only |
| An intent returned is on disk | A second connection reads the row as soon as `store_intents` returns; the store is closed and opened again and the reconcile query finds the record. The process is not killed in a test |
| Every table has exactly one owner, and SQL names a table only in its owner crate | The migration test, and the authorizer, with a test of statements it refuses. The check over the source for `Owner::X` outside crate X is owed by the audit |
| An intent token cannot be made or copied outside this crate | Two documentation tests that must fail to compile, and a test that compiles only while the token is neither `Clone` nor `Default` |
| A blob is never stored in the clear, and its row never names a missing file | Tests on the blob directory |
| A blob's name differs from the plain hash of its content, and two workspaces name the same content differently | A test with a known value |
| A finding is never stored above `discovered` | The type's refusal, the table's refusal of the row, and a planted body refused when read |
| The audit chain detects a removed, changed, or reordered entry | Planted tampering in a test |
| A migration applied twice changes nothing, and a store from a newer engine is refused | A test that opens twice, then plants a later migration and a changed checksum |
| Both databases give the same answers | The rule tests run on SQLite and Postgres in CI from Phase 6 |

## Build order

1. Done: `Store::open` on SQLite, migrations with checksums, `Tx`, `runs` and `run_events`. `vault_keys` and `attach`; `blobs`, named by keyed digest. Not built: a backup before migrating, and a lock that keeps two processes off one data directory.
2. Done: `run_records`, `record_locks`, `Fence`, the intent token, and the nine rules as tests.
3. Done: reconcile: `in_doubt`, `settle`, quarantine. Not built: a way for a person to release the quarantined lock of a run that ended final with a record still in doubt.
4. Done: assigned keys and stored outputs on record rows. Not built: values known before the commit point, recorded before `acted`.
5. Done: findings and the unknowns ledger. Findings in an older format are kept and are not listed; marking them as old in a listing is not built.
6. Done: the audit chain, and published versions with their plans. Done since (decision 0160): callers' idempotency keys and a caller's listing of their runs. Not built: the outbox, `run_steps`, `workspaces`, retention and legal hold (the columns and the triggers' exception for a run being purged exist; nothing sets them), and a token rebuilt from an acknowledgement on a runner.
7. Done (the safety audit, decision 0145): the policy engine's key; `begin_writes` reading the whole clearance; the triggers of migration 0012; a sent write in doubt whatever its class; the intent's mode, a landed record's outputs, and the evidence of a `not landed` read kept with the record; the `testing` feature.
8. Done (the kernel's second pass, [decision 0172](../../decisions/0172-the-ledger-ends-a-record-on-an-acknowledgement-and-keeps-what-the-system-said.md)): a record that ends on an acknowledgement, with migration 0015 and its triggers; `tally`; a caller key's digest under its own purpose; a stored failure that keeps what the system said, read back whole by `run_failure` and `record_failure`. Not built: a person's settlement of an `acknowledged` record whose answer was lost.
8a. Done ([decision 0180](../../decisions/0180-a-targets-offline-tests-cover-writes-and-gate-the-publish.md)): `StoreConfig::throwaway` and `Store::is_throwaway`, an offline test's own store in memory, which keeps its policy key; `WriteClearance::OfflineTest`, taken only there, recorded as a decision of the test's rule so the triggers of rule 3 hold; `PublishedVersion::gate_evidence` and `Versions::gate_evidence`, with migration 0016. Tests: `tests/offline.rs`, `tests/store.rs`.
8b. Done ([decision 0196](../../decisions/0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md)): `Ledger::recorded_before_commit`, the values recorded before the save of every record of an operation in a workspace whose write may have landed (`RecordRow::may_have_landed`); migration 0017, a trigger that keeps a version's `gate_evidence` as written (LE-1); migration 0018, `actions_scouts.project`.
9. Postgres, with the queue patterns of section 208. Phase 6. Everything that names the database crate is in `src/sqlite.rs`. The migrations are in SQLite's dialect and need a second set. The statements elsewhere use numbered parameters and `ON CONFLICT`, which both databases have; the one exception is the row number read for rule 3.

## Open points

- Durability per commit on SQLite is a setting changed per transaction on the one writing connection ([decision 0080](../../decisions/0080-the-store-is-one-sqlite-file-and-a-crate-reaches-its-own-tables-through-the-transaction.md)). It is not measured yet against hypothesis H-15; nor is the cost of preparing another crate's statement on every call, which the authorizer requires.
- The table of section 208 that says which failure class ends in which run state is the engine's. The ledger holds the part about locks ([decision 0081](../../decisions/0081-what-the-ledger-holds-as-data-the-token-doubt-locks-and-rule-three.md)) and takes the run state the engine gives it.
- A write run whose diff holds nothing to write enters `running` with no clearance, because it has no `planned` record; no intent can then be stored for it. Section 208 describes that path; whether policy should record a decision for it anyway is open.
- A run that ends in a final state with a record in doubt keeps that record's lock quarantined for good, which is safe and needs a person's action that does not exist yet.
- A chain whose last entries were cut off still verifies. `verify` gives the head's hash so that a copy kept outside the store can show it; where that copy is kept is not designed.
- Whether reads' decision rows and run rows are summarized at scale, as decision 0034 allows.
