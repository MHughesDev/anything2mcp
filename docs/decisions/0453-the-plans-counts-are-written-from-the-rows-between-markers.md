# 0453. The plan's counts are written from the rows, between markers

**Status:** Accepted. Design decision, made at the lead's request after most merges of 2026-11-25 conflicted in the same hand-typed count sentences and two left them wrong. The owner had not read this record when it was merged.

**Date:** 2026-11-25

**Changes:** `tools/progress.py` gains `--write`, `--check`, and `--root`; `tools/docs_audit.py` runs the check as its eighteenth; `tools/test_progress.py` proves it, and the documents job of `ci.yml` runs that test. In `docs/plan/build-order.md` the **Progress:** sentence is generated, and the history of added rows and the **Done last:** list each move to a paragraph of their own. In `docs/plan/roadmap.md` the sentence naming the `ready` packages and the count in each milestone's State cell are generated. No row of either file changes.

## Context

Each package that merged changed, by hand, the build order's "N of M packages done, P%", the number left, three counts with three lists (the owner's, the first release's, an agent's), the roadmap's list of `ready` packages with its number word, and often a milestone's summary. Two branches that each finished a package changed the same sentences, so their merges conflicted, and a conflict resolved by hand could leave a number one off. Before this change four of the twelve milestone summaries were already wrong (milestone 11 said 2 of 10 done; its rows said 24 of 32). `tools/progress.py` already counted done of total from the roadmap's State column, and nothing held the prose to it.

## Decision

1. **A count is written between markers, from the rows.** Each generated count sits between `<!-- progress:begin NAME -->` and `<!-- progress:end -->`, and only `python3 tools/progress.py --write` writes inside them. There are three kinds: `build-order` (the Progress sentence), `ready` (the roadmap's sentence of `ready` packages, with its number word), and `milestone-N` (done of total of milestone N, then the packages left, grouped by their state).
2. **The roadmap decides done; the line decides whose.** Done, total, and the `ready` list come from the roadmap's State column, withdrawn rows left out, as before. The three lists of packages left come from the line in the Build Order: a row whose Line cell begins `Owner` is the owner's, `After the release` the first release's, and every other row not struck or marked done (Build-ready, Needs design, In hand) an agent's. Each list is in the line's order, so a count and its list are one thing and cannot disagree.
3. **Where the two disagree, nothing is written.** `--check` and the audit name: a package left in the roadmap with no row in the line; a package the line marks done that the roadmap does not, or the reverse; a package in two rows; a name the roadmap lacks; a region that is stale, missing, or that no rows make. `--write` refuses while any disagreement stands, so a stale line is fixed in the line, by a person, and never papered over by the tool.
4. **Prose keeps no running number.** "Done last" stays hand-written history, on its own line, and gives no count. The milestone summaries keep only what a count cannot say: whether the exit test passes, and why a package waits.
5. **After a conflict in a count,** take either side and run `--write`. Because the counts and "Done last" are now on different lines, a merge that touches both conflicts only where two branches really wrote the same history.

## Alternatives considered

- **One generated block, with the prose naming no numbers.** Fewer markers, but the reader of a milestone's row would have to look elsewhere for its count, and the roadmap's sentence of `ready` packages is read where it stands. Markers in place keep each count where it is read.
- **Delete the counts and point at `progress.py`.** No conflict and no staleness, but the plan would no longer say what is left without running a program, and the first thing a new agent reads is the Progress sentence.
- **Let `--write` also change the line when the roadmap marks a package done.** It would make the line's rows generated too, and a worker's strike with its pull request number would be lost. The line's classes are judgments; the tool only counts them.

## Consequences

- A builder who marks a package done also strikes its row in the line (as the line's own rules say) and runs `python3 tools/progress.py --write`; the audit fails until both are done.
- "In hand" in the line is counted as an agent's, as 8.3 is.
