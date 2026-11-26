# 2026-11-26: the whole design for the first release

The entry before this is [handed on to a new session](2026-11-25-handed-on-to-a-new-session.md). Pull request 28 merged with every check green on Windows, macOS, and Linux. The owner then chose not to move the work to another account: it carries on here, from the next working session.

## What the owner directed

- **Build the full breadth of the design, and cut nothing to reach a release sooner.** Asked whether that includes the shared server, the owner chose yes. Written as owner decision 0494: the first release is the whole design, the shared server with it; no row waits "after the release"; a package made small by decision 0397 is built whole. Inferred: the order of the build still runs from one machine up to the server.

## State at the end of this entry

- **Main** holds everything through pull request 28. This entry's pull request carries decision 0494 and the changes it makes at once: AGENTS.md, SCOPE.md, the build order's rows, and the handoff page.
- **Progress:** 194 of 219 packages. Of the 25 left, 10 are the owner's and 15 an agent's, the server's packages among them.
- **Nothing is running.** No builder, no scheduled check-in.

## Next

- The sweep decision 0494 names: the roadmap's held rows that give "after the release" as their reason, and the spec's sentences that place a part after the first release.
- Design the shared server as decision records, from spec sections 601 and 602, then build it in slices, alongside E9, 9.2, 9.3, and FT5b.
- Set up and run builders as [continuing in a cloud session](../plan/continuing-in-a-cloud-session.md) says.
