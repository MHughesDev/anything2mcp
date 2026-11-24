# 0999. A read of a retained key that declares `found` answers a key with no message as `found: false`

**Status:** Accepted. Design decision, made by the worker on field test 7 under the lead's brief. The owner had not read this record when it was written. It takes the rule of [decision 0348](0348-a-read-by-key-of-a-row-that-is-not-there-answers-found-false.md) from the data pack to the stream pack; no public interface changes.

**Date:** 2026-11-24

**Changes:** `a2m-pack-stream` (`src/lower.rs`, `src/driver.rs`, `src/draft.rs`); the design of `a2m-pack-stream`; [spec section 309](../spec/3-packs/309-the-stream-pack.md).

## Context

In field test 7 a read of one item's level from a retained channel of a broker, given an item the broker held no message for, stopped the run as `unknown_variant`, `output_unexpected`, "found nothing to read", and offered to resume it. Field tests 3 and 5 found the same for the data pack (finding 21) and the command pack (finding 30), and each pack was given a way to say "none" as an answer. The stream pack had none: its `latest` step answers a missing key as absent, which the engine treats as a failure of a read.

## Decision

1. **A read whose operation declares an output `found` answers a key with no message as a result.** The `latest` step of such a read is no lookup: a key the broker holds no message for is answered `found: false`, and a key it holds is answered with the message's fields and `found: true`. Every other output may then be absent, and a missing key returns each as `null` ([decision 0305](0305-an-output-that-may-be-absent-is-declared-so-in-the-plan-and-the-output-schema.md), [0315](0315-every-declared-output-is-in-every-result-on-every-path.md)).
2. **A message that holds a field `found` keeps the old behaviour, or stops.** Where the channel's schema states a field `found`, the read is lowered as before. Where no schema says so and a message's body holds one, the read stops as `response_schema_mismatch`: the pack does not choose between the broker's word and its own.
3. **The drafter declares `found` on a read of a retained key it drafts,** unless the broker's schema states a field of that name.
4. **What is not changed.** A read written by hand that declares no `found` stops on a missing key as before. The read-back of a publish keeps `absent`, which reconcile needs.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A named failure, `not_found`, that offers no resume | An agent that asks about a key would meet a failure for an ordinary answer, as decision 0348 says |
| Every read of a retained key answers `found` without the operation asking | It changes the output schema of every read written by hand |
| Map absent to a result in the engine, for every pack | The engine is another crate's; decision 0348 names it as the follow-up, and this keeps the packs alike until it is done |

## Proof

`a_read_that_declares_found_answers_a_key_with_no_message_as_found_false`, in `crates/a2m-pack-stream/tests/nats.rs`, against the stand's NATS server: a held key returns its status and `found: true`, a missing key `found: false` and no status, the status is the one output that may be absent, and a read without `found` still says absent. `the_drafted_reads_return_what_the_broker_holds`, in `tests/discover.rs`, drafts the read with `found` and reads a missing key as `found: false`.
