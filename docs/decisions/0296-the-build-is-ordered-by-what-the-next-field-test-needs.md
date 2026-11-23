# 0296. The build is ordered by what the next field test needs, in eight stages

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner asked for the plan and had not read it when it was merged.

**Date:** 2026-11-23

**Changes:** [the build plan](../plan/build-order.md) (new), the [roadmap](../plan/roadmap.md) (packages S1, S2, and FT6 are new), the [plan folder's index](../plan/README.md).

## Context

The roadmap lists every package by milestone, in the order the design was thought of: the core, sign-in, writes, a second kind, the scout, fast paths, the first hour, more kinds, a server, breadth, hardening. Much of each milestone is built and none is finished. The first field test showed that the order hides what matters: the product drafts tools from a description and two reads of three stop on existing software, while work on a server and on new kinds of system was ready to start. The owner agreed that no new kind of system should be added for now, that the capture of a person's use should be built, that a read-only tool should take about three commands, and that a lighter publish for a read should be proposed; and asked for one file that plans what is left at three levels.

## Decision

1. **What is left is ordered in eight stages:** reads that work on existing software; the short path to a tool; the web path; verified writes on existing software; every kind; the first release; a server; breadth and hardening.
2. **A field test decides what is next.** Each of the first five stages ends with a field test passing, and whatever stops it is built first.
3. **No new kind of system is added before Stage 5.**
4. **The build plan gives the order and the tasks; the roadmap stays the record of each package.** Numbers are shared. On a package's proof the roadmap is right; on order the build plan is.
5. **Three packages are new.** S1 decides and builds a lighter publish for a version that holds only reads. S2 makes `add` carry a system to a requested publish. FT6 is the field test of a web application with a sign-in.

S1 is a decision still to be made, with its own record. This record only puts it in the order.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep the roadmap's order | It finishes milestones no user has asked for before the product works on one existing system |
| Replace the roadmap with the build plan | Sixty records and both skills cite the roadmap's package numbers and proofs |
| Order by the kind of system, one pack finished at a time | Every pack shares the faults the first field test found in drafting and in reading an answer; fixing them once, first, is cheaper |

## Consequences

- Packages marked ready in the roadmap that fall in Stages 5 to 8 (P2, W2d, N2b, X1, X5, X7, K4, 6.3, F3) wait, though nothing blocks them.
- The low level of Stages 7 and 8 is not written. It is written when the stage before is reached.
