# 2026-11-25: the preview made ready

The entry before this is [the first hour, twice, and faster checks](2026-11-25-the-first-hour-twice-and-faster-checks.md). This one covers the rest of 2026-11-25. The lead ran builders in parallel, one per group of crates, merged each into a staging branch, and sent everything up as pull request 27.

## State at the end of this entry

- **Pull request 27:** open, carrying everything below. Every check passed on its last whole run but one, the macOS build without TLS, which GitHub cancelled with no log at fifteen minutes of its thirty; this entry's commit runs CI again. It merges when every check is green.
- **Release 0.1.0 is prepared and not published.** The version is set in every manifest, the changelog has its dated section, and `contracts/0.1.0/` is kept. What is left is the owner's: the registry setup and the environments in [RELEASING.md](../../RELEASING.md), then starting the release workflow. No tag was made.
- **Progress:** 183 of 212 packages, 86%. What is left waits on the owner, on the first release, or is the server's.

## What the owner directed

- **Finish the build.** Inferred: build every package an agent can before the release, and do not wait on the owner's registry steps (decision 0465).
- **Run real open-source applications locally for H-50 and FT6:** reads only, in containers, no real accounts. Done as field test 11.
- **The registries and the first release: later.**
- **Shorten the way to a release that can be used, and ship 0.1 as a preview.** Written as owner decision 0476: the presence prompt checked by a person on macOS and Windows (3.6), the first hour timed with a person (5.6), and the first copy test from Windows and macOS (OS5) move to 1.0; the gaps are stated in the README and the changelog; nothing else is loosened; agents start no new feature package until 0.1 is published.
- **The owner tests the preview on a Windows 11 laptop.** CI's Windows packaging job now stores `windows-tester-kit` (the program it built and tested, the test application, the wheel, and [the guide](../plan/trying-the-preview-on-windows.md)). The owner's notes are to become the Windows halves of 3.6, 5.6, and OS5.

## Done in this stretch

- **Release pieces** (0464): archives, notices, attested provenance, and a check that holds the release workflow to every piece. Then the 0.1.0 commit: version, changelog section, kept contract.
- **H-50** refuted by its own count; **FT6**, field test 11 (0471), and its fixes FT6a and FT6b (0473, 0474), FT6c (0477: a read goes on past what an observing tab refused, and says so), and FT6d (0475: a person confirms an asserted read on the engine's page, and a try may then send it).
- **5.4b** (0466): the Node client on Windows through `anything2mcp channel`.
- **8.3**: writes on Windows and macOS counted; a start, keys, and `desktop_probe` (0467). Not done: a start and keys on Windows and macOS stop by name.
- **W2d** (0472) and **W2e** (0478): a watch compares records by key, and a watch is an MCP resource with a subscription and a REST change stream. W2e's product test found and fixed two faults of W2d's.
- **Four actions** (0468 to 0470): `version.rollback`, `run.trace`, `trace.export`, `component.list`.
- **The gate** stops a test program after fifteen minutes and names the tests still running.

## Learned

- **Reading the whole browser binary for its digest on every command hung the product's tests on Linux.** The availability check now takes a digest only when the manifest has one to compare.
- **A version bump moves more than the manifests.** The compiler's version and the stream pack's manifest are held to the crate's, the compiled target's hash carries the compiler's version, and the release tool's own test assumed no 0.1.0 section. Run the whole suite and every tool test after one.
- **An exact count of tests in a CI step breaks when a builder adds one.** Two such counts were raised this stretch; a builder that adds a test to such a step should raise it too.
- **Browser tests run in one step share the runner's cores.** The probe's tests now run in a step of their own in the build without TLS.
- **Re-running a failed job is not open to this session** (the API refuses it), so every retry is a push, and a whole CI run.

## Found, and routed

- The capture test once drafted no held read of the order totals on macOS without TLS; the cause is not found. The test now prints every finding and the report when it misses.
- 8.3's start and keys on Windows and macOS, 1.9b, 9.1b, retention of watches, and the copy test of FT6 run again stay after the release.

## Next

- Merge pull request 27 when every check is green, and bring main back into the staging branch.
- Turn the owner's Windows notes into entries in the copy test results and the rows 3.6, 5.6, and OS5.
- When the owner has set up the registries, check the release rehearsal and help with the first publish.
