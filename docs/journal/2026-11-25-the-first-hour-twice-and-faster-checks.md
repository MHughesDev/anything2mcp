# 2026-11-25: the first hour, twice, and faster checks

The entry before this is [the exit tests of the finished phases](2026-11-24-the-exit-tests-of-the-finished-phases.md). This one covers the evening of 2026-11-24 into the morning of 2026-11-25. The lead ran several builders at once, one per group of crates, merged each by hand into a staging branch, and sent the batches up as pull requests 23 to 26.

## State at the end of this entry

- **On main:** pull requests 23 to 26 are merged. Every check was green on Windows, macOS and Linux each time.
- **Pull request 27:** open, in CI. It carries:
  - N0b, the renames of decision 0330 (0463): logs, copy test, access kind, `inputs/`, `stdio`, and the rest, with stored words kept;
  - 5.1, every action with no handler named, and marked not built in the catalog.
- **Progress:** 176 of 206 packages, counted by `python3 tools/progress.py --write`. The one package ready for an agent is FT6, and it waits on the owner's answer about H-50.

## What the owner directed

- **Run every subagent that can run at once.** Inferred: keep a builder in each slot the crates and the disk allow, at one writer per crate, and start the next as each merges.
- **The owner asked for the best ways to bring people to the product, shorten builds, and raise their quality.** Inferred: the lead's to build are the build and quality items. The items that bring people need the owner:
  - holding the package names and the first release;
  - a yes on running H-50's real applications;
  - a Windows and a macOS machine.

## Done in this stretch

**The first hour.**
- H-18 is supported: three agents with no knowledge of the repository reached a published read in 1 min 49 s to 2 min 03 s.
- 5.6b fixed what they met (0445), and S3 added `ready` (0443).
- Measured again, three new agents took 1 min 08 s to 1 min 19 s in 15 to 20 commands, by `init`, `add`, `ready` and the person's press.
- What they met now is row 5.6c.

**Capture.**
- E2b: a capture is begun only by its person, on the page (0433).
- E2: the web pack watches a person's use (0442).
- E5: a capture that has ended is scouted (0451).

**Browsers.**
- 9.3e and the page-request kind: a read whose browser is lost goes on (0424, 0428). On Windows, a tab that sees its page crash now marks its browser lost.
- 9.3g: memory measured from the browser's own processes, read in CI on three systems as OS4 (0425).
- A mode states its prepare time (0427).
- A browser that stops as it starts is named with its own words (0450).

**The other operating systems** (review by lens 13; 0430 to 0432): OS1 to OS4.

**A year later** (review by lens 14; 0436 to 0439):
- YL3: the vault's salt file records its cost.
- YL4: release contracts are kept and checked, and workflow files are checked before a push.
- YL5: an action with no handler has no client method.

**Field tests.**
- Field tests 7 and 8: a broker and a device protocol, with their faults fixed (0429, 0434, 0435, 0440, 0441).
- Field test 9: a real browser in the field box with its sandbox on (0447).
- Finding 53: a decimal is shown to its type's places (0444).
- E7d (0426) and 5.4.

**Faster and surer checks.**
- The product's tests are one program: 7.8 GB to 1.5 GB, and 8 to 3.6 minutes (0446).
- Dependencies carry no debug information (0448).
- CI's Rust tests run in three shards, held to one full run (0449).
- Windows and macOS lints run in the local gate, and a nightly run looks for flaky tests (0452).
- The plan's counts are generated (0453).
- Five flaky tests were fixed at their cause: the web-ui commit tests, the `a2m-local` lock tests, `approval_surface`, the `serve` prompt test and the capture race.

**After pull request 26.**
- 5.6c: the second measurement's faults fixed; a write waiting for its person stays `ok: false` (0458).
- YL1 and YL2: a breaking change is named at publish, after where it leads (0454), and `version.diff` classifies it.
- FT5 and the review by lens 7, "Less" (0456).
- N0b: the renames of decision 0330 (0463).
- 5.1: every action with no handler is named, and marked not built in the catalog.

## Learned

- **Agents in one session share one shell directory and one scratch folder.** The first three H-18 runs crossed into each other and were thrown away. Each run's program is now fixed to its own home and folder.
- **A workflow file GitHub cannot read fails every check at once,** with nothing to read but a failed run. The local gate now reads every workflow file.
- **A test that trusts a system call's answer can be wrong on another system.** `chflags` answered success on macOS and kept the flag off. Read the state back.
- **On Windows, a killed browser's processes and its pipe end in no set order.** Recovery now listens to the tab's own sight of its page, not to the pipe alone.
- **A home-folder cargo setting outside the repository can hide what a profile change does here.** Measure on CI as well.
- **Hand-written counts conflict on nearly every merge.** They are generated now.
- **A rename sweep that changes a path in code must move the folder too.** N0b changed the command pack's test to read `inputs/` and left its fixtures in `sources/`; only the test run showed it.

## Found, and routed

- Row 5.6c: the second measurement's faults.
- The `lock_comes_free` helpers in the product's tests still poll the engine lock, which a child process can briefly hold. A public test hook or a product change is needed.
- The nightly flaky-test run and the sharded CI have each to show their first real run.

## Next

- Merge pull request 27 when every check is green, and bring main back into the staging branch.
- FT6 waits on the owner's answer about H-50. The other packages left wait on the owner or on the first release.
