# `a2m-sched`

Decides when and where work may run. It admits or refuses a run, queues its jobs, hands each job to one runner under a lease, queues waiters for record locks and other exclusive resources, spends rate budgets, and keeps per-session and per-caller limits. It never carries out a step and does not know the engine exists.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 6, Engine |
| May depend on | Layers 1 to 5. Never `a2m-engine` |
| Depends on | `a2m-core`; `a2m-ledger` for the store, its transactions, the run rows, and its lock calls; `a2m-logs` for the clock trait and metrics. Outside: `tokio`, `serde`, `serde_json` |
| Roadmap packages | Phase 3 (admission, dedupe, leases, lock waiting, and quarantine with slice 3.7); Phase 7 (slices 7.1 and 7.2, the runner link and lost leases); slice 9.4 (the cluster profile) |
| Carries out | Spec sections [211](../../spec/2-kernel/211-concurrency.md), [208](../../spec/2-kernel/208-runs-the-ledger-and-resume.md) for rule 6 and waiting for locks, [223](../../spec/2-kernel/223-the-runner-protocol.md) for leases, matching, and rate budgets, [224](../../spec/2-kernel/224-when-the-engine-itself-fails.md) for bounded queues, [225](../../spec/2-kernel/225-performance.md) for the scheduling budget; the [ledger schema](../../reference/ledger-schema.md) |

## Owns

- Admission: whether a run is accepted, returned as an existing run, or refused with a next step.
- The queue: bounded for each target and environment, ordered by priority, then fairly by caller.
- Job leases: who may work on a job, until when, under which number.
- Waiting for a record lock: the queue of runs that want a lock the ledger reports as held, woken when the ledger releases it. The lock itself, its table, and its rules are `a2m-ledger`'s, because an intent must check the lock in the same transaction.
- Exclusive resources: who holds each key, and who waits for it.
- Rate budgets, granted in batches.
- Per-session parallelism and the cap on a caller's waiting jobs.
- Holds: scopes for which no job is handed out, the stop-everything switch among them.
- Detecting lost leases and queueing the reconcile jobs that follow.
- When each check of a watch is due, the gap for due times that passed with no check, and a check's admission as the lowest work ([decision 0247](../../decisions/0247-a-watch-keeps-only-digests-and-its-due-times-come-from-the-stores-clock.md)).

It must never: call the engine, a driver, or the session keeper; read a compiled target; decide policy; change a record row of the ledger; let a job run twice at once; take, release, or quarantine a record lock itself.

## Interface

As built ([decision 0093](../../decisions/0093-the-schedulers-interface-as-building-it-settled-it.md)).

```rust
#[derive(Clone)]
pub struct Scheduler { /* the store, its clock, the settings, and who waits */ }

impl Scheduler {
    pub fn new(store: Store, settings: SchedSettings, logs_and_traces: Option<LogsAndTraces>) -> Self;   // time is the store's clock

    // Admission. Called by `a2m-actions` after policy has allowed the call.
    pub async fn submit(&self, s: Submission) -> Result<Admitted, Failure>;
    pub async fn enqueue(&self, e: Enqueue) -> Result<JobId, Failure>;      // a further job for a run that exists: execute after approval, resume
    pub async fn resume(&self, e: Enqueue) -> Result<Resumed, Failure>;     // the same, and a reconcile found by name: Queued(JobId) | Reconciling { job, leased, from, quarantine_left }
    pub async fn cancel(&self, run: RunId) -> Result<CancelOutcome, Failure>;
    pub async fn wake_reconcile(&self, run: RunId, at: Timestamp) -> Result<bool, Failure>;  // a settle time: bring a waiting reconcile forward

    // Runners. Called by the in-process runner loop in `a2m-actions`, or by the control plane's side of the runner link.
    pub async fn hello(&self, offer: RunnerOffer) -> Result<RunnerTerms, Failure>;    // heartbeat interval, lease length, protocol
    pub async fn claim(&self, runner: RunnerId, free: Slots, wait: Duration) -> Result<Vec<JobLease>, Failure>;
    pub async fn heartbeat(&self, lease: &LeaseRef) -> Result<Renewal, Failure>;      // time remaining and a directive
    pub async fn finish(&self, lease: &LeaseRef, end: JobEnd) -> Result<(), Failure>;
    pub async fn drain(&self, runner: RunnerId) -> Result<(), Failure>;
    pub async fn bye(&self, runner: RunnerId) -> Result<(), Failure>;

    // What a running job asks for, through the engine's `Control`.
    pub async fn acquire(&self, lease: &LeaseRef, keys: &[ResourceKey], wait: Duration) -> Result<(), Failure>;
    pub async fn release(&self, lease: &LeaseRef, keys: &[ResourceKey]) -> Result<(), Failure>;
    /// Taken before asking the ledger for a record lock, so a release in between is not missed.
    pub fn lock_ticket(&self, key: &ResourceKey) -> LockTicket;
    /// The ledger refused a lock as held: wait in order for it, up to `wait` and the lease, then `record_locked`.
    pub async fn wait_lock(&self, lease: &LeaseRef, ticket: LockTicket, wait: Duration) -> Result<(), Failure>;
    /// The ledger released a lock: wake the first waiter.
    pub fn lock_freed(&self, key: &ResourceKey);
    pub async fn rate(&self, lease: &LeaseRef, budgets: &[BudgetKey], want: u32) -> Result<RateGrant, Failure>;
    pub async fn set_budget(&self, key: BudgetKey, limit: RateLimit) -> Result<(), Failure>;

    // Holds. Called by `a2m-actions` when a kill switch is set or released.
    pub async fn hold(&self, scope: HoldScope) -> Result<(), Failure>;
    pub async fn release_hold(&self, scope: HoldScope) -> Result<bool, Failure>;
    pub async fn holds(&self) -> Result<Vec<HoldScope>, Failure>;

    // Time. Called by one background task in `a2m-actions`.
    pub async fn sweep(&self) -> Result<SweepReport, Failure>;            // lost leases, queue limits, waits that ran out
    pub async fn runner_ended(&self, runner: RunnerId) -> Result<SweepReport, Failure>; // every lease the runner holds, lost at once
    pub async fn next_due(&self) -> Result<Option<Timestamp>, Failure>;   // the earliest deadline the scheduler holds
    pub fn due_watch(&self) -> DueWatch;                                  // tells that task to read `next_due` again

    pub async fn job(&self, job: JobId) -> Result<JobRow, Failure>;
    pub fn settings(&self) -> &SchedSettings;

    // Watches (decisions 0247, 0262). The rows are the ledger's; the times are the scheduler's.
    pub async fn ask_watch(&self, new: NewWatch) -> Result<WatchRow, Failure>;        // the floor, the longest, the most per person
    pub async fn confirm_watch(&self, watch: WatchId, by: PrincipalId, current: Digest, limit: RateLimit)
        -> Result<WatchRow, Failure>;   // with its phase; the surface's share checked in the same transaction
    pub async fn due_watches(&self) -> Result<DueWatches, Failure>;   // ends every watch past its end; records gaps
}
pub struct DueWatches { pub checks: Vec<DueCheck>, pub cancelled: Vec<RunId> }   // runs of checks an end closed, cancelled
                                                                                  // in the transaction that ended the watch
impl Scheduler {
    pub async fn next_watch_due(&self) -> Result<Option<Timestamp>, Failure>;
    pub async fn submit_check(&self, check: &DueCheck, s: Submission) -> Result<CheckAdmission, Failure>;  // batch; Started | Deferred
}
pub struct WatchLimits { pub shortest_interval, pub longest, pub most_per_person, pub jitter_most,
                         pub budget_share_percent, pub reserve_percent }   // 60 s (never below), 7 days, 20, 5 min, 10, 50
impl WatchLimits { pub fn check_new(..); pub fn phase(&self, WatchId, Duration) -> Duration; pub fn fits_budget(..) }
pub fn due_at(confirmed_at: Timestamp, phase: Duration, interval: Duration, index: u64) -> Timestamp;
pub fn plan(watch: &WatchRow, now: Timestamp) -> Option<DuePlan>;   // Waiting | Due | Missed { gap, then } | EndReached

pub struct Submission {
    pub run: NewRun,                       // the run row; its caller, tool version, and input hash are the dedupe key, which the ledger computes
    pub job: JobRequirements,
    pub priority: Priority,                // interactive or batch
    pub health: HealthState,               // the kernel's, at the moment of submission: Ready, Degraded { cause }, Stopped { cause }
    pub queue_wait: Option<Duration>,      // how long the job may wait for a runner; the setting when absent
    pub budgets: Vec<BudgetKey>,           // a submission spends one request of each
}
pub struct Enqueue { pub run: RunId, pub job: JobRequirements, pub priority: Priority, pub health: HealthState, pub queue_wait: Option<Duration> }
pub enum Admitted { New { run: RunId, job: JobId }, Existing { run: RunId } }
pub enum CancelOutcome { Cancelled, Requested, AlreadyFinal(RunState) }

pub struct JobRequirements {
    pub kind: JobKind,                     // preview, execute, try, discover, reconcile, verify_after_loss, canary, session_keepalive
    pub target: TargetName, pub environment: Environment,
    pub modes: Vec<Mode>,                  // any of these; compared by name only
    pub os: Option<OsName>,
    pub acts_as: Option<PrincipalId>,      // a person's own device takes only jobs that act as that person
    pub session: Option<SessionNeed>,      // the session, its home runner, and whether it is movable, from the keeper's records
    pub session_limits: SessionLimits,     // reads and writes at once; the lowest across the operation's candidate bindings
    pub exclusive: Vec<ResourceKey>,       // at most one leased job per key: a screen, a device link, a file
    pub max_step: Duration,                // the longest step time limit in the tool; sets the quarantine
    pub writes: bool,
}

pub struct RunnerOffer { pub runner: RunnerId, pub workspace: WorkspaceId, pub targets: Vec<TargetName>, pub modes: Vec<Mode>, pub os: Option<OsName>, pub owner: Option<PrincipalId>, pub protocol: u32 }
pub struct RunnerTerms { pub heartbeat: Duration, pub lease: Duration, pub protocol: u32 }
pub struct Slots { pub total: u32, pub per_mode: BTreeMap<Mode, u32>, pub keeper: u32 } // keeper: slots only a session_keepalive job takes, before an ordinary one (decision 0353)

pub struct JobLease {
    pub job: JobId, pub run: Option<RunId>, pub epoch: Epoch,    // only ever increases, per run
    pub length: Duration,                  // a duration, never a clock time
    pub mode: Option<Mode>,                // the mode whose slot the job took
    pub attempt: u32,
    pub requirements: JobRequirements,
}
impl JobLease { pub fn lease_ref(&self) -> LeaseRef; pub fn fence(&self) -> Option<Fence>; }   // the fence is what the ledger checks
pub struct LeaseRef { pub job: JobId, pub epoch: Epoch }
pub struct Renewal { pub remaining: Duration, pub directive: Directive }
pub enum Directive { Continue, Cancel, Drain, Pause }
pub enum JobEnd { Done, Rests, Stopped { class: FailureClass, code: FailureCode, in_doubt: bool }, LeaseEnding }

pub struct ResourceKey(/* `kind:name`, ordered by its text */);    // ::new(kind, name), ::record(workspace, target, environment, key_hash), ::session(id),
                                                                    // ::held_alone(kind, workspace, target, environment, surface): what one write holds alone on a surface
pub struct BudgetKey(/* `kind:name` */);                            // ::target, ::surface, ::watch_share, ::session, ::caller, ::new
pub struct RateLimit { pub capacity: u32, pub refill: u32, pub per: Duration }
pub struct RateGrant { pub requests: u32, pub over: Duration }
pub enum HoldScope { Everything, Workspace(WorkspaceId), Target(WorkspaceId, TargetName), Caller(WorkspaceId, PrincipalId) }
pub struct SweepReport { pub lost: Vec<JobId>, pub reconcile: Vec<JobId>, pub expired: Vec<JobId>, pub waits_ended: usize }
pub struct SchedSettings { queue_length, caller_queue_cap, queue_wait, lease_length, heartbeat_interval, lost_after, quarantine, default_rate, reconcile_wait, reconcile_wait_ceiling, watch: WatchLimits }
```

Contracts the signatures do not show:

- **Every call from a runner carries its lease number,** and a call whose number is not the job's current one, or whose lease has run out on the scheduler's count, is refused with `policy`, `lease_superseded`. This is the scheduler's half of ledger rule 4; the ledger's own check of the run's lease number is the other half.
- **`submit` is one transaction:** the health check, the holds, the run row with its duplicate check, the queue bound, the caller's cap, the runner check, the budgets, and the job row. A refusal takes all of it back. Two identical calls get one run (ledger rule 6, held by the ledger's `open_run` and the store's unique index on `dedupe_key` among unfinished runs).
- **`claim` is one transaction, and waits on a named condition.** Granting the lease, raising the run's lease number through the ledger's `new_epoch`, and taking the job's exclusive resources commit together. A claim that finds nothing waits for work to arrive, woken by an in-process signal, up to `wait` on the store's clock. Nothing in the crate sleeps: a wait for a moment ends when `sweep` finds that the moment has come.
- **Time is the store's clock, except a lease's.** Every deadline the scheduler keeps is a moment of that clock, written to the store, so a restart finds it again, except the end of a lease, which is a moment of the scheduler's count of lease time: the time of day when the scheduler was made, carried forward by the steady clock alone, so that a jump of the machine's clock neither loses a lease nor keeps one [decision 0360](../../decisions/0360-a-lease-is-measured-on-the-steady-clock-and-another-persons-run-is-not-found.md). A wait that ends with a lease, and the moment the sweeping task is told, are turned into the store's clock when they are set. After a restart the count starts again from the time of day, so a lease written after a jump forward may be declared lost early, and is reconciled. A lease is still handed to the runner as a duration.
- **Mode names are data.** The scheduler matches a job's modes against a runner's by string equality and holds no list of modes. Limits that differ by mode, such as one write at a time on an interface binding, reach it as numbers in `JobRequirements`, filled by `a2m-actions` from the registry's mode defaults and the settings.
- **The run's state is the ledger's.** The engine writes it under the lease's fence. The scheduler writes it in three cases only, each when no runner holds the run: a lost lease (`stopped`, under a new lease number), a queue limit that ran out (`stopped`), and a cancel of a run nobody holds (`cancelled`).

## Admission

In this order. The first that refuses ends it.

| # | Check | Refusal |
| --- | --- | --- |
| 1 | The kernel's health: `Stopped` refuses everything; `Degraded` refuses writes | `busy`, `engine_degraded`, with the cause's code |
| 2 | No hold covers the caller, the target, the workspace, or everything | `busy`, `on_hold` |
| 3 | An unfinished run with the same dedupe key exists | Not a refusal: `Admitted::Existing` with that run |
| 4 | The target's queue is shorter than its length, 100 by default | `busy`, `queue_full`, with a retry time from the queue's recent rate: a minute divided by the jobs it ended in the last minute, or the lease length when it ended none |
| 5 | The caller has fewer waiting jobs than its cap, 25 by default | `busy`, `caller_queue_full` |
| 6 | A runner that could ever serve the requirements has said `hello` | `busy`, `runner_unavailable`, naming what is missing |
| 7 | Every budget the submission names holds a request | `busy`, `rate_limited`, with the time to wait |

The loop that runs work in the same process says `hello` like any runner, so check 6 needs no special case. Disk space before a pipeline (section 224) and record limits and quotas (section 212) are checked before `submit`, by the store and by policy.

## Queues and dispatch

- **Order.** Priority first, interactive before batch. Within a priority, the caller that was served longest ago goes first, and a caller that was never served goes before all of them: round robin, kept in the store. Within one caller, the order of arrival. So one caller's hundred jobs do not starve another's one. A weighted share for a caller is not built.
- **Readiness.** A job is handed out from its `available_at`. A reconcile job after a lost lease becomes available when the quarantine ends.
- **Matching.** A job goes to a runner in its workspace that serves its target, offers one of its modes with a free slot, reports its operating system when the job names one, and, when the runner is a person's own device, only if the job acts as that person. A job that needs a live session goes to the session's home runner. If that runner has said `bye` and the session is movable, any matching runner may take it and the keeper restores the session there.
- **Per-session parallelism.** Leased jobs sharing a session are counted, reads and writes apart, against the job's `session_limits`. A job over the limit stays pending; if it waits past its queue limit, its run is told `busy`, `session_in_use`.
- **Exclusive resources.** At most one job holds each `ResourceKey`; the primary key of `resource_holds` is the rule. A job is not handed out while another holds one of its keys, and it takes all of them in the transaction that leases it. This is how desktop work on one application, and anything else that changes a person's screen or holds a device link, is serialized without the scheduler knowing what a screen is. A waiting job keeps its place for its keys: a later job that wants one of them is not handed out first.
- **What a mode holds alone.** A mode's manifest says what one write holds alone: `surface`, `file`, `screen`, or `link`. `ResourceKey::held_alone` names that thing for one surface of a target in one environment, as `kind:workspace/target/environment/surface`, so that every submitter names it by the same key ([decision 0148](../../decisions/0148-the-provider-contract-and-the-scheduler-after-their-safety-audit.md)). The scheduler compares the key and does not know what the kind means. Whoever submits a job that writes puts one key in `JobRequirements::exclusive` for each need the engine reports with `exclusive_needs`; today the submitter leaves the list empty, so nothing is held. A screen belongs to a runner's machine and not to a surface, and no key is bound to the runner at lease: two applications on one desktop are kept apart only when a key can name the runner, which is not built.
- **Waiting is bounded.** A job that no runner takes within its queue limit, five minutes by default, ends, and its run stops as `busy` with the reason: `session_in_use`, `resource_in_use`, or `runner_unavailable` saying what is missing.

## Leases

| Setting | Default | Held how |
| --- | --- | --- |
| Lease length | 30 s | Granted as a duration; the runner measures it on its steady clock from when it sent the request. The scheduler keeps the moment it runs out, on its count of lease time |
| Heartbeat | Every 10 s | Each renews the lease and returns a directive: `continue`, `cancel`, `pause` when a hold covers the job, or `drain` |
| Declared lost | 5 s after the scheduler's own count runs out | By `sweep`. In those five seconds the heartbeat is refused, a result is still taken, and nobody else is given the job |
| Quarantine | 60 s, or the job's `max_step` if greater | Before anyone touches the run's records again |

Leasing a job of a run raises the run's lease number through the ledger and writes it to the job in the same transaction, so the ledger refuses anything older. A lease ends in one of three ways: `finish`; `finish` with `LeaseEnding`, which puts the job back in the queue; or loss. A `finish` with `Stopped` queues a reconcile job when the runner says a record is in doubt, and also when the ledger does: the ledger is asked for the run's records whose intent was stored and that were never read back, and the runner's `in_doubt: false` is not taken against it.

**A run that rests is not read without end** ([decision 0193](../../decisions/0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md)). Only a stop of the class `possible_commit`, `transient`, or `busy` is queued again by the scheduler on its own; a stop of any other class waits for a person, and a resume reconciles first. A reconcile queued after `n` reconciles of the same run ended in a row waits `reconcile_wait` (5 s) doubled `n - 1` times, never more than `reconcile_wait_ceiling` (15 min); the first waits nothing; after a lost lease the wait is the longer of that and the quarantine. So a paused-clock minute sees four reconciles of one run at most (at 0, 5, 15, and 35 s), and two hours some fifteen. Two named conditions wake a waiting reconcile early: `resume` (and `enqueue`, which does the same) for the run, a person's resume or execute, makes it available at once; `wake_reconcile(run, at)` brings it forward to a settle time. Neither ever brings it before the end of a lost lease's quarantine. The resume finds and wakes the reconcile in the same transaction in which it would queue its own job, so a reconcile that ends at that moment is never missed; one it finds leased is remembered, and the reconcile that one queues when it ends in doubt, or is lost, takes no growing wait (the quarantine still holds). The answer is `Resumed::Reconciling`, with when the reconcile is handed out and how long a quarantine still holds it; `Reconciling::failure()` is `busy`, `run_reconciling`, for a door. `enqueue` answers the same finding as `input`, `schema_invalid`, which its callers read as "the run has its job", with words that say when ([decision 0201](../../decisions/0201-a-resume-is-never-lost-the-browser-takes-no-proxy-and-no-product-build-speaks-a-stands-protocol.md)). The remembered wake is kept in memory: it serves the call that asked, which a restart of the engine ends too, and the next resume finds the waiting reconcile in the store.

On loss, `sweep` does section 223's steps in one transaction:

1. The job becomes `lost`.
2. The run is given a new lease number that nobody holds. From that commit on, the ledger refuses the lost runner, and the ledger sorts the run's locks: those of records in doubt are quarantined, the rest released.
3. The run stops as `possible_commit`, `runner_lost`, when it is in a state a run can stop from.
4. A `reconcile` job is queued when the job could write or a record is in doubt, with `available_at` at the end of the quarantine. What the lost job held alone passes to it, so no other job takes those resources in between.

**A runner that starts again holds nothing** ([decision 0416](../../decisions/0416-a-runner-that-starts-again-holds-nothing-and-its-earlier-leases-are-lost-at-once.md)). `runner_ended(runner)` loses every lease the runner holds at once, by the same four steps, when the process that was that runner has ended: the caller says so, and calls it only when no other process can be that runner, as the kernel does at the first start of its runner loop on one machine. The reconcile's `available_at` is then the job's longest step from the loss, the time a step the ended process sent can still be in flight; the setting's quarantine is for a runner that may only be frozen, and a process that ended cannot act again. A resume and a wake still read the quarantine with the setting, which ends later, so they never bring that reconcile earlier.

When the reconcile job is leased it gets the next lease number, and confirms the quarantined locks under it. A late result from the lost runner is refused. Storing its journal as late evidence is the ledger's `late_evidence`, called by the runner link, which is not built. The check after a loss is not a job of its own: the reconcile queued after a lost lease carries `verify_after_loss`, as does each reconcile queued after it, and when it settles the runner reads every verified record once more in the same lease ([decision 0347](../../decisions/0347-the-check-after-a-lost-runner-is-the-last-part-of-its-reconcile.md)).

## Exclusive resources during a run

A job may take further keys while it runs, with `acquire`, and give them back with `release`; the end of the job frees whatever it holds.

**The ordering rule.** Keys are ordered by their text. One call takes its keys together, all or none. A job that already holds keys, those of its requirements included, may ask only for keys that sort after every key it holds; a call that breaks the order is refused at once with `busy`, `resource_order`, and does not wait. So a job only ever waits for keys above the ones it holds, no circle of waiting jobs can form, and there is no deadlock. Those who wait are served in the order they came, and no wait outlasts its limit or the lease. A waiter whose lease has run out on the scheduler's count, and that `sweep` has not yet declared lost, is granted nothing, as `acquire` would refuse it at that moment.

## Record locks

The lock table and its rules belong to `a2m-ledger` ([section 208](../../spec/2-kernel/208-runs-the-ledger-and-resume.md), rules 4, 5, and 7): an intent checks the lock under the run's lease number in the same transaction, the ledger releases a lock when its record reaches `verified` or `unchanged`, and it quarantines a lock whose record is in doubt. The scheduler keeps only the waiting.

- The caller takes a `lock_ticket`, asks the ledger for the lock, and on `record_locked` calls `wait_lock` with the ticket. A release between the two is not missed.
- A run that wants a held lock waits behind earlier waiters, up to `wait`, bounded by its lease, and then gets `busy`, `record_locked` with a retry time.
- `Ok` from `wait_lock` means "ask the ledger again". The ledger alone grants a lock, so a run that never waited can take a freed lock before the waiter that was woken; that waiter then waits again, at the back.
- `lock_freed` wakes the first waiter of one key. When a job ends, is lost, or its run is cancelled, the ledger has sorted that run's locks, and the scheduler wakes every lock waiter to ask again.
- The ordering rule above does not reach record locks, because the ledger and not the scheduler knows who holds them. Two runs that each hold a record lock and wait for the other's are ended by their deadlines, not prevented.
- No lock is ever released by a timer.

## Rate budgets

A budget is a token bucket named by a `BudgetKey`: for a target, a surface, a session, or a caller. Its limit is set with `set_budget` from the target's files; a budget nobody set gets a conservative default, five requests and one more each second. A job asks for a grant against several budgets at once, such as 20 requests against its surface, its session, and its caller, and spends it locally. The grant is the same number from each, never more than the emptiest holds, and unspent requests are not returned. When a budget is empty, the call waits for the refill if that falls inside the lease, and otherwise answers `busy`, `rate_limited` with the time.

Buckets are kept in the store, in whole numbers, and refill by the store's clock and nothing else. A restart does not refill them. The moment a bucket is counted to never goes back: when the clock is set back, the bucket refills nothing until the clock has passed that moment again, so no stretch of time is counted twice.

## Watches

A watch is the ledger's row ([decision 0247](../../decisions/0247-a-watch-keeps-only-digests-and-its-due-times-come-from-the-stores-clock.md)); its times are the scheduler's, computed from that row and the store's clock, so a restart finds them again.

- **Due times.** Due time `n` is the moment of confirmation, plus a phase, plus `n` intervals. The phase is made from the digest of the watch's identifier, below the smaller of a tenth of the interval and `jitter_most` (five minutes): it spreads watches and is the same on every machine. The interval is whole seconds, at least `shortest_interval`, which is never below 60 seconds whatever the setting says.
- **One check at a time, and none before confirmation.** `due_watches` plans only confirmed watches; a running or paused one is passed over, and the ledger refuses a second check of a watch under way.
- **Every watch ends at its end** ([decision 0262](../../decisions/0262-watches-are-held-to-their-end-their-budget-and-their-runs-own-result.md)). Before planning, `due_watches` ends with `end_reached` every watch whose end has come that is asked, paused, or running; a confirmed one is ended by its compiled target, which records the gap before it. The run of a check it closed is cancelled in the same transaction when `Watches::run_left_to_cancel` says no other check rests on it, asked once every watch of the pass has ended, and named in `DueWatches::cancelled`; a pass the store refuses ends nothing and cancels nothing, and the next does both ([decision 0291](../../decisions/0291-a-watch-ends-when-its-condition-holds-on-a-checks-own-result.md); `a_pass_that_fails_ends_nothing_and_cancels_nothing_and_the_next_does_both`). `next_watch_due` answers the end of every watch not ended, so the task that sweeps wakes for it.
- **A missed due time is a gap.** When the latest due time that has come is past the next one not handled, the ones between are one `gap` (first, last, how many) and one check follows for the latest, marked `catch_up`. A due time is never checked late as if on time. Due times that passed while the watch was paused or held are the same gap when it goes on. A watch whose end has come records the gap of what passed before it and ends `end_reached`.
- **The lowest work.** A check is `batch`, whatever its submission said, so every interactive job is dispatched first. Its job writes nothing; the registry gives it the kind `canary`, which the engine already refuses for a tool that writes. A refusal to wait out (a hold, a full queue, a spent budget, no runner, a degraded kernel) keeps nothing. After a spent budget the watch is not tried again before that budget refills; after any other refusal, not before a lease's length; and never later than its interval (`a_check_refused_for_its_budget_waits_until_the_share_refills_and_misses_nothing`).
- **A run already open.** A check whose read, version, environment, and input match a run its person already has unfinished is begun on that run, as rule 6 gives an identical call its run: it opens no job and spends no budget, and two watches of one read and input then rest on one run, each comparing its result against its own baseline ([decision 0285](../../decisions/0285-two-watches-of-the-same-read-for-one-person-share-a-run-and-ending-one-never-cancels-a-run-another-rests-on.md)). `submit_check` tells the ledger whether the check opened its run (`CheckStarted::job` is `Some` exactly then), and the registry cancels a check's run only when a watch's check opened it and no other check under way names it ([decision 0287](../../decisions/0287-the-last-check-under-way-on-a-run-a-check-opened-cancels-it.md)). For a watch past its end the scheduler applies that rule itself, in `due_watches` (decision 0291).
- **The budget.** A check spends the same budgets as any read of its surface. Its grant, at admission and through `rate`, never takes a bucket below `reserve_percent` (half) of its capacity, so a caller's own reads always find half the burst; the scheduler knows a check's job by asking the ledger whether its run is a check under way. `confirm_watch` calls `fits_budget` inside its own transaction, against the other open watches of the same surface as the store holds them, so that all the watches on one surface are confirmed within `budget_share_percent` (a tenth) of its sustained rate and two confirmations at once cannot both take the last of it. At run time a check also spends `BudgetKey::watch_share` of its surface: a budget only checks spend, whose limit is that share of the surface budget's stored limit, or of `default_rate` when none was set, rounded down and never below one request. A read that sends more pages than it declared waits on it. Admission takes one request of it, as of every budget a submission names.
- **Waking.** `next_watch_due` answers the earliest moment `due_watches` has work; the task that sweeps waits on the earlier of it and `next_due`. `next_due` itself does not include watches, so a task that does not call `due_watches` never spins.

## Holds

`hold` puts a scope on hold: everything, a workspace, a target, or a caller. While it lasts, a submission the scope covers is refused, no job it covers is handed out, and the heartbeat of a leased job it covers answers `pause`: the runner stops at its next commit point and ends the job as `Stopped`, which leaves the run resumable. A hold does not use up a waiting job's queue limit: the time it lasted is given back on release. Holds are kept in the store.

Whether a hold may be set or released is the policy engine's question. `a2m-actions` asks it, and tells the scheduler when a kill switch changes.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-actions` | `submit`, `enqueue`, `cancel`, `hold`, `release_hold`, `set_budget`; `hello`, `claim`, `heartbeat`, `finish` from its in-process runner loop; `acquire`, `release`, `lock_ticket`, `wait_lock`, `lock_freed`, `rate` inside its `Control`; `sweep`, `next_due`, and `due_watch` from one background task |
| The control plane's side of the runner link, in a role crate | `hello`, `claim`, `heartbeat`, `finish`, `acquire`, `release`, `wait_lock`, `rate`, `drain`, `bye`, answering a runner's calls and `need`s |

It calls: `a2m-ledger` for every read and write of its tables, inside its transactions, and for the run rows (`open_run`, `new_epoch`, `set_state`, `quarantine_locks`, `run`, `in_doubt`); `a2m-logs` for the counts of admitted, refused, leased, and lost, and the time a job waited.

## State

| State | Where |
| --- | --- |
| `jobs`, `runners`, `resource_holds`, `caller_turns`, `scheduler_holds`, `budgets` | The store, through `a2m-ledger`, in one migration file the scheduler owns. The `lease_epoch`, `dedupe_key`, and state columns of `runs` are reached through the ledger's calls. `record_locks` is the ledger's |
| Who waits: claims, waits for a moment, for a resource, for a record lock | Memory. A restart ends them; the callers ask again |

A claim is one writing transaction of the store, which has one writer. Postgres is not built.

## Failures

| Class | Code | When |
| --- | --- | --- |
| `busy` | `queue_full`, `caller_queue_full` | A bounded queue is full |
| `busy` | `record_locked`, `session_in_use`, `resource_in_use` | A wait for a lock, a session slot, or an exclusive resource ran out |
| `busy` | `resource_order` | A job asked for a resource that sorts before one it holds |
| `busy` | `runner_unavailable`, `engine_degraded`, `on_hold` | No runner can serve the job; the kernel cannot take the work; a hold covers it |
| `busy` | `rate_limited` | A budget is spent and does not refill inside the lease |
| `busy` | `run_reconciling` | A resume found the run's reconcile as its one job (`Reconciling::failure()`, for a door) |
| `policy` | `lease_superseded` | A call carries another lease number than the job's, or a lease that ran out |
| `policy` | `runner_not_enrolled` | A runner asked for work before saying `hello` |
| `possible_commit` | `runner_lost` | Written on a run whose lease was lost |
| `unknown_variant` | `runner_protocol_unsupported` | A runner speaks neither the current protocol version nor the one before |
| `input` | `schema_invalid`, `not_found` | A submission is malformed; there is no such job or run |
| `input` | `watch_interval_too_short`, `watch_too_long` | A watch's interval below the floor; an end further than the longest a watch lasts |
| `busy` | `watch_too_many`, `watch_over_budget_share` | The person has the most watches they may; the watches on a surface would pass their share of its budget |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| At most one runner holds a valid lease on a job | Eight tasks claiming three jobs at once, against a store in a folder |
| A lease number only increases, and an older one is refused (ledger rule 4) | A test over the scheduler's calls, the ledger's, and a direct statement against the database |
| At most one unfinished run per dedupe key (ledger rule 6) | Two identical submissions; the store is asked how many jobs it holds |
| An expired lease is handed to nobody before the run is fenced, and the reconcile job to nobody before the quarantine ends | A runner frozen with a record in `intent`: the lock is quarantined in the transaction that marks the job lost, a second run on that record is refused the lock, and the ledger and the scheduler both refuse the thawed runner |
| Waiters for a lock are woken in order, and none waits past its lease | Two waiters and a release; a wait of ten minutes that ends with the lease |
| Two jobs that need one resource run one after the other, and a deadline frees the waiter | Three jobs on one key in the queue; three jobs on one key during their runs |
| No deadlock on resources taken in opposite order | Two jobs, two keys: the one that asks against the order is refused at once and the other is served |
| A full queue answers at once and never grows | Submissions past the queue's length and the caller's cap; the store is asked what it holds |
| An interactive job is dispatched before a batch job queued earlier, and one caller cannot starve another | A batch job, a flood from one caller, and one job from another |
| Budgets refill by the clock only, and a restart does not refill them | Sweeps and calls with the clock standing still; a restart |
| A jump of the machine's clock, forward or back, moves no lease | `a_jump_in_the_machines_clock_does_not_move_leases`: a day forward, then two back, with no time passing |
| A clock that went back refills no budget twice | A bucket counted to an hour from now, as a clock set back leaves it: the moment stays, and nothing refills before it |
| A stopped job is reconciled when the ledger holds a record in doubt, whatever its runner reported | A job that stored an intent and reports `in_doubt: false`; the store is asked for the reconcile job |
| A run that rests is not read without end: a person's stop is never queued again on its own, and a reconcile queued again waits longer each time | For each failure class, a minute and two hours of a paused clock with a reconcile that never settles; a resume, a settle time, and a quarantine; a resume whose transaction the store's one writer runs while the reconcile it found is leased, before that reconcile's end, forced by a transaction of the test's own that holds the writer (`a_resume_that_races_the_end_of_the_reconcile_it_finds_is_never_lost`) |
| A waiter whose lease has run out is granted nothing | A key freed at the moment the waiter's lease ended, before any sweep; the store is asked who holds it |
| What one write holds alone has one key, and two jobs that name it run one after the other | Three jobs on two surfaces with three free slots |
| Everything queued or leased is found again after a restart | The store closed and opened again: the queue, the lease, the runner, the hold; a job whose runner is gone is recovered by the deadline rule |
| A watch checks nothing before its person confirms it, never two at once, is admitted behind interactive work, leaves half of each budget, and turns missed due times into one gap and one check | `tests/watch.rs`, on a clock that moves only when told, with a restart |
| No fixed sleeps | Every test runs on a clock that moves only when told; a source test forbids sleeping in the crate |
| The scheduler names no mode and does not depend on the engine | The audit's layering rule; a source test |

## Build order

1. Done: `submit` with health, dedupe, the queue bound, and the caller's cap; the queue; `claim`, `heartbeat`, `finish`. With slice 3.7.
2. Done: waiting for locks, behind the ledger's lock calls. With slice 3.7.
3. Done: lost leases, quarantine, and reconcile jobs, on a controlled clock. With slice 3.7, as its fault test. The check after a loss is the last part of that reconcile, in its lease ([decision 0347](../../decisions/0347-the-check-after-a-lost-runner-is-the-last-part-of-its-reconcile.md)).
4. Done: per-session parallelism, exclusive resources, priority, and round robin between callers. Not built: a weighted share for a caller.
5. Done: rate budgets, kept in the store.
6. Done in part: `hello`, matching by capability and home runner, `drain`, `bye`, `cancel`, holds. Not built: enrollment and revoking a runner, which belong to the runner link of slices 7.1 and 7.2; marking a runner offline when it falls silent.
7. Done ([decision 0247](../../decisions/0247-a-watch-keeps-only-digests-and-its-due-times-come-from-the-stores-clock.md)): a watch's due times, gaps, and admission; the reserve in a budget's grant. Done since ([decision 0262](../../decisions/0262-watches-are-held-to-their-end-their-budget-and-their-runs-own-result.md)): every watch past its end ended, whatever its state; the share checked in the confirmation's transaction; the share of a surface each check spends at run time. Not built: a priority below `batch` and a job kind of a check's own, each a rebuilt `jobs` table; calendar forms of a schedule, which wait for named time zones; signals that bring a due time forward.
7a. Done ([decision 0291](../../decisions/0291-a-watch-ends-when-its-condition-holds-on-a-checks-own-result.md)): a watch past its end is ended and its check's run cancelled in one transaction, `DueWatches::cancelled` replacing `DueWatches::cancel`; `Scheduler::after_cancels`, what follows a cancel made in another call's transaction. Test: `a_pass_that_fails_ends_nothing_and_cancels_nothing_and_the_next_does_both` in `tests/watch.rs`.
8. Not built: several control planes over one store, a claim on Postgres, and waking a waiter in another process. Slice 9.4. On one store with one writer the same tables serve.

## Open points

- The per-caller share and cap defaults, and whether a caller's share is a policy matter or a setting. The cap is a setting today.
- Whether a pinned mode in a call narrows `modes` at submission, so that a runner without that mode is never chosen.
- Whether long polling is fast enough for interactive reads at scale (H-07), and what replaces it behind the same calls if not.
- The size of the dispatch overhead inside the 3 ms per-call budget of section 225 once locks and the dedupe check are in it (H-21). Not measured: a claim is one synced commit.
- Whether a `session_keepalive` job should be scheduled here at all, or run by the keeper's `KeeperWork` outside the queues.
- A waiter woken for a record lock can lose it to a run that never waited. Closing this needs the ledger to grant a lock to a named waiter.
- A run that ends final with a record in doubt keeps its quarantined lock, and a reconcile job is queued only for a run that is not final.
- Only a `Stopped` end asks the ledger for records in doubt. A `Done` or `Rests` end of a run that holds one is a defect of the engine and is not caught here.
- A key for a screen cannot name the runner, because the runner is chosen at lease; see "What a mode holds alone".
