# 0177. A target's offline tests run against a replay of their fixtures

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the designs of [`a2m-actions`](../architecture/crates/a2m-actions.md), [`a2m-engine`](../architecture/crates/a2m-engine.md), and [`a2m-provider`](../architecture/crates/a2m-provider.md); the guides on operations and on guidance. Builds on [decision 0164](0164-the-authoring-actions-served-through-the-engine.md), which listed every offline test as not run.

## Context

The compiler asks for an offline test per operation before a publish, and nothing could run one. A test names a tool, an input, a fixture folder, and the result it must give; the fixture is not in the plan.

## Decision

| Question | Decided | Instead of, and why |
| --- | --- | --- |
| Where the replay sits | In the contract: `Replay`, a `Driver` of recorded answers that every pack gets, handed to each binding through `Registry::replay_driver`, which keeps the chosen provider's step kinds, commit kinds, and codes, so the handle checks every step as it would the pack's own driver. `Engine::run_offline` runs a job with it in place of every provider's driver and asks for no session; a discovery job is refused | A replay port the engine answers itself: it would bypass the handle's checks. A replay driver per pack: every pack would write one |
| How an answer is matched | By what the contract knows of every step: the entry the step refers to, or that entry's last part after a `/`, or the step's kind when it refers to none; optionally by its arguments; in recorded order, each once. A step with no answer left is `unknown_variant`, `fixture_no_answer`, and nothing is sent | Matching by position alone: a binding's steps change order more often than their names |
| The fixture's form | `fixture.yaml` saying `scrubbed: true`, and `<name>.json` per answer: an object of the values observed, `null` for an absent record, or a list of those | The system's raw answer: turning it into values is the pack's driver's work, which a replay does not run |
| What runs | The offline tests of reads, as `Try` jobs (the sandbox's posture, so a committing step is refused by the handle), under a control plane that stores nothing, leases nothing, and clears nothing. The result is compared field by field with `expect`: equal JSON, equal text, or equal exact numbers; each difference is listed | Running them as runs of the store, which would put test runs in the ledger and ask a person to approve a test's write |
| A write's offline test | Listed `not_run`. It needs a ledger of its own for its intent, its save, and its read-back. What it could prove: that the preview, the save's arguments, and the read-back turn the recorded answers into the expected result. What it cannot: that the system accepts the save | Running its preview alone, which proves less than the test states |
| `passed` and `complete` | `passed` when every offline test ran and passed; `complete` stays false while gates are not built | None |
| The publish gate | Not switched on. Today it would refuse every target with a write test, and the product's test project `stockfiles`, whose fixture is a file the pack reads and not an answer | Refusing on a failed test alone: that still refuses `stockfiles` |

## Consequences

- The example target's read test passes through `test.run`; its two write tests are not run.
- The product's test projects: `orders` and `upstream` name their answers after the endpoint and the upstream tool, which the matching takes. `stockfiles/fixtures/stock_get/stock.csv` is the file the file pack reads; the replay needs `stock.csv.json` (the document's name, as the step refers to it) holding the values the read step observed, or the contract needs a recorded file a pack's own driver may read offline.

## Revisit if

A write's offline test runs against a ledger of its own: then the publish gate goes on.
