# 0493. The first release has its full scope again, and the prepared preview is not tagged

**Status:** Accepted. Owner decision, taken on 2026-11-25. It supersedes [decision 0476](0476-version-0-1-is-a-preview-and-the-checks-a-person-makes-move-to-before-1-0.md), and the part of [decision 0479](0479-agents-keep-building-while-0-1-waits-and-0-1-is-released-from-its-prepared-commit.md) that releases 0.1 from its prepared commit. The rest of 0479 stands: agents keep building.

**Date:** 2026-11-25

**Changes:** the conditions of the first release (package R1), the rows 3.6, 5.6, and OS5 of the [roadmap](../plan/roadmap.md), the status in the README, and the 0.1.0 section of the changelog.

## Context

Decision 0476 made 0.1 a preview: three checks that need a person at a machine (3.6, 5.6, OS5) moved to 1.0, and the gaps were to be stated. The release commit was prepared on main, and not tagged. On 2026-11-25 the owner directed that the work go back to the first release's original full scope, and that it carry on in a new cloud session.

Inferred: the owner wants the first release to be the release designed in [decision 0397](0397-what-the-first-release-needs-of-the-packages-left-and-what-waits-until-after-it.md), with every one of its conditions met, and not a preview with stated gaps. The packages 0397 and [decision 0465](0465-while-the-first-release-waits-on-the-owner-agents-build-the-one-machine-packages-moved-after-it.md) placed after the release stay after it; agents may still build those of them that need one machine only, as 0465 allows. The owner can correct this.

## Decision

1. The first release has its full scope: every condition of decision 0397, and rows 3.6, 5.6, and OS5 with their proofs unchanged. They are conditions of the first release again, not of 1.0.
2. The prepared commit of 0.1.0 on main (`261eee0`) is not tagged as a preview. No agent tags anything; the owner starts the release, as [RELEASING.md](../../RELEASING.md) says, when the full scope is met.
3. The first agent of the next session rewrites the preview's words: the README's status, the 0.1.0 section of the changelog (which then stays unreleased, its gaps closed or named as conditions), and the roadmap rows that say "a condition of release 1.0". The version number stays 0.1.0 unless a later record changes it.
4. The owner's steps are unchanged: the release gate (SR1), the registries (0.2p), and the checks at a Windows and a macOS machine (3.6, OS5).

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep 0.1 as a preview and add the checks in 1.0 | The owner chose the full scope |
| Renumber the first full release 1.0 | Nothing requires it; the version is a later choice, with its own record |

## Revisit if

The owner chooses a preview again, or a condition of 0397 proves impossible to meet on one of the three systems.
