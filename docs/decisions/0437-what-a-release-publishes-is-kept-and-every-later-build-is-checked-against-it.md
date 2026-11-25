# 0437. What a release publishes is kept, and every later build is checked against it

**Status:** Accepted. Design decision, made in the design review round by lens 14, a year later ([report](../reviews/a-year-later-review-2026-11-25.md), findings 3, 4, and 5). The owner had not read this record when it was written. It adds a check and a folder kept from each release, and narrows which actions get a client method; it refines point 1 of [decision 0420](0420-the-api-description-says-which-actions-have-a-client-method.md). Built by roadmap packages YL4 and YL5.

**Date:** 2026-11-25

**Changes:** a new folder `contracts/` at the top of the repository, one subfolder per release; a check in `tools/` and its CI step; the release workflow; `a2m-actions` (`RestRouteFace::sdk`), the two generators in `bindings/`; [section 605](../spec/6-running-it/605-packaging-and-release.md) ("Versions"); the technical reference's failure codes.

## Context

Section 605 says what counts as breaking: a target file that used to compile and no longer does; a tool, command, route, or client method removed or renamed; a failure code changed; a default made less safe. The technical reference says a failure code "never changes once a release has carried it". The action catalog says adding, renaming, or removing an action after its phase ships needs a decision record. Each is a promise, and nothing holds any of them: every check today compares the code with the documents of the same commit. The checked-in API description in `bindings/python/tools/openapi.json` is compared with the engine of the same build, so a change to both passes.

The rename sweep of [decision 0402](0402-the-rename-sweep-renames-every-name-and-keeps-what-is-stored.md) shows how much moves before a release: names of types, files, and failure codes. After a release the same sweep would break every agent that branches on a code and every script that calls a renamed action, and pass every check.

Three more facts:

- **Twenty-seven of the kernel's 115 actions have no handler** (among them `version.diff`, `version.migrate`, `version.rollback`, `run.undo`, `vault.rotate`, `delegation.grant`, and `plugin.install`). Each is served with a route, a command, and an MCP tool that answer `action_not_built`, and has a method in both clients (`version_diff` in the Python client, `versionDiff` in the Node one). Their inputs were designed and never used, and the first release would publish each as a client method.
- **A stored published version outlives an upgrade only if its compiled target still loads.** The engine reads the compiled target formats it lists; the format is 1. Nothing requires a change to the compiled target's shape to raise the format, and the test that holds the example target's compiled target to one hash is updated by whoever changes the shape. The store fixture of package 9.2a holds runs, records, evidence, and the audit chain, and no published version.
- **Which vocabularies are closed is said for one of them.** The technical reference says the failure classes are closed. Nothing says whether a run state, an effect class, or an evidence level may be added, and an agent that branches on one needs to know.

## Decision

1. **Each release keeps its contract.** Before a version is tagged, `contracts/<version>/` is written from the built program and committed, and never changed after: the API description as served (every built action's route, its inputs with their types and which are required, its output, and `x-sdk`); the failure-code table, each code with its class and whether it passes by itself; the names of each vocabulary of `a2m-core` that a result, a stored row, or a file carries (failure class, run state, record state, session state, effect class, hazard, archetype, evidence level, write evidence, coverage); the settings keys, each with its type; and the compiled target of the `example` target in each compiled target format the release writes, with its hash. The store is kept as package 9.2a already keeps it. The release workflow refuses a version that has no such folder.
2. **Every build is checked against every kept release.** The check fails, naming the thing, on: an action, an input, an output field, a failure code, a vocabulary name, or a settings key removed or renamed; a code moved to another class, or its "passes by itself" changed; an optional input made required; an input's or an output's type changed; a member added to a closed vocabulary (point 4); a kept compiled target that no longer loads, or loads with another hash. Additions pass.
3. **A break on purpose is listed.** Before 1.0, a minor version may break, as section 605 says: the break is listed in the next release's own kept folder, one line each, with the decision record that made it, and the changelog says so. The check passes a break so listed and nothing else. From 1.0, only a major version lists one.
4. **Closed and open vocabularies.** The failure class, the run state, the effect class, and the evidence level are closed: a client may branch on every member, and a new member is a break. A failure code is open: a release may add codes, each in one class, and a client branches on the class and reads the code as detail. Every result's text, warnings included, is open.
5. **An action's shape is promised from the release that builds it.** An action that has no handler carries `x-sdk: false` in the API description whatever its SDK face says, so neither client gets a method for it; its command and its MCP tool still answer `action_not_built`, and its route is described. The kept description lists built actions only, so an action not built may still change its inputs before it is built. This refines point 1 of decision 0420, which took `x-sdk` from the SDK face alone.
6. **What a first release does not freeze.** The provider contract and the driver interfaces have no implementer outside this repository in the first release: every pack is compiled in, the plug-in host is not built (package F3), and no crate but the product is published ([decision 0242](0242-the-owner-takes-the-recommended-answer-to-every-open-question.md)). They stay public contracts changed by decision record, and become a contract with others when the plug-in host ships; the manifest's format and contract numbers are already there for that day.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Rely on review and the changelog | Every change so far has passed review with its documents changed to match; a rename is exactly what review lets through |
| Compare with the previous release's published packages, fetched at check time | The check would need the network and a registry, and would fail when either is down. A folder in the repository is read offline and kept forever |
| Keep only the API description, which the clients are made from | Agents branch on failure codes and run states, and people's settings files and stored versions break on their own; the API description holds none of these |
| Leave unbuilt actions' methods in the clients, marked in their docstrings | A method is used whatever its docstring says, and its inputs would then be frozen before anyone has tried them |

## Proof

Package YL4: the check, run against a kept folder made from today's build, passes; a copy of the build's outputs with one action removed, one input made required, one failure code renamed, one code moved to another class, one run state added, and one settings key removed fails once for each, naming it; the same break listed in a `breaks` file with a record number passes; the kept compiled target of `example` loads with today's `a2m-core` and its hash checks; CI runs the check on Linux. Package YL5: a test over the whole catalog that every action without a handler has `x-sdk: false` in the served description; the regenerated clients have no method for any of them, the generators' tests say so, and the parity test still passes.

## What would reopen it

A published Rust crate or a plug-in host, which would add the provider contract to what is kept; or a release that needs a break the check cannot express, which would show the list of point 2 is incomplete.
