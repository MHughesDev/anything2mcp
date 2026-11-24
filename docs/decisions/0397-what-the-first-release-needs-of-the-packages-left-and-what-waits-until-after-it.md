# 0397. What the first release needs of the packages left, and what waits until after it

**Status:** Accepted. Design decision, made by the design architect at the lead's asking. The owner had not read this record when it was merged. It moves packages; it changes nothing in the product. Supersedes, of [decision 0390](0390-the-open-questions-of-2026-11-24-are-settled-by-the-lead-and-the-first-release-is-what-progress-counts-toward.md), the half of point 8 that puts package 9.1b before the first release.

**Date:** 2026-11-24

**Changes:** the [roadmap](../plan/roadmap.md) (rows moved, split, and added) and [the line](../plan/build-order.md) in the Build Order.

## Context

When the line was drawn, twelve of the packages left needed design. This close to the first release that is the wrong way round. Each was asked one question: does a person on one machine, with the product as the first release promises it, need this? Where the answer is no, the package moves after the release with its reason, and is not designed now.

## Decision

### Moved after the first release

| Package | Why the first release does not need it |
| --- | --- |
| 9.1b, canaries on a schedule of their own | A watch is a canary a person can set today, and a drift met at a call is a named stop before anything is sent. A canary nobody asked for sends requests to a person's system that nobody confirmed, which needs a design of its own |
| E3, described against observed | It serves a system that has both a published description and a capture. No field test has one, and the import and the capture each stand alone |
| W2d, the rest of watches | A watch reports changes by digest today. Records compared by key, a reported removal, the MCP resource, and the REST stream add to that and take seven crates |
| 1.9b, the split of a call and the warm tab alone | Both need time marks the trace does not carry, a change to a public contract. Section 225 says of each that it is not measured |
| 8.3, beyond writes on three systems: starting an application, keys and clicks, `desktop_probe` | The person starts their own application; a tool that finds no window stops by name |
| 5.4, the Node client on Windows | It refuses by name until its check of the server is supplied. The Python client and the command line reach the engine there |
| The scout's budget, and its comparison stage (of E5) | The comparison is E3's. A scout is bounded by each discoverer's own bounds |
| The 23 actions in the catalog with no handler and no package before the release | Each answers `action_not_built` and is not listed up front ([decision 0306](0306-the-short-path-is-add-then-a-trial-and-only-a-built-action-is-listed-up-front.md)). They are `run.trace`, `trace.export`, `version.diff`, `version.rollback`, `version.activate`, `version.bundle`, `version.import`, `version.migrate`, `provider.enable`, `provider.disable`, `component.list`, `runner.enroll`, `runner.list`, `runner.drain`, `runner.revoke`, `vault.rotate`, `delegation.grant`, `delegation.revoke`, `delegation.list`, `config.set`, `browser.install`, `plugin.install`, and `plugin.list`. The list was made by a search of the sources for each name, and package 5.1 replaces it with a test |

### Kept before the first release, and made small

| Package | What it is now |
| --- | --- |
| 1.9 | The two numbers a benchmark project can give, `page_http` and `api`, and the three systems' stored numbers read into section 225 |
| 5.1 | A test in `a2m-actions` that names every action without a handler, so that an action left unbuilt is a decision and not an oversight; the action catalog marks the same |
| 5.2 | A check: a test from an empty folder reaches a linted draft with the shortcut commands alone. If `first_hour.rs` already shows it, the row is closed on that test |
| 5.3 | A test: each step of the first hour is named in the guidance file, and every page the guidance file or a failure's next step points to is one `docs.get` serves |
| 5.4 | The Python client's test against a real engine, run in CI's Windows job |
| 5.6a, new | The number H-18 has never had: an agent with no knowledge of this repository, given the installed program and the test application's address, timed from the first command to a published read it calls, three times. The lead runs it; the person's press is the field harness's prompt |
| E5 | A capture ends as a scout does ([decision 0400](0400-a-capture-ends-as-a-scout-does.md)) |
| 9.2a, new | One machine's half of 9.2: a store written by the version before opens, and a data folder copied while the engine is stopped and put back resumes a stopped run with no duplicate. No new action |
| 9.3a and 9.3b, new | A map, written by a reader, of every failure of section 224 and every attack of section 502 that one machine can meet, to the test that exists for it; each one without a test becomes a row of its own. Documents and new rows only |
| L4, new | The review's run for a launcher ([decision 0398](0398-the-review-shows-a-launched-program-as-a-launcher-and-says-what-nothing-pins.md)) |

### Limits the lead set

Before the release, only what one machine needs. Nothing about replicas, rotation across servers, or the ordering of an upgrade across parts. Each local half is one worker's.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Design all twelve | Forty-five rows built thinly, where thirty can be built well |
| Keep 9.1b, as decision 0390 said | Decision 0390 wanted a person to see that a tool stopped without calling it. Package 9.1a gives that. What 9.1b adds is finding a drift before any call, and its cost is requests nobody confirmed |
| Drop the moved rows | Each is in the design and stays in the roadmap, held, with its reason |

## Revisit if

A field test shows a person hurt by one of the moved rows before the release: a drift nobody saw for days, a description and a capture that disagreed, a desktop application nobody could start.
