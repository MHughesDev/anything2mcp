# 0293. Existing software that we run ourselves is tested against during the build, and may be written to

**Status:** Accepted. Owner decision, given in conversation on 2026-11-23 and written here in the agent's words ([decision 0231](0231-the-owners-direction-is-written-in-exact-words-of-the-agents-own.md)).

**Date:** 2026-11-23

**Changes:** the [roadmap](../plan/roadmap.md) (package F1 is new, and package 9.5 keeps only systems that are not ours), [SCOPE.md](../../SCOPE.md), [section 704](../spec/7-project/704-readiness.md), and the reading of rule 10 in [AGENTS.md](../../AGENTS.md), whose words are unchanged.

## Context

Every part of the product was proven against the test stands under `targets/`. The project wrote both the stands and the product, so a stand can only hold what the project already thought of. Real systems were to be tried once, at the end, as roadmap package 9.5. The owner asked whether the product is set up well for its purpose, and was told that the largest unknown is that it has never met software written by someone else.

## Decision

1. **Existing software is a testing surface from now on, beside the stands, while the product is built.** It is open-source software whose source and stored state can be read, so that a claim about what it holds can be checked against the software itself.
2. **The copies are ours.** A copy is started by the project, on a machine the project works on, holds only invented data, and is thrown away after a run. No account with anyone else is used, and no system that another party runs is touched.
3. **A write to such a copy is allowed.** Rule 10 forbids testing a write against a real business application. A copy of open-source software that the project runs, holding invented data that nobody depends on, is not that: it is real software in a test environment. The owner said so for these copies. A system the project does not run, and any copy holding data that is not invented, stays under rule 10 as it was, with the owner's go-ahead in the session.
4. **Rule 9 stands.** No real application is named in this repository. What names the software lives outside it; what was learned is written here by the kind of system.

## What I inferred

The owner said writes are fine on copies we run ourselves. I inferred the three conditions in point 2 (started by us, invented data only, thrown away), because they are what makes a copy harmless to write to. I inferred that the stands stay as the gate in CI, because they run on three operating systems and can be made to fail on purpose, and a container of someone else's software cannot yet do either.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep to stands the project builds | The project already has ten. They cannot surprise it. The first field test made seven findings in one run that no stand had shown |
| Software that someone else hosts, reached over the network | Not ours to write to; its stored state cannot be read; it changes without notice |
| Wait for package 9.5 | The faults a real system shows are cheapest to fix while the part that has them is being built |

## Consequences

- Package 9.5 is no longer the first time the product meets software it did not write. It remains the first time it meets a system the project does not run.
- Every fault a field test finds is added to a stand as a case, so that CI holds it on three operating systems.
