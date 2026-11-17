# `a2m-engine`

The engine loop: one job in, one outcome out. It carries out a compiled plan through the eight parts of every operation call, chooses among verified bindings, normalizes and compares values, reconciles before it writes on resume, and runs drafts and discovery in a sandbox where a write is impossible. Everything it needs from outside its own loop it asks for through `Control`.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 6, Engine |
| May depend on | Layers 1 to 5, except `a2m-files` and `a2m-policy`, which the audit refuses. Never `a2m-sched`, never a pack |
| Depends on | `a2m-core`, including `Plan` and `Step` as data and the check of a plan's hash; `a2m-expr` to evaluate compiled expressions and field-type normalizers; `a2m-telemetry` for spans and the step trace; `a2m-ledger` for the types `IntentToken` and `RecordLock` only, never to open a store; `a2m-auth` for `SessionLease`; `a2m-sources` for `SourceReader`; `a2m-provider` for the registry, `DriverHandle`, and `SandboxPort`. Outside: `tokio` |
| Built in | Phase 1 (slices 1.1 to 1.3, reads), Phase 2 (slice 2.4, sessions), Phase 3 (slices 3.3 to 3.10, writes, preview, resume, identity, pipelines, faults), package E1 (the sandbox for discovery), Phase 4 (shadow) |
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
}

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
}
```

Every method is asynchronous. The block above leaves that out to stay readable: in the code each takes `&'a self` and returns a `BoxFuture<'a, ...>` of the type shown, so the trait is a trait object and needs no macro, and an implementation writes `Box::pin(async move { ... })`.

**A call that never answers.** The engine gives any call to `Control` up after `EngineSettings::control_timeout` (two minutes unless set) and goes on as it would after any failure of that call, with `busy`, `control_unanswered`: an intent that was not acknowledged sends nothing, and a write whose `acted` mark was not acknowledged is `possible_commit`. The future of the call is dropped, so an implementation must make what a call had begun stand or fall as one transaction. An implementation that waits, for a lock, a lease, or a rate budget, keeps its own shorter limit.

`verified` borrows the proof that the record is `acted`, and the store runs its transaction on a thread of its own, which takes only what it can own. An implementation over the store therefore cannot hand the proof to `mark_verified`; it settles the record by name with `settle(fence, record, Reconciled::Landed(read_back))`, under the lease the proof carries, where the ledger makes the same comparison. Both implementations that exist do this. Whether `verified` should take the proof by value instead is an open point below.

`GuardQuestion` is the engine's own type: the run, the tool, the effect class, the hazard, the binding chosen. `a2m-actions` turns it into a policy request; the engine never sees a policy type. A record lock is released by the ledger itself when the record reaches `verified` or `unchanged`, in the same transaction, and quarantined when the run stops with the record in doubt, so `Control` has no unlock. What used to be asked for and is not on the list arrives as data in `JobSpec`, filled by the runner when it claims the job: the input, the run snapshot, the conditions of choosing, and the lease's clock and directive.

Assumptions about other crates' types: `IntentToken` can be made only by `a2m-ledger`, and carries the run, the record key hash, and the lease number under which the intent was stored; over the link it is rebuilt from the control plane's acknowledgement by a constructor `a2m-ledger` provides for that purpose. `RecordLock` is `a2m-ledger`'s proof that a lock is held. `SessionLease` comes from `a2m-auth` and can inject credentials into a driver's context without returning them. `Plan` and `Step` are data in `a2m-core`; a step has a kind, a reference, a time limit, a failure class, compiled expressions for its inputs, and flags `commits` and `lookup`.

## The loop

Every operation call runs these parts. A read skips intend and confirm, and runs read back only when its binding declares one. A write repeats intend to record for each commit point.

| Part | What the engine does | Asks `Control` for | Stops with |
| --- | --- | --- | --- |
| 1. Guard | Checks the plan's hash; the kernel's health from `conditions` (a write needs `ok`); the decision for this call; the surface has no edge block | `plan`, `decide` | `policy`, `plan_unverified`; `busy`, `engine_degraded`; `policy` from the decision; `edge_blocked` |
| 2. Choose | Takes the plan's bindings in their declared order and picks the first that qualifies (below). Sends the binding and the reason, with the provider choice from the handle | `send` | The class of the best reason any candidate failed: `session_expired` before `edge_blocked` before `drift` before `busy` |
| 3. Prepare | Gets the session lease, then the driver's prepared context | `session_lease` | `session_expired`, `needs_login`; whatever `prepare` returns |
| 4. Intend | Takes the record lock; makes the freshness check; checks the lease leaves time for the step; stores the intent, getting a token back | `lock`, `intend` | `busy`, `record_locked`; `stale`, `changed_since_preview`; `LeaseEnding` |
| 5. Act | Evaluates each act step's inputs, spends the rate grant, calls the handle. Outputs a step returns `for_reconcile` are sent, and acknowledged, before the committing step is sent. A `commits` step goes with the token; its outcome, and any assigned key, are sent as `acted` at once | `rate`, `send` | The driver's class; a doubtful `commits` step is `possible_commit` |
| 6. Confirm | Runs the confirm steps the same way | None | `possible_commit`, `no_confirmation` |
| 7. Read back | Runs the read-back steps, or uses the values the save response carried where the binding says `prefer: response`. Normalizes and compares | None | `mismatch`, `read_back_differs`, after one re-read |
| 8. Record | Sends the commit point as `verified` with raw and normalized values and its evidence; the ledger releases the lock in the same transaction. For a read, sends the result | `evidence`, `send` | `busy`, `storage_full` if evidence cannot be stored, which leaves the record unverified |

**Choosing among verified bindings.** A binding qualifies when its status is `verified` (in the sandbox, any status but `retired`); the registry has a driver for its mode whose `health` is `Ready`; its surface has not drifted; and its session state is valid or the surface needs none. Drift, edge blocks, and session states are read from `JobSpec::conditions`; a session that turned out to be invalid is learned at prepare, when the lease is refused. A pinned mode removes every other binding, compared by name. The default order, when an operation states none, is set by the compiler from each mode's `rank` in its manifest, so the cheapest-first rule of section 201 lives in data and the engine never knows which mode is cheap. The engine only walks the plan's order.

**Falling back.** A read falls to the next qualifying binding after `drift`, an exhausted `transient`, or a driver that becomes unavailable. A write does not fall back inside a job: a record that fails stops the run there. On resume, a record in doubt is read through the mode its intent was stored for, and if that mode's binding may not act the job stops with `busy`, `no_binding`; only a record that reconcile returned to `planned`, because nothing landed, may be written through another binding. A record is never carried by two modes.

**Retrying.** `transient` is retried up to two times, only for a step that commits nothing, or for a `commits` step that was `NotSent`, or that the plan marks idempotent because the application takes the idempotency token. A retry starts the step again from evaluation; the driver never sees a retry as anything but a new step. No other class is retried here. `session_expired` is handed back to the keeper by asking `session_lease` once more; a second refusal rests the run at `needs_login`.

**Waiting.** A rate grant that names a time and a lock that is held are waited for behind `Control`, which keeps its own limit. A lookup's settle time is the engine's: during reconcile, a lookup that finds nothing waits the binding's `settle_ms` out once and looks again, and only then is the write taken as not landed.

**The lease and cancellation.** Before every commit point the engine checks the lease's remaining time in `JobSpec::lease` against the step's time limit, and reads the latest directive there; the runner updates both from its heartbeats. If the lease is ending it returns `LeaseEnding` without sending. On `cancel` it finishes the commit point it is in, records it, and stops; a read stops at once. A `commits` step's future is never dropped, so its time limit is the driver's to keep: a driver that never returns from a save holds the job until its lease is lost, and the run is then reconciled as section 208 says.

**A provider's code that fails the engine.** A step that commits nothing runs under its time limit; when the limit passes, its future is dropped and the step fails with the class and code the plan gives it. Preparing a driver runs under `prepare_timeout`. A panic in a driver or a discoverer is caught where the engine calls it: a step that commits nothing fails with `unknown_variant`, `driver_panicked`; a step that commits fails with `possible_commit`, `no_confirmation`, because nothing says whether the write left; the driver context is marked as not to be used again.

## Normalization and comparison

- The plan holds each field type's normalizer as compiled host-function calls. The engine evaluates them through `a2m-expr` on both sides: the intended value and the value read back.
- Values are compared field by field in the record's declared order, as `Value`. There is no float anywhere in a comparison; a decimal compares exactly after its type's rounding rule.
- A field labelled restricted is compared, and stored, as a hash of its normalized value.
- A comparison returns `Equal` or `Differs` with the names of the differing fields. Values go to evidence, never into the failure's message.
- `Absent` compares equal only to an absent `before`. A record that is absent after a write is a `mismatch`.

## The preview, the diff, and the hash

A `Preview` job runs the order of section 208: read the source through the registry's `SourceReader` once, apply limits, run the leading read steps with their `expect`, then take the write steps in order: work out each step's records, look each record up by its identity strategy, class it `create`, `update`, or `unchanged`, and, when every record of the step exists unchanged, give the step's result to the steps after it. Then build the diff, and hash it by the canonical form in the reference. The engine computes the hash; nothing else does, so that an approval and a run can never disagree about it. A diff with nothing to create or update and no other effect ends `completed` with no approval. Otherwise the run rests at `needs_approval`.

An `Execute` job for a write first calls `approval`, which consumes the approval bound to the diff hash and moves the run to `running` in one transaction. It then **runs the leading read steps again and stops if an `expect` that held at preview fails now.** That settles an open point of section 208 in favor of the safe answer. Each record is then written by the path above, with the freshness check against the values the preview stored.

## Reconcile and resume

A `Reconcile` job, and the start of any resumed `Execute` job, read `JobSpec::snapshot` and take each record in `intent`, `acted`, or a retryable `failed`, in key order.

| Identity strategy | How the record is read |
| --- | --- |
| `natural`, `composite` | The binding's read-back steps alone, from the identity inputs |
| `stamped` | The binding's lookup steps, which are handed the record's token as the step's stamp. At the first preview no record can carry the token, so the record is a `create` without a lookup |
| `find` | The binding's lookup steps, from the inputs. The pack's lookup step sees the matches: more than one is its failure `possible_commit`, `ambiguous_match`, which the engine carries; the engine never picks one. Zero matches during reconcile waits the settle time out once and looks again |
| Any, once a key was assigned | The read-back steps, which are handed the assigned key as the step's key and find everything the record's steps observed under `result` |
| Any, with outputs recorded before the commit point | The read-back steps, which find the recorded values under `result` |
| `none` | Cannot be read. It is a `create` in every diff; in doubt, the run stops with `possible_commit`, `cannot_reconcile` for a person |

What a lookup finds is the read: its values are compared, and a key it reports is kept as the assigned key. A binding of a `stamped` or `find` operation that has no lookup steps is read by its read-back steps.

Then the table of section 208: equal to the intended values, mark `verified` and write nothing; equal to the values seen before, return to `planned`; neither, stop with `stale`, `changed_since_preview`. Only after every doubtful record is settled does any write run. A dependent record takes its parent's outputs from the snapshot and never runs the parent again. The ledger holds a record's outputs from its `acted` mark only: a record found `unchanged`, or one that reconcile found had landed, has none, so on resume it is read once more, by its identity strategy, when its operation declares outputs.

A `VerifyAfterLoss` job reads every record the ledger says was verified in the lost run, later, and reports any difference as evidence and an alert; it changes no state.

## Failure classes and run states

Which state each class ends in, as built and recorded in [decision 0091](../../decisions/0091-the-engine-loop-as-its-first-two-passes-settled-it.md).

| Class | Run state | Lock on a record in `intent` or `acted` |
| --- | --- | --- |
| `input` | `rejected` while the run is `validating`; `failed` once it has run | None is held |
| `policy`, `stale` | `failed` | Released by the ledger; nothing is in doubt once read |
| `session_expired` | `needs_login` | Quarantined |
| `transient` after retries, `busy`, `mismatch`, `edge_blocked`, `unknown_variant`, `rejected` | `stopped` | Quarantined if a record is in doubt, released otherwise |
| `drift` | `stopped`, and pinned to the plan that drifted | As above |
| `possible_commit` | `stopped` | Quarantined until reconcile |

A run stopped by the engine's own death is resumed by its caller with `run.resume`, never by the engine alone at its next start.

## Pipelines

A pipeline is a list of steps in the plan. The engine runs its steps in the plan's order, each once or once per row of the source. The leading read steps run at the preview and again before the first write; the write steps' records are written one after another in the order of the steps; the read steps after the first write step run once every record is verified or unchanged. A record that fails stops the run there.

**The source.** The bytes of the source arrive in `JobSpec::source`. The engine reads them once through `a2m-sources`, with the built-in readers or the registry's, before any surface is touched: a source that is rejected ends the run `rejected` with every problem, and a source with more rows than the pipeline's `max_records` ends it with `policy`, `record_limit_exceeded`. A source whose reader is `operation` is not carried out yet and stops with `busy`, `job_kind_not_built`. The pipeline's `max_duration_ms` is measured on the engine's clock from the start of each job; past it the run stops before its next operation call with `busy`, `run_time_exceeded`.

**Dependent records** ([decision 0092](../../decisions/0092-a-record-that-waits-for-an-earlier-record-is-planned-under-its-place-in-the-run.md)). A write step's result is there for the steps after it once every one of its records is verified or unchanged. A `with` expression that reads `steps` and cannot be worked out while an earlier record is still to be written marks its input as waiting. A waiting input must be part of the record's key; a value that is written must be known at the preview, and one that is not is refused with `input`, `input_invalid`. A record with a waiting input is a `create` in the diff, is not looked up, and is planned under its place in the run: the operation's name, the step's position, and the record's position. When the write path reaches it, its inputs are worked out from the earlier record's outputs and it is written as any record. When a record fails, the records still waiting for it are marked `failed` with `busy`, `blocked_by_parent`; on resume each goes back to `planned` without a read, because nothing was ever sent for it.

Records are written one at a time: `JobSpec::parallelism` and the intent window are kept and not used yet.

## The sandbox

The sandbox is private to this crate. It runs a draft plan for a `Try` job, which `a2m-actions` submits for `draft.try`, and it is the `SandboxPort` each discoverer receives in a `Discover` job, which `a2m-actions` submits for `discover.run` and the scout. Discovery is a job like any other, so it runs on the runner that holds the surface's session and under a lease.

- It accepts draft bindings, and records its runs as tries, so no binding's health or shadow count changes.
- It prepares every driver with a posture other than `Live`, so the driver's own read-only guard is on.
- It never asks for an intent unless the record's key is one of the target's declared test records. Without a token the handle refuses every `commits` step unsent. A discovery job declares no test records, so through its port a write is impossible.
- Each discovery is bounded in time, size, and depth, and the bounds hit are reported. The engine holds a discoverer to the seconds its manifest declares: past them its future is dropped, which is safe because every step it can ask for only reads, and its report is the failure `busy`, `run_time_exceeded`. The findings it made before stand.
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
| `input` | `job_invalid` | The job names a tool, an environment, or a surface the plan does not hold, or is of a kind the tool cannot run as |
| `input` | `input_invalid` | An input is missing, is not declared, or is not of its type |
| `busy` | `engine_degraded`, `no_binding` | A write on a degraded kernel; no binding qualifies for a reason that is not session, edge, or drift |
| `busy` | `run_time_exceeded` | The run passed the longest time its pipeline allows |
| `busy` | `blocked_by_parent` | A record waits on a parent record that failed |
| `busy` | `evidence_not_stored` | Evidence could not be stored, so nothing can be marked verified |
| `busy` | `no_rate_budget` | No rate budget was granted for a surface |
| `busy` | `job_kind_not_built` | The job asks for something this engine does not carry out yet: a binding that saves more than once in one call, a source whose reader is `operation` |
| `busy` | `control_unanswered` | A call to `Control` was not answered within the time the engine waits |
| `unknown_variant` | `driver_panicked` | A provider's code panicked in a step that commits nothing, in preparing, or in discovery |
| `transient` | `prepare_timed_out` | A driver was not ready within the time preparing may take |
| `session_expired` | `needs_login` | The session ended and could not be had again |
| `edge_blocked` | `edge_blocked` | The surface is blocked at the edge |
| `drift` | `binding_drifted` | Every binding that could carry the call out has drifted |
| `stale` | `changed_since_preview` | A record no longer holds what the preview saw |
| `mismatch` | `read_back_differs` | The value read back differs from the value intended |
| `unknown_variant` | `expect_failed` | An `expect` does not hold: a leading read's before the first write, or a step's |
| `unknown_variant` | `field_not_read_back` | Reading a record gave no value for a field the operation writes |
| `unknown_variant` | `no_read_back` | A binding has no steps that read the record, so nothing it writes can be verified |
| `unknown_variant` | `output_unexpected` | A step returned a value the plan gives no place to |
| `unknown_variant` | `commit_skipped` | A binding ended without sending its write |
| `possible_commit` | `no_confirmation` | A write was sent and the system's answer is not known, or could not be recorded, or the driver panicked in the save |
| `possible_commit` | `cannot_reconcile` | A record in doubt cannot be read, so a person must settle it |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| No `commits` step reaches a driver before its intent is durable (ledger rule 1) | `tests/write.rs`: the order of lock, intent, save, `acted`, read-back, `verified` in one log; an intent that cannot be stored, or is never acknowledged, sends nothing (`tests/faults.rs`) |
| A record is `verified` only with a read value equal after normalization, and its evidence (rule 2) | `tests/write.rs`: a near value is a `mismatch`; `tests/faults.rs`: a record whose evidence cannot be stored stays unverified |
| A write enters `running` only through `approval` (rule 3) | `tests/write.rs`: no approval waits, an approval of another diff does not let the run write |
| Outputs a step returns before the commit point are acknowledged before the committing step is sent | `tests/identity.rs`: the order in one log; `tests/faults.rs`: when they cannot be stored the save is not sent |
| The engine depends on neither `a2m-files` nor `a2m-policy` | The audit |
| After any doubt, no record of the run is written before it is read (rule 5) | `tests/write.rs` and `tests/identity.rs`: a lost answer, by each identity strategy; the test application shows no duplicate |
| A record is never written by two modes | `tests/faults.rs`: two bindings, the first fails after sending, and the second never sees a step, not even a read |
| A doubtful write is never retried | `tests/write.rs`: a lost answer is sent once |
| The sandbox cannot write outside declared test records | `tests/sandbox.rs`: a planted `commits` step is refused by the port; a try of a record that is not a test record is refused before a driver is prepared; the test stand shows nothing stored |
| The engine names no mode | The audit's layering rule, and `tests/faults.rs`: the crate's source holds no mode name any first-party manifest registers |
| Comparison never uses a float | The `Value` type, and `tests/write.rs`: a rounded decimal |
| Cancel never abandons a commit point halfway | `tests/faults.rs`: cancel arrives during a save, which is finished and recorded before the run ends |
| A call to `Control` that never answers, and a provider that panics or hangs, end the step and not the engine | `tests/faults.rs` |
| No input changes where a request goes, which record is written, or which step runs; a value from the system stays a value | `tests/manipulation.rs` |
| Every failure code is in this document and in the technical reference under its class | `tests/faults.rs` |

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
10. Done: package E1: the sandbox as `SandboxPort`, `discover` for `Discover` jobs, `Try` jobs of reads and of writes to declared test records, and `VerifyAfterLoss` jobs (`tests/sandbox.rs`). Not built: Phase 4, shadow, where a binding predicts without acting; a canary beyond a read that refuses a tool that writes.

## Open points

- Whether `Control::verified` should take the proof of `acted` by value, as `acted` takes the intent token, so that an implementation over the store can call `mark_verified` itself and need not go through `settle`. The trait is held as it is while `a2m-actions` is built against it.
- Whether `Reconciled::Landed` should carry the record's outputs and its assigned key, so that the ledger holds them for a record reconcile verified. Until it does, the engine reads such a record once more on resume.
- A record that waits for an earlier record is locked under its place in the run, not under the key it gets once its parent exists. Two runs that create lines under the same new header cannot happen, because the header is new to each; a line added by another run to a header that already exists is locked under its real key. Whether the lock should move to the real key once it is known is open.
- A `commits` step that never returns holds its job until the lease is lost. Dropping its future after a grace time would free the job and leave the record `possible_commit`, which reconcile handles; the driver contract forbids the drop today.
- Whether `Diff` and its canonical form should move to `a2m-core` so that the approval surface can render one without the engine.
- How a shadow binding predicts: a posture in which the handle refuses every step that reaches the system and the driver returns what it would have sent.
- Whether the conditions of choosing, read once at claim, go stale during a long pipeline; if so, `Control` needs a way to read them again, which is not on its list today.
- The intent window across bindings in one pipeline step, when records of two bindings interleave. Records are written one at a time today.
- Whether a canary is an `Execute` job of a read with a flag, or a kind of its own as the jobs table has it.
- A read step that stands between two write steps runs after every write, not in its place. A write step that needs such a read's result cannot be previewed, and the compiler should refuse it (an open point of section 208).
