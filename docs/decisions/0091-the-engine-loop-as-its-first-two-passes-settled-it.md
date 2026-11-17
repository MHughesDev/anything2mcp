# 0091. The engine loop, as its first two passes settled it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the open point of spec section 208 on which run state each failure class ends in; what [decision 0063](0063-the-kernels-interfaces-as-the-crate-designs-settle-them.md) left to the slice that builds the engine.

## Context

The first pass built `a2m-engine` through its build-order steps 1 to 7 and wrote steps 8 to 10 without a test. The second pass wrote the tests for steps 8 to 10 first, against the built-in test application, and fixed what they exposed. This record states what the code now does where the design left a choice. The design of [`a2m-engine`](../architecture/crates/a2m-engine.md) holds the result in full.

## Decision

**The unit of work**

- One job in, one outcome out. A job is one attempt at one run under one lease, and its kind says what it may do: `Preview`, `Execute`, `Try`, `Reconcile`, `VerifyAfterLoss`, `Canary`, `Discover`. *Instead of:* one call per operation, which would put the order of a run's parts in the caller.
- The engine moves a run only to `validating` and `running`. Every other state is the job's outcome, which the caller applies. A new run is moved to `validating` before its plan is fetched. *Instead of:* moving it once the tool was found, which left a job that named an unknown tool or environment ending `rejected` from `queued`, a move the ledger refuses.
- A failure's class decides the outcome: `input` ends `rejected` while the run is `validating` and `failed` once it has run; `policy` and `stale` end `failed`; `session_expired` rests at `needs_login`; every other class stops the run, resumable. A run that waits for approval goes on waiting whatever fails.
- One record row is one commit point, and a binding saves once in one call. A binding with more than one `commits` step is refused by name. *Instead of:* several rows for one call, which needs a place in the plan to name each.
- Records are written one after another, in the order of the steps. A record that fails stops the run there. The read steps after the first write step run once every record is verified or unchanged.

**How a binding is chosen, and what the trace says of it**

- The engine walks the plan's bindings in their declared order and takes the first that qualifies: its status is `verified` (in the sandbox, anything but `retired`); the registry has a driver for its mode that reports itself ready; its surface is not blocked at the edge, has not drifted, and has a usable session or needs none; its precondition holds. A pinned mode removes every other binding, by name.
- The choice is sent as an event and written at the start of each pass of the trace: the mode, the reason (`first_in_order`, `earlier_did_not_qualify`, `fell_back`, `pinned`), the provider with its version and digest, and each binding passed over with the class and code of why. When none qualifies, the failure is the best reason: `session_expired` before `edge_blocked` before `drift` before `busy`.
- A read falls to the next qualifying binding after `drift` or a `transient` failure that outlasted its retries. A write never falls back inside a job. *Instead of:* falling back when nothing was sent, as the design had it: the driver's word that nothing was sent would then decide which mode writes a record, and the gain is one saved resume.
- A record in doubt is read, and written, through the mode its intent was stored for. When that binding may not act the job stops with `busy`, `no_binding`. Only a record that reconcile returned to `planned` may be written through another binding.

**How reconcile decides**

- Before any write of a resumed run, every record in `intent`, `acted`, or a `failed` whose class leaves doubt is taken in key order: its lock is taken or confirmed, it is read, and what was read is normalized and compared. Equal to the intended values: settled as landed, with the read as evidence. Equal to the values seen before: back to `planned`. Neither: `stale`, `changed_since_preview`. The ledger makes the transition; the engine only says what it saw.
- A record `failed` with `policy`, `stale`, or `input` is not in doubt, and ends the run with that failure. A record `failed` with `blocked_by_parent` goes back to `planned` without a read ([decision 0092](0092-a-record-that-waits-for-an-earlier-record-is-planned-under-its-place-in-the-run.md)).
- A record that cannot be found again (`none`) stops the run with `possible_commit`, `cannot_reconcile`, for a person.

**How each identity strategy is carried out**

- `natural`, `composite`: the key is the identity inputs, normalized. The record is read by the binding's read-back steps alone.
- `find`: the key is the identity inputs. The record is read by the binding's lookup steps. The pack's lookup step sees the matches and reports more than one as its own failure `possible_commit`, `ambiguous_match`; the engine carries it and defines no such code. *Instead of:* the engine counting matches, which would need a shape for "several records" in the driver contract.
- `stamped`: the lookup steps are handed the record's idempotency token as the step's stamp. At the first preview no record can carry it, so the record is a `create` without a lookup.
- Once a key was assigned, by the save's answer or by a lookup, the record is read by the read-back steps, which are handed the assigned key as the step's key and find everything the record's steps observed under `result`. The assigned key and those values are stored with the `acted` mark. *Instead of:* a fresh scope for the read-back, as the first pass had it, in which a read-back could not name the key the system had just assigned.
- Values a step returns for reconcile before the commit point are stored, and acknowledged, before the save is sent; if they cannot be stored the save is not sent. On resume the record is read by them, not looked up.
- What a lookup finds is the read: its values are compared, and no second read follows. During reconcile, a lookup that finds nothing waits the binding's settle time out once and looks again.
- The ledger holds a record's outputs from its `acted` mark only. A record found `unchanged`, or one reconcile found had landed, is read once more on resume when its operation declares outputs.

**What `Control` is asked, and when**

- `plan` once per plan, then the hash is checked. `decide` once per job, after the inputs and the source are validated, with the number of records. `session_lease` when a driver is prepared for a surface that needs a session, and once more after a sign of expiry, which is relayed with `report_expiry`. `rate` before a pass on a surface that declares a limit.
- For a write: `evidence` and `store_preview` at the preview; `approval` at the start of an execute that was not cleared before; then for each record `lock`, the freshness read when the preview is older than the window, `intend`, the save, `acted`, the read-back, `evidence`, `verified`. `failed` when a record stops. `settle` for each record in doubt on resume.
- The freshness window and a pipeline's time limit are measured on the engine's clock, which the process hands in, so a test moves it.
- The engine gives any call to `Control` up after a time limit (`control_timeout`, two minutes unless set) and treats it as failed with `busy`, `control_unanswered`. *Instead of:* holding no timer, as the trait's contract said: a control plane that never answers would hold a job, and its lease, for ever.
- A panic in a provider's code is caught where the engine calls it and comes back as a named failure: `unknown_variant`, `driver_panicked` for a step that commits nothing, `possible_commit`, `no_confirmation` for a save. *Instead of:* letting it cross the crate boundary and end the runner.
- A `commits` step's future is never dropped, so the engine does not end a save that hangs: its time limit is the driver's, and the lease bounds the job from outside.

## Consequences

- `a2m-actions` and `a2m-runner` must hand the engine the source's bytes in `JobSpec::source`, and what was recorded before a commit point in `RecordSnapshot::before_commit`; neither is filled by `a2m-actions` yet.
- An implementation of `Control` must make each call stand or fall as one transaction, because the engine may stop waiting for it.
- A pack whose mode can find records by a search declares `ambiguous_match` in its manifest and reports it from its lookup step.
- What is not built is named in the design's build order: a source whose reader is `operation`, a binding that saves more than once in one call, records in parallel with the intent window, and shadow.
