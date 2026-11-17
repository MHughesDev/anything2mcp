# 0161. The plan and the engine after the packs were built

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the designs of [`a2m-core`](../architecture/crates/a2m-core.md), [`a2m-files`](../architecture/crates/a2m-files.md), [`a2m-expr`](../architecture/crates/a2m-expr.md), and [`a2m-engine`](../architecture/crates/a2m-engine.md); the plan's shape and its pinned sample hash; sentences of spec sections [206](../spec/2-kernel/206-the-compiler-and-the-plan.md), [208](../spec/2-kernel/208-runs-the-ledger-and-resume.md), and [209](../spec/2-kernel/209-record-identity-and-dependent-records.md). Builds on decisions [0091](0091-the-engine-loop-as-its-first-two-passes-settled-it.md), [0092](0092-a-record-that-waits-for-an-earlier-record-is-planned-under-its-place-in-the-run.md), [0147](0147-the-engine-after-its-safety-audit.md), and [0153](0153-what-the-compiler-the-plan-and-the-redactor-hold-by-themselves-after-the-safety-audit.md). The contract's half is [decision 0162](0162-the-provider-contracts-second-pass-what-a-step-is-handed-and-hands-back.md).

## Context

The packs' audits agreed on what the plan and the engine could not express. The lead decided each point; this record says what was built.

## Decision

| Point | Built | Instead of, and why |
| --- | --- | --- |
| 7. Evidence when nothing can be read back | `WriteEvidence`, in `a2m-core`,: `read_back`, `response`, `acknowledged`, held to the list in section 208 by a test. `BindingPlan::evidence` replaces `prefer_response`; a lowering's `prefer_response` compiles to `response` (`Lowering::evidence`, with that default). `acknowledged` is only for an operation declared `irreversible`: the compiler derives `irreversible` from it, the plan's check refuses it elsewhere, and the engine refuses it again before anything is sent. Nothing is read or compared. The record ends `acted` through `Control::acknowledged`, never `verified`; the diff carries the evidence (`DiffRecord::not_read_back`), in its hash only when it is not `read_back`, so earlier diffs hash the same. A lost answer leaves the record in doubt; where the binding has no lookup or read-back, reconcile stops with `possible_commit`, `cannot_reconcile`, and the failure's `said` shows what was recorded before the save, redacted. The sample plan's hash moved on purpose; the format stays 1 | Adding `acknowledged` to `EvidenceLevel`: that is the ladder of how far a finding or a tool is proven, ordered, and a write's evidence is not a rung of it. Ending the record with `verified` and an empty comparison: it would call verified what was never read |
| 8. A key is never guessed | A step is given the record's key in the system: its key inputs, or the key the system assigned, or, for a record that waited for its parent, the key its whole inputs now give. A record's place in the run is the ledger's and the trace's name for it and is never handed to a step. The compiler refuses a write whose identity is `none` and whose binding reads the record back (`kernel.identity_invalid`), and the engine refuses it before anything is sent (`unknown_variant`, `no_read_back`) | Refusing at compile time every binding whose read-back might need a key it has not got: whether a save assigns one cannot be known before it is sent. `natural` and `composite` always have a key, and `stamped` and `find` find theirs |
| 9. What a lowering may name | `LowerContext::expr` reads `current`, by the operation's inputs, each optional, and `record`, whose `key` is the record's key in the system, its parts as text; the engine binds both for a binding's steps | A typed key by input name: an assigned key's parts have no names |
| 10. Deletes and open effects | Stated as two rules in the `a2m-files` design: a delete is `irreversible` unless its binding declares how the record is restored, which none can yet; a file kind entry whose effect is an open question counts as a write. The compiler holds a pack to the first through its `least_effect` | A verb the kernel reads: decision 0068 keeps the verb's spelling out of the kernel |
| 11. A draft is never a published plan | `compile` refuses a draft under any version but `DRAFT_VERSION`, `kernel.draft_under_release_version`. Every pack's helpers pass `DRAFT_VERSION`; the one test that did not, in `a2m-provider`, now does | A mark of the mode in the plan, which would change the plan's shape for every published plan too |
| 12. Each operation's own destinations | A driver context is prepared with the surfaces at the run's environment and the running operation's own declared destinations, and is used again only for the same list | One list pooled from every operation of the plan, which let a read reach where only a write was allowed |
| 13. A masked number | The engine takes the text `[redacted]` where an output is a number, a date, a moment, or a length of time, and never fails a run for it; the compiler writes each such output's schema as that type or that text | Failing the run with `output_unexpected`: the system answered, and the mask is the redactor's |
| 14. A caller key's digest purpose | `DigestPurpose::CallerKey` in `a2m-core`. The ledger's keyed digest of a caller's idempotency key still uses the record-key purpose with a leading zero byte: that code is in `a2m-ledger`, which this package does not own | None |

## Consequences

- The ledger must let a record end `acted` on an acknowledgement, release its lock, and let its run complete; until it does, the action registry's `Control::acknowledged` cannot be built, its default refuses, and such a record stays in doubt.
- A pack that wants `acknowledged` overrides `Lowering::evidence`; a pack that sends a key it took from `StepInput::key` gets none for a record that has no key in the system, where it got the engine's place name before.
- A door or a client that checks a result against a tool's output schema now accepts the mask where a number is declared.

## Revisit if

A binding can declare how a deleted record is restored; or a snapshot of a run says how each `acted` record ended.
