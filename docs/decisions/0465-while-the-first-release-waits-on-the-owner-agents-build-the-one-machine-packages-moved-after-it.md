# 0465. While the first release waits on the owner, agents build the one-machine packages moved after it

**Status:** Accepted. Design decision, made by the lead. The owner had not read this record when it was merged. It adds to [decision 0397](0397-what-the-first-release-needs-of-the-packages-left-and-what-waits-until-after-it.md) and does not reverse it: nothing it moved becomes a condition of the first release.

**Date:** 2026-11-25

**Changes:** the order of work in the [roadmap](../plan/roadmap.md). No row changes its text or its proof here.

## Context

Decision 0397 moved some packages after the first release so that the packages before it could be built well. On 2026-11-25 every package an agent can build before the release was built or in CI. What the release waits on now is the owner's: holding the names on the registries, setting up trusted publishing, and approving the publish (package 0.2p, and [RELEASING.md](../../RELEASING.md)). The owner asked to finish the build and chose to do the registry steps later. Inferred: the owner wants agents to keep building while the release waits, and wants the release not to wait on that building.

## Decision

1. Until the owner starts the first release, agents build the packages that decision 0397 moved after it and that serve one person's machine. In this order, as crates allow: the rest of 8.3 (starting an application, keys and clicks, `desktop_probe`); the Node client on Windows (of 5.4); the actions with no handler that serve one machine; then W2d, 1.9b and 9.1b, each with its design first where its row asks for one.
2. Every such package merges only by the same gates as any other. A package that changes a public contract carries its own decision record, as always.
3. None of them becomes a condition of the first release. When the owner starts the release, it is cut from main as it stands, and a package not merged by then waits for the next.
4. The server's packages (milestone 9 and the runner rows) stay after the first release: a shared server comes after it by the owner's direction.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Stop and wait for the owner's registry steps | Leaves agent work undone for no gain; the release does not get nearer by waiting |
| Make the moved packages part of the first release | Undoes decision 0397's reason, and makes the release wait on building |
| Start the server's packages | A shared server comes after the first release by the owner's direction |

## Revisit if

The owner starts the release, or a moved package turns out to change something the release ships in a way that needs the release to wait.
