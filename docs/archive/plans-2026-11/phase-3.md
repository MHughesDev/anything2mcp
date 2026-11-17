# Phase 3: Writes

> Archived on 2026-11-17. This plan was written for the first design and is kept as the record of it. The order of work is now the [roadmap](../../plan/roadmap.md).

A draft task list, to be revised at the end of Phase 2 and agreed with the owner before starting.

**Outcome.** A person can have an agent enter data: a pipeline reads a spreadsheet, shows exactly what will change, waits for the person's approval, writes each record, reads it back, and can be stopped and resumed without duplicating anything. This completes the first usable product.

**Exit test.** A spreadsheet-entry pipeline on the test application passes the round-trip, idempotency, resume, and fault-injection gates; killing the engine at a random step and resuming leaves no duplicate and no unverified record, as reported by the test application itself; an agent with a shell cannot approve a write.

**Read first.** The Phase 3 entry in the [phase map](phase-map.md). Briefs 27 (user-presence prompts) and 31 (the policy engine spike) decide parts of groups B and F; run them first.

## Task groups

| # | Group | Tasks | Done when |
| --- | --- | --- | --- |
| A | Ledger | The tables of the [ledger schema](../../reference/ledger-schema.md); the nine rules of spec section 42 as tests; record locks; the intent window; caller idempotency keys | Each rule has a test that fails when the rule is broken |
| B | Policy | The policy engine with the built-in local policy set and guardrails; record limits and quotas; kill switches; protected settings | A write without an approval is refused; a guardrail cannot be switched off by any setting; a protected setting needs the user-presence prompt |
| C | Values | Field types and every normalization rule of spec section 12; the starting library of expression functions | Test-application case 16: a value the application rounds or reformats is compared correctly, and a real difference is a `mismatch` |
| D | Sources | The xlsx, csv, and json readers; template fingerprints; mappings; allowed file roots and content-type checks | A changed template is rejected with its differences; a path outside the allowed roots is refused |
| E | Preview | Reading current values; classing each record create, update, or unchanged; the diff and its hash | The same proposal against the same data gives the same hash on three operating systems |
| F | Approval | The diff page, the short-lived loopback listener, notifications, and the operating-system prompt or its fallback, as in spec section 49 | A person approves a forty-record diff; the page alone cannot approve; a stale diff is refused |
| G | Writing | The write verbs of the `ui` driver; widget drivers for text, number, select, checkbox, date, and the filtering dropdown; read-back; version preconditions | Cases 2, 3, 7, and 10 pass as writes |
| H | Identity | The five identity strategies; storing assigned keys and results; dependent records; one save with nested records | Cases 18, 19, and 20 pass |
| I | Resume | Reconcile with its three outcomes; `run.resume`, `run.cancel`, `run.list`; the freshness check | Cases 17 and 21 pass; a resumed run repeats nothing |
| J | Gates | `test.run` with the round-trip, idempotency, resume, and create gates; `fixture.record` with scrubbing | The gates fail a pipeline that writes twice |
| K | Engine faults | The database, browser, and process-death rows of spec section 52; the three health states | Each fault is induced and the test application shows no duplicate and no unverified record |
| L | Manipulation tests | The first cases of the manipulation suite: case 27, a crafted path input, a crafted file path | Each attempt is refused or neutralized, with the right failure code |

## Not in this phase

HTTP bindings for writes, endpoint mining, the full authoring toolkit, servers, runners, and undo. A write here always goes through the interface.

## Questions for the owner

- Whether the measured cost of read-back and approval is acceptable for the first real pipeline.
- For the first real write: which test record or test environment may be written to, and who has agreed.
