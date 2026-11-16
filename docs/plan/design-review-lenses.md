# Design review lenses

A design review round looks at the whole design through one question, finds what is wrong or missing, and fixes the documents. It writes no product code. Run one at the end of each phase and whenever five slices have passed without one, as the [working method](working-method.md) says.

## How to run a round

1. Pick one lens. Prefer one that has not been used recently; the table records the last use.
2. Read the sections the lens points at, with the question in mind.
3. List findings. For each: what is wrong, where, and how serious.
4. Fix what can be fixed in the documents. Put what needs evidence into the [hypothesis register](../hypotheses/README.md). Put what needs the owner into a decision record with status Proposed.
5. Run `python3 tools/docs_audit.py`.
6. Record the round in the journal and update the last-used date below.

A round that finds nothing has probably not looked hard enough. A round that rewrites everything has gone too far: prefer the smallest change that fixes each finding.

## Lenses

| # | Lens | The question | Last used |
| --- | --- | --- | --- |
| 1 | Consistency | Do any two documents disagree? | 2026-11-15 |
| 2 | The builder | What would someone implementing this have to invent? | 2026-11-15 |
| 3 | Weakness | Where is the design weaker than it should be, even if complete? | 2026-11-15 |
| 4 | The steered agent | What could an agent do if text it read told it to? | 2026-11-15 |
| 5 | Self-failure | What happens when each of the engine's own parts fails? | 2026-11-15 |
| 6 | One pipeline, line by line | Walk one real pipeline through every section it touches. Where do two steps not connect? | Not yet |
| 7 | Less | What can be removed from the first release without losing the core? What is designed and not needed? | Not yet |
| 8 | The security reviewer | Read the company-server path as an organization's security team would. What would they refuse? | Not yet |
| 9 | The first-time user | Follow the walkthrough as someone who has never seen the project. Where do they get stuck? | Not yet |
| 10 | The measured system | Now that numbers exist, which sections assumed something the numbers contradict? | Not yet; needs Phase 1 |
| 11 | The application owner | Read the design as the vendor of a wrapped application. What would make them object, and is the objection fair? | Not yet |
| 12 | Simpler | For each major part: is there a design half as complex that gives most of the value? | Not yet |
| 13 | The other operating systems | What quietly assumes one operating system? | Not yet |
| 14 | A year later | What will be hardest to change once people depend on it? Are those things right? | Not yet |

## Adding a lens

Add one when a problem is found that no existing lens would have caught. A good lens is a question a specific kind of person would ask, not a topic.
