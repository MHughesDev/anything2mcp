# 0164. The authoring actions served through the engine

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the design of [`a2m-actions`](../architecture/crates/a2m-actions.md); its guides; sentences of spec sections [220](../spec/2-kernel/220-actions-and-the-authoring-interface.md) and [403](../spec/4-building-tools/403-testing-and-publish-gates.md); the failure codes of the [technical reference](../reference/technical-reference.md#failure-codes). Uses the route of [decision 0163](0163-a-persons-act-an-agent-asks-for-is-carried-out-on-a-page-of-the-person-surface.md) for publishing.

## Context

A hand-written target could be compiled and published by a person at a terminal in one process, and nothing more of the build loop was served. The kernel below the actions holds the compiler and the drafter (`a2m-files`), the checked path (`a2m-provider`), the findings store (`a2m-ledger`), and the engine's `Try` and `Discover` jobs. This package walks the catalog's authoring actions in the order of the walkthrough and serves each whose parts exist.

## Decision

| Action | Decided | Alternative weighed |
| --- | --- | --- |
| `target.list` | The targets of the project folder of the call (each folder under `targets/` that holds a `target.yaml`), and those with a published version, each with `in_project` and its versions | Published targets only, as before. The catalog says "targets in the project" |
| `target.status` | What the engine sees itself: the files, whether the drafts compile and their problems, and the published versions; the state is `added`, `drafted`, or `published`, and the states between that need a system (`signed_in`, `explored`, `tried`, `tested`) are named in `unknown`, because nothing keeps them yet. `next` follows from the state | Inferring the missing states from sessions, findings, and runs. They would be guesses, and unknown means stop |
| `draft.read` | One file of a target's folder, by a path `RelPath` of `a2m-files` accepts and with no part that starts with a dot, reached through the checked path; a link, a folder, or a device is not read; at most a target file's 1 MiB; UTF-8 only | Reading any file of the project. The action stands in for a file system over the target's own files, and the project file and the sources are not drafts |
| `draft.write` | The file is read as its kind (`SourceFile::read`) and the target linted with it in place (`lint_one`). A file that does not parse, or does not fit its kind's schema, is not written. One that does is written, through the checked path (a new file renamed over the name, never through a link), and the lint of the whole target comes back with it | Refusing a file while the target has any lint error. A target being built has problems in files not yet written, so no first file could be written |
| `draft.try` | The drafts compiled as a draft, the plan kept in memory only (the newest 16), a run of its own with a `Try` job, waited for as `tool.call` waits, and its outcome shown as `tool.call` shows one. Only a drafted read is tried; a drafted write is refused with `action_not_built`, because a trial that writes a declared test record needs the publish gates' test records | Storing the drafted plan as a version. A version is a person's act; a draft is not |
| `discover.run` | For each surface of the target, or the one named: a run of its own with a `Discover` job, whose stored call names its surface and the focus. The runner gives the engine the surface as the job's tool and the focus alone as its input. The plan is the drafts' when the call is made in a project that holds the target, and the current version's otherwise. The findings are stored by the run's control as the engine sends them; the result counts them by kind | A column for the surface in the ledger's runs. The stored call is this crate's, and the runs table's tool kind allows only operations and pipelines |
| `finding.list`, `finding.describe`, `unknown.list` | Reads of the findings store and the unknowns ledger, paged where it says so. `finding.describe` takes the target as well as the identifier, because a finding is stored under its target | |
| `test.run` | The schema and lint gate, which is the compiler's, at the version a publish would use; every offline test the plan holds is listed as not run, because no replay of a recorded fixture through a driver exists in the engine or the provider contract; `passed` and `complete` are false, always, until the gates are built | Calling `test.run` not built. The lint gate is real and the list of tests waiting for a replay is what a builder needs to know |
| `publish.request` | Hands `version.publish` to the person surface, as decision 0163 says; it publishes nothing itself | |
| `init` | Section 220 gives it no action, and none is added: a project file is what makes a folder a project, so no action can write the first one through the checked project folder, and the client that runs `init` runs as the person. `init::init_project` of `a2m-actions` writes, new and never through a link, `anything2mcp.toml`, `AGENTS.md` (the text of `guides/guidance.md`), `.gitignore`, and `targets/`, and keeps every one already there. The product crate's `init` shortcut is to call it | A `project.init` action. It would need a row of the catalog, and a door that serves a remote caller must never write a folder of the caller's choosing |

Not built, and why:

- `target.probe`, `auth.detect`, `scout.run`, `scout.report`: the probe is a `Discover` job of the identify stage, and the scout's stages need a table of stage results whose migration is the ledger's (the design names it) and the drafter run on stored findings. `discover.run` is the piece they rest on.
- `target.add` writes the target folder and a first file. The `add` shortcut still stops before `target.probe`.
- `screen.snapshot`, `capture.*`, `endpoints.infer`, `spec.import`, `fixture.record`, `shadow.report`, `version.diff`: they need driver calls outside a run, recordings, or a comparison the kernel does not have.

## Consequences

- An agent can add a target, write and read its files, lint them, try its drafted reads, run its discoverers, read what they found, run the lint gate, and ask a person to publish, all through a running engine and any door.
- A trial's run and a discovery's run are ordinary runs: `run.get` and `trace.show` read them, and they are the caller's own.
- `test.run` never passes today. What the engine or the contract must add for the offline gate: a driver that answers from a fixture's recorded files, or a `Replay` job kind whose port reads them, so that an offline test runs the tool's steps against what was recorded.
