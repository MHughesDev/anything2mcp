# 0494. The first release is the whole design, the shared server with it

**Status:** Accepted. Owner decision, taken on 2026-11-26 by the owner's choice between two answers the lead put. It supersedes [decision 0169](0169-the-first-release-is-for-one-machine.md), and the words "for the first release" and "not in the first release" of [decision 0208](0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md) where they keep a shared server out. It supersedes, in whole, the moves of [decision 0397](0397-what-the-first-release-needs-of-the-packages-left-and-what-waits-until-after-it.md), point 1 of [decision 0390](0390-the-open-questions-of-2026-11-24-are-settled-by-the-lead-and-the-first-release-is-what-progress-counts-toward.md), the framing of [decision 0465](0465-while-the-first-release-waits-on-the-owner-agents-build-the-one-machine-packages-moved-after-it.md), and the point of [decision 0493](0493-the-first-release-has-its-full-scope-again-and-the-prepared-preview-is-not-tagged.md) that keeps packages after the release.

**Date:** 2026-11-26

**Changes:** what the first release is, in AGENTS.md, SCOPE.md, the roadmap, and the build order; the handoff page; the spec's sentences that place a part "after the first release".

## Context

To reach a first release sooner, decisions 0169, 0390, and 0397 drew a line: the first release served one machine, and the shared server (packages 6.1, 6.2, 6.4, 6.5, 6.6, 7.1, 7.2, 7.3, and 9.4) and the rest of several packages (E9, 9.2, 9.3, FT5b) waited until after it. Some packages kept before the release were made small. On 2026-11-26 the owner directed that the work stop shrinking the product to reach a release sooner and carry on with the full breadth of the design. Asked whether the shared server is part of that, the owner chose yes.

Inferred: nothing in the design is cut or deferred to reach the release. The order of the build still follows the design: the parts that serve one machine first, as they stand, then the server on top of them. The owner can correct this.

## Decision

1. **The first release is the whole design.** Every package in the roadmap that is not withdrawn is a condition of the first release: the machine-local product, the shared server and what it carries (milestone 9 and its packages), E9, the whole of 9.2 and 9.3, and FT5b. No row is held "after the release" any more.
2. **Nothing is made small to reach the release.** Where decision 0397 made a package small (1.9, 5.1 to 5.4, 9.2a, 9.3a, 9.3b), the full package as the design writes it is the work; the small one, done, is its first part.
3. **The one-machine boundary stays a boundary of the local engine, not of the product.** The local engine still listens only on its own machine, as decision 0208 says. A shared server is another configuration of the same binary, designed in [spec section 601](../spec/6-running-it/601-deployment-configurations.md) and [602](../spec/6-running-it/602-identity-on-a-shared-server.md), and it comes into the release by its own packages.
4. **The ten rules and every other owner decision stand.** A part that was not designed because it waited, such as the server's packages, is designed now, by records, before it is built.
5. **The owner's steps are unchanged:** SR1, 0.2p, the checks at a Windows and a macOS machine (3.6, OS5), and starting the release when everything is met.

## What this leaves to the next session

- Each row of the build order marked "After the release" is marked for design and build, and each held row in the roadmap whose reason is "after the release" gets a reason that says what it waits on now.
- The spec's sentences that place a part "after the first release" (sections 404, 601, 602, 604, 704, and others: `grep -rn "after the first release" docs`) are rewritten to say it is part of it, with what it waits on.
- The packages are sequenced: the server's design first, as decision records, then its slices.

## Alternatives

| Alternative | Why not |
| --- | --- |
| The first release waits for all the one-machine work, and the server comes right after it | The owner chose the whole design |
| Keep the lines of 0397 and 0493 | The owner directed that nothing be cut to reach a release sooner |

## Revisit if

The owner sets a date for a release, or a part of the design proves impossible to build on one of the three systems.
