# Journal

One entry per working session. An agent starts each session with no memory of the last, so this folder is how work carries over. Read the newest entry first. Write a new one before the session ends.

## Entries

Newest first.

| Date | Entry | In one line |
| --- | --- | --- |
| 2026-11-26 | [The whole design for the first release](2026-11-26-the-whole-design-for-the-first-release.md) | Pull request 28 merged; the owner puts the whole design, the shared server with it, into the first release (0494) |
| 2026-11-25 | [Handed on to a new session](2026-11-25-handed-on-to-a-new-session.md) | 9.1c and the stretch's fixes in pull request 28; the first release back to its full scope (0493); a page for the next cloud session |
| 2026-11-25 | [The preview made ready](2026-11-25-the-preview-made-ready.md) | FT6 and its four fixes, W2d and W2e, the desktop pack's start and probe, four actions; 0.1 made a preview by the owner, its release commit prepared, and a Windows tester kit from CI; 183 of 212 |
| 2026-11-25 | [The first hour, twice, and faster checks](2026-11-25-the-first-hour-twice-and-faster-checks.md) | H-18 measured twice (about 2 min, then 1 min 13 s); capture through to a scout; three systems' faults; CI in shards; 168 of 203 |
| 2026-11-24 | [The exit tests of the finished phases](2026-11-24-the-exit-tests-of-the-finished-phases.md) | For Phases 1, 2 and 4: the exit test, the test that is it, and its result on three systems; two gaps in Phase 1; what the other phases still lack |
| 2026-11-24 | [A gate before every push](2026-11-24-a-gate-before-every-push.md) | CI's Linux checks run here or in sibling sessions before a push; CI keeps its dependencies; 9.1, 8.4a, E4, 4.4 and 3.6's automated half; 119 of 150 |
| 2026-11-24 | [Many lanes at once](2026-11-24-many-lanes-at-once.md) | Up to twenty workers, one per group of crates; FT4's verified write in a field test; plain names; 81 of 149 |
| 2026-11-23 | [The design settled for the next lead](2026-11-23-the-design-settled-for-the-next-lead.md) | The holding folder, the secret-named field, and eight decisions that settle stages 1 to 4, with the build order written for a lead that takes over |
| 2026-11-23 | [The first field test](2026-11-23-the-first-field-test.md) | The owner makes existing software that the project runs itself a testing surface and hands the open questions to the agents; a runner for field tests; the first one, reads of an HTTP service, makes seven findings |
| 2026-11-18 | [The wider reach](2026-11-18-the-wider-reach.md) | The owner widens the scope to what the machine reaches and names the North Star; sign-in, page writes, shadow bindings, the desktop pack on three systems, the network facts, watches' kernel halves; two safety audit rounds answered; pull request 14 merged after five CI rounds, pull request 15 open |
| 2026-11-17 | [The overnight build](2026-11-17-the-overnight-build.md) | The kernel built layer by layer by parallel agents; six test stands; the harness; two spikes; the first pull requests to run on three operating systems, merged as 12 and 13 |
| 2026-11-17 | [The restructure, configuration, and the bar](2026-11-17-the-restructure.md) | The specification rebuilt as 59 sections in seven parts; the first design archived; one plan; configuration and the bar designed; findings built in the core |
| 2026-11-17 | [The architecture, ahead of the code](2026-11-17-the-architecture-ahead-of-the-code.md) | A spine of who defines and joins every interface; a design for each of the 24 crates; five more packs designed; one roadmap to 1.0; the layers redrawn; no product code changed |
| 2026-11-17 | [The rename, the owner's answers, and the rules for working without waiting](2026-11-17-the-rename-and-the-rules.md) | Everything renamed to Anything2MCP; kernel and packs enforced by the audit; MIT; agents no longer wait; the scout designed; the public files written; the core types built; nothing published |
| 2026-11-17 | [The universal design, sections 53 to 66](2026-11-17-the-universal-design.md) | A deep study of REA from source; fourteen new spec sections; six decision records; nothing built or renamed |
| 2026-11-16 | [The owner renames the product Anything2MCP](2026-11-16-anything2mcp-direction.md) | An audit, the REA study, and one long-term plan; nothing renamed or built yet |
| 2026-11-16 | [Lens 6, then slices 0.1, 0.2 up to the publish, and 0.3](2026-11-16-lens-6-and-slice-0-1.md) | One pipeline walked through the design and its joins fixed; the workspace and CI built; the release workflow ready and nothing published; the test application built |
| 2026-11-15 | [Design handoff](2026-11-15-design-handoff.md) | The design is complete and audited; no code exists; Phase 0 is next |

## What an entry contains

File name: the date, then a few words, such as `2026-11-20-slice-0-1-skeleton.md`. Keep it to one screen where possible.

```text
# Date: what the session was about

## State at the end
Where the project stands, in three or four lines. What works that did not before.

## Done
What was built or changed, with the slice number.

## Learned
What the work showed about the design. Hypotheses tested and their results.

## Changed in the design
Sections corrected, decision records added, hypotheses added.

## Noticed, not acted on
Doubts and ideas about parts not yet built.

## Decided without the owner having read it
Records and choices the owner should know about.

## For the owner
Only what needs the owner's own hands. Nothing here stops the work.

## Next
The first thing the next session should do.
```

An entry records what happened, not what was hoped. If something failed or was left half-done, say so plainly; the next agent has only this to go on.
