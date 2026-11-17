# 0162. The provider contract's second pass: what a step is handed, and what it hands back

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the designs of [`a2m-provider`](../architecture/crates/a2m-provider.md), [`a2m-engine`](../architecture/crates/a2m-engine.md), and [`a2m-core`](../architecture/crates/a2m-core.md); sentences of spec sections [203](../spec/2-kernel/203-the-provider-contract.md) and [502](../spec/5-security/502-defending-against-a-manipulated-agent.md). Builds on decisions [0089](0089-the-provider-contract-as-building-it-settled-it.md) and [0148](0148-the-provider-contract-and-the-scheduler-after-their-safety-audit.md); the plan and the engine's half is [decision 0161](0161-the-plan-and-the-engine-after-the-packs-were-built.md).

## Context

Eleven packs were built against the contract and audited. Their lists of what the contract could not express agreed on six points. The lead decided each; this record says what was built, and where the code showed that the form decided could not hold.

**One constraint shaped every point.** `StepInput`, `Observed`, `StepFailure`, and `PrepareContext` have public fields and are built field by field in the packs' code and tests: some 25 sites for the first, and as many for the third. A field added to any of them breaks every pack, and the packs are not this package's to change. So each addition went where it is an addition: a type with private fields, a method with a default, or a name the contract reserves.

## Decision

| Point | Built | Instead of, and why |
| --- | --- | --- |
| 1. What earlier steps recorded | `Prepared::recorded()`: the values the record's earlier steps returned in `for_reconcile`, set by the handle for the one call of `act` (`DriverHandle::act_recorded`) and empty outside it. The engine fills it within a pass and, on resume, from `RecordSnapshot::before_commit`. A read-back, a lookup, and a reconcile read can use a message id, a log position, a digest, or a built frame | A field `StepInput::recorded`, as decided: it breaks every pack. A second `act` method with a default on `Driver`: two methods to keep in step in every pack. `StepInput::new` and `for_record` are added so that the field can come once the packs build their inputs through them |
| 2. How a step was done | The capture named `acting`, of `application/json`, is the step's `a2m_telemetry::Acting`: `Capture::acting(&acting)`, `Observed::acting()`. The engine puts it in the step's trace record and keeps it as no evidence. Five runs of one read give one signature (`crates/a2m-engine/tests/contract.rs`); a run that does something else gives another | A field `Observed::acting`: two packs build `Observed` whole, outside `..Default::default()`. The HTTP and web packs already write this very capture, so they need no change |
| 3. Captures are kept | `Control::keep_capture(KeptCapture)`, with a default that refuses by name (`busy`, `capture_not_kept`). Each capture but `acting` is offered to the trace as an artifact at its step, kept as the run's capture level says, redacted once more, stored, and named by its digest in the step's trace record. A capture is kept when its step ends, so one taken before a committing step is kept before the save. One that cannot be kept is dropped and the run goes on | Failing the step when a capture cannot be kept: a capture is evidence, never what a write depends on, and the default refusal would stop every pack's run until the action registry stores captures |
| 4. What the system said | `Failure::said`: at most 400 characters, control characters as spaces, redacted wherever a failure is (`Redact::redact_failure`), and carried by a failure that stands for another (`carrying_said`): the handle's write in doubt and the engine's. `StepFailure::with_said` sets it. Never joined into the sentence or the next step | A field `StepFailure::said`: it breaks every pack. On `Failure` it reaches the run's failure, the ledger's stored form, and every door with no new plumbing. It is boxed, so that the errors that hold a failure stay under clippy's limit for a large error |
| 5. The run's project folder | `CheckedPaths::with_project`, `project()`, and `project_folder()`, which stops by name, `input`, `project_folder_missing`. The engine asks `Control::project_folder()` (default `None`) and checks the folder once more as a root named `project` | A field of `PrepareContext`, which every pack's tests build. A field of `JobSpec`, which the action registry builds |
| 6. A verified save | `CheckedPath::create_temporary` gives a `Temporary` open for reading and writing; `write`, `flush`, `read_back`, `replace`, and `CheckedPath::flush_folder` are separate calls; `CheckedPath::identity` and `CheckedPath::may_write` | None. Errors tell `busy`/`file_locked`, `busy`/`storage_full`, `input`/`file_access_denied`, and `input`/`file_missing` apart. On Windows the file's identity takes one unsafe call, compiled and linted from a copy and not run |

## Consequences

- Every pack compiles and behaves as before. What each must adopt is listed in the report of this package.
- A door that shows a failure must show `said` marked as the system's words, apart from the sentence.
- The action registry must implement `Control::keep_capture` and `Control::project_folder`, or captures stay dropped and no run has a project folder.

## Revisit if

The packs build `StepInput`, `Observed`, and `StepFailure` through constructors: then `recorded` and `acting` belong on them as fields.
