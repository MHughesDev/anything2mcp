---
name: build-slice
description: Build one slice of the Web2MCP core, slowly and with proof. Use when implementing any slice from docs/plan/core-first.md or any other piece of product code.
---

# Build one slice

Follow the eight steps in `docs/plan/working-method.md`. This is the checklist form.

## Before writing code

1. **Frame.** State the slice in one sentence, taken from `docs/plan/core-first.md`. List the hypotheses it tests from `docs/hypotheses/README.md`. Confirm that the slice before this one is proven; if it is not, stop and finish that first.
2. **Read** only the spec sections and references the slice needs. Check `docs/research/11-open-research.md` for a brief that should be run first.
3. **Question.** Name at least one alternative to the documented design for this slice, and say in a few lines why the documented design is or is not still the better bet. If the alternative looks better, stop and raise it with the owner; do not quietly build either.
4. **Plan.** List the crates touched, the interfaces added, and the tests that will prove the slice. Show the owner and wait for a go-ahead.

## While building

5. **Spike** anything uncertain in `spikes/`, in the cheapest way that gives an answer. Spike code is never promoted; real code is written fresh.
6. **Build** the smallest real implementation. Follow every rule in `AGENTS.md`. Verify each new dependency before adding it: it exists, is maintained, has a permissive license, and supports Windows, macOS, and Linux.
7. **Prove** it with tests against the test application, asking the application what it stored. No test, no claim.
8. **Measure** anything with a budget in spec section 9 or in the slice's own section.

## After building

9. **Reflect.** What did this show about the design? Write hypothesis results. Correct sections that turned out wrong. Add a decision record for any decision made or changed. Add new hypotheses for doubts that came up.
10. **Show** the owner: what works, the numbers, what changed in the design, what is next.

## Limits

- One slice. If it grows, split it and finish the smaller part.
- Nothing from a later phase. Note the need in the journal and stop.
- If a long time has passed without anything being proven, stop and say so.
