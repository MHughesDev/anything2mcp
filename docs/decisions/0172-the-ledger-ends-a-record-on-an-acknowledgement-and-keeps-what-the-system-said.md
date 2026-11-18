# 0172. The ledger ends a record on an acknowledgement, and keeps what the system said

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the design of [`a2m-ledger`](../architecture/crates/a2m-ledger.md), the [ledger schema](../reference/ledger-schema.md), and sentences of spec section [208](../spec/2-kernel/208-runs-the-ledger-and-resume.md). Adds migration `0015-ledger-records-ended-on-an-acknowledgement.sql`. Builds the ledger's half of [decision 0161](0161-the-plan-and-the-engine-after-the-packs-were-built.md) (points 7 and 14) and of [decision 0162](0162-the-provider-contracts-second-pass-what-a-step-is-handed-and-hands-back.md) (point 4). Keeps [decisions 0081](0081-what-the-ledger-holds-as-data-the-token-doubt-locks-and-rule-three.md) and [0145](0145-only-the-policy-engine-clears-a-write-and-the-ledger-reads-the-whole-clearance.md): every rule they hold still holds, and each is still a test.

## Context

A write whose only possible evidence is the system's own answer is planned with evidence `acknowledged`, and the engine calls `Control::acknowledged` for it. Nothing in the ledger could record that, so such a record stayed `acted`, in doubt, and its run could not complete. A caller's idempotency key was digested under the record-key purpose with a leading zero byte. And the ledger wrote a failure's sentence into its events but dropped what the system said.

## Decision

1. **The record stays `acted` and gains `acknowledged_at`.** `Ledger::mark_acknowledged(token, evidence, diff)` sets it, keeps the answer's evidence in `evidence_ref`, writes a second `acted` event whose payload says `"evidence": "acknowledged"`, and releases the lock, in one transaction. The record is never `verified`, has no read-back digest, and is settled: `in_doubt` and rule 5 pass over it, a resume skips it, `take_lock` and `settle` refuse it, and a run all of whose records are verified, unchanged, or acknowledged may complete. `RecordRow::acknowledged()`, `RecordRow::ended_on()`, and `Ledger::tally` say so to whoever shows a result. *Instead of:* a seventh record state, which is `a2m-core`'s and which decision 0161 rejected; and a new event kind, which needs `run_events` rebuilt, because its `CHECK` lists the kinds. *Instead of:* the `EvidenceLevel` of `a2m-core`, which the package named: decision 0161 put `acknowledged` in `WriteEvidence`, not on the ladder of how far a tool is proven, so `ended_on` answers a `WriteEvidence`.
2. **The evidence a binding offers is read from the run's own stored diff.** `Store::acknowledged_in_diff(record)` opens the diff blob the run stored at preview, outside any transaction, because a sealed blob opens through the vault, which waits; it reads the diff as data, by `records` and in each `step_seq`, `record_seq`, `operation`, and `evidence`; and it answers an `AcknowledgedInDiff` that only it makes. `mark_acknowledged` asks for one, checks inside its transaction that the run's diff is still that blob, and refuses otherwise. `Store::store_acknowledged(token, evidence)` does both, as `store_verified` does for its call. The ledger refuses, too, a run that is not `running`, not cleared to write (rule 3), or not irreversible; a record that is not `acted` under the token's lease, ended before, or holds no lock under it; and evidence that names no blob. Evidence is required, not optional: where the engine could not keep the answer, the record stays in doubt. *Instead of:* a column filled when records are planned, which needs a field on `PlannedRecord` and so breaks every crate that builds one, and holds the caller's word, not the diff's. *Cost:* the ledger knows four names of the engine's diff. Its own tests write a diff by those names; a test that ends a record through a diff the engine wrote, in the action registry, would hold the two to each other, and is owed.
3. **Migration 0015 holds it in triggers.** A record starts unacknowledged; is acknowledged only from `acted`, staying `acted`, with an intent and a sent write, evidence, under the lease of a `running`, cleared, irreversible run that has a stored diff; and never changes once acknowledged. Rule 5's trigger of migration 0012 is replaced by one that passes over an acknowledged record. A run enters `completed` only when every record is verified, unchanged, or acknowledged, which until now only the code held. What no trigger can see is whether the diff says `acknowledged`, because the diff is sealed.
4. **A caller key's digest has its own purpose.** `DigestPurpose::CallerKey`, with no leading zero byte. Nothing is released, so no stored digest needed to survive; a store made before this change would not find its callers' keys again, and none is kept.
5. **A stored failure keeps `said`.** `set_state` and `mark_failed` write the failure's class, code, sentence, next step, and `said` into the event, after the redactor. `Ledger::run_failure` and `Ledger::record_failure` hand the failure back whole, `said` apart through `Failure::with_said`, which bounds it again; it is never joined into the sentence.

## Consequences

- The action registry implements `Control::acknowledged` with `Store::store_acknowledged`; until it does, the default refusal stands and such a record stays in doubt.
- The engine's `RecordSnapshot` needs a field that says a record ended on an acknowledgement, filled from `RecordRow::acknowledged()`, so that a resume does not read it.
- A door that shows a run's or a record's failure reads it through `run_failure` or `record_failure`, and shows `said` marked as the system's words.

## Revisit if

A person must settle an acknowledged record whose answer was lost; or the engine's diff renames the fields the ledger reads.
