# 0476. Version 0.1 is a preview, and the checks a person makes move to before 1.0

**Status:** Accepted. Owner decision, taken on 2026-11-25 by the owner's choice of the lead's recommendation. It changes what [decision 0397](0397-what-the-first-release-needs-of-the-packages-left-and-what-waits-until-after-it.md) made a condition of the first release, and adds to [decision 0465](0465-while-the-first-release-waits-on-the-owner-agents-build-the-one-machine-packages-moved-after-it.md).

**Date:** 2026-11-25

**Changes:** the conditions of release 0.1 (package R1), the rows 3.6, 5.6, and OS5 of the [roadmap](../plan/roadmap.md), the status in the README, and the 0.1.0 section of the changelog when it is written.

## Context

On 2026-11-25 every check in CI passed on Windows, macOS, and Linux with everything agents can build for one machine merged. Three rows before the first release each need a person at a machine: 3.6 (one check by a person of the presence prompt on macOS and on Windows), 5.6 (the exit test of Phase 5, the first tool in under ten minutes, measured with a person or an agent in the loop, on all three systems), and OS5 (the first copy test on Windows and macOS, from a machine of each). The owner asked how to shorten the way to a release that can be published and used, and chose to move these three.

Inferred: the owner wants a release people can install and try now, with its gaps stated, rather than a release that waits on checks only a person at each machine can make.

## Decision

1. Release 0.1.0 is a **preview**. It is cut from main once the owner has set up the registries (package 0.2p and [publishing setup](../plan/publishing-setup.md)) and starts the release workflow.
2. Rows 3.6, 5.6, and OS5 are no longer conditions of 0.1. Each becomes a condition of release 1.0 (package R2), with its proof unchanged. 3.6 and OS5 stay the owner's.
3. The 0.1.0 section of the changelog, and the README while 0.1 is current, state the gaps in words: the presence prompt and the approval page are proven by a person on Linux only, and on macOS and Windows are built and tested in CI with a stand-in for the person; the first hour is measured as machine time, not with a person; no copy test has been run from a Windows or a macOS machine.
4. Nothing else is loosened. Every rule, every check in CI, and every gate of a run holds in 0.1 as on main. A presence prompt that cannot be shown stops the act it guards, on every system, as it does today.
5. Agents start no new feature package from here until 0.1 is published. They finish what is in flight, fix what CI or a field test finds, and prepare the release commit: the 0.1.0 section of the changelog, the version 0.1.0 in every manifest, and the kept contract set, on a branch, with no tag.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep the three checks in 0.1 | The release waits on a person at a Mac and a Windows machine; the owner chose not to |
| Release for Linux only | The program is built and tested on all three systems in CI; leaving two out loses more than stating the gaps does |
| Drop the presence prompt on macOS and Windows for 0.1 | Rule 2: safe comes first. The prompt is built and fails closed; only a person's check of it is moved |

## Revisit if

A user of 0.1 meets a fault of the presence prompt on macOS or Windows, or the owner makes one of the three checks before 1.0 is near.
