# Action catalog

The single list of everything a person, an agent, or a program can ask Anything2MCP to do, apart from calling a target's own tools. Every row is one entry in the action registry ([spec section 40](../spec/40-authoring-interface.md)). Its command, MCP tool, REST route, and SDK method are generated from it and always share its name:

| Face | Form | For `target.probe` |
| --- | --- | --- |
| Command | `anything2mcp <noun> <verb>` | `anything2mcp target probe` |
| MCP tool | `a2m_<noun>_<verb>` | `a2m_target_probe` |
| REST | `POST /api/v1/actions/<noun>.<verb>` | `POST /api/v1/actions/target.probe` |
| SDK | `client.<noun>.<verb>()` | `client.target.probe()` |

This is a design draft. The list is settled action by action as each phase builds it; adding, renaming, or removing an action after its phase ships is a change to a public interface and needs a decision record.

**Columns.** *Who* is the lowest role that may use the action on a server ([spec section 20](../spec/20-access-control-toolsets-and-tenancy.md)); on one machine the user holds every role. *Listed* says whether the MCP tool appears in a tool list (`use` or `build` toolset) or is reached through `a2m_action_run`. *Phase* is when it is built.

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
| `run.undo` | Build the reverse diff of a completed write run, for approval | Operator | No | 9 |
| `run.trace` | Summary of a run's trace, with links to artifacts | Viewer | No | 1 |
| `session.status` | Whether each target is signed in for the caller | Viewer | use | 2 |
| `session.login` | Ask the caller's person to sign in to a target | Viewer | use | 2 |
| `session.revoke` | End one of the caller's own sessions | Viewer | No | 2 |

## Building a target

| Action | Purpose | Who | Listed | Phase |
| --- | --- | --- | --- | --- |
| `target.add` | Register a target in the project | Builder | No | 5 |
| `target.probe` | Platform, framework, support tier, and surfaces at an address or in a running desktop app | Builder | build | 5 |
| `target.status` | The target's build state and the next action | Builder | build | 5 |
| `target.list` | Targets in the project and their versions | Builder | No | 5 |
| `auth.detect` | The sign-in method, with evidence, and a recommended identity mode | Builder | build | 2 |
| `screen.snapshot` | Compact accessibility snapshot of the current page or window | Builder | build | 1 |
| `capture.start` | Begin recording actions and traffic, redacted | Builder | build | 4 |
| `capture.stop` | End the recording and store it | Builder | build | 4 |
| `endpoints.infer` | Endpoint templates, schemas, and dependencies from recordings | Builder | build | 4 |
| `spec.import` | Operations from an OpenAPI, GraphQL, or OData description, or an upstream MCP server | Builder | build | 4 |
| `draft.read` | Read a draft file; stands in for a file system | Builder | build | 5 |
| `draft.write` | Write a draft file after validation and lint | Builder | build | 5 |
| `draft.lint` | Compile drafts and report every problem | Builder | build | 1 |
| `draft.try` | Run a draft binding in the sandbox | Builder | build | 5 |
| `test.run` | Run the publish gates and return evidence | Builder | build | 3 |
| `fixture.record` | Turn captured evidence into a scrubbed fixture | Builder | No | 3 |
| `shadow.report` | How often a shadow binding matched, and where it differed | Builder | build | 4 |
| `plan.show` | Print a compiled plan as text | Builder | No | 1 |
| `version.diff` | Compare two versions and classify each change | Builder | No | 5 |
| `publish.request` | Submit a version for human review | Builder | build | 5 |

## Providers and discovery

Added with spec sections 56, 58, 64, and 67. Built with the stage of the [long-term plan](../plan/anything2mcp-master-plan.md) named.

| Action | Purpose | Who | Listed | Phase |
| --- | --- | --- | --- | --- |
| `provider.list` | Every pack and provider: installed, enabled, ready, or what it lacks | Viewer | build | Stage B |
| `provider.describe` | One provider's manifest, effects, and requirements | Viewer | No | Stage B |
| `provider.enable`, `provider.disable` | Switch a provider on or off; a protected setting on one machine where it starts processes or reaches a network | Admin | No | Stage B |
| `component.list` | Outside programs providers need: found or not, with path and digest | Viewer | No | Stage B |
| `discover.run` | Run the discoverers that suit a target's surfaces; returns findings | Builder | build | Stage E |
| `finding.list`, `finding.describe` | Findings for a target, by level, kind, or source | Builder | build | Stage E |
| `unknown.list` | The open questions about a target | Builder | build | Stage E |
| `unknown.settle` | Settle an open question with evidence | Builder | No | Stage E |
| `scout.run` | Identify a target, run the discoverers that suit it, draft what follows by rule, and try the drafted reads; resumable | Builder | build | Stage E |
| `scout.report` | The last scout's report: found, drafted, proven, open, and what to look at first | Builder | build | Stage E |

## Publishing and versions

| Action | Purpose | Who | Listed | Phase |
| --- | --- | --- | --- | --- |
| `version.publish` | Publish a reviewed version; a person, never an agent | Publisher | No | 5 |
| `binding.promote` | Promote a shadow binding to verified; a person | Publisher | No | 4 |
| `version.activate` | Move a toolset to a version, through the candidate stage | Publisher | No | 6 |
| `version.rollback` | Make an earlier version current | Publisher | No | 5 |
| `version.bundle` | Pack a version into a signed file | Publisher | No | 6 |
| `version.import` | Import a signed bundle | Publisher | No | 6 |
| `version.migrate` | Recompile stored sources for a newer engine | Publisher | No | 9 |

## Administration

| Action | Purpose | Who | Listed | Phase |
| --- | --- | --- | --- | --- |
| `toolset.create`, `toolset.update`, `toolset.list` | Manage toolsets | Admin | No | 6 |
| `grant.add`, `grant.remove`, `grant.list` | Manage grants | Admin | No | 6 |
| `policy.publish` | Publish a policy set after its tests pass | Admin | No | 6 |
| `policy.test` | Run a policy set's decision table | Admin | No | 6 |
| `policy.explain` | Why a past call was allowed or denied | Auditor | No | 6 |
| `session.list` | Sessions in a workspace, without contents | Admin | No | 6 |
| `session.end` | End someone's session; never use it | Admin | No | 6 |
| `runner.enroll`, `runner.list`, `runner.drain`, `runner.revoke` | Manage runners | Admin | No | 7 |
| `killswitch.set` | Stop a target, a workspace, or the whole system | Admin | No | 3 |
| `audit.query` | Read the audit log | Auditor | No | 6 |
| `secret.set` | Store a secret from a prompt or standard input | Admin | No | 2 |
| `vault.rotate` | Rotate a key layer | Admin | No | 9 |
| `delegation.grant`, `delegation.revoke`, `delegation.list` | A person lets an agent act for them, and withdraws it | The person | No | 7 |

## The machine itself

| Action | Purpose | Who | Listed | Phase |
| --- | --- | --- | --- | --- |
| `engine.status`, `engine.start`, `engine.stop`, `engine.logs` | Control the local engine | Local user | No | 1 |
| `config.show` | Effective settings and where each came from | Local user | No | 1 |
| `config.set` | Change a setting; protected settings need the user-presence prompt | Local user | No | 3 |
| `doctor.run` | Check browser, key store, network, sessions, clock; print fixes | Local user | No | 2 |
| `browser.install` | Fetch a pinned browser | Local user | No | 1 |
| `plugin.install`, `plugin.list` | Manage plug-ins | Admin | No | 8 |
| `client.connect` | Write the configuration for an MCP client | Local user | No | 5 |
| `trace.show`, `trace.export` | View or export a run's evidence | Auditor | No | 1 |
| `action.search`, `action.describe`, `action.run` | Reach any action without listing it | Any | use and build | 5 |
| `docs.get` | Built-in reference pages | Any | No | 5 |
| `self.update` | Update a standalone install | Local user | No | 9 |

## What is deliberately absent

- No action approves a write or a publish on a local machine ([spec section 39](../spec/39-local-engine-and-clients.md)).
- No action reads a secret back.
- No action runs as another person, and `session.end` cannot be used to enter a session.

## Shortcuts

Single-word commands for people. Each is a fixed sequence of the actions above and adds no behavior.

| Shortcut | Does |
| --- | --- |
| `init` | Create a project and the guidance file; choose a profile |
| `add <address>` | `target.add`, `target.probe`, `auth.detect`, `session.login` |
| `learn` | `capture.start`, `capture.stop`, `endpoints.infer`, then drafts |
| `lint`, `try`, `test` | `draft.lint`, `draft.try`, `test.run` |
| `publish` | `publish.request`, then waits for a person |
| `status` | `target.status` |
| `call`, `run` | `tool.call` for an operation or a pipeline |
| `diff`, `bundle`, `rollback`, `migrate` | The matching `version` actions |
| `connect <client>` | `client.connect` |
| `doctor` | `doctor.run` |
| `docs <topic>` | `docs.get` |

## Process commands

These start a long-lived role and are not actions.

| Command | Starts |
| --- | --- |
| `serve` | A server in the configured profile |
| `runner` | A runner that dials a control plane |
| `bridge` | The link an SDK or a stdio MCP client uses to reach the local engine |
