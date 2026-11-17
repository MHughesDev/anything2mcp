# `a2m-ledger`

The store: one schema on SQLite and Postgres, its migrations and transactions, and the tables that say what happened. It holds runs, the ledger of commit points and its nine rules, run events, evidence blobs, findings and open questions, published versions, and the audit chain. Everything in the kernel that persists goes through it.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 3, Stores |
| May depend on | Layers 1 and 2 |
| Depends on | `a2m-core`; `a2m-vault` to seal blobs and to implement its `KeyRecords`; `a2m-telemetry` for spans. Outside: an SQL crate that speaks both databases, a time-ordered UUID crate, and a SHA-256 crate, each to be verified |
| Built in | Phase 1, group H: runs, events, blobs, migrations, on SQLite. Phase 3, group A and slice 3.1: record rows, locks, the nine rules. Stage E, package E1: findings and open questions. Phase 6: Postgres |
| Carries out | Spec sections [10](../../spec/10-runs-ledger-and-resume.md), [42](../../spec/42-run-ledger-design.md), [50](../../spec/50-record-identity-and-dependent-records.md) (assigned keys and stored outputs), [47](../../spec/47-traces-and-evidence.md) (where evidence is kept), [52](../../spec/52-when-the-engine-itself-fails.md) (no ledger, no write), [58](../../spec/58-discovery-and-findings.md) (the findings store and the unknowns ledger); the [ledger schema](../../reference/ledger-schema.md) |

## Owns

- Opening the store, checking its integrity, and running migrations: numbered, forward-only, one sequence for the whole schema, tested on both databases.
- Transactions. A `Tx` is the only way to read or write a table, and every crate that persists is handed one.
- The ledger proper: run rows, pipeline step rows, one record row per commit point, and run events, with the nine rules of section 42 enforced where the data is, not by callers' good behavior.
- The intent token: proof, as a type, that an intent was durably stored.
- Evidence blobs, named by the keyed digest of their plain content and sealed through the vault; their retention and legal hold.
- The vault's wrapped keys, kept through the vault's `KeyRecords`, which this crate implements.
- Findings and the unknowns ledger, as stored data.
- Published versions and their plan bytes, with the hash checked on every load.
- The audit chain, one per workspace.
- The outbox and callers' idempotency keys.

It must never: read or write a target system; decide whether something may happen; normalize or compare business values itself (it compares digests it is given); decode a plan; hold a secret in the clear; let a row be written outside a transaction.

## How a service owns its tables

[Decision 0061](../../decisions/0061-the-layers-are-redrawn-so-that-every-needed-dependency-is-allowed.md) says a service owns the meaning of its tables and the store owns the connection, the transactions, and the migrations. Concretely:

| Concern | Who | How |
| --- | --- | --- |
| The connection, pooling, the single SQLite writer, durability per commit | `a2m-ledger` | `Store` and `Tx` |
| The migration that creates a table | `a2m-ledger` holds the file; the file's first line names the owning crate | One numbered sequence, so a store is never half one crate's version and half another's |
| The queries on a table, its row types, its state rules | The owning crate | A module in that crate that takes `&mut Tx` |
| Atomicity across owners | The caller that began the `Tx` | Two crates' writes in one `Tx` commit together or not at all |

Table owners, provisional until each table's first slice:

| Owner | Tables |
| --- | --- |
| `a2m-ledger` | `workspaces`, `runs`, `run_steps`, `run_records`, `run_events`, `record_locks`, `blobs`, `outbox`, `caller_idempotency`, `target_versions`, `findings`, `unknowns`, `audit`, `vault_keys`, `schema_migrations` |
| `a2m-policy` | `principals`, `grants`, `policy_versions`, `decisions`, `approvals`, `consents`, `delegations`, `kill_switches`, `quota_counters` |
| `a2m-auth` | `sessions`, `session_events`, `session_state` |
| `a2m-sched` | `jobs`, with its `session_key` column; `runners`; `budgets`; later a rate-bucket table |
| `a2m-engine` | `bindings_health`, written through `Control` |
| `a2m-actions` | `toolsets`, `toolset_items`; `scout_stages` (run, stage, input hash, result reference); `outbox_deliveries`, the outcomes of delivering the outbox's webhooks |

A test reads every migration and fails if a table has no owner or two. A check over the source fails if SQL naming a table appears outside its owner crate. The one declared exception is the ledger's check for rule 3, which reads `approvals` and `decisions` (below).

## Interface

```rust
// Opening. Called once by Kernel::open in a2m-actions.
pub struct StoreConfig { /* Sqlite { path } | Postgres { url_ref }; blob location; retention classes */ }
pub struct Store { /* pool; the SQLite writer; the sealer; the redactor */ }
impl Store {
    /// Opens, checks integrity, backs up and migrates. Refuses a half-migrated or newer store.
    pub async fn open(cfg: StoreConfig) -> Result<Store, Failure>;
    /// What the vault is opened with. Reads and writes only `vault_keys`.
    pub fn key_records(&self) -> StoreKeyRecords;
    /// Joins the opened vault. Until then, blobs, events, and audit refuse, and health says degraded.
    pub fn attach(&self, seal: Arc<dyn Seal>, name: Arc<dyn KeyedDigest>, redact: Arc<dyn Redact>);
    pub async fn begin(&self, durability: Durability) -> Result<Tx<'_>, Failure>;
    pub fn health(&self) -> StoreHealth;               // can commit; evidence space; integrity
    pub fn ledger(&self) -> Ledger<'_>;  pub fn blobs(&self) -> Blobs<'_>;
    pub fn findings(&self) -> Findings<'_>;  pub fn audit(&self) -> Audit<'_>;  pub fn versions(&self) -> Versions<'_>;
}
pub enum Durability { Synced, Relaxed }   // Synced for intents, approvals, audit; Relaxed for read runs
pub struct Tx<'s> { /* one database transaction; rolled back on drop unless committed */ }
impl Tx<'_> {
    pub fn now(&self) -> Instant;                      // process clock on SQLite, database clock on Postgres
    pub async fn exec(&mut self, q: Query<'_>) -> Result<u64, Failure>;
    pub async fn rows<R: FromRow>(&mut self, q: Query<'_>) -> Result<Vec<R>, Failure>;
    pub async fn commit(self) -> Result<Committed, Failure>;
}

// Defined elsewhere, used here: `Seal` and `KeyRecords` in a2m-vault; `KeyedDigest` and `Redact` in a2m-core.
// Each sealed item is bound to its context; a keyed digest names a thing without revealing it.
pub struct StoreKeyRecords { /* the store */ }
impl KeyRecords for StoreKeyRecords { /* get, put, list by layer, retire a generation */ }
pub struct VaultKeyRow { pub id: KeyId, pub layer: KeyLayer, pub generation: u32, pub wrapped: Vec<u8>, pub wrapped_by: Option<KeyId> }

// The run store and the ledger. Every mutating call carries a Fence and appends an event
// in the same Tx as the current-state change.
pub struct Fence { pub run: RunId, pub epoch: u64 }
impl Ledger<'_> {
    pub async fn open_run(&self, tx: &mut Tx<'_>, new: NewRun) -> Result<Opened, Failure>; // Opened::New | Opened::Existing (rule 6)
    pub async fn set_state(&self, tx: &mut Tx<'_>, f: &Fence, to: RunState, why: Option<&Failure>) -> Result<(), Failure>;
    pub async fn store_diff(&self, tx: &mut Tx<'_>, f: &Fence, diff: BlobRef, hash: Digest) -> Result<(), Failure>;
    /// Rule 3. The first entry of a write run into `running`.
    pub async fn begin_writes(&self, tx: &mut Tx<'_>, f: &Fence, c: WriteClearance) -> Result<(), Failure>;
    pub async fn plan_records(&self, tx: &mut Tx<'_>, f: &Fence, rs: &[PlannedRecord]) -> Result<(), Failure>;
    pub async fn take_lock(&self, tx: &mut Tx<'_>, f: &Fence, k: &LockKey) -> Result<LockHeld, Failure>;
    /// Rule 1. Commits with Durability::Synced before returning tokens. Up to the intent window at once.
    pub async fn store_intents(&self, f: &Fence, rs: Vec<RecordIntent>) -> Result<Vec<IntentToken>, Failure>;
    pub async fn mark_acted(&self, tx: &mut Tx<'_>, t: IntentToken, a: Acted) -> Result<ActedToken, Failure>;
    /// Rule 2. Refuses unless the read-back digest equals the intended digest and evidence is given.
    pub async fn mark_verified(&self, tx: &mut Tx<'_>, t: ActedToken, rb: ReadBack) -> Result<(), Failure>;
    pub async fn mark_failed(&self, tx: &mut Tx<'_>, f: &Fence, rec: RecordRef, why: &Failure) -> Result<(), Failure>;
    /// Rule 5. Records in `intent`, `acted`, or a retryable `failed`, in key order.
    pub async fn in_doubt(&self, tx: &mut Tx<'_>, run: RunId) -> Result<Vec<DoubtfulRecord>, Failure>;
    pub async fn settle(&self, tx: &mut Tx<'_>, f: &Fence, rec: RecordRef, seen: Reconciled) -> Result<(), Failure>;
    pub async fn new_epoch(&self, tx: &mut Tx<'_>, run: RunId) -> Result<Fence, Failure>;   // lease granted
    pub async fn quarantine_locks(&self, tx: &mut Tx<'_>, run: RunId) -> Result<(), Failure>;
    pub async fn late_evidence(&self, tx: &mut Tx<'_>, run: RunId, e: BlobRef) -> Result<(), Failure>;
}
pub enum WriteClearance { Approval(ApprovalId), PreApproved(DecisionId) }
pub enum Reconciled { Landed { after: BlobRef, evidence: BlobRef }, NotLanded, Neither { evidence: BlobRef } }

/// Proof that one record's intent is durably stored under the current lease.
/// No public constructor, not Clone, not Serialize, not Default.
#[must_use]
pub struct IntentToken { /* run, record, epoch, intended digest, idempotency token, intent event seq */ }
impl IntentToken {
    pub fn record_key(&self) -> &RecordKey;
    pub fn idempotency_token(&self) -> &str;   // stable per run and key; the stamp for `stamped` identity
    pub fn intended(&self) -> &Digest;
    /// Only a runner calls this, with the control plane's acknowledgement of a remote intent.
    pub fn from_acknowledgement(ack: IntentAck) -> Result<IntentToken, Failure>;
}
#[must_use] pub struct ActedToken { /* the same, after `acted` */ }

// Evidence, findings, versions, audit.
impl Blobs<'_> {
    pub async fn put(&self, kind: BlobKind, owner: OwnerRef, plain: &[u8], class: RetentionClass) -> Result<BlobRef, Failure>;
    pub async fn get(&self, r: &BlobRef) -> Result<Vec<u8>, Failure>;
}
impl Findings<'_> {
    pub async fn put(&self, tx: &mut Tx<'_>, target: &TargetName, f: &Finding) -> Result<(), Failure>; // idempotent by id
    pub async fn open_question(&self, tx: &mut Tx<'_>, q: NewUnknown) -> Result<UnknownId, Failure>;
    pub async fn settle(&self, tx: &mut Tx<'_>, id: UnknownId, s: Settlement) -> Result<(), Failure>;
}
impl Versions<'_> {
    pub async fn put(&self, tx: &mut Tx<'_>, v: PublishedVersion) -> Result<(), Failure>;     // version.publish only
    pub async fn load(&self, target: &TargetName, hash: &Digest) -> Result<PlanBytes, Failure>; // hash checked
}
impl Audit<'_> {
    pub async fn append(&self, tx: &mut Tx<'_>, e: AuditEntry) -> Result<AuditSeq, Failure>;
    pub async fn verify(&self, ws: WorkspaceId, from: AuditSeq) -> Result<ChainCheck, Failure>;
}
```

Contracts that the signatures do not show:

- **The intent token is the write gate.** The engine receives it through `Control`, and the driver's handle in `a2m-provider` refuses a step that commits without one, so a driver cannot be called for a write without it. In one process the type is the guard. Across the runner link, the ledger is: `mark_acted` refuses a record whose row is not in `intent` under the same epoch and intent event, so a token made from a forged or replayed acknowledgement changes nothing. A caller may read the token's fields and pass it by reference. It may not keep it past `mark_acted`, which consumes it.
- **Rule 5 is enforced by the data.** `store_intents` refuses while any record of the run is in `intent` or `acted` with an intent epoch below the current one. There is no flag to forget to set.
- **Rule 3 reads two policy tables.** `begin_writes` checks, in SQL and in the same `Tx`, that the approval row names this run, carries the run's diff hash, and was marked used in this transaction by `a2m-policy`; or that the decision row allowed this run to write without a person. The ledger never marks an approval used.
- **Events and state together.** Every mutating call appends a `run_events` row with no gap in `seq` and updates the current row in the same `Tx`. A test rebuilds current state from events and compares.
- **Append-only by trigger.** Migrations install triggers that refuse an update or delete on `run_events` and `audit`, except deletes of a run whose retention state is `purging`, which only the retention job sets.
- **Redaction on the way in.** Event payloads, audit entries, and failure messages pass the `Redact` given at open before they are written.
- **A blob is named by a keyed digest.** `BlobRef` is the workspace's keyed digest of the plain content, never its plain hash, so someone who can read the store cannot confirm a guessed value by hashing it. Identical evidence within a workspace is still stored once. A digest that must be the same on every machine, such as a diff hash or a plan hash, is never a blob name.
- **Wrapped keys are rows, nothing more.** `vault_keys` holds what the vault hands over, already wrapped. The ledger never unwraps, chooses, or rotates a key; the vault does, through `KeyRecords`.
- **Blobs are sealed before they are written,** with the blob's name, kind, and owner as the sealing context, and synced to disk before the row that names them commits. A blob whose row never committed is collected by retention.
- **An `Opened::Existing` is not an error.** A second identical call gets the first run's identifier.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-policy`, `a2m-auth`, `a2m-sched` | `Store::begin`, `Tx`, and their own tables through it |
| `a2m-actions` | `Store::open`, `key_records`, `attach`; versions, audit, findings and questions, retention. As the in-process `Control`: the run-path calls (opening a run, intents, acted, verified, reconcile, locks, events, evidence) on the engine's behalf |
| `a2m-runner`, control-plane side of the runner link | The same run-path calls, answering a runner's `need` and `event` messages |
| `a2m-runner`, runner side | `IntentToken::from_acknowledgement` |
| `a2m-engine` | The `IntentToken` type only. It never opens the store |
| `a2m-provider` | `IntentToken`, which the driver's handle checks on every step that commits |
| `a2m-vault` | `StoreKeyRecords`, through its own `KeyRecords` trait |

It uses `a2m-vault` through `Seal`, the `Redact` and `KeyedDigest` of `a2m-core` as implemented by the vault, and `a2m-telemetry` for spans on each commit.

## State

The tables in the owner table above that are its own, the migration files, and the blob directory `blobs/` in the data directory (object storage or the database on Postgres). In memory: the connection pool, and on SQLite one writer connection behind an async queue, run on a blocking thread so that no database call blocks an async thread.

## Failures

| Class | Code | When |
| --- | --- | --- |
| `busy` | `storage_full` | A blob or row cannot be written for lack of space |
| `busy` | `store_unavailable` (new) | The database cannot commit. Health becomes `stopped` |
| `busy` | `store_corrupt` (new) | The integrity check, a stored plan's hash, or the audit chain fails |
| `busy` | `record_locked` | Another run holds the lock |
| `possible_commit` | `runner_lost` | A call carries an epoch older than the run's |
| `policy` | `approval_missing`, `approval_stale` | Rule 3 finds no clearance, or a clearance for another diff |
| `mismatch` | `read_back_differs` | `mark_verified` is given a digest other than the intended one |
| `stale` | `changed_since_preview` | `settle` with `Neither` |
| `unknown_variant` | `ledger_state_unexpected` (new) | A transition no rule allows: a final run changed, a record skipping a state |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Each of the nine rules of section 42 | One test per rule, with no application, that breaks the rule and sees the refusal. Slice 3.1 |
| Current state equals the state rebuilt from events | A property test over random sequences of calls |
| No event or audit row can be updated or deleted outside retention | A test that tries, through raw SQL, on both databases |
| An intent returned is on disk | A test that kills the process right after `store_intents` returns and finds the rows |
| Every table has exactly one owner, and SQL names a table only in its owner crate | The migration test and the source check |
| A blob is never stored in the clear, and its row never names a missing file | Tests on the blob directory |
| A blob's name differs from the plain hash of its content, and two workspaces name the same content differently | A test with a known value |
| The audit chain detects a removed, changed, or reordered entry | Planted tampering in a test |
| Both databases give the same answers | The rule tests run on SQLite and Postgres in CI from Phase 6 |

## Build order

1. `Store::open` on SQLite, migrations with checksums, `Tx`, `runs` and `run_events` for reads. `vault_keys` and `attach` with the first slice of `a2m-vault`; `blobs`, named by keyed digest, once it is attached. Phase 1, group H; `run.get` and `run.trace` read them.
2. `run_records`, `record_locks`, `Fence`, the intent token, and the nine rules as tests. Slice 3.1.
3. Reconcile: `in_doubt`, `settle`, quarantine. Slice 3.7.
4. Assigned keys and stored outputs on record rows. Slice 3.8.
5. Findings and the unknowns ledger. Package E1.
6. The audit chain and the outbox, with their first writers.
7. Postgres, with the queue patterns of section 42. Phase 6.

## Open points

- Which outside SQL crate, and how per-commit durability is chosen on SQLite, where synchronous mode is a connection setting: two writer connections, or a setting changed per transaction. Measured against hypothesis H-15.
- The open points of section 42 about locks when a run stops and which failure class ends where. The ledger stores whatever they decide; the decision is the engine's.
- Whether reads' decision rows and run rows are summarized at scale, as decision 0034 allows.
