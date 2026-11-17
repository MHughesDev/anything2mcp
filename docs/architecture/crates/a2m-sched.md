# `a2m-sched`

Decides when and where work may run. It admits or refuses a run, queues its jobs, hands each job to one runner under a lease, queues waiters for record locks and other exclusive resources, spends rate budgets, and keeps per-session and per-caller limits. It never carries out a step and does not know the engine exists.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 6, Engine |
| May depend on | Layers 1 to 5. Never `a2m-engine` |
| Depends on | `a2m-core`; `a2m-ledger` for the `jobs` and `runners` tables, the scheduling columns of `runs`, its transactions, and its lock calls; `a2m-telemetry` for spans and metrics. Outside: `tokio` |
| Built in | Phase 3 (admission, dedupe, leases, lock waiting, and quarantine with slice 3.7); Phase 7 (slices 7.1 and 7.2, the runner link and lost leases); slice 9.4 (the cluster profile) |
| Carries out | Spec sections [211](../../spec/2-kernel/211-concurrency.md), [208](../../spec/2-kernel/208-runs-the-ledger-and-resume.md) for rule 6 and waiting for locks, [223](../../spec/2-kernel/223-the-runner-protocol.md) for leases, matching, and rate budgets, [224](../../spec/2-kernel/224-when-the-engine-itself-fails.md) for bounded queues, [225](../../spec/2-kernel/225-performance.md) for the scheduling budget; the [ledger schema](../../reference/ledger-schema.md) |

## Owns

- Admission: whether a run is accepted, returned as an existing run, or refused with a retry time.
- Queues: one per target and environment, bounded, ordered by priority, then fairly by caller.
- Job leases: who may work on a job, until when, under which number.
- Waiting for a record lock: the queue of runs that want a lock the ledger reports as held, woken when the ledger releases it. The lock itself, its table, and its rules are `a2m-ledger`'s, because an intent must check the lock in the same transaction.
- Rate budgets per surface, granted in batches.
- Per-session parallelism, exclusive resources such as a screen or a device link, and per-caller shares.
- Detecting lost leases and scheduling the reconcile and verification jobs that follow.

It must never: call the engine, a driver, or the session keeper; read a plan; decide policy; change a record row of the ledger; let a job run twice at once; take, release, or quarantine a record lock itself.

## Interface

```rust
pub struct Scheduler { /* store handle, clock, settings, wakers, rate buckets */ }

impl Scheduler {
    pub fn new(store: Store, clock: Arc<dyn Clock>, settings: SchedSettings) -> Self;   // Store from `a2m-ledger`

    // Admission. Called by `a2m-actions` after policy has allowed the call.
    pub async fn submit(&self, s: Submission) -> Result<Admitted, Failure>;
    pub async fn cancel(&self, run: &RunId) -> Result<(), Failure>;       // a directive on the next heartbeat; final when the job ends

    // Runners. Called by the in-process runner loop in `a2m-actions`, or by the control plane's side of the runner link.
    pub async fn hello(&self, offer: RunnerOffer) -> Result<RunnerTerms, Failure>;    // heartbeat interval, lease length, protocol
    pub async fn claim(&self, runner: &RunnerId, free: &Slots, wait: Duration) -> Result<Vec<JobLease>, Failure>;
    pub async fn heartbeat(&self, lease: &LeaseRef) -> Result<Renewal, Failure>;      // time remaining and a directive
    pub async fn finish(&self, lease: &LeaseRef, end: JobEnd) -> Result<(), Failure>;
    pub async fn drain(&self, runner: &RunnerId) -> Result<(), Failure>;

    // What a running job asks for, through the engine's `Control`.
    /// The ledger refused a lock as held: wait in order for it, up to `wait` and the lease, then `record_locked`.
    pub async fn wait_lock(&self, lease: &LeaseRef, key: &LockKey, wait: Duration) -> Result<(), Failure>;
    /// The ledger released or handed on a lock: wake the first waiter.
    pub fn lock_freed(&self, key: &LockKey);
    pub async fn rate(&self, lease: &LeaseRef, surface: &SurfaceKey, want: u32) -> Result<RateGrant, Failure>;

    // Time. Called by one background task in `a2m-actions`, which waits on `next_due`.
    pub async fn sweep(&self) -> Result<SweepReport, Failure>;   // lost leases, quarantines ended, waits expired
    pub fn next_due(&self) -> Instant;                           // the earliest deadline the scheduler holds
}

pub struct Submission {
    pub run: NewRun,                       // the run row: caller, tool, version hash, input hash, effect, target, environment
    pub dedupe: DedupeKey,                 // caller, tool version hash, input hash; a file's content, not its path
    pub job: JobRequirements,
    pub priority: Priority,                // interactive or batch
    pub health: HealthState,               // the kernel's, at the moment of submission
}
pub enum Admitted { New { run: RunId, job: JobId }, Existing { run: RunId } }

pub struct JobRequirements {
    pub kind: JobKindName,                 // preview, execute, try, discover, reconcile, verify_after_loss, canary, session_keepalive
    pub target: TargetName, pub environment: Environment,
    pub modes: Vec<Mode>,                  // any of these; compared by name only
    pub os: Option<OsName>,
    pub identity: Option<Identity>,
    pub session: Option<SessionKey>,       // with its home runner and whether it is movable, from the keeper's records
    pub session_limits: SessionLimits,     // reads and writes at once; the lowest across the operation's candidate bindings
    pub exclusive: Vec<ResourceKey>,       // at most one leased job per key: a screen, a device link, a file
    pub max_step: Duration,                // the longest step time limit in the tool; sets the quarantine
    pub writes: bool,
}

pub struct JobLease {
    pub job: JobId, pub run: Option<RunId>, pub epoch: Epoch,    // only ever increases, per run
    pub length: Duration,                  // a duration, never a clock time
    pub parallelism: Parallelism,          // records the job may run at once
    pub requirements: JobRequirements,
}
pub enum JobEnd { Done(RunState), Rests(RunState), Stopped { class: FailureClass, code: FailureCode, in_doubt: bool }, LeaseEnding }
pub struct RateGrant { pub requests: u32, pub over: Duration } // or a time to wait until, inside the lease
pub trait Clock: Send + Sync { fn now(&self) -> Instant; }      // steady locally; the database clock on Postgres
```

Contracts the signatures do not show:

- **Every call from a runner carries its lease number,** and a number lower than the job's current one is refused with `policy`, `lease_superseded`. This is the scheduler's half of ledger rule 4; the store's insert checks are the other half.
- **`submit` is one transaction:** the dedupe check, the queue bound, the run row, and the job row. Two identical calls racing get one run (ledger rule 6, held by the store's unique index on `dedupe_key` among unfinished runs).
- **`claim` waits on a named condition.** In one process it is woken by an in-process signal; on Postgres by a notification, with a poll as the fallback. It never sleeps a fixed time.
- **Mode names are data.** The scheduler matches a job's modes against a runner's by string equality and holds no list of modes. Limits that differ by mode, such as one write at a time on an interface binding, reach it as numbers in `JobRequirements`, filled by `a2m-actions` from the registry's mode defaults and the settings.

## Admission

In this order. The first that refuses ends it.

| # | Check | Refusal |
| --- | --- | --- |
| 1 | The kernel's health: `stopped` refuses everything; `degraded` refuses writes | `busy`, `engine_degraded`, with the cause |
| 2 | An unfinished run with the same dedupe key exists | Not a refusal: `Admitted::Existing` with that run |
| 3 | The target's queue is shorter than its length, 100 by default | `busy`, `queue_full`, with a retry time from the queue's recent rate |
| 4 | The caller has fewer queued jobs than its cap | `busy`, `caller_queue_full` |
| 5 | A runner that could ever serve the requirements is enrolled, or the profile runs work in process | `busy`, `runner_unavailable`, naming what is missing |

Disk space before a pipeline (section 224) and record limits and quotas (section 212) are checked before `submit`, by the store and by policy.

## Queues and dispatch

- **Order.** Within a queue: priority first, interactive before batch; then `available_at`; then round robin across callers, so one caller's hundred jobs do not starve another's one. A caller's share is a setting.
- **Matching.** A job goes to a runner that serves its target, environment, one of its modes, its operating system, and its identity, and has a free slot for that mode. A job that needs a live session goes to the session's home runner. If that runner is gone and the session is movable, any matching runner may take it and the keeper restores the session there. A session bound to a device waits for its device.
- **Per-session parallelism.** Leased jobs sharing a session key are counted against `session_limits`. A job over the limit stays pending; if it waits past its limit, its run is told `busy`, `session_in_use`.
- **Exclusive resources.** At most one leased job holds each `ResourceKey`. This is how desktop work on one application, and anything else that changes a person's screen or holds a device link, is serialized without the scheduler knowing what a screen is.
- **Waiting is bounded.** A job that no runner can take within its queue limit ends its run with `busy`, `runner_unavailable`, saying what is missing.

## Leases

| Setting | Default | Held how |
| --- | --- | --- |
| Lease length | 30 s | Granted as a duration; the runner measures it on its steady clock from when it sent the request |
| Heartbeat | Every 10 s | Each renews the lease and returns any directive |
| Declared lost | 5 s after the scheduler's own count runs out | By `sweep` |
| Quarantine | 60 s, or the job's `max_step` if greater | Before anyone touches the run's records again |

Leasing a job increments the run's lease number and writes it to the job in the same transaction, so the ledger can refuse anything older. A lease ends in one of three ways: `finish`; `drain` followed by `finish`; or loss. On loss, `sweep` does section 223's steps in one transaction: the job becomes `lost`; the run stops with `possible_commit`, `runner_lost`; every lock the run held becomes `quarantined`; a `reconcile` job is created with `available_at` at the end of the quarantine. When that reconcile job is leased, the quarantined locks pass to it under its new lease number. A `verify_after_loss` job follows later. A late result from the lost runner is refused for state and stored by the store as late evidence.

## Record locks

The lock table and its rules belong to `a2m-ledger` ([section 208](../../spec/2-kernel/208-runs-the-ledger-and-resume.md), rules 4, 5, and 7): an intent checks the lock under the run's lease number in the same transaction, the ledger releases a lock when its record reaches `verified` or `unchanged`, and it quarantines a lock whose record is in doubt. The scheduler keeps only the waiting.

- A run that wants a held lock waits in order, behind earlier waiters, up to `wait`, bounded by its lease, and then gets `busy`, `record_locked` with a retry time.
- The waiting is the same mechanism as for `exclusive` resource keys; a record key is one more kind of key, whose holder the ledger and not the scheduler records.
- On a lost lease, `sweep` asks the ledger, in the same transaction that marks the job lost, to quarantine every lock the run holds. When the reconcile job is leased, the ledger hands those locks to it under its new lease number.
- A run that stops or is cancelled tells the scheduler whether a record is in doubt (`JobEnd::Stopped::in_doubt`); the ledger has already quarantined or released its locks. The scheduler creates the reconcile job.
- No lock is ever released by a timer.

## Rate budgets

Each surface has a token bucket: its declared limit from the target's files, or a conservative default. A job asks for a grant, such as 20 requests over 5 seconds, and spends it locally. When the bucket is empty the answer is the time at which a grant will be possible; the job waits on it if that falls inside its lease, and otherwise the run stops `busy` with that time. Rate budgets protect the application, so a grant is never larger than what the bucket holds, and unspent requests in a grant are not returned.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-actions` | `submit`, `cancel`; `claim`, `heartbeat`, `finish` from its in-process runner loop; `wait_lock`, `lock_freed`, `rate` inside its `Control`; `sweep` and `next_due` from one background task |
| The control plane's side of the runner link, in a role crate | `hello`, `claim`, `heartbeat`, `finish`, `wait_lock`, `rate`, `drain`, answering a runner's calls and `need`s |

It calls: `a2m-ledger` for every read and write of its tables, inside its transactions; `a2m-telemetry` for queue length, wait time, and lease metrics.

## State

| State | Where |
| --- | --- |
| `jobs`, `runners`; the `lease_epoch`, `dedupe_key`, and state columns of `runs` | The store, through `a2m-ledger`; the scheduler owns their meaning. `record_locks` is the ledger's |
| A `session_key` column on `jobs`, for counting per-session parallelism | Proposed; not yet in the ledger schema |
| Rate buckets | Memory, on a single control plane. A table is needed for the cluster profile; until then a second replica must not grant from the same bucket |
| Wakers for waiting claims and locks | Memory |

On SQLite a claim is an immediate transaction; on Postgres an atomic update that selects with `FOR UPDATE SKIP LOCKED`. The same tests run on both.

## Failures

| Class | Code | When |
| --- | --- | --- |
| `busy` | `queue_full`, `caller_queue_full` | A bounded queue is full |
| `busy` | `record_locked`, `session_in_use` | A wait for a lock or a session slot ran out |
| `busy` | `runner_unavailable`, `engine_degraded` | No runner can serve the job; the kernel cannot take writes |
| `policy` | `lease_superseded` | A call carries an older lease number than the job's |
| `possible_commit` | `runner_lost` | Written on a run whose lease was lost |
| `input` | `schema_invalid` | A submission is malformed |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| At most one runner holds a valid lease on a job | A test that claims from many tasks at once, on both databases |
| A lease number only increases, and an older one is refused (ledger rule 4) | Unit test |
| At most one unfinished run per dedupe key (ledger rule 6) | Racing identical submissions |
| Waiters for a lock are served in order, and none waits past its lease | Racing lock requests against a ledger stub |
| A lost lease's locks are quarantined in the same transaction that marks the job lost | Slice 3.7: a run killed with a record in `acted`; a second run on that record waits until reconcile |
| After a lost lease, no other job touches the run's records before the quarantine ends | Slice 7.2: a runner frozen past its lease |
| A full queue answers at once and never grows | A test that submits past the bound |
| An interactive job is dispatched before a batch job queued earlier | Unit test |
| One caller cannot starve another | A test with a flood from one caller and one job from another |
| No fixed sleeps | Every test runs on a controlled `Clock`; a lint forbids sleeping in the crate |
| The scheduler names no mode and does not depend on the engine | The audit's layering rule; a source test |

## Build order

1. `submit` with health, dedupe, and the queue bound; one in-process queue; `claim`, `heartbeat`, `finish` for the in-process runner loop. With slice 3.7.
2. Waiting for locks, behind the ledger's lock calls. With slice 3.7.
3. Lost leases, quarantine, and reconcile jobs, on a controlled clock. With slice 3.7, as its fault test.
4. Per-session parallelism, exclusive resources, priority, and caller shares. With slice 3.9, the first pipeline of many records.
5. Rate budgets. With the first binding that declares a limit, in Phase 4.
6. Runners over the link: `hello`, matching by capability and home runner, `drain`. Slices 7.1 and 7.2.
7. Shared rate buckets and several control planes. Slice 9.4.

## Open points

- The per-caller share and cap defaults, and whether a caller's share is a policy matter or a setting.
- Whether a pinned mode in a call narrows `modes` at submission, so that a runner without that mode is never chosen.
- Whether long polling is fast enough for interactive reads at scale (H-07), and what replaces it behind the same calls if not.
- The size of the dispatch overhead inside the 3 ms per-call budget of section 225 once locks and the dedupe check are in it (H-21).
- Whether a `session_keepalive` job should be scheduled here at all, or run by the keeper's `KeeperWork` outside the queues.
