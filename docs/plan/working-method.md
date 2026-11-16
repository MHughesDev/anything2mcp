# Working method

How this project is built: slowly, core first, one small proven step at a time, with the design treated as something to keep improving, not a finished blueprint to type in.

## The stance

- **Slow is the plan.** The design is large. The way through it is a narrow path of small steps that each work, not a wide front of half-built parts. One slice per session is a good pace. Fewer is fine.
- **Core first.** The engine loop, one driver, sessions, the ledger, the policy gate, and the compiler are the product. Servers, runners, extra modes, and consoles are built on a core that already works. [Core first](core-first.md) gives the order.
- **The design is a set of bets.** Much of it has never met running code, and most of its research is second-hand. Every slice is a chance to find out that a section is wrong. Finding that out is success, not a setback.
- **Improving the design is part of the job.** An agent working here is expected to question the documents, propose better designs, and record what it learns, as well as write code.

## The loop for every slice

| Step | What happens | Output |
| --- | --- | --- |
| 1. Frame | State the slice in one sentence and the hypotheses it tests, from the [register](../hypotheses/README.md) or new ones | A short plan the owner has seen |
| 2. Question | Before building, ask what else could work. Name at least one alternative to the documented design and say why the documented one is or is not still the better bet | A paragraph in the plan |
| 3. Spike | If anything is uncertain, try it in `spikes/` in the cheapest way that gives an answer. Spike code is thrown away, never promoted | A measurement or a clear answer |
| 4. Build | The smallest real implementation that makes the slice work, following every rule in AGENTS.md | Code with tests |
| 5. Prove | Tests against the test application, asking the application what it stored | Passing tests |
| 6. Measure | Time and size for anything with a budget | Numbers |
| 7. Reflect | What did this teach about the design? Update the documents: a hypothesis result, a decision record, a corrected section | Documents that are true again |
| 8. Show | Report to the owner: what works, what was learned, what changed in the design, what is next | The owner's go-ahead for the next slice |

Steps 2 and 7 are the ones most easily skipped and the ones that make the design better. Do not skip them.

## Three kinds of change to the design

| Kind | Example | What to do |
| --- | --- | --- |
| Clarification | A section is ambiguous, or two sections disagree | Fix the documents in the same change; no approval needed; run the audit |
| Discovery | A spike or measurement shows a section is wrong | Write the hypothesis result, correct the section, and add a decision record if a decision changed. Tell the owner in the slice report |
| Proposal | A different design looks better, with no evidence yet | Add it to the hypothesis register, or write a decision record with status Proposed. Do not build on it until the owner accepts or a test supports it |

A decision the owner made is changed only by the owner. Everything else is open to challenge with evidence.

## Thinking ahead while building

While building one slice, an agent will notice things about parts not yet built. Capture them without acting on them:

- A doubt about a later section goes into the hypothesis register.
- A missing piece goes into the journal entry under "noticed".
- A better idea for a later phase goes into a Proposed decision record.

None of these justify building the later part now.

## Design review rounds

At the end of each phase, and whenever five slices have passed without one, run a design review round with one of the [lenses](design-review-lenses.md). A round finds problems and fixes the documents. It writes no product code.

## Continuity between sessions

An agent starts each session knowing nothing of the last one. The repository is the memory.

- **Start** by reading the latest entry in the [journal](../journal/README.md).
- **End** by writing a new entry: what was done, what was learned, what is undecided, what to do first next time.
- A session that ends without a journal entry has lost its work for the next agent.

## When to stop and ask the owner

- A hypothesis test contradicts a decision.
- A slice needs something from a later phase.
- A spike shows the documented design cannot meet a budget or a guarantee.
- Anything on the "ask the owner before" list in AGENTS.md.
- The agent notices it has been building for a long time without proving anything.
