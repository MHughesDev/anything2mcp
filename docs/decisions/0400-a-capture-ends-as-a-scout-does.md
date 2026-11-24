# 0400. A capture ends as a scout does

**Status:** Accepted. Design decision, made by the design architect. The owner had not read this record when it was merged. Nothing in it is built.

**Date:** 2026-11-24

**Changes:** what is left of package E5, and what it needs; how the capture job of package E2b ends.

## Context

[Decision 0392](0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md) says what a capture finds and what is drafted from it. It does not say what runs the drafter, or what a person and their agent are handed when the capture ends. The scout already has every stage but the first: its findings are stored stage by stage, drafted by each pack's rules, tried, and reported, with the report hashed apart from the system's own words ([decision 0170](0170-the-scout-identifies-each-surface-with-one-discovery-job-and-stores-each-stage.md), [decision 0171](0171-the-scouts-report-is-hashed-data-with-the-systems-words-kept-apart.md)).

## Decision

1. **A capture is a scout whose first stage a person drove.** When the capture job ends, its findings enter the scout's pipeline at the draft stage: the same drafter, the same try, the same report, stored and resumable as a scout's stages are.
2. **`capture.stop` answers with the report's reference,** and `scout.report` reads it. No second kind of report exists.
3. **The try stage tries what a scout may try.** A read drafted from a `GET` is tried in the sandbox. A held draft is not tried and is listed with its question, as decision 0298 says. An asserted read cannot be tried under the drafts ([decision 0394](0394-an-asserted-read-of-a-web-surface-is-a-page-request-matched-by-one-shared-shape-and-shown-at-publish.md), point 4), and the report says so of each, with the step that comes next.
4. **The report says it came from a capture,** with how long it ran, how many screens and requests it saw, and each bound it hit. Its coverage is `partial`: it says nothing of what the person did not use.
5. **E5 no longer waits on E3.** The comparison stage and the scout's own budget are after the first release ([decision 0397](0397-what-the-first-release-needs-of-the-packages-left-and-what-waits-until-after-it.md)).

## Alternatives

| Alternative | Why not |
| --- | --- |
| A report of its own for a capture | Two forms of one thing, and an agent would have to learn both |
| Leave drafting to a separate call, `endpoints.infer` then a draft action for each finding | More for the person and their agent to do, for nothing they decide in between. `endpoints.infer` stays, for running the inference again over stored findings |

## Proof

Package E5's row: with a stub pack whose person-driven discoverer returns fixed findings, the capture's report lists the drafted reads, the held drafts with their questions, and what was tried; the same findings give the same report twice, by hash; nothing that may write is tried.
