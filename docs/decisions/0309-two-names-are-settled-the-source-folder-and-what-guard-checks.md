# 0309. Two disagreements between documents are settled: the two folders named `sources`, and what guard checks

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-23

**Changes:** the [glossary](../reference/glossary.md), [section 205](../spec/2-kernel/205-target-files.md), [section 207](../spec/2-kernel/207-the-engine-loop.md), the design of `a2m-engine`, the architecture page. No code changes with this record; package GD1 holds the code to it.

## Context

Drawing the architecture showed two places where the documents do not agree. One name, `sources`, is used for a folder of a target (section 205: readers, fingerprints, mappings) and for a folder of the project (sections 218, 220, 402: files a person hands to a scout or a pipeline). And guard is said to check four things in section 207 and four different things in the engine's design.

## Decision

1. **There are two folders, and each has a proper name** ([decision 0301](0301-every-part-has-a-proper-name.md)). The **Source Definitions** are the files under a target's `sources/`: they say how a kind of handed file is read and mapped, and are compiled with the target. The **Handed Files** are the files in the project's `sources` folder: what a person gives a scout or a pipeline, named from the project folder, never compiled. Neither folder is renamed on disk. Every document says which it means by its proper name.
2. **Guard checks five things, in this order, and each has one owner.** The plan's hash against the published version (the engine). The kernel's health; a write needs `ok` (the engine, from `conditions`). The session's state for the call (the session keeper answers; the engine asks). The decision for this caller and this call (the policy engine decides and records). That the surface has no edge block (the engine, from what drivers reported). The first that fails names the stop, and nothing after it is asked.
3. **A driver's own stop for a missing session stays** as the second line, for a session that ends between guard and a step.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Rename the project's folder | Every project a person has made holds it, and the name is in the commands' help |
| Take either document's list of guard's checks as it is | Each leaves out something the other has and the code does |

## Proof

The documentation audit passes with both sections corrected. Package GD1: one test for each of guard's five checks, in order, that fails when the check is removed.
