# 0324. An idempotency key is an addition to the read-back, and holds only inside its window

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-23

**Changes:** [section 208](../spec/2-kernel/208-runs-the-ledger-and-resume.md): its opening promise, "The problem it solves", "Idempotency tokens where the system takes them", the `response` row of "What a binding offers as evidence", the durability row, and the open points; the provider manifest gains one field, `idempotency_window`, when the first binding that sends a documented key is built; hypothesis H-15.

## Context

[Research note 38](../research/38-recording-intent-and-reconciling-after-a-crash.md) read, from their public repositories, what three durable-execution engines record and when, an idempotency-key draft's source, and two public API guidelines. The engines record a step's result after it ran, never an intent before it, and hand a side effect on an outside system to the application, with a key; the ledger records the intent first and reads back on resume. The note also found four things section 208 did not say: a system keeps a key only for a window, sometimes not stated; an answer to a repeated send is the first answer or a newer state; a key reused with a changed payload is refused by some systems and silently treated as a duplicate by others; and a conflict answer to a retry means the first send is running or has landed. Its point on power loss in SQLite's write-ahead mode is from memory, so it is an open point and a test, not a rule.

## Decision

1. **Section 208 promises what holds:** a stopped run resumes with no second send while the first may have landed, unless a person has approved it. It does not promise "no duplicates", which no design can give for a system with no key and no read-back.
2. **A token is an addition to the read-back, never a replacement.** A resume reads before it writes, whether or not the token was sent.
3. **A key holds inside its window.** A binding that sends the token states in its manifest `idempotency_window`, the time it relies on, or that the window is not documented. Past the window, or with none documented, the system is taken to have forgotten the token. The field is added to the provider manifest, a public contract, when the first such binding is built.
4. **A conflict to a repeated send means read.** The binding reads and does not send again.
5. **A replayed answer is not `response` evidence,** unless the provider's binding states that its answers to a repeated send are the original. The record is read back instead.
6. **A token is never reused for changed values.** Changed intended values are a new preview, approval, run, and token.
7. **An intent must survive power loss, not only a process kill.** Whether the SQLite setting in use meets that is measured under H-15, with a test that discards the last committed transaction before restart.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Trust the system's key on resume, and skip the read | A system that forgot the key takes the same send again |
| Take a replayed answer as proof | It may describe a later state, or another send's |
| Raise the SQLite setting now | The loss is recalled, not read, and the cost is unmeasured; the test decides |

## Proof

In `a2m-ledger` (package 3.3): a record whose `intended_hash` changed cannot be given the old token. In package 3.7's random-kill proof: a lost tail of the ledger file, as well as a killed process, and what the resume then does. Points 3 to 5 are proven with the first binding that sends a documented key, against a test stand that keeps keys for a set window and answers a repeat with a conflict.

## Not taken

Writing the `stamped` token in the same transaction as the write, for a database target of the data pack, which closes the window for that target. It belongs to [section 209](../spec/2-kernel/209-record-identity-and-dependent-records.md) and takes its own record when the data pack's write path is built; it is an open point of section 208.
