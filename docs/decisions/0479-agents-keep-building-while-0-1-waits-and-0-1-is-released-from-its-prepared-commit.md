# 0479. Agents keep building while 0.1 waits, and 0.1 is released from its prepared commit

**Status:** Accepted. Owner decision, taken on 2026-11-25 by the owner's direction to keep many builders at work around the clock with the lead as orchestrator. It replaces point 5 of [decision 0476](0476-version-0-1-is-a-preview-and-the-checks-a-person-makes-move-to-before-1-0.md) and leaves its other points in force.

**Date:** 2026-11-25

**Changes:** the order of work in the [roadmap](../plan/roadmap.md), and how release 0.1.0 is started ([RELEASING.md](../../RELEASING.md)).

## Context

Decision 0476 stopped new feature work until 0.1 was published, so that main stayed the release. The owner then directed the lead to orchestrate builders without pause. Inferred: the owner wants the build to go on, and the preview not to change under it.

## Decision

1. Agents go on building the packages an agent can take, one builder per group of crates, merged by the same gates as always.
2. Release 0.1.0 is the commit that prepared it on main: the version, the changelog's dated section, and `contracts/0.1.0/`. It is started by pushing the tag `v0.1.0` on that commit, which the workflow accepts as a commit on main. Later merges add their lines under Unreleased and never change the 0.1.0 section or the kept contract; a break against 0.1.0 is listed in `contracts/next-breaks`.
3. The server's packages stay after the release, as before.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Hold all work until the owner publishes | The owner asked for the opposite |
| Release whatever main holds when the owner starts | The preview the owner tests tonight would not be the one released |

## Revisit if

The release workflow refuses the prepared commit, or the owner tests a later build and wants that one released.
