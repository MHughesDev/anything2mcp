# 0399. A session that ends after a write was sent leaves the write in doubt

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** what a failure after the save becomes in `a2m-engine` (`after_send` in `tool.rs`); the table of the loop's parts in [`a2m-engine`'s design](../architecture/crates/a2m-engine.md); the `session_expired` row of [spec section 207](../spec/2-kernel/207-the-engine-loop.md); the roadmap row 3.10, whose note named this as left.

## Context

After a record's write was sent, every failure of a step that follows it, an act step after the save, a confirm step, the read-back, made the record in doubt: `possible_commit`, `no_confirmation`. Two classes stood as they were: `possible_commit` itself, and `session_expired`, which rested the run at `needs_login`. Package 3.10's test through the product showed what that does: a session that ended between the write and its read-back left the run `needs_login`, and a front door reports that class as safe to call again (`retry_safe: true`). The write had reached the system. An agent that called the tool again would have started a second run and a second write.

## Decision

1. **A session that ends after the save is a write in doubt.** The failure is `possible_commit`, `no_confirmation`, naming `session_expired` and the driver's code as the failure it stands in for, and the front door says `retry_safe: false`. The lock is kept. The driver's sign that the session ended is relayed to the keeper first, as before; the engine never renews.
2. **The resume reconciles once a person has signed in.** A resume asks for a session lease like every job and reads the record by its key before anything is written. While the session is refused, it does not complete and sends nothing; once a person has signed in, it reads the record, settles it, and goes on.
3. **What it leaves alone.** A session that ends before the save still rests the run at `needs_login`, with nothing sent. A session that ends while a record in doubt is being reconciled still rests the run at `needs_login`, because only a sign-in lets it go on, and the record stays in doubt in the ledger.

*Instead of:* keeping `needs_login` after the save and making the front door say `retry_safe: false` for any run with a record in doubt. The class is what every reader of the failure acts on, the ledger's run event, the trace, an agent, and the door would have to ask the ledger to tell the truth that the class did not.

## Proof

`crates/anything2mcp/tests/faults.rs`, `a_session_that_ends_during_the_read_back_leaves_the_write_in_doubt_until_it_is_read`: the stand takes the write, refuses every read after it as under an ended session, and the run stops `possible_commit`, `no_confirmation`, `retry_safe: false`, with one `acted` and no `verified`; resumed while still refused, it does not complete and nothing more reaches the stand; signed in again, the resume reads the record first, finds the write, completes, and the stand holds one write and the trace one `intent`.
