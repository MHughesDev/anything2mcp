# 0376. A stamped record never sent is not looked for before its intent

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** the freshness check in `a2m-engine` (`commit_point` in `tool.rs`); the `stamped` row and the intend step of [`a2m-engine`'s design](../architecture/crates/a2m-engine.md); the roadmap row J3. Closes what [decision 0356](0356-a-scan-with-nothing-recorded-says-absent-of-a-log-that-has-lost-nothing-from-its-front.md) left open. Nothing is added to the ledger.

## Context

Before it stores a record's intent, the engine checks that the record still holds what the preview saw: the freshness check, a read of the record. A record of the `stamped` strategy is read by its binding's lookup, given the record's token. For a publish to a log, that lookup is a scan of the log for the token, and a scan with nothing recorded can say "absent" only of a log that has lost nothing from its front (decision 0356). A log in use has lost its front, so every keyed publish to one stopped at this first look as `scan_inconclusive`, with nothing sent. Decision 0356 named two ways out, both the kernel's: record the log's position before the first look, or not ask the lookup about a record that has no intent.

The first way needs the engine to run a step of the binding before the look: the `position` step that the stream pack's lowering puts before the publish. Nothing in the plan says which of a binding's steps before its commit point only look. The engine cannot tell the stream pack's `position` from the data pack's `execute`, which runs an insert inside the transaction the `commit` step ends: run before the look, it would make the row the look then finds. Marking such a step would be the packs' lowering's change, and a change to the plan.

## Decision

1. **A record of the `stamped` strategy that is still `planned` is not read before its intent.** It was never sent: the engine sends a record's committing step only with the token the ledger issued for its stored intent, and the driver handle refuses any committing step without one. No record of the system can carry the token of a record never sent, which is why the preview does not look for one either (it is a `create` with no lookup), and the freshness check confirms what the preview saw. A record returned to `planned` by reconcile is not read again either: reconcile has just read it, and settled it as not landed.
2. **Every other look stands.** A record in `intent` or `acted`, or one that failed after it was sent, is read by its lookup at reconcile, as before; the read-back after the publish scans from the position confirmed for this run's publish, or from the `scan_from` its `position` step recorded before the publish. A record of the `find` strategy, which a search may find whoever made it, is looked up at the freshness check as before. The stream pack's scan is unchanged, and still says "absent" only of a range it read whole.
3. **Nothing is added to the ledger.** The position a resumed scan needs is already recorded before the publish is sent, by the binding's own `position` step, as the record's values before its commit point.

*Instead of:* running a binding's steps before its commit point ahead of the first look, to record the position (the way decision 0356 named first): unsafe for any pack whose step before the commit point does part of the write, as the data pack's `execute` does, until the plan marks the steps that only look. *Instead of:* recording the position in the ledger at the intent: the engine has no step it may run to learn it, for the same reason.

## Proof

`crates/anything2mcp/tests/stream_write.rs`, against a real NATS server with JetStream, on a log whose two oldest messages were purged so that it begins at its third position: `a_publish_to_a_log_whose_front_was_trimmed_is_sent_once_and_read_back` (approved, published once, verified by its scan; the server holds one message more, and a resume sends nothing), and `a_publish_cut_on_a_log_whose_front_was_trimmed_is_found_from_where_the_log_ended` (the connection is cut after the server stored the message; the resume's scan starts from the recorded position, finds the message, and the server holds it once, with one `intent`). Before the change both stopped as `scan_inconclusive`. `crates/a2m-engine/tests/identity.rs` holds the steps a stamped record's write and reconcile take, with no lookup before the intent.
