# 0171. The scout's report is hashed data, with the system's words kept apart

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the design of [`a2m-actions`](../architecture/crates/a2m-actions.md) and its guides; one sentence of spec section [402](../spec/4-building-tools/402-the-scout-and-who-does-what.md). Follows [decision 0170](0170-the-scout-identifies-each-surface-with-one-discovery-job-and-stores-each-stage.md), which stores the stages the report is made from.

## Context

Section 402 asks for one report: what was found, what was drafted, what was proven, what is open, and what to look at first; the same system in the same state gives the same report, and the report carries a hash to show it. Section 502 asks that text a system supplied is never shown as an instruction. `scout.report` returns the last report.

## Decision

| Question | Decided | Alternative weighed |
| --- | --- | --- |
| What the report holds | `content`: the surfaces as identified, the sign-in claims, the findings (identifier, kind, level, source, coverage, limitation codes), the comparison, the drafts file by file with their state and problems, the check, the tried reads with their state, tries, how many completed, whether all gave one result, and their level, the target's questions that are not settled, what was not looked at and why, and what to look at first. Beside it: the target, the scout's identifier, the plan's hash, `report_hash`, and the runs (the discovery run of each surface, the five trials of each read) | A report that lists runs inline. A run identifier is new on every scout, so the hash would never repeat |
| What is hashed | The digest of `content`, written as JSON with its keys in order and its lists in a fixed order. The plan's hash is left out: it changes when the scout's own drafts join the target, and the scout that drafts and the one after it then report the same thing | Hashing the whole stored result. Identifiers and the changing plan hash make every report differ |
| The system's words | Every text a system gave, a finding's `found` and a tried read's result, is kept under `untrusted` and nowhere else. Names the scout made (files, tools) come from the system's words by a pack's rule and are checked names; the report says they are provisional | Quoting findings in sentences. A finding's text may hold words aimed at the agent that reads the report |
| What to look at first | Fixed, in section 402's order: meaning (the drafted files, whose names are provisional), writes (none drafted; what the drafter refused), and the open questions (with their count) | Ranking by an estimate of importance. That is judgment, which the runtime does not do |
| What was not looked at | Every surface that was not identified, with its failure code; every discoverer bound reached and every discoverer failure; every stage that needs a project folder and had none; every read not tried and why; the comparison of each pack's own rule (not built); the budget for a whole scout (not kept) | Leaving out what is not built. A report that does not say what it skipped reads as complete |
| The rendering | `text`: one line per surface, per kind of finding by level, per draft, per tried read, the count of open questions, each thing not looked at, and the order to look in. It is made of the engine's words and checked names, and quotes no text of the system; it says where that text is | A rendering by each face. The result is the data and every face renders the same result |
| `scout.report` | The newest finished scout of the target by the caller's person, as stored by its last stage, with a warning when a newer scout has not finished. With none, `input`, `not_found`, and `scout.run` as the fix | Returning a scout in progress. A partial report would be read as a finished one |

## Consequences

- Two scouts of the same stand in the same state, with the same drafts, give the same `report_hash`; the tests check it on the test application and in a project through every stage.
- An agent that reads the report reads the system's words only where they are marked, and the terminal rendering shows none of them.
- `next` after a report is `unknown.list` when a question is open, and `target.status` otherwise.
