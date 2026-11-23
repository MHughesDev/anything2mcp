# 0326. "Plan" names only the compiled plan, and the document of what to build next is the Build Order

**Status:** Accepted. Owner decision.

**Date:** 2026-11-23

**Changes:** the name of `docs/plan/build-plan.md`, now `docs/plan/build-order.md`, and every link to it; the glossary.

## Context

The word "plan" named three parts: the compiled, hashed form of a target version that the engine runs; the document that says what is built next and in what order; and the notes a lead writes to brief a worker. Decision 0301 gives every part one proper name. The owner found the three meanings confusing and chose which keeps the word.

## Decision

1. **Plan** names only the compiled plan, as the glossary already defines it: what the compiler makes from a target's files and the engine runs.
2. **The Build Order** is the document that says what is left to build, in what order, and how far the build has come (`docs/plan/build-order.md`). It holds the progress line.
3. **A build brief** is what a lead hands a worker for one package: the goal, the proof, the files it owns, and the checks to run. Briefs are kept outside the repository.
4. The folder `docs/plan/` keeps its name: it holds the roadmap, the Build Order, and the other documents of how the work is run.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Rename the compiled plan | The word is in the code, the file formats, and every crate's design; the owner chose to keep it there |
| Keep all three meanings and say which each time | Against decision 0301, and the owner found it confusing |
