# 0399. An input marked precondition is sent with a write and never compared with its read-back

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** `InputPlan` in `a2m-core` gains `precondition`; the operation file's `inputs` take `precondition: true` and the compiler (`a2m-files`) holds it with the rule `kernel.precondition_invalid`; the engine's compare (`a2m-engine`, `compare.rs`) leaves such an input out of the record's fields, and `DiffRecord` gains `preconditions`. Supersedes point 2 of [decision 0358](0358-the-data-pack-writes-are-proven-through-the-product-and-a-version-guard-is-not-verified-by-the-engine-yet.md).

## Context

The engine verifies a write by comparing the operation's inputs that are not part of the key with the record read back. Decision 0358 found that a data-pack update guarded by version cannot pass that compare: the version the write is sent with, given to the statement under `params: {version: input.…}`, is no field of the record, so the read-back has no value for it (`field_not_read_back`), or, named as the version column, the store has raised it (`read_back_differs`). The plan had no place that said an input is a precondition of the write rather than a field: only the data pack knows that its statement parameter `version` is a guard, and the engine may not name a pack's parameters.

## Decision

1. **An operation's input may be marked `precondition: true`.** It is a value the write is sent with and the system checks, and that no read gives back as it was sent: the version a guard compares, an expected state. The plan carries it as `InputPlan::precondition`, left out of the encoded plan when false, so a plan that marks none keeps its bytes.
2. **The engine leaves a precondition out of the record's fields.** It is never compared: not at the preview, the freshness check, the read-back, or reconcile. It is still a checked input of the tool, evaluated and handed to the steps as every input is.
3. **The diff shows it and the approval binds it.** `DiffRecord::preconditions` lists each by name with its normalized value, or its hash for a restricted field type; the diff hash covers them when there are any, so a diff with none hashes as before. A person approves the value the write is sent with.
4. **Only an input of an operation that writes, and not of its key, may be one.** An input of a read, a part of the key, or an input of a pipeline marked so is refused as `kernel.precondition_invalid`. A key is what the record is, never a condition of writing it; a pipeline writes through its operations, whose own inputs say it.
5. **What it leaves alone.** The data pack's guard is unchanged: the store compares the version, and a write that changes no row is `stale`, `version_conflict`, with nothing written. The engine still learns nothing of a pack's parameter names. The data pack drafts no write, so no drafter writes the mark; a person or an agent writing a guarded operation writes it. What the approval page shows of a precondition is `a2m-actions`'s, and is not changed here: the stored diff and its hash carry it.

*Instead of:* the plan naming the guard parameter of a step (`StepArg` marked by the pack's lowering): it would have put the knowledge in the pack, where it is, but the operation file would still not say to its reader which input is not a field, and the engine would have to trace each input through the arguments of every committing step. *Instead of:* skipping any input the read-back does not return: that is the check `field_not_read_back` itself, and would let a write whose field no read shows be called verified. *Instead of:* the guard reading its version from the record as the preview saw it (`current.…`): it would guard against what the engine saw, not what the caller read, and a caller that read the record earlier could not say so.

## Proof

`crates/anything2mcp/tests/data_write.rs`, `file_a_guarded_write_is_verified_and_a_changed_version_writes_nothing` and `server_…` against a PostgreSQL 16 server: an operation `example_line_item_reprice` whose statement has `guard: version` and whose input `seen_version` is marked `precondition`. Sent with the version the line has, it is approved, written once, and verified, and the stand's own client holds the price and the raised version; sent again with the version the line no longer has, the store's guard refuses it as `stale` and the stand holds what it held; sent with the version it has now, it is written and verified. `crates/a2m-files/src/tests/example.rs` holds the rule `kernel.precondition_invalid` with a part of the key marked so.
