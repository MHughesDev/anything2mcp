# `a2m-engine`

The engine loop: one job in, one outcome out. It carries out a compiled plan through the eight parts of every operation call, chooses among verified bindings, normalizes and compares values, reconciles before it writes on resume, and runs drafts and discovery in a sandbox where a write is impossible. Everything it needs from outside its own loop it asks for through `Control`.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 6, Engine |
| May depend on | Layers 1 to 5, except `a2m-files` and `a2m-policy`, which the audit refuses. Never `a2m-sched`, never a pack |
| Depends on | `a2m-core`, including `Plan` and `Step` as data and the check of a plan's hash; `a2m-expr` to evaluate compiled expressions and field-type normalizers; `a2m-telemetry` for spans and the step trace; `a2m-ledger` for the types `IntentToken` and `RecordLock` only, never to open a store; `a2m-auth` for `SessionLease`; `a2m-sources` for `SourceReader`; `a2m-provider` for the registry, `DriverHandle`, and `SandboxPort`. Outside: `tokio` |
| Roadmap packages | Phase 1 (slices 1.1 to 1.3, reads), Phase 2 (slice 2.4, sessions), Phase 3 (slices 3.3 to 3.10, writes, preview, resume, identity, pipelines, faults), package E1 (the sandbox for discovery), Phase 4 (shadow) |
| Carries out | Spec sections [207](../../spec/2-kernel/207-the-engine-loop.md), [201](../../spec/2-kernel/201-archetypes-and-modes.md) for choosing, [208](../../spec/2-kernel/208-runs-the-ledger-and-resume.md) for the write path and resume, [209](../../spec/2-kernel/209-record-identity-and-dependent-records.md), [224](../../spec/2-kernel/224-when-the-engine-itself-fails.md), [201](../../spec/2-kernel/201-archetypes-and-modes.md), [401](../../spec/4-building-tools/401-overlaying-tools-on-an-existing-system.md) and [218](../../spec/2-kernel/218-discovery-and-findings.md) for the sandbox; the [diffs](../../reference/steps-snapshots-and-diffs.md#diffs) |

## Owns

- The loop, its order, and which part may call what.
- Choosing a binding, and recording why.
- Retrying, which happens here and nowhere else, within the limits of the failure class.
- Evaluating each step's inputs before a driver sees them.
- Normalization and comparison of values, the preview, the diff and its hash, and the freshness check.
- Reconcile: deciding from a read whether a doubtful write landed.
- The sandbox for drafts and for discovery.
- The `Control` trait.

It must never: name a mode, a driver, or a pack; branch on a mode's name; call a driver except through `DriverHandle`; open the store, call policy, or read a target file, all of which it reaches only through `Control` or not at all; renew a session; approve; sleep for a fixed time.

## Interface

```rust
pub struct Engine { /* what it was built from, and the plans it has loaded */ }

pub struct EngineDeps {
    pub registry: Arc<Registry>,               // from `a2m-provider`
    pub redact: Arc<dyn Redact>,               // from `a2m-core`, implemented by `a2m-vault`
    pub keyed: Arc<dyn KeyedDigest>,           // the vault's keyed digest, for the trace
    pub clock: Arc<dyn Clock>,                 // from `a2m-telemetry`: durations, the freshness window, a run's time limit
    pub settings: EngineSettings,              // retries, the freshness window, time limits, source limits
}

impl Engine {
    pub fn new(deps: EngineDeps) -> Self;
    /// Carry out one job under one lease. Returns when the job ends, stops, or the lease is about to end.
    pub async fn run_job(&self, job: &JobSpec, control: &dyn Control) -> JobOutcome;
    /// A discovery job: the registry's discoverers for the job's surface, each handed the sandbox as its `SandboxPort`.
    pub async fn discover(&self, job: &JobSpec, control: &dyn Control) -> JobOutcome;
    /// What a job that carries out a writing tool must hold alone: for each binding that may write,
    /// what its mode's manifest says one write holds alone, on the binding's surface. Data for the job's requirements.
    pub fn exclusive_needs(&self, plan: &Plan, tool: &OperationName) -> Vec<ExclusiveNeed>;
}
pub struct ExclusiveNeed { pub kind: Exclusive, pub surface: SurfaceName }   // `Exclusive` is the manifest's word, from `a2m-provider`

/// Everything about the job that is data. The runner fills it when it claims the job.
pub struct JobSpec {
    pub run: RunId,
    pub workspace: WorkspaceId,
    pub attempt: u32,                          // which attempt of the run, from 1
    pub epoch: Epoch,                          // the lease number; intent tokens carry the same
    pub kind: JobKind,
    pub plan: Digest,                          // the plan's hash; the plan is fetched through Control and checked
    pub tool: ToolRef,                         // operation or pipeline in the plan; for Discover, the surface
    pub environment: Option<Environment>,      // none for the target's default
    pub identity: Option<Identity>,            // who the system sees, for the diff
    pub input: Values,                         // the stored input, fetched once by the runner
    pub source: Option<Arc<Vec<u8>>>,          // the bytes of the source input, read once and stored by the caller
    pub snapshot: Option<RunSnapshot>,         // on resume: each record's state, outputs, assigned key, values before
    pub conditions: Conditions,                // at claim: the kernel's health, drifted bindings, edge blocks, session states
    pub pin: Option<Mode>,                     // a caller or setting pinned one mode; compared by name only
    pub binding: Option<BindingPin>,           // a test or trial runs this one binding: place, mode, digest
    pub parallelism: Parallelism,              // records at once inside this job, granted by the scheduler
    pub lease: LeaseClock,                     // the lease's remaining time on a steady clock, and the latest directive
    pub capture: CaptureLevel,                 // how much evidence the run keeps
}
pub enum JobKind { Preview, Execute, Try, Reconcile, VerifyAfterLoss, Canary, Discover }
pub enum JobOutcome {
    Finished { state: RunState, failure: Option<Failure> },  // completed, failed, rejected, cancelled
    Rests { state: RunState, failure: Option<Failure> },     // needs_approval, needs_input, needs_login
    Stopped(Failure),                          // resumable
    LeaseEnding,                               // stopped acting before the lease ran out; nothing was left half sent
    Settled { remaining: u32 },                // a Reconcile or VerifyAfterLoss job ended; the run's state is not changed
    Left(Failure),                             // a VerifyAfterLoss or Discover job failed; the run's state is not changed
}
```

The engine's caller applies the outcome: it moves the run to the state the outcome names. The engine itself moves a run only to `validating` and `running`, through `Control`. A new run is moved to `validating` before its plan is fetched, so that a job that names a tool or an environment the plan does not hold ends `rejected`, which the ledger refuses from `queued`.

### `Control`

Everything the engine needs from the control plane while it runs a job, and nothing more. In one process `a2m-actions` implements it by calling the scheduler, the keeper, policy, and the store directly; on a runner `a2m-runner` implements it as `need` and `events` calls over the link. Every call carries the job's lease number, attached by the implementation, so the engine can neither forge nor forget it. The same test suite runs against both implementations.

```rust
pub trait Control: Send + Sync {
    /// The plan by its hash. The engine checks the hash again before using it.
    fn plan(&self, hash: &Digest) -> Result<Arc<Plan>, Failure>;
    /// The policy decision at guard, made and recorded by `a2m-policy` behind the implementation.
    fn decide(&self, question: &GuardQuestion) -> Result<(), Failure>;
    /// A session lease. The keeper renews behind it if it must; a refusal is of class `session_expired`.
    fn session_lease(&self, want: &SessionWant) -> Result<SessionLease, Failure>;
    /// A driver's sign that the session has ended, relayed to the keeper. The engine never renews.
    fn report_expiry(&self, lease: &SessionLease, sign: ExpirySign);
    /// A rate budget of up to `want` requests, waiting through the scheduler up to its limit.
    fn rate(&self, surface: &SurfaceName, want: u32) -> Result<u32, Failure>;
    /// Stores what the preview found: the commit points and the diff.
    fn store_preview(&self, preview: &Preview) -> Result<(), Failure>;
    /// The approval check: uses the approval bound to `diff` and moves the run to `running`,
    /// in one transaction. `Waits` when no approval has been given yet.
    fn approval(&self, diff: &Digest) -> Result<Clearance, Failure>;
    /// A record lock, from the ledger, waiting through the scheduler up to its limit; then `record_locked`.
    fn lock(&self, record: RecordRef) -> Result<LockHeld, Failure>;
    /// Record intents and get their tokens. Durable before it returns.
    fn intend(&self, intents: Vec<RecordIntent>) -> Result<Vec<IntentToken>, Failure>;
    /// Marks a record `acted`: its write was sent. Uses the token up.
    fn acted(&self, token: IntentToken, acted: Acted) -> Result<ActedToken, Failure>;
    /// Marks a record `verified`, with what was read back and its evidence.
    fn verified(&self, token: &ActedToken, read_back: ReadBack) -> Result<(), Failure>;
    /// Marks a record `failed` with the failure that stopped it.
    fn failed(&self, record: RecordRef, why: &Failure, evidence: Option<BlobRef>) -> Result<(), Failure>;
    /// Settles a record in doubt with what reading it showed.
    fn settle(&self, record: RecordRef, seen: Reconciled) -> Result<Settled, Failure>;
    /// Events: ledger transitions, outputs known before a commit point, the result, the step trace,
    /// binding health, what a discoverer found. Each says whether it must be acknowledged.
    fn send(&self, event: EngineEvent) -> Result<(), Failure>;
    /// Evidence, already redacted.
    fn evidence(&self, blob: Evidence) -> Result<BlobRef, Failure>;
    /// A step's capture, kept as evidence tied to the run, the record, and the step (pass, phase, position).
    /// Default: `busy`, `capture_not_kept`; the capture is dropped and the run goes on (decision 0162).
    fn keep_capture(&self, capture: KeptCapture) -> Result<BlobRef, Failure> { /* refuses */ }
    /// Ends a record whose only evidence is the system's acknowledgement: it stays `acted`, never verified,
    /// and its lock is released. Default: `busy`, `acknowledgement_not_kept`; the record stays in doubt (decision 0161).
    fn acknowledged(&self, token: &ActedToken, evidence: Option<BlobRef>) -> Result<(), Failure> { /* refuses */ }
    /// The checked project folder of the request that started the run; `None` by default.
    fn project_folder(&self) -> Option<PathBuf> { None }
    /// The files of the project's `sources` folder the call that started a discovery hands it: each name a
    /// discoverer declares, to the path the call gave (`sources/...`). Empty by default (decision 0263).
    fn discovery_files(&self) -> BTreeMap<String, String> { BTreeMap::new() }
    /// Whether the control plane keeps shadow comparisons; only then does a binding run in shadow. `false` by default.
    fn keeps_shadow(&self) -> bool { false }
    /// A binding in shadow's rate budget: all of `want` at once or nothing, never waiting, never below the
    /// share left for callers' own reads (decisions 0247, 0251). Default: `false`, so it is not compared.
    fn shadow_rate(&self, surface: &SurfaceName, want: u32) -> Result<bool, Failure> { Ok(false) }
    /// Keeps how a binding in shadow compared, in the ledger, under the job's run and lease (decision 0207).
    /// Called only after the call's result was sent, within the job's shadow budget (decision 0251).
    /// Default: `busy`, `shadow_not_kept`. A failure is dropped and counted: a shadow never changes the call.
    fn shadow(&self, compared: ShadowCompared) -> Result<(), Failure> { /* refuses */ }
}
pub struct ShadowCompared { pub operation: OperationName, pub shadow: BindingPin, pub verified: BindingPin,
                            pub writes: bool, pub outcome: ShadowOutcome,     // `ShadowOutcome` from `a2m-ledger`
                            pub input: Values, pub requests: u32 }           // the ledger keeps a keyed digest of `input`
// What the registry asks before a promotion (decision 0251):
pub fn shadow_evidence_environments(plan: &Plan) -> Vec<Environment>;   // production's, or all where none
pub fn verified_binding(plan: &Plan, operation: &OperationPlan) -> Option<BindingPin>; // the first verified
impl Engine { pub fn shadow_comparisons_lost(&self) -> u64; }
```

Every method is asynchronous. The block above leaves that out to stay readable: in the code each takes `&'a self` and returns a `BoxFuture<'a, ...>` of the type shown, so the trait is a trait object and needs no macro, and an implementation writes `Box::pin(async move { ... })`.

**A call that never answers.** The engine gives any call to `Control` up after `EngineSettings::control_timeout` (two minutes unless set) and goes on as it would after any failure of that call, with `busy`, `control_unanswered`: an intent that was not acknowledged sends nothing, and a write whose `acted` mark was not acknowledged is `possible_commit`. The future of the call is dropped, so an implementation must make what a call had begun stand or fall as one transaction. An implementation that waits, for a lock, a lease, or a rate budget, keeps its own shorter limit.

`verified` borrows the proof that the record is `acted`, and the store runs its transaction on a thread of its own, which takes only what it can own. An implementation over the store therefore cannot hand the proof to `mark_verified`; it settles the record by name with `settle(fence, record, Reconciled::Landed { read, acted })`, under the lease the proof carries, where the ledger makes the same comparison. Both implementations that exist do this. Whether `verified` should take the proof by value instead is an open point below.

`GuardQuestion` is the engine's own type: the run, the tool, the effect class, the hazard, the binding chosen. `a2m-actions` turns it into a policy request; the engine never sees a policy type. A record lock is released by the ledger itself when the record reaches `verified` or `unchanged`, in the same transaction, and quarantined when the run stops with the record in doubt, so `Control` has no unlock. What used to be asked for and is not on the list arrives as data in `JobSpec`, filled by the runner when it claims the job: the input, the run snapshot, the conditions of choosing, and the lease's clock and directive.

Assumptions about other crates' types: `IntentToken` can be made only by `a2m-ledger`, and carries the run, the record key hash, and the lease number under which the intent was stored; over the link it is rebuilt from the control plane's acknowledgement by a constructor `a2m-ledger` provides for that purpose. `RecordLock` is `a2m-ledger`'s proof that a lock is held. `SessionLease` comes from `a2m-auth` and can inject credentials into a driver's context without returning them. `Plan` and `Step` are data in `a2m-core`; a step has a kind, a reference, a time limit, a failure class, compiled expressions for its inputs, and flags `commits` and `lookup`.

## The loop

Every operation call runs these parts. A read skips intend and confirm, and runs read back only when its binding declares one. A write repeats intend to record for each commit point.

| Part | What the engine does | Asks `Control` for | Stops with |
| --- | --- | --- | --- |
| 1. Guard | Five checks, in this order, the first that fails naming the stop (decision 0309): the plan's hash; the kernel's health from `conditions` (a write needs `ok`); the session's state for the call, the keeper's answer carried in `conditions.sessions`; the decision for this call; the surface has no edge block, from `conditions.edge_blocked`. The session and edge checks ask about each operation the call is sure to run (a step with no `when` and no `for_each`): it stops when every binding choose could pick for it acts on a surface whose session has ended, or, of those with a usable session, on a surface blocked at the edge. An operation with no binding choose could pick is left to choose | `plan`, `decide` | `policy`, `plan_unverified`; `busy`, `engine_degraded`; `session_expired`, `needs_login`; `policy` from the decision; `edge_blocked`, `edge_blocked` |
| 2. Choose | Takes the plan's bindings in their declared order and picks the first that qualifies (below). Sends the binding and the reason, with the provider choice from the handle | `send` | The class of the best reason any candidate failed: `session_expired` before `edge_blocked` before `drift` before `busy` |
| 3. Prepare | Gets the session lease, then the driver's prepared context | `session_lease` | `session_expired`, `needs_login`; whatever `prepare` returns |
| 4. Intend | Checks the binding it chose saves once; takes the record lock; makes the freshness check; checks the directive and that the lease leaves time for the record's steps, once before the waits and once after them; stores the intent, getting a token back | `lock`, `intend` | `busy`, `record_locked`; `stale`, `changed_since_preview`; `LeaseEnding` |
| 5. Act | Evaluates each act step's inputs, spends the rate grant, calls the handle. Outputs a step returns `for_reconcile` are sent, and acknowledged, before the committing step is sent. Immediately before the `commits` step the lease is checked once more. The step goes with the token; its outcome, and any assigned key, are sent as `acted` at once | `rate`, `send` | Before the save, the driver's class. The save itself, and everything after it, as "A write that was sent" says below |
| 6. Confirm | Runs the confirm steps the same way | None | `possible_commit`, `no_confirmation`; the application's own refusal, `rejected`, stands |
| 7. Read back | Runs the read-back steps, or uses the values the save response carried where the binding's evidence is `response`. Normalizes and compares. A binding whose evidence is `acknowledged` reads nothing and compares nothing: the record ends `acted` through `Control::acknowledged`, never verified | None | `mismatch`, `read_back_differs`, after one re-read; a read-back that fails is `possible_commit`, `no_confirmation` |
| 8. Record | Sends the commit point as `verified` with raw and normalized values and its evidence; the ledger releases the lock in the same transaction. For a read, sends the result | `evidence`, `send` | `busy`, `storage_full` if evidence cannot be stored, which leaves the record unverified |

**A write that was sent is in doubt until it is verified** ([decision 0147](../../decisions/0147-the-engine-after-its-safety-audit.md)). The engine holds this rule by itself, whatever a driver or the control plane reports: from the moment the committing step was sent until the record is verified, no failure settles the record as not written. A class that would, `policy`, `stale`, or `input`, makes the ledger release the lock and ends the run `failed`, so nothing would ever read the record.

| What fails | What the record is failed with |
| --- | --- |
| A step before the save | The step's class. Nothing was sent |
| The save, not sent | Its class. Nothing was sent |
| The save, fate unknown | `possible_commit`, `no_confirmation` |
| The save, answered by the system | The class, when it is the application's verdict that it refused the write: `rejected` for the input, `stale` for the version sent with the write. `policy` and `input` are no such verdict, only an answer the driver did not accept, and become `possible_commit`, `no_confirmation`. Every other class stands, and leaves the record in doubt at the ledger |
| An act step after the save, a confirm step, a read-back step | `possible_commit`, `no_confirmation`, naming the class and code it stands in for. Only `possible_commit` stands. A session that ends after the save is told to the keeper and is `possible_commit`, `no_confirmation` like any other failure there, `retry_safe` false, with the lock kept: the write may have landed. The resume waits for a session, as every job does, and reconciles before anything is written ([decision 0399](../../decisions/0399-a-session-that-ends-after-a-write-was-sent-leaves-the-write-in-doubt.md)). From a confirm step, `rejected` stands when the system answered with it: that is the application's verdict. It does not settle the record: the lock is kept and a resume reads the record first |
| The comparison, storing evidence, a call to `Control` after the save | Its own class, unless that class would settle the record: then `possible_commit`, `no_confirmation` |
| Reading a record in doubt, during reconcile | Its own class, unless that class would settle the record: then `possible_commit`, `cannot_reconcile`. The record was not read, so the run stops and is not ended |

How a pack marks the application's verdict: a failure the system answered with (`Sent::Answered`), of the class `rejected`, or, on the committing step, `stale`. Anything a driver could not read, accept, or follow after the save was sent is not a verdict, whatever class the pack gives it.

The binding that writes a record is checked for its one committing step where it is chosen to write, not only where the run's records were worked out, and every step from the save to the read-back runs in the one driver context the save ran in: no binding is switched after anything was sent. The plan check in `a2m-core` also refuses a binding whose committing steps are not the operation's commit points.

**Choosing among verified bindings.** A binding qualifies when its status is `verified` (in the sandbox, any status but `retired`); the registry has a driver for its mode whose `health` is `Ready`; its surface has not drifted and is not blocked at the edge; its session state is valid or the surface needs none; and its precondition, when it has one, holds for the call's inputs. Drift, edge blocks, and session states are read from `JobSpec::conditions`; a session that turned out to be invalid is learned at prepare, when the lease is refused. A pinned mode removes every other binding, compared by name. A named binding (`JobSpec::binding`, for a test, a trial, or a recorded fixture) removes every other binding of its operation, with no fall-back, and stops the job `stale`/`binding_changed` where the binding at its place no longer has its mode and `binding_digest`: evidence names the binding that earned it (decision 0200). The default order, when an operation states none, is set by the compiler from each mode's `rank` in its manifest, so the cheapest-first rule of section 201 lives in data and the engine never knows which mode is cheap. The engine only walks the plan's order.

**A binding in shadow** ([decision 0207](../../decisions/0207-a-shadow-binding-reads-beside-the-verified-one-and-a-person-promotes-it-on-a-run-of-exact-matches.md), [decision 0251](../../decisions/0251-the-evidence-for-promoting-a-shadow-binding-says-what-it-covers-and-a-shadow-costs-what-it-says.md)). In a live `Execute` job of a tool that only reads, with no mode pinned and no binding named, and only when `Control::keeps_shadow` says the control plane keeps comparisons, each binding of an operation whose status is `shadow` runs once in the job, after the first call of the operation that the verified binding answered. It must qualify as any binding must (a ready driver, no drift, no edge block, a usable session, its precondition); one that does not is kept as not compared. It reads the same inputs in a context of its own prepared in the posture `Sandbox` with no test records, so its driver's read-only guard is on and the handle refuses every step that commits, and no context is shared with the live call. It reads only through a surface on which the job already holds a context under a session lease, and never asks the keeper for a session, so it can never ask a person to sign in; one that would need to is not compared, `busy`/`no_binding`. Its rate budget is asked for through `Control::shadow_rate`, all at once or nothing, never waiting and never below the share left for callers' own reads; one refused is not compared, `busy`/`no_rate_budget`, and sent nothing. Its steps are kept out of the run's trace and its pass count, so the trace signature is the live call's alone; what it cost, one request per step granted, is in its comparison and, through the ledger, in a run event. What it observed is normalized output by output by the operation's field types and compared exactly with the verified binding's outputs: equal, or the names of the outputs that differ (`differs`), that it lacks (`missing`, also every output of a read that found nothing), or that cannot be read as their type (`unreadable`); no value leaves the engine. A shadow that fails is kept as not compared with its class and code. Every shadow of a job shares one budget, `EngineSettings::shadow_limit` (2 seconds), set when the first starts and cut short by the call's own time limit and lease: a shadow starts only within its first three quarters and is cancelled at their end, its contexts not used again, kept as not compared with `busy`, `shadow_time_exceeded`; never after a cancel or a pause. The comparisons go to `Control::shadow` as `ShadowCompared` (both bindings by `BindingPin`, the call's inputs, the requests) only after the call's result was sent, mismatches first, within what is left of the budget; one not kept in time, or whose keeping fails, is dropped and counted in `Engine::shadow_comparisons_lost`. What the caller is given is the verified binding's result in every case; the call is held up by at most the one budget. The shadow of a write is not built: a writing tool's job runs no binding in shadow, so the binding that writes is always the approved one (decision 0185). What it would need is the pack's: a step that builds its request and returns it unsent, and a form in which the verified binding's sent request can be compared with it (section 217, request shape).

**Falling back.** A read falls to the next qualifying binding after `drift`, an exhausted `transient`, or a driver that becomes unavailable. A write does not fall back inside a job: a record that fails stops the run there. On resume, a record in doubt is read through the mode its intent was stored for, and if that mode's binding may not act the job stops with `busy`, `no_binding`; only a record that reconcile returned to `planned`, because nothing landed, may be written through another binding. A record is never carried by two modes.

**Retrying.** `transient` is retried up to two times, only for a step that commits nothing, or for a `commits` step that was `NotSent`, or that the plan marks idempotent because the application takes the idempotency token. A retry starts the step again from evaluation; the driver never sees a retry as anything but a new step. No other class is retried here. `session_expired` is handed back to the keeper by asking `session_lease` once more; a second refusal rests the run at `needs_login`.

**Waiting.** A rate grant that names a time and a lock that is held are waited for behind `Control`, which keeps its own limit. A lookup's settle time is the engine's: during reconcile, a lookup that finds nothing waits the binding's `settle_ms` out once and looks again, and only then is the write taken as not landed. A record of identity `none` waits for its lookup twice: once until `settle_ms` has passed since the ledger recorded what followed its send (its `acted` mark or the failure that ended the attempt, `RecordSnapshot::sent_by`; with neither, `intent_at` plus the time its act steps may take), and once more between the two looks. These are the timed waits in the engine, and the condition each waits on is the one the binding declares: the settle time of its lookup has passed. They are made only when the lease covers the waits and the looks; otherwise the job returns `LeaseEnding` with the record still in doubt.

**The lease and cancellation.** Before every commit point the engine checks the lease's remaining time in `JobSpec::lease`, and reads the latest directive there; the runner updates both from its heartbeats. The lease is checked three times for a record: before the waits, against the time limits of all the record's steps; after the lock, the rate wait, and the freshness read, before the intent is stored; and immediately before the committing step is sent, against that step's time limit and the limits of the steps that verify it. A step whose plan states no time limit is counted at `EngineSettings::step_timeout`. If the lease does not cover what is asked, the engine returns `LeaseEnding` without sending; when that happens after the intent, the record stays in `intent` with nothing sent, and the next lease reads it first. A save that failed in a way that allows another try is sent again only while the lease covers it. On `cancel` it finishes the commit point it is in, records it, and stops; a read stops at once. A `commits` step's future is never dropped by the engine, by a time limit, a cancel, or the run's own time limit, so its time limit is the driver's to keep: a driver that never returns from a save holds the job until its lease is lost, and the run is then reconciled as section 208 says. When the engine's caller drops the job's own future while a save is under way, nothing is recorded and nothing needs to be: the record's intent was stored before the save, so the ledger holds it in doubt, and a save that completes afterwards is found by the read that begins the next lease.

**A provider's code that fails the engine.** A step that commits nothing runs under its time limit; when the limit passes, its future is dropped and the step fails with the class and code the plan gives it. Preparing a driver runs under `prepare_timeout`. A panic in a driver or a discoverer is caught where the engine calls it: a step that commits nothing fails with `unknown_variant`, `driver_panicked`; a step that commits fails with `possible_commit`, `no_confirmation`, because nothing says whether the write left; the driver context is marked as not to be used again.

## Normalization and comparison

- The plan holds each field type's normalizer as compiled host-function calls. The engine evaluates them through `a2m-expr` on both sides: the intended value and the value read back.
- Values are compared field by field in the record's declared order, as `Value`. There is no float anywhere in a comparison; a decimal compares exactly after its type's rounding rule.
- The record's fields are its operation's inputs that are not part of its key and not marked `precondition` (`InputPlan::precondition`). A precondition, such as the version a guard compares, is sent with the write and checked by the system; no read gives it back as it was sent, so it is never compared, at the preview, the freshness check, the read-back, or reconcile. The diff shows each one, normalized, in `DiffRecord::preconditions`, and its hash covers them when there are any, so the approval binds the value the write is sent with ([decision 0399](../../decisions/0399-an-input-marked-precondition-is-sent-with-a-write-and-never-compared-with-its-read-back.md)).
- A field labelled restricted is compared, and stored, as a hash of its normalized value.
- A comparison returns `Equal` or `Differs` with the names of the differing fields. Values go to evidence, never into the failure's message.
- `Absent` compares equal only to an absent `before`. A record that is absent after a write is a `mismatch`.

## The preview, the diff, and the hash

**The record is read in the context that writes it.** A read is fresh when it is younger than the freshness window and was made in the driver context that is about to write. A driver may keep what it read of a record, such as its version, with its context; a context prepared since the read, in place of one that failed or for the sandbox's write, knows nothing of the record, so the record is read again in it before the intent.

A `Preview` job runs the order of section 208: read the source through the registry's `SourceReader` once, apply limits, run the leading read steps with their `expect`, then take the write steps in order: work out each step's records, look each record up by its identity strategy, class it `create`, `update`, or `unchanged`, and, when every record of the step exists unchanged, give the step's result to the steps after it. Then build the diff, and hash it by the canonical form in the reference. The engine computes the hash; nothing else does, so that an approval and a run can never disagree about it. A diff with nothing to create or update and no other effect ends `completed` with no approval. Otherwise the run rests at `needs_approval`.

An `Execute` job for a write first calls `approval`, which consumes the approval bound to the diff hash and moves the run to `running` in one transaction. It then **runs the leading read steps again and stops if an `expect` that held at preview fails now.** That settles an open point of section 208 in favor of the safe answer. Each record is then written by the path above, with the freshness check against the values the preview stored.

## What is stored and shown

Every value the engine hands to `Control` passes the redactor first, whatever passed it before: the diff's current, raw, and proposed values; the intended values; the values seen before the write and after it; the evidence of every read, raw and normalized; the values recorded before a commit point; the outputs stored with `acted`; a key the system assigned; and the tool's result. The trace is redacted by `a2m-telemetry`, and a failure by the handle or by `Control`. A finding that still holds a registered secret is not passed on, and is counted as dropped. A record's own key is the caller's input and is stored as the ledger needs it.

- **Comparing.** The values seen before a write are held as they are stored, in the job and on resume, and every later read is compared with them in that form. Two values that differ only inside a registered secret compare equal, at the freshness check and at reconcile. The handle redacts what a driver returns before the engine sees it, so the engine could not compare more closely if it tried.
- **A write that carries a registered secret is refused,** with `input`, `input_invalid`, when its records are worked out. What is read back is masked, so such a value could never be compared with what the system holds, and rule 4 keeps secrets out of tool inputs.
- **A mask is never sent to the system.** An argument of a step that holds the redaction mark stops the step unsent, with `unknown_variant`, `argument_masked`: the system showed a registered secret where the step takes a value, such as the record's current values. On resume, a value the ledger holds that was masked is left out, never used as an argument, a key, or an output, and a verified or acknowledged record one of whose outputs the ledger holds masked stops with `output_unexpected` rather than hand back the mask or `null` ([decision 0315](../../decisions/0315-every-declared-output-is-in-every-result-on-every-path.md)); a record in doubt that could only be found by it stops for a person with `possible_commit`, `cannot_reconcile`, because looking for it under the mask could find nothing, and nothing found would be taken for a write that did not land.
- **Captures** (decision 0162). The capture named `acting` is how the step was done: it goes into the step's trace record, `means` in the signature and `measured` not, and is kept as no evidence. Every other capture of a step that did not fail is offered to the trace as an artifact taken at the step, an image as a screenshot and anything else as an exchange; what the run's capture level keeps, redacted once more and checked by the trace, is kept through `Control::keep_capture`, tied to the run, the record, and the step, and the trace record names its digest. It is kept when its step ends, before the next step is sent, so what was captured before a committing step is kept before the save. A capture that cannot be kept is dropped, and the run goes on. An image arrives with no report of what was masked, so the trace drops it.
- **What the system said** (decision 0162). A driver's failure may carry the system's own words, a validation message or a constraint's name, on `Failure::said`: bounded, redacted by the handle, and carried into the run's failure, also by a failure that stands for a write in doubt. It is never joined into the engine's sentence or next step. A record that cannot be read after a lost acknowledgement shows what was recorded before its save there, redacted.

## Reconcile and resume

A `Reconcile` job, and the start of any resumed `Execute` job, read `JobSpec::snapshot` and take each record in `intent`, `acted`, or a retryable `failed`, in key order.

| Identity strategy | How the record is read |
| --- | --- |
| `natural`, `composite` | The binding's read-back steps alone, from the identity inputs |
| `stamped` | The binding's lookup steps, which are handed the record's token as the step's stamp. At the first preview no record can carry the token, so the record is a `create` without a lookup |
| `find` | The binding's lookup steps, from the inputs. The pack's lookup step sees the matches: more than one is its failure `possible_commit`, `ambiguous_match`, which the engine carries; the engine never picks one. Zero matches during reconcile waits the settle time out once and looks again |
| Any, once a key was assigned | The read-back steps, which are handed the assigned key as the step's key and find everything the record's steps observed under `result` |
| Any, with outputs recorded before the commit point | The read-back steps, which find the recorded values under `result` |
| `none` | It has no key, so "absent" is never proof by itself ([decision 0185](../../decisions/0185-a-record-with-no-key-is-never-proved-absent-by-one-look.md)). A record whose recorded values were masked is not read at all. Its binding's read-back, given the values its own attempt recorded before the save, may find it: what was found is compared field by field after normalization, as every read-back is; equal settles it `verified` on the digest of what was read, different is `mismatch`, `read_back_differs`, never verified and left in doubt. What a read-back does not find proves nothing. It returns to `planned` only when the binding's lookup is one the mode's manifest states authoritative for absence (`absence_steps`, asked through `DriverHandle::proves_absence`), asked once `settle_ms` has passed since the send was recorded, and it answers "absent" twice, the second after another `settle_ms`. In every other case the run stops with `possible_commit`, `cannot_reconcile`, what was recorded (and what was found) shown as `said`, and nothing is sent. A binding whose evidence is `acknowledged` that a look finds stays in doubt: the ledger ends a record on an acknowledgement only under the lease that sent it. It is a `create` in every diff |

What a lookup finds is the read: its values are compared, and a key it reports is kept as the assigned key. A binding of a `stamped` or `find` operation that has no lookup steps is read by its read-back steps.

Then the table of section 208: equal to the intended values, mark `verified` and write nothing; equal to the values seen before, return to `planned`; neither, stop with `stale`, `changed_since_preview`. Only after every doubtful record is settled does any write run. A dependent record takes its parent's outputs from the snapshot and never runs the parent again. The ledger holds a record's outputs from its `acted` mark, and, for one that reconcile found had landed, from `Reconciled::Landed`, which carries what the read gave (decision 0145). A record found `unchanged` has none, so on resume it is read once more, by its identity strategy, when its operation declares outputs. A `failed` record for which nothing was ever sent goes back to `planned` with `Reconciled::NeverSent`, which the ledger checks from its row; any other goes back only on a read, with its evidence.

A `VerifyAfterLoss` job reads every record the ledger says was verified in the lost run, later, and reports any difference as evidence and an alert; it changes no state.

## Failure classes and run states

Which state each class ends in, as built and recorded in [decision 0091](../../decisions/0091-the-engine-loop-as-its-first-two-passes-settled-it.md).

| Class | Run state | Lock on a record in `intent` or `acted` |
| --- | --- | --- |
| `input` | `rejected` while the run is `validating`; `failed` once it has run | None is held |
| `policy`, `stale` | `failed` | Released by the ledger; nothing is in doubt once read. The engine fails a record with one of these, or with `input`, only when nothing was sent or the system itself refused the write |
| `session_expired` | `needs_login` | Quarantined |
| `transient` after retries, `busy`, `mismatch`, `edge_blocked`, `unknown_variant`, `rejected` | `stopped` | Quarantined if a record is in doubt, released otherwise |
| `drift` | `stopped`, and pinned to the plan that drifted | As above |
| `possible_commit` | `stopped` | Quarantined until reconcile |

A run stopped by the engine's own death is resumed by its caller with `run.resume`, never by the engine alone at its next start.

## Pipelines

A pipeline is a list of steps in the plan. The engine runs its steps in the plan's order, each once or once per row of the source. The leading read steps run at the preview and again before the first write; the write steps' records are written one after another in the order of the steps; the read steps after the first write step run once every record is verified or unchanged. A record that fails stops the run there.

**The source.** The bytes of the source arrive in `JobSpec::source`. The engine reads them once through `a2m-sources`, with the built-in readers or the registry's, before any surface is touched, on a thread for blocking work, so that parsing a large source holds up no other job and no heartbeat; a reader that panics ends the read with `unknown_variant`, `driver_panicked`: a source that is rejected ends the run `rejected` with every problem, and a source with more rows than the pipeline's `max_records` ends it with `policy`, `record_limit_exceeded`. A source whose reader is `operation` is not carried out yet and stops with `busy`, `job_kind_not_built`. The pipeline's `max_duration_ms` is measured on the engine's clock from the start of each job; past it the run stops before its next operation call with `busy`, `run_time_exceeded`.

**Dependent records** ([decision 0092](../../decisions/0092-a-record-that-waits-for-an-earlier-record-is-planned-under-its-place-in-the-run.md)). A write step's result is there for the steps after it once every one of its records is verified or unchanged. A `with` expression that reads `steps` and cannot be worked out while an earlier record is still to be written marks its input as waiting. A waiting input must be part of the record's key; a value that is written must be known at the preview, and one that is not is refused with `input`, `input_invalid`. A record with a waiting input is a `create` in the diff, is not looked up, and is planned under its place in the run: the operation's name, the step's position, and the record's position. When the write path reaches it, its inputs are worked out from the earlier record's outputs and it is written as any record. When a record fails, the records still waiting for it are marked `failed` with `busy`, `blocked_by_parent`; on resume each goes back to `planned` without a read, because nothing was ever sent for it. The code alone is not trusted for that: the record must also have failed with the class `busy`, never have had an intent, and have nothing recorded of it. Any other record that failed under that name is read like every record in doubt.

Records are written one at a time: `JobSpec::parallelism` and the intent window are kept and not used yet.

## The sandbox

The sandbox is private to this crate. It runs a draft plan for a `Try` job, which `a2m-actions` submits for `draft.try`, and it is the `SandboxPort` each discoverer receives in a `Discover` job, which `a2m-actions` submits for `discover.run` and the scout. Discovery is a job like any other, so it runs on the runner that holds the surface's session and under a lease.

- It accepts draft bindings, and records its runs as tries, so no binding's health or shadow count changes.
- It prepares every driver with a posture other than `Live`, so the driver's own read-only guard is on. In a `Try` job the posture is `Sandbox` with no test records for every driver context, whatever the environment declares: the leading reads, the preview's lookups, the reads of other operations, and the reads after the last write. In that posture the handle refuses every `commits` step.
- It never asks for an intent unless the record's key is one of the target's declared test records. The key is checked where the records are worked out and again where the record is written. Without a token the handle refuses every `commits` step unsent. A discovery job declares no test records, so through its port a write is impossible.
- **What a `Try` of a drafted write may do.** It may read, under the guard, as any try. It may write one thing: a record whose key is a declared test record, after its diff was approved and its intent stored, through its one committing step. For that it prepares one more driver context, in the posture `Sandbox` with test records, and runs in it only that record's read before the intent, its act steps, its confirm steps, and its read-back. Nothing else of the job runs there. The kernel cannot see what a step that is not marked `commits` does in that context: there the guard is the pack's, which must let through the committing step, which the handle has checked against the token, and nothing else that changes the system. Nor can the kernel prove that the request a draft builds addresses the record its key names: the driver is given the key with the step, and holding the request to it is the pack's half.
- The driver behind a discoverer's port is the surface's own, never a binding's: discovery is about a surface, so a target that declares only its surface is observed. Of the modes the provider that serves the surface's kind declares, those of the kind's archetype whose driver the registry hands over, the one of the lowest `rank` (the one an operation would try first) is prepared, in the `Discovery` posture, through `Run::hold_on`. Two of one rank are a choice nothing states: the port then answers every step `busy`, `no_binding`, as it does when no mode has a driver, and nothing is guessed ([decision 0189](../../decisions/0189-a-developers-first-hour-runs-without-hand-steps.md)).
- Each discovery is bounded in time and in the size of what it hands over, and the bounds hit are reported. The engine holds a discoverer to the seconds its manifest declares, and never to more than the job's lease has left: past them its future is dropped, which is safe because every step it can ask for only reads, and its report is the failure `busy`, `run_time_exceeded`. The findings it made before stand. Findings are kept up to the bytes the manifest declares and up to 10,000; what comes after is counted as dropped and `bytes` is added to the bounds hit. Depth, and the bytes a discoverer reads through the port, are the discoverer's own to keep to: the engine gives it the bounds and cannot count either.
- A discovery is handed the files of the project's `sources` folder that the call named, from `Control::discovery_files`, and no other file ([decision 0263](../../decisions/0263-a-person-hands-the-scout-files-of-the-projects-sources-folder-by-names-the-discoverers-declare.md)). Before policy is asked or a driver is held, each is checked again: its name must be one that a discoverer of the surface declares (`Registry::files_taken`), else `input`, `job_invalid`; its path is `sources/` and a file below it, joined by `CheckedPath::join` under a root of the project's `sources` folder made for reading only, so an absolute path, `..`, and a link that leads out of the folder are refused; no part may start with a dot, and what stands there must be a regular file, not a link, else `input`, `path_not_allowed`. One refused path ends the job and no discoverer runs. Each checked file is added to `DiscoveryScope::paths` under its name.
- A `Try` job of a write goes through the preview and the approval check as an `Execute` job does, and its record's intent is stored before the write: only the posture and the test-record check differ.

The kernel's half of "cannot write" is the token. The other half, what a step that commits nothing may still do to a system of its kind, is stated per archetype by each pack and proven by its conformance test against its test stand.

| Archetype | What the pack guarantees outside `Live` |
| --- | --- |
| Interface | Only plain navigation; no control whose role or name commits is pressed; requests other than reads are blocked in the browser |
| Request | Only methods the service's own description marks safe, or read endpoints a person marked safe |
| Command | A program is started only in its declared help and version forms |
| Artifact | A file is opened for reading only, and never replaced |
| Stream | A queue is never consumed and nothing is published; a sample is read without acknowledging |
| Device | Only frames the device lists as reads |
| Data, within request | A statement runs in a read-only transaction |

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-actions` | `Engine::new`; `run_job` and `discover` from its in-process runner loop, for every job kind including `Try` and `Discover`; implements `Control` |
| `a2m-runner` | `run_job` and `discover`; implements `Control` over the link |

It calls: `Registry::driver` and `DriverHandle`; `Registry::discoverers_for` and `Discoverer::discover` in a `Discover` job; `SourceReader::read`, with bytes the runner fetched; `a2m-expr` to evaluate; `a2m-telemetry` for spans; `Control` for everything else. It never calls `a2m-files`, `a2m-policy`, or a store.

## State

Memory only: the plans it has loaded, by hash, in a bounded cache; for each job in hand its prepared driver contexts, its leases, its locks, and its intents ahead. Nothing it holds survives the process, and nothing needs to: the ledger, reached through `Control`, is the authority, and resume begins by reading it.

## Failures

It produces every class through the loop. `possible_commit`, `ambiguous_match` is a pack's code: its lookup step sees the matches and reports it, and the engine carries it. Codes the engine defines itself, each held to this table and to the technical reference by a test:

| Class | Code | When |
| --- | --- | --- |
| `policy` | `plan_unverified` | The plan's bytes do not give the hash the job names |
| `policy` | `record_limit_exceeded` | The run would touch more records than its limit allows |
| `policy` | `sandbox_read_only` | A write was asked for where nothing may be written |
| `policy` | `intent_token_missing` | An intent was stored and no token came back for it; nothing was sent |
| `policy` | `approval_stale` | The binding that would write a record is not one the approved diff names for it, or the diff read back is not the approved one; nothing is sent ([decision 0185](../../decisions/0185-a-record-with-no-key-is-never-proved-absent-by-one-look.md)) |
| `input` | `job_invalid` | The job names a tool, an environment, or a surface the plan does not hold, or is of a kind the tool cannot run as |
| `input` | `input_invalid` | An input is missing, is not declared, or is not of its type |
| `busy` | `engine_degraded`, `no_binding` | A write on a degraded kernel; no binding qualifies for a reason that is not session, edge, or drift |
| `busy` | `run_time_exceeded` | The run passed the longest time its pipeline allows |
| `busy` | `blocked_by_parent` | A record waits on a parent record that failed |
| `busy` | `evidence_not_stored` | Evidence could not be stored, so nothing can be marked verified |
| `busy` | `no_rate_budget` | No rate budget was granted for a surface |
| `busy` | `job_kind_not_built` | The job asks for something this engine does not carry out yet: a binding that saves more than once in one call, a source whose reader is `operation` |
| `busy` | `control_unanswered` | A call to `Control` was not answered within the time the engine waits |
| `busy` | `capture_not_kept` | `Control::keep_capture` by default: the control plane keeps no captures, and the capture is dropped; the run goes on |
| `busy` | `acknowledgement_not_kept` | `Control::acknowledged` by default: a record whose only evidence is the system's acknowledgement cannot be ended, and stays in doubt |
| `busy` | `shadow_not_kept` | `Control::shadow` by default: the control plane keeps no shadow comparisons |
| `busy` | `shadow_time_exceeded` | A binding in shadow did not end within the reading part of the job's shadow budget (`EngineSettings::shadow_limit`); it was cancelled and kept as not compared |
| `unknown_variant` | `driver_panicked` | A provider's code panicked in a step that commits nothing, in preparing, or in discovery |
| `transient` | `prepare_timed_out` | A driver was not ready within the time preparing may take |
| `session_expired` | `needs_login` | The session ended and could not be had again |
| `edge_blocked` | `edge_blocked` | The surface is blocked at the edge |
| `drift` | `binding_drifted` | Every binding that could carry the call out has drifted |
| `stale` | `changed_since_preview` | A record no longer holds what the preview saw |
| `stale` | `binding_changed` | The binding a test or a trial named is no longer the one at its place: it moved, changed its mode, or changed what it does (decision 0200) |
| `mismatch` | `read_back_differs` | The value read back differs from the value intended |
| `unknown_variant` | `expect_failed` | An `expect` does not hold: a leading read's before the first write, or a step's |
| `unknown_variant` | `field_not_read_back` | Reading a record gave no value for a field the operation writes |
| `unknown_variant` | `no_read_back` | A binding has no steps that read the record, so nothing it writes can be verified |
| `unknown_variant` | `output_unexpected` | A step returned a value the plan gives no place to; or no value, or `null`, for an output that may not be absent, on any path ([decision 0315](../../decisions/0315-every-declared-output-is-in-every-result-on-every-path.md)) |
| `unknown_variant` | `commit_skipped` | A binding ended without sending its write |
| `unknown_variant` | `condition_not_boolean` | A condition of the plan, a step's `when`, a binding's precondition, or an `expect`, gave a value that is neither true nor false |
| `unknown_variant` | `argument_masked` | An argument of a step holds the redaction mark: the system showed a registered secret where the step takes a value. The step is not sent |
| `possible_commit` | `no_confirmation` | A write was sent and the system's answer is not known, or could not be recorded, or the driver panicked in the save; or a write was sent and a later step, or the answer itself, failed before the record was verified |
| `possible_commit` | `cannot_reconcile` | A record in doubt cannot be read, so a person must settle it; or reading it failed with a class that would have ended the run |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| No `commits` step reaches a driver before its intent is durable (ledger rule 1) | `tests/write.rs`: the order of lock, intent, save, `acted`, read-back, `verified` in one log; an intent that cannot be stored, or is never acknowledged, sends nothing (`tests/faults.rs`) |
| A record is `verified` only with a read value equal after normalization, and its evidence (rule 2) | `tests/write.rs`: a near value is a `mismatch`; `tests/faults.rs`: a record whose evidence cannot be stored stays unverified |
| The diff hash is the same on every machine, and the reference's worked example is that hash | `src/diff.rs`: `a_diff_hashes_the_same_on_every_machine`, a fixed diff with a create, an update, an unchanged record, a restricted field, and a decimal, against `DIFF_SAMPLE_HASH`, in CI on Windows, macOS, and Linux; `the_worked_example_in_the_reference_hashes_to_the_value_it_prints` |
| A write enters `running` only through `approval` (rule 3) | `tests/write.rs`: no approval waits, an approval of another diff does not let the run write |
| Outputs a step returns before the commit point are acknowledged before the committing step is sent | `tests/identity.rs`: the order in one log; `tests/faults.rs`: when they cannot be stored the save is not sent |
| The engine depends on neither `a2m-files` nor `a2m-policy` | The audit |
| After any doubt, no record of the run is written before it is read (rule 5) | `tests/write.rs` and `tests/identity.rs`: a lost answer, by each identity strategy; the test application shows no duplicate |
| A record is never written by two modes | `tests/faults.rs`: two bindings, the first fails after sending, and the second never sees a step, not even a read |
| A doubtful write is never retried | `tests/write.rs`: a lost answer is sent once |
| Killed at any call to `Control` and resumed, a write is sent at most once, is read before it is written again, and ends verified or in doubt with the run stopped as `possible_commit` | `tests/resume.rs`: the job's future dropped after each call of a clean write in turn (`Faults::abort_after`), the lease ended, and, every other time, the run stopped and reconciled as a lost lease leaves it; the test application holds one write |
| The sandbox cannot write outside declared test records | `tests/sandbox.rs`: a planted `commits` step is refused by the port; a try of a record that is not a test record is refused before a driver is prepared; the test stand shows nothing stored |
| The engine names no mode | The audit's layering rule, and `tests/faults.rs`: the crate's source holds no mode name any first-party manifest registers |
| From the save until the record is verified, no failure settles it as not written | `tests/faults.rs`: a confirm step, a read-back step, a later act step, and the save's own answer, each failing with `policy`, `input`, and `stale` after the save was sent; the test application holds the write, the run stops in doubt with the lock quarantined, and a resume reconciles it as landed and writes nothing. The same for the read of a record in doubt |
| A save is not sent unless the lease covers it | `tests/faults.rs`: a lease that runs out during the freshness read sends no intent and no save; one that runs out after the intent sends no save and the record is read first by the next lease; a step with the default time limit is counted at that limit; a settle time the lease does not cover is not waited out (`tests/identity.rs`) |
| A save is never abandoned | `tests/faults.rs`: a save that outlasts its time limit while a cancel arrives is waited for and recorded; a job dropped during a save whose commit completes afterwards leaves the record in `intent`, and the late write is found and not sent again |
| In the sandbox the driver's guard is on for everything but the write of a declared test record | `tests/sandbox.rs`: a try of a read where test records are declared prepares no driver that may write, and a drafted step that changes a record is refused; a try of a write looks its record up under the guard, and writes in a context of its own |
| Nothing the engine stores or shows holds a registered secret, and a mask is never sent to the system | `tests/write.rs`: a planted secret the system shows is in no evidence; a write that carries one is refused; a masked value is not sent as an argument; a record in doubt whose recorded values were masked stops for a person and is not written twice; `tests/identity.rs`: so does one of identity `none`, unread |
| A record of no key in doubt is never sent again on an "absent" that proves nothing, and a found record is compared | `tests/identity.rs`: a read-back's "absent" leaves it in doubt with the stand's write count at one; an authoritative lookup answering "absent" twice after its settle time returns it to `planned`; one that finds it on the second look verifies it; no lookup before the settle time; found with other values is `mismatch` |
| The binding that writes is the one the approval names | `tests/write.rs`: a write through another binding stops unsent with `approval_stale`; a record whose binding is chosen later is shown with every binding it could take |
| Guard makes its five checks in order, and the first that fails names the stop | `tests/guard.rs`: one test for each check, each of which fails when its check is taken out, because nothing after the check (the policy question, the move to `running`, a driver) was reached; and one run per check with every later check failing too, each naming the earlier |
| A condition that is neither true nor false stops the call | `tests/read.rs` |
| A discoverer that floods is held to its declared size | `tests/sandbox.rs` |
| Comparison never uses a float | The `Value` type, and `tests/write.rs`: a rounded decimal. Against the test application, in the product (`crates/anything2mcp/tests/normalized_write.rs`): `2.53"` is rounded by `length_ft_in` to 40 sixteenths, which the application stores, and is verified; the same length typed as plain text reads back as the application shows it, stops with `mismatch`, `read_back_differs`, is written once, and is never verified |
| Cancel never abandons a commit point halfway | `tests/faults.rs`: cancel arrives during a save, which is finished and recorded before the run ends |
| A call to `Control` that never answers, and a provider that panics or hangs, end the step and not the engine | `tests/faults.rs` |
| No input changes where a request goes, which record is written, or which step runs; a value from the system stays a value | `tests/manipulation.rs` |
| Every failure code is in this document and in the technical reference under its class | `tests/faults.rs` |
| A binding in shadow never changes the call: the caller is given the verified binding's result when the shadow matches, differs, fails, or hangs, or its keeping fails or hangs; the call is held up by at most the one budget; and the trace signature is the live call's alone | `tests/shadow.rs` |
| A binding in shadow never asks for a session and never takes a caller's share of the rate budget; its cost is in its comparison and the run's events; one input called again is one input toward promotion | `tests/shadow.rs` |
| A binding in shadow reads only, in a context of its own with its driver's guard on, and runs only beside a live read: never in a try, an offline test, a pinned call, a named binding, or a write | `tests/shadow.rs`: the postures prepared; no step of the shadow mode in each of those jobs; a write with a binding in shadow sends its save through the approved binding, and the stand holds one write |

## Build order

1. Done: slice 1.1: a hand-written plan, guard with a stub decision, choose, prepare, act, record for one read, with an in-memory `Control` used by tests.
2. Done: slice 1.2: the step trace through `Control::send`, and its signature.
3. Done: slice 1.3: plans fetched by hash; the hash check at guard.
4. Done: slice 2.4: `session_lease` and the session states in `conditions`; `needs_login`.
5. Done: slice 3.3: intend, the token, confirm, read back, record, for one field.
6. Done: slices 3.4 and 3.5: normalization and comparison; the preview, the diff, the hash.
7. Done: slice 3.7: reconcile and resume; the state table above.
8. Done: slices 3.8 and 3.9: every identity strategy, a key assigned with the save or before it, a header with its lines, pipelines over a source read from bytes, and their limits (`tests/identity.rs`, `tests/pipeline.rs`). Not built: a source whose reader is `operation`; a binding that saves more than once in one call; records in parallel and the intent window.
9. Done: slice 3.10: the rows of section 224 that are the engine's (`tests/faults.rs`): the store cannot commit before the intent, after the write was sent, and at `verified`; the evidence store full; a call to `Control` that never answers; a driver that panics, hangs, or never gets ready; cancel during a save; a lease that is ending; a degraded kernel. Not the engine's, and not tested here: the runner's local journal, memory pressure, migrations.
10. Done: package E1: the sandbox as `SandboxPort`, `discover` for `Discover` jobs, `Try` jobs of reads and of writes to declared test records, and `VerifyAfterLoss` jobs (`tests/sandbox.rs`). Not built: a canary beyond a read that refuses a tool that writes. Shadow: step 17.

11. Done: the safety audit of [decision 0147](../../decisions/0147-the-engine-after-its-safety-audit.md): a write that was sent is in doubt until verified; the lease at the commit point; the sandbox's posture by context; redaction of everything stored and shown; and the smaller findings. Not built: taking what a mode holds alone inside the engine (it is named for the job's requirements, see the open points); keeping captures.
12. Done (decisions 0161 and 0162), `tests/contract.rs`: what the record's earlier steps recorded is handed to its later steps, within a pass and on resume from `before_commit`; how a step was done is in its trace record, and five runs of one read give one signature; captures kept through `Control::keep_capture`; what the system said carried into the run's failure; the run's project folder in `PrepareContext::paths`; a binding's evidence, `read_back`, `response`, or `acknowledged`; a step is given the record's key in the system or none, never its place in the run; a driver's destinations are the running operation's, and a context is used again only for the same; a number masked as a secret is the mask's text in the result. Done since (decision 0176): `RecordSnapshot::acknowledged`, so a resume and reconcile pass over a record that ended on the system's acknowledgement; a record of no identity in doubt read through what it recorded (`tests/identity.rs`). Done since ([decision 0185](../../decisions/0185-a-record-with-no-key-is-never-proved-absent-by-one-look.md)), `tests/identity.rs` and `tests/write.rs`: a record of no identity is never returned to `planned` on one "absent"; what a look finds is compared; a record whose recorded values were masked is not read; the outputs of a record ended on an acknowledgement are handed back on resume; each diff record names the binding that writes it and its evidence (`DiffRecord::binding`, or `could_take` for a record whose binding is chosen later), `RunSnapshot::approved` hands the stored diff back, and a write through any other binding stops unsent with `policy`, `approval_stale`.
13. Done ([decision 0177](../../decisions/0177-a-targets-offline-tests-run-against-a-replay-of-their-fixtures.md)): `Engine::run_offline`, which hands every binding a replay of recorded answers through `Registry::replay_driver` in place of its provider's driver, asks for no session, and refuses a discovery job (`tests/contract.rs`). Against a replay a binding of any status but `retired` may act, and a list output is normalized one item at a time ([decision 0180](../../decisions/0180-a-targets-offline-tests-cover-writes-and-gate-the-publish.md)).

14. Done ([decision 0189](../../decisions/0189-a-developers-first-hour-runs-without-hand-steps.md)), `tests/sandbox.rs`: a discovery prepares the surface's own mode, with no binding (`a_surface_with_no_binding_is_observed_through_its_own_mode`).
15. Done ([decision 0196](../../decisions/0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md)): a keyed record whose binding is `acknowledged` with no read-back step is not read before it is written (`DiffAction::Write`, `NOW_CANNOT_BE_READ`), and in doubt is reconciled as a record with no key is, through a lookup its manifest states proves absence, and stays in doubt when one finds it (`tests/contract.rs`); `Control::other_records`, handed to each act through `DriverHandle::act_asking`, `CannotTell` without one; `Control::step_observed`, what each reading step of a `Try` job observed, for `fixture.record`; `DiscoveryScope::paths`, the run's project folder; `run_offline` takes an `Arc<Replay>` and no other driver (EN-1); a record keyed by its place in the run is never a test record (E-6, `tests/sandbox.rs`); `said` of a keyless record in doubt holds data alone, `recorded` and `found` (E-7, `tests/identity.rs`).
16. Done ([decision 0200](../../decisions/0200-evidence-names-its-binding-and-a-recorded-fixture-says-nothing-of-the-original.md)), `tests/sandbox.rs`: `JobSpec::binding`, a `BindingPin` (the operation, the binding's place and mode, and `binding_digest`): a test, a trial, or a recorded fixture runs that one binding of its operation and no other, with no fall-back, and stops `stale`/`binding_changed`, nothing sent, where the place no longer holds that binding as it was (`a_trial_runs_the_binding_it_names_and_never_another_in_its_name`). `binding_digest` takes the binding's mode, verb, surface, every lowered step with each expression's text and the whole of each pack section it refers to, its settle time, evidence, and precondition, and not its status, so a promotion keeps it and a change elsewhere in the plan does not move it; `default_binding` is the binding that would act live, the first `verified`, else the first not retired (`a_tested_binding_is_the_one_that_would_act_live_and_its_digest_ignores_its_status`). A job that names no binding is chosen for as before.
18. Done ([decision 0263](../../decisions/0263-a-person-hands-the-scout-files-of-the-projects-sources-folder-by-names-the-discoverers-declare.md)): `Control::discovery_files` and the check of each file in `sandbox.rs` (`tests/sandbox.rs`: `a_named_file_of_the_sources_folder_reaches_the_discoverer_under_its_name`, `a_link_out_of_the_sources_folder_is_refused_and_nothing_is_read`). The files reach the engine through `Control`, as the project folder does, and not as a field of `JobSpec`: `JobSpec` is built field by field in `a2m-actions`, and a field would have broken each of those places. Nothing fills them yet: the action's `files` input and the runner's half are `a2m-actions`'s.
17. Done ([decision 0207](../../decisions/0207-a-shadow-binding-reads-beside-the-verified-one-and-a-person-promotes-it-on-a-run-of-exact-matches.md)), `tests/shadow.rs`: Phase 4, shadow of a read, as "A binding in shadow" above; `Control::keeps_shadow` and `Control::shadow`, `ShadowCompared`, `EngineSettings::shadow_limit`. Its counting, the promotable question, and the record of a promotion are the ledger's (`Shadows`). Not built: the shadow of a write, which runs nothing (it needs the pack's half, above); the setting `engine.promoted_writes`, which the engine does not read. Done since ([decision 0251](../../decisions/0251-the-evidence-for-promoting-a-shadow-binding-says-what-it-covers-and-a-shadow-costs-what-it-says.md)): one shadow budget per job, the comparisons kept after the result, `Control::shadow_rate`, no session asked for, the input and the cost in `ShadowCompared`, `shadow_evidence_environments`, `verified_binding`, `Engine::shadow_comparisons_lost`. Not built: shadowing more than the first call of an operation in a job (decision 0251 says why).

19. Tested, nothing built ([decision 0343](../../decisions/0343-the-decompiling-engine-is-a-discoverer-of-the-provider-contract-that-starts-only-a-pinned-program-and-reads-one-form.md), package X6): `tests/decompile.rs` runs the decompiling engine's discoverer through a discovery job: an engine that is not installed or not pinned is reported by the job's event and the other discoverers go on, and the findings it makes reach `Control` only as findings. `Scene::with_packs` lets a test add packs to the scene's registry.

## Open points

- **What a mode holds alone.** A mode's manifest says what one write holds alone: nothing, the surface, a file, a screen, a link. The engine does not take it. `Engine::exclusive_needs` names it for a tool, as the kind and the surface of each binding that may write, and whoever submits the job puts one resource key for each into the job's requirements, so the scheduler takes them together when it leases the job: before any record lock, so no two jobs can wait for each other, and for as long as the job is leased, so what a lost job held passes to its reconcile job. Nothing fills the requirements from it yet. Taking a key for each write inside the job, and not for the whole job, would need a call on `Control` and an order between that key and the record lock; it is not designed.
- `Control` has no way to ask whether the record's mode is known after a restart: a record in doubt whose snapshot carries no mode is read through the first binding that qualifies. The ledger should keep the mode with the intent.
- The ledger settles a failed record by its class alone. The engine no longer fails a record that was sent with a class that settles it; the ledger should also keep the lock of any record that reached `acted`, whatever the class.
- A record that ended `acted` on an acknowledgement is, to a resumed run, a record in doubt: `RecordSnapshot` does not say how it ended, and `a2m-actions` builds that type field by field. Until it does, such a record whose binding has no lookup and no read-back stops the resumed run with `possible_commit`, `cannot_reconcile`.
- Whether `Control::verified` should take the proof of `acted` by value, as `acted` takes the intent token, so that an implementation over the store can call `mark_verified` itself and need not go through `settle`. The trait is held as it is while `a2m-actions` is built against it.
- A record that waits for an earlier record is locked under its place in the run, not under the key it gets once its parent exists. Two runs that create lines under the same new header cannot happen, because the header is new to each; a line added by another run to a header that already exists is locked under its real key. Whether the lock should move to the real key once it is known is open.
- A `commits` step that never returns holds its job until the lease is lost. Dropping its future after a grace time would free the job and leave the record `possible_commit`, which reconcile handles; the driver contract forbids the drop today.
- Whether `Diff` and its canonical form should move to `a2m-core` so that the approval surface can render one without the engine.
- How a shadow binding of a write predicts: a posture in which the handle refuses every step that reaches the system and the driver returns what it would have sent, and the verified binding's driver keeps the request it did send in the same form. Both are the provider contract's to offer; a read's shadow is built (decision 0207).
- Whether the conditions of choosing, read once at claim, go stale during a long pipeline; if so, `Control` needs a way to read them again, which is not on its list today.
- The intent window across bindings in one pipeline step, when records of two bindings interleave. Records are written one at a time today.
- Whether a canary is an `Execute` job of a read with a flag, or a kind of its own as the jobs table has it.
- A read step that stands between two write steps runs after every write, not in its place. A write step that needs such a read's result cannot be previewed, and the compiler should refuse it (an open point of section 208).
