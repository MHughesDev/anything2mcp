# 0468. Which actions with no handler serve one machine, and what each waits on

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-25

**Changes:** the reasons in `crates/a2m-actions/src/tests/unbuilt.rs`. It adds to [decision 0465](0465-while-the-first-release-waits-on-the-owner-agents-build-the-one-machine-packages-moved-after-it.md), point 1, and [decision 0456](0456-the-first-release-leaves-out-what-its-first-hour-never-meets-and-a-shortcut-is-offered-only-when-built.md), point 6, and reverses neither.

## Context

Decision 0465 has agents build the actions with no handler that serve one machine. On 2026-11-25 the catalog held 26 such actions. Each was asked two questions. Does it serve one person's machine, or only a shared server? And does the specification say enough to build it, or does it leave a real choice open?

## Decision

**Built now**, with their records: `version.rollback` ([decision 0470](0470-a-rollback-is-a-persons-act-and-moves-to-any-version-a-person-published.md)), `run.trace` and `trace.export` ([decision 0469](0469-a-runs-trace-is-summarized-for-its-person-and-exported-as-one-file-in-the-working-folder.md)), and `component.list`. Section 203 and section 220 specify `component.list` in full: each requirement looked for at its manifest's fixed places, with the path, the digest, and whether the digest is known. It shares one check with the availability that `provider.list` reports (`component_state` in `a2m-provider`).

**Serve one machine, and wait on a choice or a part not built.** Each stays unbuilt, with this reason:

| Action | What it waits on |
| --- | --- |
| `config.set` | It writes a person's settings file. Comments and layout must be kept, which needs a TOML editor the workspace does not have. A loosened protected value must also be kept as a record bound to the presence check, which the loader must then read in place of the user's layer. Section 226 names the record and does not design it |
| `provider.enable`, `provider.disable` | They change `providers.enabled` through the same record as `config.set`, and the registry must be built again while the engine runs |
| `vault.rotate` | The vault's design, step 7, leaves open what rotating an owner's key means, and when a second engine sees the rotation |
| `version.activate` | The candidate stage, with shadowed reads and a toolset pinned to an exact version. Toolsets do not name versions yet |
| `version.bundle`, `version.import` | The bundle's format, and the key that signs it, are not designed |
| `version.migrate` | The store keeps compiled targets, not the sources a newer engine would compile again |
| `run.undo` | A reverse write that a person approves: a design of its own (Phase 9) |
| `screen.snapshot` | A snapshot operation that each pack would supply, which the provider contract does not have |
| `endpoints.infer` | Decision 0456, point 2: running the inference again over stored findings, after the release |
| `unknown.settle` | What counts as evidence that settles a question is not designed. A held draft's question is settled by writing the draft |
| `browser.install` | Where a pinned browser is fetched from, and with which digests on each system. It sends a request off the machine, which only a person switches on |
| `plugin.install`, `plugin.list` | The plug-in host and its signature check (package F3), and where a search for installed plug-ins looks |

**Serve only a shared server.** `runner.enroll`, `runner.list`, `runner.drain`, and `runner.revoke`: a runner dials a control plane, and a shared server comes after the first release by the owner's direction. `delegation.grant`, `delegation.revoke`, and `delegation.list`: on one machine an agent acts for the person because it reaches the engine as that user ([section 215](../spec/2-kernel/215-sessions-and-the-session-keeper.md)). A delegation is the third case, an agent with an identity of its own, which only a server meets.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Build `config.set` with a writer that rewrites the whole file | A person's comments and layout would be lost on the first change |
| Build `plugin.list` over the Python search alone | It would list plug-ins the registry never loads, and the place it looks is still a choice |

## Revisit if

A settings editor that keeps comments is added to the workspace, or the vault's step 7 is settled. Either would make the actions that wait on it ready to build.
