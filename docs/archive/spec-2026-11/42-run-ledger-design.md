# 42. Run ledger design

> Archived on 2026-11-17. This is section 42 of the first design, kept as it was written, with the names and numbers of its time. What it says now lives in [section 208, Runs, the ledger, and resume](../../spec/2-kernel/208-runs-the-ledger-and-resume.md).

The ledger is the part of Anything2MCP that makes a write safe to stop, resume, and audit. This section turns [section 10](10-runs-ledger-and-resume.md) into a concrete design: what is stored, the states each thing moves through, the rules that must always hold, and the exact resume procedure. The table definitions are in the [ledger schema reference](../../reference/ledger-schema.md).

## The problem it solves

No engine can make a write to someone else's system happen exactly once. Durable-execution systems say so themselves: the step that touches the outside world may run more than once, so the effect has to be given an identity before it runs and that identity reused on every retry ([research note 13](../../research/13-ledger-leases-and-runner-links.md)). Payment APIs solve this by accepting an idempotency key. The applications Anything2MCP wraps usually accept nothing of the kind.

Anything2MCP therefore builds the same guarantee from three parts it controls:

1. **Identity before action.** Each write is tied to a record key and recorded as an intent before anything is sent.
2. **Read instead of trust.** After a stop, the engine reads the record back from the application to learn whether the write landed.
3. **Keyed upserts.** Writing the same values to the same key twice leaves the same result.

## What is stored

| Thing | One row per | Purpose |
| --- | --- | --- |
| Run | Call of an operation or pipeline | The unit a caller sees and holds a `run_id` for |
| Step | Pipeline step within a run | Order, chosen binding, and timing |
| Record | **Commit point in the application** | The ledger proper: what was intended, what was seen, how far it got |
| Event | Thing that happened | Append-only history; the source for traces, streams, and webhooks |
| Approval | Decision by a person | Binds a person to one exact diff |
| Record lock | Record being written | Keeps two runs off the same record |
| Job | Piece of a run handed to a runner | Scheduling and leases; see [section 43](../../spec/2-kernel/223-the-runner-protocol.md) |
| Blob | Piece of evidence | Values, snapshots, and traces, stored once by content hash |
| Outbox entry | Message owed to the outside | Webhooks and notifications that must not be lost |

**One record row is one commit point.** A commit point is the moment the application itself saves: one Save click, one PUT request. If an operation saves twice, for example on two tabs of a form, it has two rows. This rule is what makes resume exact, because every row is either saved in the application or not.

**Events and current state are written together.** Every change appends an event and updates the current-state row in the same database transaction. The current state can always be rebuilt from the events, and a test does exactly that.

## Run states

| State | Meaning | Leaves by |
| --- | --- | --- |
| `queued` | Accepted, waiting for a session or runner | Scheduler |
| `validating` | Checking inputs and source; reading current values; building the diff | Engine |
| `rejected` | Inputs invalid; nothing was changed | Final |
| `needs_approval` | Diff ready; waiting for a person | Approval, expiry, or cancel |
| `expired` | Approval declined or timed out | Final |
| `running` | Steps executing | Engine |
| `needs_input` | A declared decision point is waiting for a person | Input or cancel |
| `needs_login` | The session expired and could not renew itself | Sign-in |
| `stopped` | Halted with a failure class; can be resumed | Resume or cancel |
| `completed` | Every record verified or unchanged | Final |
| `failed` | Halted and cannot be resumed | Final |
| `cancelled` | Stopped at a person's or caller's request | Final |

The `status` a caller sees is this state. Reads go from `validating` straight to `running`. Writes cannot reach `running` without an approval, unless policy pre-approved them. Resume does not create a new run: the same run gets a new attempt number and a new lease.

## Record states

| State | Meaning | On resume |
| --- | --- | --- |
| `planned` | Known from the source; nothing done | Proceed |
| `unchanged` | Already equals the intended values; no write needed | Skip |
| `intent` | Intended values recorded; about to act | Reconcile |
| `acted` | Write sent; no confirmation recorded yet | Reconcile |
| `verified` | Read back and matched | Skip |
| `failed` | Stopped with a failure class | Reconcile, then retry if the class allows |

Each row holds the record key, a hash of the intended values, the values seen before the write, the values seen after, the binding used, the attempt count, and links to evidence. The values seen before the write are what make undo possible ([section 32](32-components-added-by-the-gap-review.md)).

## Rules that must always hold

These are written as tests, and a change that breaks one cannot merge.

1. **Intent before action.** A record reaches `acted` only if its `intent` was durably stored before the write was sent.
2. **Proof before verified.** A record reaches `verified` only with a value read from the application that equals the intended value after normalization, and a link to that evidence.
3. **Approval bound to the diff.** A write run first enters `running` only with an approval whose diff hash (defined in [steps, snapshots, and diffs](../../reference/steps-snapshots-and-diffs.md)) equals the run's, that is unexpired and unused. Marking it used and changing the run's state happen in one transaction. A resumed run is the same run: it continues under the approval it has already used and writes only what that diff holds, with rule 5 and the freshness check guarding each record.
4. **One writer per run.** Only the holder of the run's current lease number may change its records. Anything carrying an older number is refused.
5. **Reconcile after any doubt.** After a lease is lost or an engine stops, no record of that run is written until it has been read back.
6. **One run per identical call.** At most one unfinished run exists for the same caller, tool version, and input hash. A second identical call returns the first run. For a file input the hash covers the file's content, not its path.
7. **One lock per record.** At most one run holds the lock for a record key in a target and environment.
8. **Final means final.** A run in a final state never changes again.
9. **History is append-only.** Events are never updated or deleted, except by the retention job, which removes whole runs.

## The preview, step by step

A write run builds its diff while it is `validating`, before anyone is asked to approve. Sections 18, 45, and 50 each describe a part of this; here is the order.

1. Validate the inputs and read the whole source ([section 12](../../spec/2-kernel/210-sources-and-value-normalization.md)). A file is read once and stored as a blob ([section 51](../../spec/5-security/502-defending-against-a-manipulated-agent.md)).
2. Apply the record limit and quotas ([section 45](45-policy-engine.md)).
3. Run the read steps that come before the first write step, checking each `expect`. A failure here ends the run before anyone is asked to approve. Read steps after the first write step run only in the write phase.
4. For each record the write steps would touch: evaluate its inputs, look the record up by its identity strategy, and class it `create`, `update`, or `unchanged` ([section 50](../../spec/2-kernel/209-record-identity-and-dependent-records.md)). For `natural` and `composite` identity the lookup is the read-back rule of the binding that will write the record, used on its own. The values found are kept as the values seen before the write.
5. Build the diff, hash it, and store both on the run.
6. If the diff holds nothing to create or update and the tool has no other effect, such as an upload, no approval is needed: the run writes nothing, runs its remaining read steps, and ends `completed`. This is what makes a second run of the same sheet change nothing. Otherwise the policy engine is asked whether the write may proceed without a person, and the run waits at `needs_approval` if not.

## The write path, step by step

For each record in a write run:

1. Take the record lock.
2. If the preview is older than the freshness window (60 seconds by default), or policy is strict, read the record again and compare it with what the preview saw. A difference stops the run with the class `stale` and the code `changed_since_preview`, because the approved diff no longer describes what would happen.
3. Store the intent. In a single process this is one synchronous database commit; on a split deployment the runner waits for the control plane's acknowledgement.
4. Act: one commit point in the application.
5. Mark `acted`.
6. Read back, normalize, compare. Mark `verified`, or stop with `mismatch`.
7. Release the lock.

**Intent window.** Waiting for an acknowledgement per record costs a round trip. A runner may therefore store intents for several upcoming records at once: 8 for HTTP bindings and 1 for UI bindings by default. If it stops, at most that many records are in `intent` and need a read on resume. Nothing is lost, because an intent is only a claim that a write may have been sent.

**Idempotency keys where the application takes them.** Each record carries a stable token derived from the run and the record key. When an endpoint documents an idempotency header, the binding sends the token, so the application itself refuses a duplicate.

**Version checks where the application offers them.** If the application exposes a record version, such as an `ETag` or a version field, the binding sends it as a precondition. A change by someone else then fails cleanly as `version_conflict` instead of being overwritten.

## Resume, exactly

When a run is resumed, for each record in `intent`, `acted`, or a retryable `failed`, in key order:

1. Take or confirm the record lock under the new lease.
2. Read the record's current values by its key.
3. Compare, after normalization:

| Current values equal | Meaning | Action |
| --- | --- | --- |
| The intended values | The write landed | Mark `verified`; write nothing |
| The values seen before the write | The write did not land | Return the record to `planned` and continue |
| Neither | Someone else changed the record, or the application applied part of the write | Stop with `changed_since_preview`; a new preview and approval are needed for that record |

Records in `planned` then continue through the normal write path. Records in `verified` or `unchanged` are skipped.

**Records with no way to be found again cannot be read back,** so they cannot be reconciled. [Section 50](../../spec/2-kernel/209-record-identity-and-dependent-records.md) gives the ways a record can be found when the application assigns its identifier, and how later records use an earlier record's result. An operation that creates them is classed `irreversible`, is approved per run, and is never resumed automatically, as section 10 already states.

## Reads

A read has a run row and events but no record rows. Reads are recorded for audit without forcing a disk sync on every call, so the ledger adds well under a millisecond to a read. Writes always sync at intent.

## Locks

- A lock names a workspace, target, environment, and record key.
- It is taken just before the intent and released after verification, so a long pipeline holds one lock at a time, not all of them.
- A second run that needs a locked record waits in the queue up to a limit, then returns `record_locked` with a retry time.
- When a lease is lost, the locks its run held are quarantined, not released. They pass to the reconcile job, so no other run can write those records before they have been read back.

## Evidence and retention

- Values, diffs, snapshots, and traces are stored as blobs named by their content hash, encrypted, and referenced from rows. Identical evidence is stored once.
- Each blob has a retention class: short for traces and screenshots, longer for diffs and before-and-after values, longest for the audit log.
- A retention job deletes whole runs and their unreferenced blobs once they are past their class. Legal hold on a run suspends it.
- Values classified as sensitive ([section 32](32-components-added-by-the-gap-review.md)) are stored only as hashes unless policy allows the values themselves, which limits undo for those fields.

## One schema, two databases

| Concern | SQLite (local profiles) | Postgres (servers) |
| --- | --- | --- |
| Claiming a job | One writer at a time; an immediate transaction | An atomic update that selects with `FOR UPDATE SKIP LOCKED`, the standard pattern for database queues |
| Waking a waiting worker | An in-process signal | `LISTEN` and `NOTIFY`, with polling as the fallback |
| Clock | The process's clock | The database clock, so replicas agree |
| Structured columns | Text holding JSON | `jsonb` |
| Blobs | Files beside the database | Object storage, or the database for small ones |
| Durability of an intent | Synchronous commit in write-ahead mode | Synchronous commit |

Migrations are numbered, forward-only, and run against both databases in CI. Identifiers are time-ordered UUIDs, shown to callers with a short type prefix such as `r_` for runs.

## What this does not solve

- **A write the application applied partly.** The third row of the resume table detects it and stops; a person decides.
- **A stale runner writing after its lease was lost.** The ledger refuses its updates, but the application cannot. Section 43 describes how the window is narrowed and what remains.
- **A change made by someone else between read-back and the end of the run.** The run verified what it wrote at the time it wrote it; it does not own the record afterwards.

## Speed targets

Storing an intent: under 2 ms locally and under 5 ms to a control plane on the same network. Ledger overhead on a read: under 1 ms. Both are targets until measured in Phase 3.

## Open points

Found by walking the sheet-entry pipeline through this section on 2026-11-16. Each is to be settled before slices 3.5 and 3.7, with the owner where the answer changes what a caller sees.

- **Leading read steps in the write phase.** Approval can take minutes. Whether the read steps that ran in the preview run again before the first write, and what happens when an `expect` that held then fails now, is not stated. The safe answer is to run them again and stop.
- **What a write step's inputs may depend on.** A proposed value has to be known at the preview to be approved. A write step whose `with` uses the result of a read step that runs after an earlier write cannot be previewed. The compiler should refuse it, and no rule says so yet. An earlier write step's assigned key is the one case section 50 already covers.
- **Which run state each failure class leads to.** `stopped` can be resumed and `failed` cannot, but no table says which class ends where. A run stopped as `stale` cannot succeed by resuming, since its approved diff no longer describes the application, yet while it stays unfinished rule 6 hands it back to an identical call. A run stopped as `drift` is pinned to the plan that drifted.
- **Locks when a run stops.** Only a lost lease is covered. A stopped or cancelled run with a record in `intent` or `acted` still has a record in doubt; whether its lock is held, quarantined, or released is not stated.
- **Who resumes after the engine dies.** The caller, with `run.resume`, or the engine by itself at its next start. The safe answer is the caller.
