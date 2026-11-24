# 0399. A surface its driver holds off is refused before the run sends anything, with the driver's code

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** how a binding qualifies in `a2m-engine` (`Run::admits` in `run.rs`); what a reconcile that cannot read a record in doubt says (`reconcile` in `tool.rs`); the choosing and reconcile paragraphs of [`a2m-engine`'s design](../architecture/crates/a2m-engine.md).

## Context

A driver says from memory whether it may be chosen for a surface now: `DriverHealth::Ready`, `Degraded(code)`, or `Unavailable(code)`. The web pack's driver says `Unavailable` for an origin whose edge refused a visit, for 60 seconds, with the code of the refusal ([decision 0355](0355-the-web-pack-handles-the-edge-a-fixed-language-and-the-pages-of-cases-22-to-27.md)). The engine read any health but `Ready` as `busy`, `no_binding`, "the driver is not ready", which a caller cannot act on and which hides what the driver knew. A run could not learn that the surface was held off at the edge until a step sent a request and was refused again.

The same work showed a second gap. A run whose write was sent and is in doubt is reconciled by a job the scheduler queues. When that reconcile could not read the record, because the edge refused the read, its failure became the run's, `edge_blocked`, which a front door reports as safe to call again, while the write was still in doubt.

## Decision

1. **A driver's `Unavailable(code)` is the binding's failure, in the class its provider's manifest declares.** The engine looks the code up in the failure codes of the provider that drives the mode, and the binding does not qualify with that class and code: `edge_blocked`, `forbidden_by_edge`, for the web pack's held origin. When no binding qualifies, the run stops with it as with any other reason, ranked as before (a session first, then an edge block, then drift). Nothing reaches the driver: the check is made when the binding is chosen, before the preview's read, the freshness check, or the write. A code the provider does not declare is not guessed at: the binding does not qualify as `busy`, `no_binding`, as before. `Degraded` is unchanged.
2. **On a resume, the binding is checked when the record is read, not when the run's records are worked out.** The approved diff names the binding; whether it may act now is asked when the record is read, after the records in doubt are reconciled, so a check of the binding never speaks for a run with a write in doubt.
3. **A record in doubt that a reconcile cannot read leaves the run `possible_commit`, by the same rule as a failure after the save.** A failure of the read becomes `possible_commit`, `no_confirmation`, naming the class and code it stands in for (`after_send`, the one place both paths use): an edge block, a surface its driver holds off, a lost connection, a busy surface. The record stays in doubt, its lock is kept, and the scheduler reconciles again after a wait that grows with each reconcile in a row. Two cases stand apart, as before: a failure of a class that would settle the run (`policy`, `input`, `stale`) is `possible_commit`, `cannot_reconcile`, and waits for a person; and `session_expired` rests the run at `needs_login`, because a reconcile queued again would only spend the sign-in's attempts against a credential the system refuses, which the product test showed (the person's own sign-in was then refused as `credential_refused`).

*Instead of:* reading the driver's health at the start of the job, outside the choosing: the choosing is where the other conditions of a binding are read (the session, the edge block, drift), and an operation with a second binding on another surface should still be carried out by it. *Instead of:* making the front door say `retry_safe: false` for any run with a record in doubt, whatever its failure says: the class is what every reader acts on.

## Cost

The class comes from the provider's manifest through the registry's listing, read only when a driver says `Unavailable`. The engine's test that no failure is made with a class of its own counts this one place in `run.rs`: the class is the manifest's, which is the provider's table.

## Proof

`crates/a2m-engine/tests/faults.rs`: `a_surface_its_driver_says_is_unavailable_starts_nothing_and_names_the_drivers_code` (a preview and an approved run on a surface the test driver holds off stop with `edge_blocked`, `example_held_at_edge`, and no step reaches the driver; an undeclared code is `busy`, `no_binding`; ready again, the run writes once), and `a_record_in_doubt_that_cannot_be_read_leaves_the_run_possible_commit`. Through the product, `crates/anything2mcp/tests/faults.rs`, `a_surface_an_edge_blocks_writes_nothing_and_a_block_during_the_read_back_leaves_doubt`: the record fails in doubt when its read-back is refused at the edge, the reconcile is refused as well, and the run says `possible_commit`, `no_confirmation`, `retry_safe: false`; once the edge lets reads through, the resume reads the record and sends nothing more. The test is the one package 3.10 wrote, unchanged.
