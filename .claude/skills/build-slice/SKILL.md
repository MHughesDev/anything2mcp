---
name: build-slice
description: Build one slice of the Anything2MCP core, slowly and with proof. Use when implementing any package from docs/plan/roadmap.md or any other piece of product code.
---

# Build one slice

Follow the eight steps in `docs/plan/working-method.md`. This is the checklist form.

## Before writing code

1. **Frame.** State the slice in one sentence, taken from `docs/plan/roadmap.md`. Read the design of each crate it touches in `docs/architecture/crates/`; build the first unbuilt step of its build order. List the hypotheses it tests from `docs/hypotheses/README.md`. Confirm that the slice before this one is proven; if it is not, stop and finish that first.
2. **Read** only the spec sections and references the slice needs. Check `docs/research/11-open-research.md` for a brief that should be run first.
3. **Question.** Name at least one alternative to the documented design for this slice, and say in a few lines why the documented design is or is not still the better bet. If the alternative looks better, test it with the `test-hypothesis` skill or record the change of design as a decision; do not quietly build either.
4. **Plan.** List the crates touched, which side of the kernel boundary each piece falls on, the interfaces added, and the tests that will prove the slice. Write the plan in the pull request or the journal, then build. Nobody has to approve it.

## While building

5. **Spike** anything uncertain in `spikes/`, in the cheapest way that gives an answer. Spike code is never promoted; real code is written fresh.
6. **Build** the smallest real implementation. Follow every rule in `AGENTS.md`. Verify each new dependency before adding it: it exists, is maintained, carries a licence on the closed list in `deny.toml`, and supports Windows, macOS, and Linux.
7. **Prove** it with tests against a test stand, asking the stand what it stored. No test, no claim. Write the fewest tests that prove the slice; tests secure the system and do not advance it.
8. **Measure** anything with a budget in spec section 225 or in the slice's own section.

## After building

9. **Reflect.** What did this show about the design? Write hypothesis results. Correct sections that turned out wrong. Add a decision record for any decision made or changed. Add new hypotheses for doubts that came up.
10. **Ship and report.** Mark the package `done` in `docs/plan/roadmap.md`, and change what it unblocks to `ready`. Push, open the pull request, and merge it when every check is green. Report what works, the numbers, what changed in the design, and what is next.

## Limits

- One slice. If it grows, split it and finish the smaller part.
- Nothing from a later phase. Note the need in the journal and stop.
- If a long time has passed without anything being proven, stop and say so.
