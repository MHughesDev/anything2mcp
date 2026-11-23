# 0306. The short path is `add`, then a trial, and only a built action is listed up front

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-23

**Changes:** the registry's toolset listing and the `add` shortcut in `a2m-actions`, the scout's report, the answer of the trial action, roadmap package S2.

## Context

[Decision 0300](0300-publishing-always-needs-a-person-and-the-short-path-is-the-sandbox.md) made the sandbox the short path: an agent reads a value through a trial, with no publish. Three facts in the code stand in the way. The build toolset lists twelve marked actions up front, and the trial action is the thirteenth; three of the twelve (`capture.start`, `capture.stop`, `endpoints.infer`) have no handler. `add` takes an address and cannot be handed a published description. And the report says which reads were proven, not how to call one.

## Decision

1. **An action with no handler is never listed up front in a toolset.** It stays in the catalog, is found by `action.search`, and answers that it is not built. The listing takes marked actions that are built, in catalog order, to the cap. With today's catalog the trial action is then listed.
2. **`add` takes the descriptions a person has.** `add <address> --description <file>` puts the file in the project's `sources` folder and hands it to the scout under the name `description`, exactly as `spec.import` does. With no file, `add` is as it was.
3. **`add` ends at the report and does not ask for a sign-in unless a read waits for one** ([decision 0303](0303-a-declared-sign-in-is-kept-and-a-read-that-waits-for-one-is-reported-as-waiting.md)). Where every drafted read is proven, nothing is asked.
4. **Each proven read in the report carries the call that reads it**: the trial action's name with the target, the tool, and an input that was accepted. An agent copies it.
5. **A trial's answer says what it is.** Beside the result it states `sandbox: true` and `published: false`, and that a published tool is called with `tool.call`. The words are fixed text, not the builder's.
6. **The short path is measured.** A field test records the time from an empty folder to the first value read by an agent through `connect`, and the count of commands. The target is three commands; the time is recorded, and a budget is set from the first three measurements.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Raise the toolset's cap | The cap is there so a client's list stays short (decision 0094). Slots held by actions that cannot run are the waste |
| Reorder the catalog so the trial comes sooner | The order would then be a second place that says what is built |
| A new action that lists what can be tried | The report already holds it; a third list would disagree with the two there are |

## Proof

From an empty folder: `init`, `add` with a description of the test application, `connect`; a client with the build toolset sees the trial action in its list, copies a call from the report, and reads a value. No action without a handler is in any toolset's list, by a test over the whole catalog.
