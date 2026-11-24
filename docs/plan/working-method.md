# Working method

How this project is built: slowly, core first, one small proven step at a time, with the design treated as something to keep improving, not a finished blueprint to type in. Agents do this work without waiting for anyone ([decision 0055](../decisions/0055-agents-plan-build-and-merge-without-waiting-for-the-owner.md)); the care is in the size of each step and the proof of it.

## The stance

- **Small steps are the plan.** The design is large. The way through it is small steps that each work, not a wide front of half-built parts. One slice per session is a good pace for one agent. To go faster, add agents on separate packages; never make the step bigger.
- **Core first.** The engine loop, one driver, sessions, the ledger, the policy gate, and the compiler are the product. Servers, runners, extra modes, and consoles are built on a core that already works. [Core first](roadmap.md) gives the order.
- **The design is a set of bets.** Much of it has never met running code, and most of its research is second-hand. Every slice is a chance to find out that a section is wrong. Finding that out is success, not a setback.
- **Improving the design is part of the job.** An agent working here is expected to question the documents, propose better designs, and record what it learns, as well as write code.

## The loop for every slice

| Step | What happens | Output |
| --- | --- | --- |
| 1. Frame | State the slice in one sentence and the hypotheses it tests, from the [register](../hypotheses/README.md) or new ones | A short written plan, in the pull request or the journal |
| 2. Question | Before building, ask what else could work. Name at least one alternative to the documented design and say why the documented one is or is not still the better bet | A paragraph in the plan |
| 3. Spike | If anything is uncertain, try it in `spikes/` in the cheapest way that gives an answer. Spike code is thrown away, never promoted | A measurement or a clear answer |
| 4. Build | The smallest real implementation that makes the slice work, following every rule in AGENTS.md | Code with tests |
| 5. Prove | Tests against a test stand, asking the stand what it stored. The fewest that prove the slice | Passing tests |
| 6. Measure | Time and size for anything with a budget | Numbers |
| 7. Reflect | What did this teach about the design? Update the documents: a hypothesis result, a decision record, a corrected section | Documents that are true again |
| 8. Ship | Run `python3 tools/gate.py` until it is green, then push, open the pull request, merge when every check is green, and write the journal entry: what works, what was learned, what changed in the design, what is next | Merged work and a journal entry |

Steps 2 and 7 are the ones most easily skipped and the ones that make the design better. Do not skip them.

## Three kinds of change to the design

| Kind | Example | What to do |
| --- | --- | --- |
| Clarification | A section is ambiguous, or two sections disagree | Fix the documents in the same change; run the audit |
| Discovery | A spike or measurement shows a section is wrong | Write the hypothesis result, correct the section, and add a decision record if a decision changed |
| Choice | A different design looks better, and the decision it changes is a design decision | Test it if a cheap test exists. Otherwise weigh it in writing and record the choice as a decision that supersedes the old one, saying the owner has not read it |
| Proposal | The change touches an owner decision or one of the ten rules | Write a decision record with status Proposed and carry on with other work. Do not build on it |

A decision the owner made, and the ten rules in AGENTS.md, are changed only by the owner. Everything else is open to any agent with evidence.

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

## Working in parallel

- One work package, one branch, one pull request. The [roadmap](roadmap.md) says which packages can run at once.
- Two agents do not change the same crate at the same time. Documents that many packages touch, such as the indexes, are changed last and in small edits.
- An agent that leads others gives each a package with a stated proof, checks the proof, and merges what passes. It does not hand over its conclusion and ask for it to be confirmed.
- Whoever merges second brings their branch up to date first. Nobody rewrites another agent's history.
- A lead that gathers several agents' work into one batch runs `python3 tools/gate.py` on the whole batch, in a build folder of its own, before it pushes; a worker's checks scoped to its own crates do not show what its change broke in another crate ([decision 0372](../decisions/0372-a-push-waits-for-the-checks-of-cis-linux-job-run-here-and-ci-keeps-the-compiled-dependencies.md)).
- On a machine that many agents share, no agent stops a process by its name (`pkill`, `killall`, `taskkill /IM`, or the like). Another agent's engine or test stand runs under the same name, so a stop by name ends that agent's work too. An agent stops only a process it started, by the identifier the system gave it. On 2026-11-18 an engine was asked to stop from outside in the middle of another agent's test, and a stop by name was the likely cause.

## When to stop and change course

None of these waits for a person. Each is a point to stop building, write down what was found, and choose again.

- A hypothesis test contradicts a decision: record the result, then supersede the decision or propose the change, by whose decision it is.
- A slice needs something from a later phase: note it in the journal and finish the smaller part.
- A spike shows the documented design cannot meet a budget or a guarantee: this is a discovery; correct the design before building on it.
- The work would touch a real application, a credential, or a registry token: this is one of the three things an agent does not do alone (AGENTS.md).
- The agent notices it has been building for a long time without proving anything: stop, and prove the smallest part.
