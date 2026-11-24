# 0399. A scan with nothing recorded says absent of a log that has lost nothing from its front

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md), in package J3. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** the `scan` step of [`a2m-pack-stream`](../architecture/crates/a2m-pack-stream.md) (the bullet "A scan says absent only of a log it covered"); the roadmap row J3.

## Context

A publish to a log that declares an idempotency header is a stamped record, and its lookup is a `scan` for the record's token. The engine asks the lookup of a record that has not been sent, before it records the intent, to learn whether the record exists. The design had the scan say "absent" only of a range that began at a position recorded before the publish was sent. A record not yet sent has no such position, so the scan stopped with `scan_inconclusive`, and no keyed publish to a log could ever begin. The pack's own tests drove the driver step by step and never ran the engine's first look, so they did not show it; J3's test through the product, against a real NATS server with JetStream, did.

## Decision

1. **With no position recorded, the scan reads the log from its first position, and says "absent" only if the log's first position is 1.** A log whose first position is 1 has lost nothing from its front: what it holds is everything it ever held, so a token not in it was never stored. Every other condition for "absent" stays: the stream's configuration cannot lose a message from inside its range, and every position from the first to the end was read, none missing. A log whose front has been trimmed (a message limit, an age limit, a deletion) still stops with `scan_inconclusive` when nothing is recorded, because the token may have been in what was removed.
2. **What it does not change.** A record with a recorded position, and a position this run's own publish was confirmed at, are scanned as before. The pack still never relies on the broker's duplicate window to say "absent".

*Instead of:* saying "absent" of any log read whole from its first retained message, on the argument that a record's publish is never sent before its position is recorded. That is sound only while the ledger keeps every step's record, and the rule is "unknown means stop"; the narrower claim is a fact about the log and rests on nothing else.

## What this leaves open

A log in use for long has lost its front, so a keyed publish to it stops at its first look with `scan_inconclusive` (nothing is sent). That is safe and not useful. The way out is for the engine to record a position before its first look, or to ask the lookup of a record that has no intent only for the position; both are the kernel's, and this record only names the gap.

## Proof

`crates/a2m-pack-stream/tests/nats.rs`, `a_scan_with_nothing_recorded_says_absent_only_of_a_log_read_whole_from_its_first_message`; and package J3's tests through the product against a real server, `crates/anything2mcp/tests/stream_write.rs`.
