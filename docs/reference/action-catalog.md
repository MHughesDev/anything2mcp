# Action catalog

The single list of everything a person, an agent, or a program can ask Anything2MCP to do, apart from calling a target's own tools. Every row is one entry in the action registry ([spec section 220](../spec/2-kernel/220-actions-and-the-authoring-interface.md)). Every action listed is defined in the registry; one that has a handler is served, and one that has none answers `action_not_built`. Its command, MCP tool, REST route, and SDK method are generated from it and always share its name; an action with no handler has its command, its MCP tool, and its route, and no SDK method until it is built:

| Face | Form | For `target.probe` |
| --- | --- | --- |
| Command | `anything2mcp <noun> <verb>` | `anything2mcp target probe` |
| MCP tool | `a2m_<noun>_<verb>` | `a2m_target_probe` |
| REST | `POST /api/v1/actions/<noun>.<verb>` | `POST /api/v1/actions/target.probe` |
| SDK | One method for each action, named from its noun and verb: `client.<noun>_<verb>()` in Python, `client.<noun><Verb>()` in Node | `client.target_probe()`, `client.targetProbe()` |

This is a design draft. The list is settled action by action as each phase builds it; adding, renaming, or removing an action after its phase ships is a change to a public interface and needs a decision record. An action's inputs and output are promised from the release that builds it: one with no handler is to carry `x-sdk: false` and get no client method, and each release's built actions are kept and checked against every later build ([decision 0437](../decisions/0437-what-a-release-publishes-is-kept-and-every-later-build-is-checked-against-it.md); package YL4 keeps and checks them, and no release is kept yet; package YL5 built the rest: the API description says `x-sdk: false` for every route of an action with no handler, so neither client has a method for one, and `tools/contracts.py check` holds `x-sdk` to the actions `handler_for` builds).

**Columns.** *Who* is the lowest role that may use the action on a server ([spec section 214](../spec/2-kernel/214-access-control-toolsets-and-tenancy.md)); on one machine the user holds every role. *Listed* says whether the MCP tool appears in a tool list (`use` or `build` toolset) or is reached through `a2m_action_run`. *Phase* is when it is built. *Not built* after the phase marks an action with no handler: it answers `action_not_built`, is not listed up front, and has no SDK method. Each is left unbuilt on purpose, for the reason [decision 0456](../decisions/0456-the-first-release-leaves-out-what-its-first-hour-never-meets-and-a-shortcut-is-offered-only-when-built.md), point 6, gives, and [decision 0468](../decisions/0468-which-actions-with-no-handler-serve-one-machine-and-what-each-waits-on.md) says what each waits on; the list with each reason is the test `every_action_without_a_handler_is_named_with_its_reason` in `crates/a2m-actions/src/tests/unbuilt.rs` (package 5.1), and `tools/docs_audit.py` holds these marks to it. An action that gains its handler loses its mark, and leaves the list, in the same change.

## Using tools

| Action | Purpose | Who | Listed | Phase |
| --- | --- | --- | --- | --- |
| `tool.search` | Find operations and pipelines by words or module | Viewer | use | 1 |
| `tool.describe` | Full description and schemas of one tool | Viewer | use | 1 |
| `tool.call` | Call one operation or pipeline by name | Viewer or Operator, by effect | use | 1 |
| `run.get` | Status, progress, result, and trace link of a run | Viewer | use | 1 |
| `run.list` | The caller's recent runs | Viewer | No | 3 |
| `run.resume` | Continue a stopped run after reconciling | Operator | use | 3 |
| `run.cancel` | Stop a running or waiting run | Operator | use | 3 |
| `run.undo` | Build the reverse diff of a completed write run, for approval | Operator | No | 9, not built |
| `run.trace` | Summary of one of the caller's own runs' trace: each step's kind, outcome, and time, and each kept capture by its reference, never a value ([decision 0469](../decisions/0469-a-runs-trace-is-summarized-for-its-person-and-exported-as-one-file-in-the-working-folder.md)) | Viewer | No | 1 |
| `session.status` | Whether each target is signed in for the caller | Viewer | use | 2 |
| `session.login` | Ask the caller's person to sign in to a target; the person gives any credential on the engine's own page, never through an input | Viewer | use | 2 |
| `session.revoke` | End one of the caller's own sessions | Viewer | No | 2 |
| `watch.create` | Ask for a watch: one published read with one fixed input, repeated at an interval until an end, recording what was added, changed, or went out of view, and ending early when a condition over its result holds ([decision 0291](../decisions/0291-a-watch-ends-when-its-condition-holds-on-a-checks-own-result.md)); nothing runs until the caller's person confirms it ([decision 0283](../decisions/0283-watches-reach-an-agent-through-the-registry-and-their-person-confirms-each-on-the-page.md)) | Viewer | No | 5 |
| `watch.confirm` | Confirm a watch, so that its checks begin: a person's act on the engine's page, never an agent's | The person | No | 5 |
| `watch.get`, `watch.list`, `watch.changes` | One watch, the person's watches, and the changes a watch recorded after a number, each with the runs of its checks and never a value | Viewer | No | 5 |
| `watch.pause`, `watch.resume`, `watch.end` | Pause, go on with, or end a watch: its person, or for `watch.end` also the caller that asked for it | Viewer | No | 5 |

## Building a target

| Action | Purpose | Who | Listed | Phase |
| --- | --- | --- | --- | --- |
| `target.add` | Register a target in the project | Builder | No | 5 |
| `target.probe` | Platform, framework, support tier, and surfaces at an address or in a running desktop app | Builder | build | 5 |
| `target.status` | The target's build state and the next action | Builder | build | 5 |
| `target.list` | Targets in the project and their versions | Builder | No | 5 |
| `auth.detect` | The sign-in method, with evidence, and a recommended identity kind | Builder | build | 2 |
| `screen.snapshot` | Compact accessibility snapshot of the current page or window | Builder | build | 1, not built |
| `capture.start` | Ask for a capture of the caller's person's own use of a surface: nothing records until the person begins it on the engine's page, past the presence prompt, where they are told what is kept; then only structure is kept, never a value ([decision 0392](../decisions/0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md), point 2; [decision 0433](../decisions/0433-a-capture-is-asked-for-by-capture-start-begun-by-its-person-on-the-page-and-held-in-the-engines-memory.md)). The press on the page carries the capture's identifier, with the person's proof | Builder | build | 4 |
| `capture.stop` | End a capture the caller asked for, or read how it ended: by the asker, by its person on its page, or at its discoverer's bound. What it found is stored as findings, and goes through the scout's stages from the draft stage on: drafted by rule, the drafted reads tried, and reported, stored stage by stage. Answers with `report`, the report's reference (the scout's identifier and the report's hash, or the stage it stopped at), which `scout.report` reads by `scout`; `wait_seconds` bounds the wait for the trials, and a call again goes on (decision 0451) | Builder | build | 4 |
| `endpoints.infer` | Endpoint templates, schemas, and dependencies from recordings | Builder | build | 4, not built |
| `spec.import` | A new scout of a target handed one file of the project's `inputs` folder, a description the system publishes about itself: `scout.run` with `files` naming it `description`, and nothing more | Builder | build | 4 |
| `draft.read` | Read a draft file; stands in for a file system | Builder | build | 5 |
| `draft.write` | Write a draft file after validation and lint | Builder | build | 5 |
| `draft.delete` | Remove draft files of the project, one or several, all or none; a published version keeps its copy | Builder | build | 5 |
| `draft.lint` | Compile drafts and report every problem | Builder | build | 1 |
| `draft.try` | Run a draft binding in the sandbox | Builder | build | 5 |
| `test.run` | Run the publish gates and return evidence; with `tools`, the gates of a publish of only those tools ([decision 0457](../decisions/0457-a-publish-of-named-tools-compiles-only-their-files.md)) | Builder | build | 3 |
| `fixture.record` | Record a read's offline test and fixture from one real read in the sandbox, every value scrubbed by rule | Builder | No | 3 |
| `assertion.confirm` | Confirm the reads a target's drafts assert, so that a try of the drafts in an environment of kind `test` may let a page send each, with a body its closed schema allows: a person's act on the engine's page, never an agent's. What is confirmed is the digest of each read as the page showed it, kept in the engine's store; a change to its method, path, body, or address undoes it ([decision 0999](../decisions/0999-a-person-confirms-an-asserted-read-on-the-engines-page-and-a-try-of-the-drafts-may-then-send-it.md)) | The person | No | 5 |
| `draft.ready` | Every drafted read a trial in this project read a value through, made ready to publish: its offline test and fixture recorded, its first binding promoted on the fixture's own trial, the publish gates run, and a person asked to publish, each step by the action that does it, for the same caller. A write, a pipeline, and a read no trial read through are left alone, each with its reason and the step that moves it on; a call that names one is refused before anything is done. With `tools`, it publishes only the reads named, and `tools` under `publish` says what the version holds ([decision 0457](../decisions/0457-a-publish-of-named-tools-compiles-only-their-files.md)). Waiting for the person's press, it answers `ok: true`, exit code 3, with `state: needs_person` and a text that starts `Succeeded:` ([decision 0458](../decisions/0458-an-answer-that-did-its-part-and-waits-for-a-person-is-ok.md); [decision 0401](../decisions/0401-the-steps-between-a-proven-read-and-a-publish-are-one-action.md), [decision 0443](../decisions/0443-draft-ready-asks-for-the-publish-and-takes-the-callers-own-trial.md)) | Builder | build | 5 |
| `shadow.report` | How often a shadow binding matched, and where it differed | Builder | build | 4 |
| `target.show` | Print a target's compiled target as text | Builder | No | 1 |
| `version.diff` | Compare two versions and classify each change | Builder | No | 5 |
| `publish.request` | Submit a version for human review; with `tools`, a version of only those tools ([decision 0457](../decisions/0457-a-publish-of-named-tools-compiles-only-their-files.md)) | Builder | build | 5 |

## Providers and discovery

Added with spec sections 203, 218, 220, and 402. Built with the stage of the [long-term plan](../plan/roadmap.md) named.

| Action | Purpose | Who | Listed | Phase |
| --- | --- | --- | --- | --- |
| `provider.list` | Every pack and provider: installed, enabled, ready, or what it lacks | Viewer | build | Stage B |
| `provider.describe` | One provider's manifest, effects, and requirements | Viewer | No | Stage B |
| `provider.enable`, `provider.disable` | Switch a provider on or off; a protected setting on one machine where it starts processes or reaches a network | Admin | No | Stage B, not built |
| `component.list` | Outside programs providers need: found or not, with path and digest, known or unverified; starts nothing | Viewer | No | Stage B |
| `discover.run` | Run the discoverers that suit a target's surfaces; returns findings. `files` hands them files of the project's `inputs` folder, each under a name a discoverer reads | Builder | build | Stage E |
| `finding.list`, `finding.describe` | Findings for a target, by level, kind, or source | Builder | build | Stage E |
| `unknown.list` | The open questions about a target | Builder | build | Stage E |
| `unknown.settle` | Settle an open question with evidence | Builder | No | Stage E, not built |
| `scout.run` | Identify a target, run the discoverers that suit it, draft what follows by rule, and try the drafted reads; resumable. `files` hands its discoverers files of the project's `inputs` folder, as for `discover.run`, and a resumed scout hands the same ones. A scout whose discovery of every surface it observed failed answers `ok: false`, with that failure and the report; one where only some failed answers `ok` with a warning for each (decision 0451) | Builder | build | Stage E |
| `scout.report` | The last scout's report, or the one `scout` names, such as a capture's: found, drafted, proven, open, and what to look at first; each list counted and cut to its first entries, one read whole in pages with `part`, `limit`, `offset`. A report from a capture says so, how it ended, and that its coverage is partial | Builder | build | Stage E |

## Publishing and versions

| Action | Purpose | Who | Listed | Phase |
| --- | --- | --- | --- | --- |
| `version.publish` | Publish a reviewed version; a person, never an agent; with `tools`, of only those tools ([decision 0457](../decisions/0457-a-publish-of-named-tools-compiles-only-their-files.md)) | Publisher | No | 5 |
| `binding.promote` | Make a drafted operation's first binding verified on the evidence of its tests and a trial, or a shadow binding on its run of exact matches; for a write or a shadow binding, a person | Publisher | No | 4 |
| `version.activate` | Move a toolset to a version, through the candidate stage | Publisher | No | 6, not built |
| `version.rollback` | Make another published version current, earlier or later; a person, never an agent; recorded in the audit and read back ([decision 0470](../decisions/0470-a-rollback-is-a-persons-act-and-moves-to-any-version-a-person-published.md)) | Publisher | No | 5 |
| `version.bundle` | Pack a version into a signed file | Publisher | No | 6, not built |
| `version.import` | Import a signed bundle | Publisher | No | 6, not built |
| `version.migrate` | Recompile stored sources for a newer engine | Publisher | No | 9, not built |

## Administration

| Action | Purpose | Who | Listed | Phase |
| --- | --- | --- | --- | --- |
| `toolset.create`, `toolset.update`, `toolset.remove` | Create, change, or remove a toolset: who may connect, which tools it selects, the highest effect class and the identity kind of its runs. A toolset only narrows what grants allow. A person's act, published as a new version of the policy set ([decision 0328](../decisions/0328-a-toolset-only-narrows-a-role-is-a-bundle-of-grants-and-a-second-administrator-is-a-named-check.md), [decision 0339](../decisions/0339-the-access-actions-the-built-in-toolsets-and-where-a-reach-is-kept.md)) | Admin | No | 6 |
| `toolset.list` | The toolsets in force, each with the published tools it selects | Admin | No | 6 |
| `grant.add`, `grant.remove` | Add or remove a grant of the use of tools up to an effect class. A person's act, published as a new version | Admin | No | 6 |
| `grant.list` | The grants, the grants of roles, and who holds which role, in the set in force | Admin | No | 6 |
| `policy.publish` | Publish a policy file after its tests pass; where the set in force asks for a second administrator, the reviewer proves presence of their own. A person's act | Admin | No | 6 |
| `policy.test` | Run a policy file's decision table against its own layers | Admin | No | 6 |
| `policy.versions` | The history of published policy versions, and which is in force | Admin | No | 6 |
| `policy.tools_allowed`, `policy.who_can` | Which published tools a principal may use, and which principals may use one tool, through a toolset where one is named; records no decision | Admin | No | 6 |
| `policy.explain` | Why a past call was allowed or denied | Auditor or Admin | No | 6 |
| `session.list` | Sessions in a workspace, without contents | Admin | No | 6 |
| `session.end` | End someone's session; never use it | Admin | No | 6 |
| `runner.enroll`, `runner.list`, `runner.drain`, `runner.revoke` | Manage runners | Admin | No | 7, not built |
| `killswitch.set` | Stop a target, a workspace, or the whole system | Admin | No | 3 |
| `audit.query` | Read the audit log | Auditor | No | 6 |
| `secret.set` | Store a secret from a prompt or standard input | Admin | No | 2 |
| `vault.rotate` | Rotate a key layer | Admin | No | 9, not built |
| `delegation.grant`, `delegation.revoke`, `delegation.list` | A person lets an agent act for them, and withdraws it | The person | No | 7, not built |

## The machine itself

| Action | Purpose | Who | Listed | Phase |
| --- | --- | --- | --- | --- |
| `engine.status`, `engine.start`, `engine.stop`, `engine.logs` | Control the local engine | Local user | No | 1 |
| `config.show` | Effective settings and where each came from | Local user | No | 1 |
| `config.set` | Change a setting; protected settings need the user-presence prompt | Local user | No | 3, not built |
| `doctor.run` | Check browser, key store, network, sessions, clock; print fixes | Local user | No | 2 |
| `net.interfaces`, `net.routes`, `net.resolvers`, `net.neighbours`, `net.listeners`, `net.proxy` | What this machine already knows of its own networks, read from the operating system; sends nothing. `net.proxy` reads this engine's own environment on Linux and the user's system setting on Windows, and removes any credentials | Local user | No | 2 |
| `net.standing` | Where this machine stands: the kind of place each address is, and which interface carries each family's preferred default route; sends nothing | Local user | use | 2 |
| `net.current` | Which facts of the network this machine is on were read, to recognize it again, and which were left out and why; shows no digest and no address; sends nothing | Local user | No | 2 |
| `reach.name`, `reach.withdraw` | A person names the network this machine is on now, with the destinations that may be looked at and sent to, or forgets a reach; the network is read from the machine and never taken from an input; an agent cannot do either ([decision 0339](../decisions/0339-the-access-actions-the-built-in-toolsets-and-where-a-reach-is-kept.md)) | The person | No | N3 |
| `reach.list` | The reaches a person has named: label, the names of the facts each states, whether it is in force here, and the person's own entries; no digest | The person | No | N3 |
| `browser.install` | Fetch a pinned browser | Local user | No | 1, not built |
| `plugin.install`, `plugin.list` | Manage plug-ins | Admin | No | 8, not built |
| `client.connect` | Give the configuration an MCP client needs to start the stdio proxy: printed for `generic`, or set in the file a client profile names, shown first, by a local person | Local user | No | 5 |
| `trace.show` | View a run's evidence | Auditor | No | 1 |
| `trace.export` | Export a run's evidence to one file, `.anything2mcp/exports/<run>.json` in the project: its events, its step trace, and its text captures, with a manifest naming each piece and what was left out and why; read back before its digest is given ([decision 0469](../decisions/0469-a-runs-trace-is-summarized-for-its-person-and-exported-as-one-file-in-the-working-folder.md)) | Auditor | No | 1 |
| `action.search`, `action.describe`, `action.run` | Reach any action without listing it | Any | use and build | 5 |
| `docs.get` | Built-in reference pages | Any | No | 5 |
| `self.update` | Update a standalone install | Local user | No | 9 |

## What is deliberately absent

- No action approves a write or a publish on a local machine ([spec section 222](../spec/2-kernel/222-front-doors-and-the-local-engine.md)).
- No action reads a secret back.
- No action runs as another person, and `session.end` cannot be used to enter a session.

## Shortcuts

Single-word commands for people. Each is a fixed sequence of the actions above and adds no behavior. A shortcut is offered only when every one of its steps is built ([decision 0456](../decisions/0456-the-first-release-leaves-out-what-its-first-hour-never-meets-and-a-shortcut-is-offered-only-when-built.md)); this table lists those that are. `bundle` and `migrate`, each the `version` action of its name, are defined and not offered, because their actions are not built; each joins this table with its action's handler.

| Shortcut | Does |
| --- | --- |
| `init` | Make the folder a project: write `anything2mcp.toml`, the guidance as `AGENTS.md`, the `targets` folder, and a `.gitignore`, each only where it is not there; the command does it itself, with no action beneath it ([decision 0164](../decisions/0164-the-authoring-actions-served-through-the-engine.md)) |
| `add <name>` | `target.add`, `scout.run`, `scout.report`; takes `--address` and `--kind` for `target.add` and `--description <file>` for `scout.run` (a description of the system in the project's `inputs` folder, handed over as `spec.import` hands it). It ends at the report, whose `next` is `session.login` only where a read waits for a sign-in; each proven read in the report carries the `draft.try` call that reads it ([decision 0306](../decisions/0306-the-short-path-is-add-then-a-trial-and-only-a-built-action-is-listed-up-front.md)) |
| `learn <name>` | `capture.start`: asks for a capture of the person's own use and gives the page where the person begins and ends it; `capture.stop` afterwards says how it ended and scouts what it found ([decision 0451](../decisions/0451-a-capture-is-scouted-when-capture-stop-finds-it-ended-and-a-scout-that-saw-nothing-does-not-answer-ok.md)) |
| `lint`, `try`, `test` | `draft.lint`, `draft.try`, `test.run` |
| `ready <name>` | `draft.ready`: a read-only tool that was tried is then `add`, `ready`, and a person's press ([decision 0443](../decisions/0443-draft-ready-asks-for-the-publish-and-takes-the-callers-own-trial.md)) |
| `publish` | `publish.request`, then waits for a person |
| `status` | `target.status` |
| `call`, `run` | `tool.call` for an operation or a pipeline |
| `diff` | `version.diff` |
| `rollback` | `version.rollback`: a person's act ([decision 0470](../decisions/0470-a-rollback-is-a-persons-act-and-moves-to-any-version-a-person-published.md)) |
| `connect <client>` | `client.connect` |
| `doctor` | `doctor.run` |
| `docs <topic>` | `docs.get` |

## Process commands

These start a long-lived role and are not actions.

| Command | Starts |
| --- | --- |
| `serve` | A server in the configured deployment shape |
| `runner` | A runner that dials a control plane |
| `stdio` | The link an SDK or a stdio MCP client uses to reach the local engine |
| `channel` | The local channel as bytes on standard input and output, after the engine's owner is checked, for a client that cannot make the check itself: the Node client on Windows ([decision 0466](../decisions/0466-the-node-client-on-windows-reaches-the-engine-through-the-channel-command.md)) |
| `completion` | Nothing: prints a completion script for a shell, answered by the program itself |
