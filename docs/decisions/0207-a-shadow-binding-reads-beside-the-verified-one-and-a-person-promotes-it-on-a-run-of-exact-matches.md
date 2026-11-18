# 0207. A shadow binding reads beside the verified one, and a person promotes it on a run of exact matches

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the designs of [`a2m-engine`](../architecture/crates/a2m-engine.md) and [`a2m-ledger`](../architecture/crates/a2m-ledger.md); sentences of spec sections [201](../spec/2-kernel/201-archetypes-and-modes.md), [207](../spec/2-kernel/207-the-engine-loop.md), [219](../spec/2-kernel/219-the-evidence-ladder.md), and [225](../spec/2-kernel/225-performance.md); failure codes of the [technical reference](../reference/technical-reference.md). Builds roadmap package 4.3 for reads; the shadow of a write is named and not built.

## Plan, stated before the build

The slice: a binding of status `shadow` runs beside the binding that does the real work of a live read, its outputs are compared with the verified binding's after normalization, the outcome is kept in the ledger by the shadow binding's digest (decision 0200), and the ledger answers whether a person may promote it and records the promotion. The hypothesis it tests is H-10 for reads only: that comparing what two bindings return catches what one of them gets wrong. Kernel only: the engine runs and compares, the ledger counts; no pack knows a binding is in shadow.

**Alternatives weighed.**

| Question | Taken | Weighed and not taken |
| --- | --- | --- |
| When the shadow runs | After the verified binding's read returned, before the call's result is sent, within a time bound (`EngineSettings::shadow_limit`, 2 seconds). The result is the verified binding's whatever the shadow does | Beside the verified read, at the same time: the loop holds one driver context at a time by `&mut`, and two reads at once on one session is a concurrency the scheduler has not granted. Detached, after the job: the job's driver contexts and session lease end with it, and a shadow that outlives its job reads under nothing |
| How often | At most once per operation in a job: the first call of it that succeeded | Every call: a pipeline over ten thousand rows would pay the bound ten thousand times |
| Which jobs | An `Execute` job of a tool that only reads, with no mode pinned and no binding named, run live: never a `Try`, an offline test, a canary, a preview, or the reads of a writing tool | Shadowing the reads of a writing tool: they decide what is written, and a shadow there would sit between a preview and its approval |
| The posture of a shadow's context | `Sandbox` with no test records: the driver's own read-only guard is on and the handle refuses every step that commits, because the binding has not earned trust. Its contexts are never used for the live call, nor the live call's for it | `Live`, as any read: the shadow binding's steps say they only read, but nothing has proved it |
| Consecutive or total | Consecutive: a mismatch starts the count again; a shadow that was not compared (it failed, or passed its bound) neither counts nor resets. Section 201 says "a set number of exact matches" and is silent on order | A total: a binding that matched twenty times and then differed would still be promotable |
| Where counting lives | The ledger, in tables of its own (migration `0020`), from append-only rows: the count is a query, so it cannot drift from the evidence | The engine's `bindings_health` table, written through `Control`: no such table exists, and the rule "a mismatch resets" belongs where the data is, as the nine rules do |
| What is kept of a mismatch | The names of the outputs that differed and how (`differs`, `missing`, `unreadable`); no value and no digest of one | Keyed digests of each side's value: they need a digest purpose of their own in `a2m-core`, which this package does not own |
| The shadow of a write | Not built. It runs nothing and stores nothing; a binding of an operation that writes is never promotable through shadow (`shadow_write_not_built`) | Running the shadow binding's read-back after the verified write: it would prove the shadow reads the record, not that it would write it, and counting it toward promotion would claim more than was shown |

## Decision

[filled in after the build]
