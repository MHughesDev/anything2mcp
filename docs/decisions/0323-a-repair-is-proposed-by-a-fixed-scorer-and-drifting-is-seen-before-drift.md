# 0323. A repair is proposed by a fixed scorer, and drifting is seen before drift

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-23

**Changes:** [section 217](../spec/2-kernel/217-traces-evidence-and-repair.md), "Drift and the repair loop"; hypothesis H-49. The web pack's files and the screen model's format are to follow (see the end).

## Context

Section 217 never heals at run time, stops on an ambiguous match, and hands a repairing agent a fixed bundle. [Research note 37](../research/37-locator-drift-and-repair.md) found that every mature open tool repairs by similarity, and that the documented failure of each is a repair that passes on the wrong element ([read] for one tool's threshold; [search] for the failure). Its figures for how often locators break, and which kinds break most, are from search summaries of papers and are not built on here. No study compares role-and-name locators with test ids; hypothesis H-49 is to measure it.

## Decision

1. **The repair bundle may hold ranked candidates.** Where a pack has a repair proposer, the bundle holds the failure snapshot's candidate elements, each with its score by property. The proposer is deterministic, under fixed weights the pack declares, and proposes nothing when the top two candidates are closer than a margin the pack declares; the failure then stays `locator_ambiguous`.
2. **No score applies anything.** A repair is a file change a person approves, as before; it never applies at run time, whatever its score.
3. **A canary reports `drifting`** when a field or action still passes but its matched locator is not the first in its list. A `drifting` tool stays available, is shown in the drift metrics and the engine's health, and does not stop a run. A drop in the fingerprint score since the last verified run is a second sign, built when the screen model keeps a reference of the matched element.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Heal at run time above a threshold | The documented failure of every tool that does it: a write to the wrong element that passes |
| No proposer; the agent reads the snapshot | Works, but leaves no record of why a candidate was chosen, and two agents may choose differently |
| Treat a fallback match as `drift` | It stops tools that still work; the signal is wanted before the stop, not as one |

## Proof

In the test application's changed-screen proof: a screen reworded so that the first locator fails and the second matches reports `drifting` and keeps running; a screen with two equally good candidates proposes nothing and stays `locator_ambiguous`; no write reaches a wrong element in any case.

## Owed to files this record does not edit

- The screen model's field and action (file formats): an optional record, from the last verified run, of the matched element's role, name, label, test id, section, nearby text, and order among its siblings, as data for the proposer only, never read by the driver to find an element. It changes a file format and takes its own record.
- Section 301: that a locator list should include one that does not depend on the element's words, such as a stable test id, with a lint warning when every locator in a list depends on the same visible string; and that `drift_risk` is a judgment until H-49 gives numbers, which then set it and the canary interval, with a record.
- Roadmap package 9.1's proof: each changed-screen case of H-49 is caught as `drifting` or `drift`, repaired by a reviewed patch, and no write reaches a wrong element.
