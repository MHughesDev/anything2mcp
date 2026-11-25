# 0207. A shadow binding reads beside the verified one, and a person promotes it on a run of exact matches

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the designs of [`a2m-engine`](../architecture/crates/a2m-engine.md) and [`a2m-ledger`](../architecture/crates/a2m-ledger.md); sentences of spec sections [201](../spec/2-kernel/201-access-kinds-and-modes.md), [207](../spec/2-kernel/207-the-engine-loop.md), [219](../spec/2-kernel/219-the-evidence-ladder.md), and [225](../spec/2-kernel/225-performance.md); failure codes of the [technical reference](../reference/technical-reference.md). Builds roadmap package 4.3 for reads; the shadow of a write is named and not built.

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

| Part | Built | Proven by |
| --- | --- | --- |
| The ledger's evidence | Migration `0020`: `shadow_comparisons`, one row per comparison by the shadow binding's digest, with the binding it was compared against and the run; `shadow_promotions`, one row per promoted digest. Both append-only by trigger; a comparison is refused unless its run is `running`. `Tx::shadow` gives `Shadows`: `record` (refuses a writing operation with `busy`/`shadow_write_not_built`, an offline test's store, and a run that is not a read under way with `policy`/`shadow_not_counted`, a stale fence with `runner_lost`; takes workspace, target, and environment from the run), `standing`, `promotable`, `promote`, `report` | `a2m-ledger/tests/shadow.rs`, through the calls and a second connection; the rule tests still pass |
| Shadow of a read | `Control::keeps_shadow` (false by default) and `Control::shadow` (`busy`/`shadow_not_kept` by default), `ShadowCompared`, `EngineSettings::shadow_limit`. As the plan above says, with the comparison output by output after normalization | `a2m-engine/tests/shadow.rs`: the caller's result is the verified binding's when the shadow matches, differs, fails, or hangs; the shadow's posture; the trace signature unchanged; a mismatch named by output; nothing in a try, an offline test, a pinned call, or a named binding |
| Promotion | `Shadows::promotable` answers `Yes` at the threshold's matches in a row (`ShadowThreshold`, 5 for a read and 20 for a write, the defaults of section 226), else why not: nothing compared for the digest as it is now, too few, a mismatch since (with what differed), promoted already, or a writing operation. `Shadows::promote` asks again in its own transaction and records who promoted it on what evidence; it refuses with `policy`/`shadow_not_promotable` | Both test files: below the threshold, after a mismatch, for a changed digest, for a write, a second time |
| After a promotion | The ledger does not change a binding's status; the caller rewrites the file. The binding that was verified stays `verified` as a fall-back, and the order of bindings, the operation's or the modes' ranks, puts the faster first. Section 201 is silent on it; this follows section 225's "fastest verified mode first" | Not tested here: the rewrite is the action registry's |
| The shadow of a write | Not built, and named: a writing tool's job runs no binding in shadow, the ledger refuses its comparison, and such a binding is never promotable through shadow | `a_write_never_runs_its_shadow_binding_and_goes_through_the_approved_one`: the stand holds one write, sent through the approved binding |

## Consequences

- A live read whose operation has a binding in shadow may take up to `shadow_limit` longer, once per operation in a job. A cheap shadow (a direct request beside an interface read) adds its own time only.
- `shadow.report` and the shadow branch of `binding.promote` can be built in the action registry on `Shadows::report`, `Shadows::promotable`, and `Shadows::promote`, with the binding's digest from the engine's `BindingPin::of` over the drafts; they rely on a binding's digest being the same in the drafts and in the published plan when its files did not change, which decision 0200 states and the registry should test.
- Hypothesis H-10 is tested for reads only, and only in the sense that an output the shadow binding gives differently is caught; logic that only shows in some data is caught only where a live call carries such data before promotion.
- Not built: the shadow of a write; the setting `engine.promoted_writes`, which nothing reads; a keyed digest of the differing values; a setting key for `shadow_limit` (section 226 owes one).
