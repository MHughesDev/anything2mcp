# 0251. The evidence for promoting a shadow binding says what it covers, and a shadow costs what it says

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Refines** [decision 0207](0207-a-shadow-binding-reads-beside-the-verified-one-and-a-person-promotes-it-on-a-run-of-exact-matches.md) and [decision 0219](0219-shadow-bindings-reach-the-product-and-the-first-hour-is-mended.md): the count that makes a binding promotable, the budget a shadow holds a call up by, and what a shadow read tells. It answers findings S5, M8, M9, M10, M11, and M12 of the fifth safety audit (answers that are not JSON, writes from inside a page, shadow bindings).

**Changes:** the designs of [`a2m-ledger`](../architecture/crates/a2m-ledger.md) and [`a2m-engine`](../architecture/crates/a2m-engine.md); the [ledger schema](../reference/ledger-schema.md). Migration `0022-ledger-shadow-evidence-names-what-it-covers.sql`, the next free number. The action registry's half (`binding.promote`, `shadow.report`, the control plane, the settings schema) is stated below as what it must do; it is applied in `a2m-actions`.

## The findings, checked in the code

| Finding | Verified | What the code did | Fix | Test (fails before the fix) |
| --- | --- | --- | --- | --- |
| S5. A steered agent can make the evidence cheaply | Yes. `Shadows::standing` counted by workspace, target, operation, and binding digest only; the table held no input | Five calls with one input, in any environment, gave "5 in a row" and `promotable: true` | A comparison keeps the workspace's keyed digest of the call's input. A binding is promotable when, in every environment its evidence must come from, at least the threshold of **distinct inputs** matched, since the last mismatch, against the verified binding that will act beside it | `a2m-ledger/tests/shadow.rs`: five matches of one input are one distinct input and are not promotable, five distinct inputs are; `a2m-engine/tests/shadow.rs`: five live calls of one order are `too_few` with one distinct input |
| S5, second half: only the first call of an operation in a job is shadowed | Yes (`run.shadowed`) | | Kept as it is, on purpose: see "Alternatives" | |
| M12. Matches count across verified bindings | Yes. `against_digest` was stored and never read | A match against a verified binding that was later replaced still counted | Only matches against the subject's verified binding count; a mismatch against any verified binding resets | `matches_against_an_earlier_verified_binding_do_not_count` |
| M11. The rewrite rechecks only the operation's file | Yes (`promote.rs`, `rewrite`) | An endpoint or screen file the binding's digest covers could change between `locate` and the write | The ledger's half: the promotion row names the binding digest it was promoted as (it did) and now the verified binding's digest and each environment's evidence. The registry's half: `rewrite` locates the binding again from disk and refuses when its digest is not the one promoted | The ledger's: the row reads back with `against` and `evidence`. The registry's test is owed with its lines |
| M10. Promoting from shadow has no presence check | Yes (`promote.rs`: `!call.caller.is_agent` only) | A process with a shell on the machine could run the command as the person | Promoting from shadow asks for the machine's presence prompt, as `version.publish` does | Owed with the registry's lines |
| M8. A shadow can hold the call up by several times its limit, plus a ledger write | Yes. One `timeout` per shadow binding, then a `Synced` write outside any bound; the setting `engine.shadow.time_limit_ms` up to 60 000 and unprotected | N bindings held the call by N times the limit, and a slow store by more | One budget per job for all its shadows, cut short by the call's own time limit and lease. Reads may take three quarters of it. Comparisons are kept only after the call's result was sent, in what is left of it, mismatches first. A keeping that does not end in time or fails is dropped and counted. The setting's safer direction is lower, and it is protected | `every_shadow_of_a_call_shares_one_budget_and_its_keeping_never_holds_the_answer`: a shadow that hangs beside a keeping that hangs delays the call by at most the budget, and a failed keeping does not change the answer |
| M9. A shadow read is shown to nobody, and spends the caller's budget | Yes: `observe` asked `Control::rate` as the live call does. The sign-in the auditor suspected is real: `session_lease` for another surface asks the keeper, which can wait for a person (`Wait::Interactive`) | A second request to a person's system that nobody saw, which could leave a caller with `no_rate_budget`, and could prompt a sign-in | Each comparison keeps what its read cost in requests (one per step granted), and its keeping appends a run event that says a shadow read was made, how it compared, and what it cost. `shadow.report` shows the reads and requests by environment. A shadow asks for its budget through `Control::shadow_rate`: all at once or nothing, never waiting, and never below the share a watch's check leaves for callers (decision 0247). A shadow reads only through a session the call already holds and never asks for one | `a_shadow_never_takes_what_a_callers_read_needs_of_the_rate_budget`; `a_shadow_never_asks_for_a_session_the_call_does_not_hold`; the run event in `a_shadow_read_runs_beside_the_live_one_...`; `the_report_shows_counts_and_cost_and_never_an_input_or_its_digest` |

## The counting rule

A binding is promotable when, for **every** environment its evidence must come from, the number of distinct inputs whose comparison matched, against the verified binding named, after the last mismatch, is at least the threshold (`engine.shadow.matches_to_promote.read`, 5).

- **Distinct inputs.** An input is the call's inputs, by the workspace's keyed digest of their canonical bytes, under a label of their own. Two calls with equal inputs are one input. The threshold applies to this count, never to the count of matches.
- **Per environment, and which environments.** The environments are the plan's environments of kind `production`; a plan with none needs every environment it has (the engine's `shadow_evidence_environments`). Each must meet the threshold on its own. Matches in a test environment never count toward production: its data may have been written by the caller whose calls are the evidence, and a binding that is promoted acts in every environment, so the evidence must be from the environment it will serve.
- **Against the verified binding that will act.** Only matches against the digest of the first `verified` binding of the operation, as the drafts hold it at promotion (the engine's `verified_binding`), count.
- **A mismatch resets everywhere.** The last mismatch in any environment, against any verified binding, starts the count again in every environment. Evidence against a binding is never narrowed by where it came from.
- **Not compared** neither counts nor resets, as before.
- A comparison kept before migration 0022 names no input, so it counts as a match or mismatch in the totals and as no distinct input: it never makes a binding promotable.

### Alternatives weighed

| Question | Taken | Weighed and not taken |
| --- | --- | --- |
| What is counted | Distinct inputs, since the last mismatch | Matches in a row (decision 0207): one input called five times is five. Distinct records by key: needs the operation's identity, which a read of a list or a search lacks; the input is what the caller controls, so it is what must vary |
| Where the input is digested | In the ledger, in the transaction, with the vault's keyed digest it already holds, as a watch's record keys are (decision 0247) | In the engine with `EngineDeps::keyed`: two places that hold the key's use. A plain digest: a reader of the store could test a guess at a low-entropy input such as an order number |
| The digest's purpose | `DigestPurpose::CallerKey`, behind one function, with a label no other digest of that purpose writes | A purpose of its own: `a2m-core` is not this package's to change; the one line it needs is below |
| Environments | Production's own, or all where there is none | Any environment (S5's open door); per environment with a promotion per environment: a binding's status is not per environment in the target's files |
| A mismatch elsewhere | Resets everywhere | Resets only its own environment: a binding that differs anywhere has shown it can differ |
| Which calls are shadowed | The first call of each operation in a job, as before | Every distinct input in a job: a pipeline of a thousand rows would read the system twice a thousand times. Sampling: random choice would make the evidence depend on chance. The count of distinct inputs makes the bias visible instead: the person sees how few inputs the evidence covers |
| What a person sees | The distinct inputs and the matches in each environment, the reads and their cost in requests, and the matches against the binding verified now | Showing the inputs: they are business data, and the evidence does not need them. Showing the digests: they would let a reader match calls across reports |

## The budget

`EngineSettings::shadow_limit` (setting `engine.shadow.time_limit_ms`, 2 000 ms) is one budget per job, shared by every binding in shadow of every operation in it, set when the first starts and cut short by the call's own time limit and by its lease. A shadow starts only inside the reading part, the first three quarters; it is cancelled at its end, and counted as not compared. The comparisons are kept by `Control::shadow` only after the call's result was sent, within the budget's end, mismatches first. One that is not kept in time, or whose keeping fails, is dropped and counted in `Engine::shadow_comparisons_lost`. So the call's final state is applied at most one budget after its first shadow began, and its result is stored before any comparison. A dropped mismatch is the one loss that matters: written first, it is lost only when the store is failing or slow past the budget, and the store's writer may still finish it after the engine stops waiting.

The setting's safer direction is lower: a longer budget holds every call longer and lets a shadow read more. It is protected against raising, as the thresholds are against lowering.

## What a shadow read tells, and what it may spend

- The run's events hold one `need` event per comparison kept, with `shadow_read: true`, the operation, mode, position, outcome, the comparison's number, and its `requests`; never a value or a digest.
- `shadow.report` shows, per binding and per environment, the reads, the requests they cost, the matches since the last mismatch, how many were against the binding verified now, and the distinct inputs.
- The rate budget: a shadow's request is all or nothing, does not wait, and leaves the share decision 0247 leaves for a caller's own reads (`watch.reserve_percent`, 50). The scheduler's admission for a watch's check reads that share from the run being a check's; a shadow runs inside the caller's own run and lease, so it cannot be told that way. It asks through `Control::shadow_rate`, which the control plane answers with the scheduler's take, with that share and without waiting. By default the answer is no, so a shadow on a surface that declares a rate is not compared until the control plane implements it.
- The session: a shadow reads only through a surface on which the job already holds a context under a session lease, so the session is live; it never asks the keeper for one, and so can never ask a person to sign in. One that would need to is not compared, `busy`/`no_binding`. Residue: the lease it is prepared under is a second lease of a live session, which the keeper grants without a person unless the session ended in between.

## What a person sees before promoting

`shadow.report` for the binding: in each environment, how many distinct inputs matched the verified binding that will act, since the last mismatch, beside the threshold; how many reads were made and what they cost; and the last mismatch, with the outputs that differed. `binding.promote` then asks for the machine's presence prompt, refuses unless every environment the evidence must come from meets the threshold, records the verified binding and each environment's counts in the promotion row and the audit line, and rewrites the file only while the binding still has the digest it was promoted as.

## A shadow and a driver's read-only guard

A shadow's context is prepared in `Sandbox { test_records: false }`, the posture of a try, and a driver cannot tell the two apart. No contract change is proposed, because the shadow needs no more than a try is given: the HTTP driver's guard (decision 0249) refuses, unsent, any request that may change the system outside a live run, and lets an endpoint marked `read_asserted` through only against an environment of kind `test`. So a shadow of a binding whose read is such a request is refused in production with `policy`/`read_only_posture` and kept as not compared, and in a test environment it is sent exactly as a try of it would be, on the developer's own declaration that it only reads. The engine adds no refusal of its own: knowing which endpoint may write is the pack's, and the pack's guard already answers it (Choice, this record).

## Codes

No new failure code. A shadow refused its rate budget is `busy`/`no_rate_budget`; one whose surface's session the call does not hold is `busy`/`no_binding`: both say what they say elsewhere, and both are kept as not compared.

## The registry's half (to apply in `a2m-actions`)

- `InProcessControl::shadow`: pass `input: compared.input` and `requests: compared.requests` into `ShadowComparison`.
- `InProcessControl::shadow_rate`: the surface's budget key as `rate` makes it, then the scheduler's grant of all `want` at once, leaving `watch.reserve_percent`, without waiting.
- `promote_from_shadow`: the presence prompt; the subject's `against` and `environments`; the evidence in the output and the audit line; `rewrite` rechecks the binding's digest from disk.
- `shadow.report`: `against` by operation from the drafts when the call has a project; `distinct_inputs`, `environments`, `requests`, and `against_verified_now` in each binding's entry.
- The settings schema: `engine.shadow.time_limit_ms` protected, `Safer::Lower`.
