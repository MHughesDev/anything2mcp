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
| Carries out | Spec sections [8](../../spec/08-execution-engine.md), [6](../../spec/06-modes-and-drivers.md) for choosing, [10](../../spec/10-runs-ledger-and-resume.md) and [42](../../spec/42-run-ledger-design.md) for the write path and resume, [50](../../spec/50-record-identity-and-dependent-records.md), [52](../../spec/52-when-the-engine-itself-fails.md), [54](../../spec/54-interaction-archetypes.md), [26](../../spec/26-agent-toolkit-how-tools-get-built.md) and [58](../../spec/58-discovery-and-findings.md) for the sandbox; the [diffs](../../reference/steps-snapshots-and-diffs.md#diffs) |

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
pub struct Engine { /* registry snapshot, settings, telemetry, redact, plan cache */ }

pub struct EngineDeps {
    pub registry: Arc<Registry>,               // from `a2m-provider`
    pub redact: Arc<dyn Redact>,               // from `a2m-core`, implemented by `a2m-vault`
    pub telemetry: Telemetry,                  // from `a2m-telemetry`
    pub settings: EngineSettings,              // freshness window, retry counts, evidence level
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
    pub kind: JobKind,
    pub plan: Digest,                          // the plan's hash; the plan is fetched through Control and checked
    pub tool: ToolRef,                         // operation or pipeline in the plan; for Discover, the surface
    pub input: Values,                         // the stored input, fetched once by the runner
    pub snapshot: Option<RunSnapshot>,         // on resume: each record's state, outputs, assigned key, values before
    pub conditions: Conditions,                // at claim: the kernel's health, drifted bindings, edge blocks, session states
    pub pin: Option<Mode>,                     // a caller or setting pinned one mode; compared by name only
    pub parallelism: Parallelism,              // records at once inside this job, granted by the scheduler
    pub lease: LeaseClock,                     // the lease's remaining time on a steady clock, and the latest directive
}
pub enum JobKind { Preview, Execute, Try, Reconcile, VerifyAfterLoss, Canary, Discover }
pub enum JobOutcome {
    Finished(RunState),                        // completed, failed, rejected, cancelled
    Rests(RunState),                           // needs_approval, needs_input, needs_login
    Stopped(Failure),                          // resumable
    LeaseEnding,                               // stopped acting before the lease ran out; nothing was left half sent
}
```

### `Control`

Everything the engine needs from the control plane while it runs a job, and nothing more. In one process `a2m-actions` implements it by calling the scheduler, the keeper, policy, and the store directly; on a runner `a2m-runner` implements it as `need` and `events` calls over the link. Every call carries the job's lease number, attached by the implementation, so the engine can neither forge nor forget it. The same test suite runs against both implementations.

```rust
#[async_trait]
pub trait Control: Send + Sync {
    /// The plan by its hash. The engine checks the hash again before using it.
    async fn plan(&self, hash: &Digest) -> Result<Arc<Plan>, Failure>;
    /// The policy decision at guard, made and recorded by `a2m-policy` behind the implementation.
    async fn decide(&self, q: &GuardQuestion) -> Result<(), Failure>;
    /// A session lease. The keeper renews behind it if it must; a refusal is `session_expired` or `needs_login`.
    async fn session_lease(&self, want: &SessionWant) -> Result<SessionLease, Failure>;
    /// A record lock, from the ledger, waiting through the scheduler up to its limit; then `record_locked`.
    async fn lock(&self, key: &LockKey) -> Result<RecordLock, Failure>;
    /// A rate budget to spend locally, or the time at which one will be possible.
    async fn rate(&self, surface: &SurfaceRef, want: u32) -> Result<RateGrant, Failure>;
    /// The approval check: uses the approval bound to `diff` and moves the run to `running`, in one transaction.
    async fn approval(&self, run: &RunId, diff: &Digest) -> Result<(), Failure>;
    /// Record intents and get their tokens. Durable before it returns.
    async fn intend(&self, intents: &[Intent]) -> Result<Vec<IntentToken>, Failure>;
    /// Events: ledger transitions, outputs known before a commit point, the step trace, binding health.
    /// Each says whether it must be acknowledged before the engine goes on.
    async fn send(&self, ev: EngineEvent) -> Result<(), Failure>;
    /// Evidence, already redacted.
    async fn evidence(&self, blob: Evidence) -> Result<BlobRef, Failure>;
}
```

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

**Choosing among verified bindings.** A binding qualifies when its status is `verified` (in the sandbox, any status but `retired`); the registry has a driver for its mode whose `health` is `Ready`; its surface has not drifted; and its session state is valid or the surface needs none. Drift, edge blocks, and session states are read from `JobSpec::conditions`; a session that turned out to be invalid is learned at prepare, when the lease is refused. A pinned mode removes every other binding, compared by name. The default order, when an operation states none, is set by the compiler from each mode's `rank` in its manifest, so the cheapest-first rule of section 6 lives in data and the engine never knows which mode is cheap. The engine only walks the plan's order.

**Falling back.** A read falls to the next qualifying binding after `drift`, an exhausted `transient`, or a driver that becomes unavailable. A write falls back only if nothing was sent for the current record (`Sent::NotSent` on every `commits` step so far), or after reconcile has shown that nothing landed. A record is never carried by two modes.

**Retrying.** `transient` is retried up to two times, only for a step that commits nothing, or for a `commits` step that was `NotSent`, or that the plan marks idempotent because the application takes the idempotency token. A retry starts the step again from evaluation; the driver never sees a retry as anything but a new step. No other class is retried here. `session_expired` is handed back to the keeper by asking `session_lease` once more; a second refusal rests the run at `needs_login`.

**Waiting.** A rate grant that names a time, a lock that is held, and a lookup's settle time are named conditions with deadlines. The engine waits on them with timers bounded by the step's limit and the lease, never with a fixed sleep.

**The lease and cancellation.** Before every commit point the engine checks the lease's remaining time in `JobSpec::lease` against the step's time limit, and reads the latest directive there; the runner updates both from its heartbeats. If the lease is ending it returns `LeaseEnding` without sending. On `cancel` it finishes the commit point it is in, records it, and stops; a read stops at once. A `commits` step's future is never dropped.

## Normalization and comparison

- The plan holds each field type's normalizer as compiled host-function calls. The engine evaluates them through `a2m-expr` on both sides: the intended value and the value read back.
- Values are compared field by field in the record's declared order, as `Value`. There is no float anywhere in a comparison; a decimal compares exactly after its type's rounding rule.
- A field labelled restricted is compared, and stored, as a hash of its normalized value.
- A comparison returns `Equal` or `Differs` with the names of the differing fields. Values go to evidence, never into the failure's message.
- `Absent` compares equal only to an absent `before`. A record that is absent after a write is a `mismatch`.

## The preview, the diff, and the hash

A `Preview` job runs the order of section 42: read the source through the registry's `SourceReader` once, apply limits, run the leading read steps with their `expect`, look each record up by its identity strategy, class it `create`, `update`, or `unchanged`, build the diff, and hash it by the canonical form in the reference. The engine computes the hash; nothing else does, so that an approval and a run can never disagree about it. A diff with nothing to create or update and no other effect ends `completed` with no approval. Otherwise the run rests at `needs_approval`.

An `Execute` job for a write first calls `approval`, which consumes the approval bound to the diff hash and moves the run to `running` in one transaction. It then **runs the leading read steps again and stops if an `expect` that held at preview fails now.** That settles an open point of section 42 in favor of the safe answer. Each record is then written by the path above, with the freshness check against the values the preview stored.

## Reconcile and resume

A `Reconcile` job, and the start of any resumed `Execute` job, read `JobSpec::snapshot` and take each record in `intent`, `acted`, or a retryable `failed`, in key order.

| Identity strategy | How the record is read |
| --- | --- |
| `natural`, `composite` | The binding's read-back steps alone, from the identity inputs |
| `stamped` | The lookup by the record's token |
| `find` | The declared lookup. More than one match stops with `possible_commit`, `ambiguous_match`; zero matches inside the settle time waits |
| Any, once a key was assigned | By the assigned key in the snapshot |
| Any, with outputs recorded before the commit point | By what was recorded: a key assigned inside an open transaction, or the digest a file would have |
| `none` | Cannot be read. The run is never resumed by itself and stops for a person |

Then the table of section 42: equal to the intended values, mark `verified` and write nothing; equal to the values seen before, return to `planned`; neither, stop with `stale`, `changed_since_preview`. Only after every doubtful record is settled does any write run. A dependent record takes its parent's assigned key from the snapshot and never runs the parent again.

A `VerifyAfterLoss` job reads every record the ledger says was verified in the lost run, later, and reports any difference as evidence and an alert; it changes no state.

## Failure classes and run states

Section 42 leaves open which state each class ends in. This design proposes the following, to be recorded.

| Class | Run state | Lock on a record in `intent` or `acted` |
| --- | --- | --- |
| `input` | `rejected` | None is held |
| `policy`, `stale` | `failed` | Released by the ledger; nothing is in doubt once read |
| `session_expired` | `needs_login` | Quarantined |
| `transient` after retries, `busy`, `mismatch`, `edge_blocked`, `unknown_variant`, `rejected` | `stopped` | Quarantined if a record is in doubt, released otherwise |
| `drift` | `stopped`, and pinned to the plan that drifted | As above |
| `possible_commit` | `stopped` | Quarantined until reconcile |

A run stopped by the engine's own death is resumed by its caller with `run.resume`, never by the engine alone at its next start.

## Pipelines

A pipeline is a step graph in the plan. The engine runs its pipeline steps in the plan's order, each once or once per row; it stores each record's outputs with `send`; it marks children `blocked_by_parent` when a parent fails. Records run concurrently up to `JobSpec::parallelism`, with one intent window per binding as the plan gives it from the mode's manifest default.

## The sandbox

The sandbox is private to this crate. It runs a draft plan for a `Try` job, which `a2m-actions` submits for `draft.try`, and it is the `SandboxPort` each discoverer receives in a `Discover` job, which `a2m-actions` submits for `discover.run` and the scout. Discovery is a job like any other, so it runs on the runner that holds the surface's session and under a lease.

- It accepts draft bindings, and records its runs as tries, so no binding's health or shadow count changes.
- It prepares every driver with a posture other than `Live`, so the driver's own read-only guard is on.
- It never asks for an intent unless the record's key is one of the target's declared test records. Without a token the handle refuses every `commits` step unsent. A discovery job declares no test records, so through its port a write is impossible.
- Each discovery is bounded in time, size, and depth, and the bounds hit are reported.

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

It produces every class through the loop. Codes it defines itself:

| Class | Code | When |
| --- | --- | --- |
| `policy` | `plan_unverified` | The plan's hash does not match its reference |
| `busy` | `engine_degraded`, `no_binding` | A write on a degraded kernel; no binding qualifies for a reason that is not session, edge, or drift |
| `unknown_variant` | `expect_failed` | A leading read's `expect` no longer holds before the first write |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| No `commits` step reaches a driver before its intent is durable (ledger rule 1) | A `Control` stub that delays `intend` and a driver stub that checks |
| A record is `verified` only with a read value equal after normalization, and its evidence (rule 2) | Unit tests with a stub driver returning near values |
| A write enters `running` only through `approval` (rule 3) | A test that a stale or used approval stops the run |
| Outputs a step returns before the commit point are acknowledged before the committing step is sent | A `Control` stub that delays `send`, and a driver stub that checks |
| The engine depends on neither `a2m-files` nor `a2m-policy` | The audit |
| After any doubt, no record of the run is written before it is read (rule 5) | Slice 3.7: killed at a random step, resumed, the test application shows no duplicate |
| A record is never written by two modes | A test with two bindings whose first fails after sending |
| A doubtful write is never retried | A stub driver returning `transient` with `Sent::Unknown` |
| The sandbox cannot write outside declared test records | A draft with a planted `commits` step; the test stand shows nothing stored |
| The engine names no mode | The audit's layering rule, and a test that the crate's source holds no registered first-party mode name |
| Comparison never uses a float | The `Value` type, and a test of a rounded decimal |
| Cancel never abandons a commit point halfway | Fault test: cancel arrives during a save |

## Build order

1. Slice 1.1: a hand-written plan, guard with a stub decision, choose, prepare, act, record for one read, with an in-memory `Control` used by tests.
2. Slice 1.2: the step trace through `Control::send`, and its signature.
3. Slice 1.3: plans from the compiler, fetched by hash; the hash check at guard.
4. Slice 2.4: `session_lease` and the session states in `conditions`; `needs_login`.
5. Slice 3.3: intend, the token, confirm, read back, record, for one field.
6. Slices 3.4 and 3.5: normalization and comparison; the preview, the diff, the hash.
7. Slice 3.7: reconcile and resume; the state table above.
8. Slices 3.8 and 3.9: identity strategies, dependent records, pipelines, sources.
9. Slice 3.10: the fault rows of section 52.
10. Package E1: the sandbox as `SandboxPort`, and `discover` for `Discover` jobs. Phase 5: `Try` jobs. Phase 4: shadow, where a binding predicts without acting.

## Open points

- Whether `Diff` and its canonical form should move to `a2m-core` so that the approval surface can render one without the engine.
- How a shadow binding predicts: a posture in which the handle refuses every step that reaches the system and the driver returns what it would have sent.
- Whether the conditions of choosing, read once at claim, go stale during a long pipeline; if so, `Control` needs a way to read them again, which is not on its list today.
- The intent window across bindings in one pipeline step, when records of two bindings interleave.
- Whether a canary is an `Execute` job of a read with a flag, or a kind of its own as the jobs table has it.
