# 2026-11-25: handed on to a new session

The entry before this is [the preview made ready](2026-11-25-the-preview-made-ready.md). This one covers the evening of 2026-11-25. The lead ran up to four builders at once and merged them into a staging branch, sent up as pull request 28. Then the owner stopped the work here, to carry it on in a new cloud session.

## State at the end of this entry

- **Main** holds everything through pull request 27, with the prepared commit of 0.1.0, `261eee0`, untagged.
- **Pull request 28**, in draft, holds everything since: decisions 0479 to 0493. It merges when every check is green on Windows, macOS, and Linux. Its runs found a keeper timer test racing on Windows, and an E8 test whose HTTP client was made by hand, which a build with TLS cannot run; both are fixed, and the last push's run is the one to read.
- **Progress:** counted by `python3 tools/progress.py`, about 194 of 219 packages.
- **Nothing is running.** The hourly check-in of this session is switched off, and no builder is left.

## What the owner directed

- **Go back to the first release's full scope.** Written as owner decision 0493: rows 3.6, 5.6, and OS5 are conditions of the first release again, and the prepared preview is not tagged. Inferred: the packages placed after the release stay after it.
- **Carry the work on in a new cloud session, on another account,** with what an agent needs to set up its machine written in the repository. Written as [continuing in a cloud session](../plan/continuing-in-a-cloud-session.md): the environment and its setup script, where the work stands, how the lead runs builders, and what the last lead knew that was written nowhere else. The README, the changelog's 0.1.0 section, and the roadmap now say the first release waits for its full scope. The lead's renumbering script is now `tools/renumber_decision.py`.

## Done in this stretch

- The warm observing tab (0488), the window memory fix (0487), FU1 and FU2 (0485), SR2 to SR5 (0484, 0489), E3 (0486), E8 (0491), 9.1b's canaries (0482), the capture race (0483), the Windows desktop start (0480), 1.9b (0481), and why the desktop pack starts nothing on macOS (0490).
- **9.1c** (0492): a drift yields a repair bundle, a pack proposes a repair by rule, and a person accepts it.

## Learned

- **A test that counts a background check races it** unless it waits for the keeper to be idle. The Windows runner showed it first.
- **A copy of the test program, not the running one,** is what a test of a start on macOS must name: a running program is found as already started.

## Next

- Set up the environment, as the new page says, and merge pull request 28 when green.
- Build what is left of the full scope that agents can build; prepare the owner's checks at Windows and macOS machines.
