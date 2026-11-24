# 0358. The data pack's writes are proven through the product, and a version guard is not yet verified by the engine

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** package G3 is proven through the product, with a test and two test projects in `crates/anything2mcp/tests/`. No product code changes. A limit between the data pack and the engine is written down.

## Context

[Package G3](../plan/roadmap.md) had its proof only in the data pack's own crate, through the driver handle: `src/tests/writes.rs` against the embedded stand and `src/tests/server.rs` against a real PostgreSQL server. Package D5 had shown the proof the roadmap means for a write: through the product, with a person's approval, a real program that is ended after the store has the write, and the stand asked what it holds. G3 was to be proven the same way.

Building it found one limit that the pack's own tests could not show. The engine verifies a write by comparing the operation's inputs that are not part of the key with the record read back (`record_fields` in `a2m-engine`'s `compare.rs`). An operation of the data pack that guards its update by version takes the version it read as one more input. That input is no field of the record, or, when it is named as the version column, it is a field the store raised on the write. The first fails the write as `field_not_read_back`; the second as `read_back_differs` on `row_version`, because the store held 2 where the person sent 1. The pack's tests do not meet this: they compare the fields they choose.

## Decision

1. **The proof of G3 through the product is an update by key with no version guard.** `example_line_item_update` sets the length and the price of one line, `guard: none`, with the read-back by key of the pack's own form. A staleness is then found by the engine's freshness check at resume, which is the check every pack's write has: the approval is bound to the record as the person saw it, and a change made by someone else after that stops the run as `stale` with nothing written. The version guard stays proven in the data pack's crate (`a_row_changed_since_the_preview_is_stale_and_nothing_is_written`, `a_stale_version_and_a_refused_constraint_write_nothing`), where the compare is the test's own.
2. **A version-guarded write through the engine is not built.** It needs the engine to leave out of the record's fields an input that a binding gives to a guard parameter (`params: {version: input.…}`), or the plan to name the guard input. That is `a2m-engine`'s change and is not made here. Until it is, the pack's design note says that an operation with a version guard cannot be verified by the engine, and a target should use the guard `none` and rely on the engine's check.
3. **How the program is ended after the store has the write.** On the server, the stand's relay passes the `COMMIT`, lets the server answer, and holds the answer back, so the real program waits and is ended while it does. The embedded file has no relay, and the commit of a file is not a message that can be held. The test ends the real program as soon as the stand's own log shows the write, and, if the program had already finished, puts the stand back and tries again with a new run, up to forty times; the run it goes on with is one that the engine found in doubt. The first try has always been the one so far.
4. **The server tests run only in a build that has the store.** The test asks the data pack's own manifest whether it has the surface kind `database_server`. A build without the feature `postgres` skips the server tests and says so; with `A2M_REQUIRE_POSTGRES=1` it fails them instead, as the pack's own tests do. The `postgres` job of CI runs the file with `--features a2m-pack-data/postgres`.

## Why not

| Alternative | Why not |
| --- | --- |
| Change the engine to leave a guard's input out of the compared fields | It is another crate's, and the change is a design question: the plan has no place today that says which input is a guard's. It is the follow-up of point 2 |
| Declare the version as an output and compare it | The compare is of what was intended; a version is raised by the store and is never intended |
| A read-back written by hand that waits, to hold the file's program after the commit | The compiler refuses a read-back that is not the read of the record by its key (`data.read_back_invalid`), rightly: a hand-written read is never a read-back |
| A trigger in the store that holds the program after the commit | A trigger the target did not declare stops the write as drift, and no trigger runs after the commit |
| End the file's program after a fixed time | A fixed sleep is not a wait on a condition; the log of the stand is the condition |

## Cost

The embedded file's kill is a race the test closes by trying again, not a hold. A machine that is so fast that the program finishes before the test reads the log tries again; if forty tries all finish, the test fails by name.

## Proof

`crates/anything2mcp/tests/data_write.rs`, each on the embedded file (`file_…`) and on a PostgreSQL 16 server (`server_…`): `an_approved_write_is_made_once_and_read_back` (nothing written before the approval, one write after, the stand's own client holds the values and the raised version, the trace has `approval_bound`, `intent`, `acted`, `verified` in that order), `a_program_killed_after_the_write_was_sent_is_reconciled_and_writes_no_second_time` (one write and one `intent` after the resume, a `reconcile_read`), `a_stale_approval_writes_nothing`, and `bad_input_starts_nothing` (a value that is no number, a missing key part, a key no row has, and a key with quotes in it write nothing and leave every table).
