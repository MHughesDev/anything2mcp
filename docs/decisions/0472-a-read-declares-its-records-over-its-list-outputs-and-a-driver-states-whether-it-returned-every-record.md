# 0472. A read declares its records over its list outputs, and a driver states whether it returned every record

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-25

**Builds** two of the parts [decision 0283](0283-watches-reach-an-agent-through-the-registry-and-their-person-confirms-each-on-the-page.md) left unbuilt, as the first half of package W2d, brought forward by [decision 0465](0465-while-the-first-release-waits-on-the-owner-agents-build-the-one-machine-packages-moved-after-it.md): records compared one by one under a declared record key, and the engine's completeness flag with the reported removal it allows. **Keeps** the comparison of [decision 0247](0247-a-watch-keeps-only-digests-and-its-due-times-come-from-the-stores-clock.md) and the rule of [decision 0262](0262-watches-are-held-to-their-end-their-budget-and-their-runs-own-result.md) that what a check observed is never the caller's word.

**Changes two public contracts:** the provider contract (`Observed` gains `coverage`) and the file formats (`operations/*.yaml` gains `records`). Also: the compiled target (`CompiledOperation.records`, left out of the bytes when absent); the engine's `EngineEvent::Result`, now `{ values, coverage }`; the ledger's `store_result`, which takes the coverage, and migration `0029`; the [file formats](../reference/file-formats.md); the [ledger schema](../reference/ledger-schema.md); the [technical reference](../reference/technical-reference.md); the designs of [`a2m-files`](../architecture/crates/a2m-files.md), [`a2m-provider`](../architecture/crates/a2m-provider.md), [`a2m-engine`](../architecture/crates/a2m-engine.md), [`a2m-ledger`](../architecture/crates/a2m-ledger.md), and [`a2m-actions`](../architecture/crates/a2m-actions.md); the watches row of [SCOPE.md](../../SCOPE.md); the roadmap (W2d split, W2e).

## Plan, stated before the build

The slices, in order: (a) a record key declared for a read's list outputs, and records compared one by one; (b) the engine's completeness flag, a field of the provider contract, and with it a reported removal; (c) the MCP resource and its subscription; (d) the REST change stream; retention only if retention of runs exists. This record builds (a) and (b). (c) and (d) are package W2e. Retention still waits: no retention job exists for runs.

The alternative weighed for (a) was a list output whose items are records, `lines: {type: line, list: true}` with a record field type. It was not taken: a field type is one value (`types.yaml` has no record base), the packs return a list read as one list per column (the data pack's drafted `_list` read is the first example), and a record field type would change every pack's extraction and every output schema. Columns need nothing new of any pack.

The alternative weighed for (b) was completeness declared in the operation's file, by naming a boolean output such as `more`. It was not taken: it makes the builder's word the proof, and a read whose answer is cut short by the system with no such output would claim every record. The driver is the only part that sees whether the system held more, and the data pack's driver already asks the store for one row past its limit.

## Decision

| Part | Built | Proven by |
| --- | --- | --- |
| The record key | `operations/*.yaml` takes `records: {key: [...]}`. Every list output of the operation is a column of its records; the record `i` is the `i`th item of each column; `key` names the columns whose items make a record's key, parent first. Only on an operation whose effect is `read`; each name a list output, none twice; anything else is `kernel.records_invalid`. An output that is not a list, such as `more`, is a fact of the whole result and no part of a record | `a_read_declares_its_records_by_the_list_outputs_that_make_their_key` and the `kernel.records_invalid` case in `crates/a2m-files/src/tests/example.rs` |
| What a watch compares | `watch.create` takes the shape from the published compiled target: `WatchShape::Columns { columns, key }` for a read that declares records, else `Whole`. Columns of two lengths, a column missing, or a key part that is `null` make the check a gap with `watch_result_not_its_shape`. `watch.changes` finds a change's record in the check's run by the same rows | `records_over_columns_are_rows_across_the_lists_and_their_key_is_never_empty` (ledger) |
| The completeness flag | `Observed.coverage`, the kernel's `Coverage`: `complete` when the driver knows the system holds no record it did not return, `partial` when it knows it holds more, `unknown` by default. The data pack's list read states `complete` when the store gave no row past the row limit and `partial` when it did; every other driver states nothing | `a_list_stops_at_its_limit_and_says_that_more_exist` (data pack) |
| What the engine states | Of the steps of a pass that returned values, the weakest coverage stands; a step that returned none states nothing. The run's result carries it only for a tool that is one operation called once; a pipeline, a step called for several inputs, and a write state `unknown`. `EngineEvent::Result { values, coverage }` | `a_pass_is_complete_only_when_every_step_that_returned_values_said_so` (engine) |
| Where it is kept | Migration `0029` adds `runs.result_coverage`, `unknown` by default. `store_result` writes it with the result in one statement; a trigger refuses a coverage stated without a result and any change once the run is final | `a_runs_coverage_is_stated_with_its_result_and_never_changes_after_it_ends` (ledger) |
| A reported removal | `end_check` reads the coverage from the run's row, never from its caller. A check is complete when the watch compares records one by one, its run's coverage is `complete`, and it saw a record or its baseline held none; the comparison then reports `removed` between a complete baseline and a complete check, as decision 0247 built it | `a_record_is_removed_only_between_two_checks_that_returned_every_record` (ledger) |

## Choices

- **An empty complete check proves nothing** (decision 0262, B9). A check that saw no record after a baseline that held some is not complete: an empty answer is as often a read that went wrong as a table emptied. Its records are `unseen`; the next complete check that sees any record proves the removals.
- **`complete` is of the read as written.** A statement whose own text limits its rows makes its answer the whole of what the read returns, and a record that leaves that answer is removed from the read. The driver does not read the statement's text to guess otherwise (rule 7).
- **The coverage is a column of the run, not a field of the stored result.** The result is the tool's outputs, which a caller sees with `run.get`; a field there would be an output no operation declares.
- **No new failure code.** The ledger's `watch_result_not_its_shape` covers a result that is not of the watch's columns.

## Not built

The MCP resource and its subscription, and the REST change stream (package W2e); retention of watches, which waits on retention of runs; a completeness flag from any driver but the data pack's (the HTTP pack cannot tell whether a server cut its answer short, and no pager is built); a product test through the real command against a stand (the test application's HTTP reads state no coverage, and the data pack's product test was not run in this slice for want of disk on the build machine).

## What the work showed

- **H-47, records by key.** The comparison of decision 0247 needed no change: what was missing was a way for a published read to say what its records are, and for a run to say that it held all of them. Both now come from the compiled target and the driver, and neither from the caller.
