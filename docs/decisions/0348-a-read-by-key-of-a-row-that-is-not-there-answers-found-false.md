# 0348. A read by key of a row that is not there answers `found: false`

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** the data pack's lowering, driver, and drafter. Field findings 19, 21, and the data pack's half of 22 of [field test 3](../reviews/field-tests.md).

## Context

A drafted read of one row by its key, given a key the store does not hold, stopped the run as `unknown_variant` (`output_unexpected`, "found nothing to read") and offered to resume it. A missing row is an ordinary answer to a read by key, and there is nothing to resume. The engine treats a step's `absent` outcome as a failure for a read; it keeps `absent` for the read-back of a write, where reconcile needs it. The engine is not changed here.

## Decision

1. **A read by key answers a missing row as a result.** An operation that reads by key and declares an output `found` (boolean) has a binding whose `fetch` step answers a missing row as done, with `found: false`, and a present row with `found: true`. Every other output of such an operation may be absent, and a missing row returns each as `null` ([decision 0305](0305-an-output-that-may-be-absent-is-declared-so-in-the-plan-and-the-output-schema.md), [0315](0315-every-declared-output-is-in-every-result-on-every-path.md)).
2. **The drafter declares `found` on every read by key it drafts,** unless a column of the record is named `found`, and then the old behaviour stays for that read. An operation written by hand that declares no `found` keeps the old behaviour: a missing row stops it as before. The read-back of a write is not changed.
3. **A moment of a server is drafted as `text`.** A `timestamp with time zone` is read as the same moment in UTC and a `timestamp` as the figures it holds with no zone added, each to the microsecond. The built-in `datetime` reads whole seconds in one fixed form and would refuse or round them, so the drafted type is `text`; a person who wants a calendar type writes it with a format and a zone. A column with one of these types no longer holds its table back. A wall time was refused by the driver before and is read now. The embedded store keeps `datetime` for a column declared `TIMESTAMP`, as before.
4. **The committing step carries the write's arguments.** `commit` carries the same arguments as the write's `execute`, so that the offline test's `<name>.request.json` can hold the values sent. The driver reads the arguments at `execute` and ignores them at `commit`.

## Why not

| Alternative | Why not |
| --- | --- |
| A named failure of class `input`, `not_found`, that offers no resume | It needs the engine to map an absent read to that code; the engine is another crate's, and an agent that asks about a key would meet a failure for an ordinary answer |
| Every read by key answers `found` without the operation asking | It would change the output schema of every operation written by hand |
| Draft a server moment as the built-in `datetime` | The format cannot hold fractions of a second, so a stored value would be refused or rounded |

## Cost

Every output of a drafted read by key now admits `null`, even one whose column is not null, because the one result shape serves both answers. A follow-up for the engine is to let an absent read be a result with `found`, which would keep non-null outputs certain.

## Proof

In `a2m-pack-data`: `a_drafted_read_by_key_answers_a_row_that_is_not_there_with_found_false` (the embedded store), `a_table_of_moments_drafts_its_reads_and_a_missing_row_is_found_false` (PostgreSQL), `numbers_and_moments_are_read_exactly_and_a_type_with_no_rule_stops`, and `an_update_is_written_once_and_reads_back` for the commit's arguments.
