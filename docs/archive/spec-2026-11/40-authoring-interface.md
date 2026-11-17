# 40. The authoring interface

> Archived on 2026-11-17. This is section 40 of the first design, kept as it was written, with the names and numbers of its time. What it says now lives in [section 220, Actions and the authoring interface](../../spec/2-kernel/220-actions-and-the-authoring-interface.md).

This section defines what a coding agent actually holds in its hands when it wraps an application: commands and files first, MCP tools second, both generated from one list of actions. It extends [section 26](26-agent-toolkit-how-tools-get-built.md), which describes what the actions do.

## Which interface, for whom

| Interface | Role | Why |
| --- | --- | --- |
| Commands plus target files | **Primary for building.** Coding agents with a shell, and people | Agents are strongest at running commands and editing files; nothing enters the agent's context until it is used; works with every provider |
| MCP authoring tools | **Second door for building.** Agents with no shell, such as desktop chat clients and hosted agents | The only route for those agents; structured results; but every listed tool costs context, so the list stays short |
| Python and TypeScript SDKs | For software that calls finished tools, and for tests | Building through code would replace the declarative files that keep tools reviewable |
| REST API | For other software and the admin console | An agent gains nothing from it over commands |
| Guidance file | Always | Teaches the loop; makes every other interface work better |

Published comparisons of commands and MCP for coding agents agree on the main point: tool definitions that load up front cost context on every turn, while a command costs nothing until it runs. Several also note that the real difference is loading eagerly versus loading on demand, not the protocol. Anything2MCP therefore makes both faces load on demand. See [research note 12](../../research/12-packaging-and-agent-interfaces.md).

## One action registry, every face generated

Every authoring and administration action is defined once, in Rust, in the `a2m-actions` crate. The [action catalog](../../reference/action-catalog.md) is the full list. A definition holds the action's name, input and output schemas, effect class, required role, whether it is long-running, a one-line summary, and one example.

From each definition the build generates:

| Face | Form | Example for the action `target.probe` |
| --- | --- | --- |
| Command | `anything2mcp <noun> <verb>` | `anything2mcp target probe https://app.example.com` |
| MCP tool | `a2m_<noun>_<verb>` | `a2m_target_probe` |
| REST route | `POST /api/v1/actions/<noun>.<verb>` | `POST /api/v1/actions/target.probe` |
| SDK method | `client.<noun>.<verb>()` | `client.target.probe(url=...)` |
| Reference page | `anything2mcp docs <name>` | `anything2mcp docs target.probe` |

- **No hand-written face.** A check in CI fails if a command, tool, route, or method exists that the registry did not generate, or if one is missing.
- **Same behavior everywhere.** All faces call the same function in the engine, so a result is identical whichever way it was asked for.
- **Short commands for people.** `init`, `add`, `learn`, `test`, `publish`, `connect`, and `serve` remain as shortcuts. Each is a fixed sequence of registry actions and adds no behavior of its own.

## The command face, as an agent uses it

**Output.**

- With `--json`, or whenever output is not going to a terminal, a command prints exactly one JSON object to standard output. Text meant for people goes to standard error.
- Success: `{"ok": true, "action": ..., "result": ..., "warnings": [...], "next": {...}}`.
- Failure: `{"ok": false, "error": {"code": ..., "class": ..., "message": ..., "fix": ...}}`, using the failure classes and codes in the technical reference.
- `next` names the most useful following action and its arguments, so an agent that has read nothing else can still proceed.

**Exit codes.**

| Code | Meaning |
| --- | --- |
| 0 | Succeeded |
| 1 | The action failed; the error object says why |
| 2 | The command was malformed |
| 3 | A person is needed: sign-in, approval, or input |
| 4 | The engine could not be reached or started |

**Large results stay out of the context.** A snapshot, capture, trace, or diff is written to a file under `.anything2mcp/out/` in the project. The command returns the path, the size, and a short summary. The agent reads the parts it needs with its own file tools. Defaults are compact: a screen snapshot is limited to roughly 2,000 tokens, with a flag to widen it and a cursor to page through it.

**No prompts.** When output is not a terminal, a command never waits for typed input. If a person is needed it returns exit code 3 and says exactly what the person must do.

**Discovery on demand.** `anything2mcp action search` returns names and one-line summaries only. `anything2mcp docs <topic>` prints reference pages built into the binary, so file formats, verbs, and failure codes are available offline and only when asked for.

**Stable output.** Fields keep a fixed order, lists are sorted, and element references stay the same within one snapshot, so two identical commands give identical text.

## The MCP face, for agents without a shell

The build toolset lists at most 15 tools:

- The authoring tools named in section 26, which cover the build loop.
- `a2m_action_search`, `a2m_action_describe`, and `a2m_action_run`, which reach every other action in the registry without listing it.
- `a2m_draft_read` and `a2m_draft_write`, which stand in for a file system. A draft is validated and linted before it is written.

Large results come back as resource links, the MCP counterpart of the files above.

An agent with no shell also has no guidance file and no `docs` command, so the same material is offered through the protocol where the client supports it:

- **Server instructions** carry the handful of rules every caller needs.
- **Prompts** carry the build loop as a guided start: add a target, explore it, draft a read tool.
- **Resources** carry the built-in reference pages and the project's draft files.

All three are generated from the same sources as the guidance file and the `docs` command.

## The project folder

Target files live in a project folder, which is any directory holding a `anything2mcp.toml`. It should be its own private Git repository.

```text
my-targets/
  anything2mcp.toml         # project settings: which targets, which environments
  AGENTS.md            # written by init: the build loop, the rules, a command list
  targets/
    example/           # one folder per target, laid out as in section 7
  .anything2mcp/            # local working data, ignored by Git
    out/               # snapshots, captures, traces, diffs
  .gitignore
```

- **Finding the project.** A command looks for `anything2mcp.toml` in the current directory and then in each parent; `--project` overrides.
- **What is not in the project.** The store, the vault, and browser profiles live in the user's application-data directory, never in the project, so cloning or sharing a project never shares a session.
- **Drafts and versions.** Files in the project are drafts. `publish` copies them into the store as an immutable, hashed version. Normal callers run only published versions; `try` and `test` run drafts, inside the sandbox.
- **Real targets stay private.** This is where target files for real applications belong; they are never added to the Anything2MCP repository.

## The build loop as a visible state

Each target has a build state, and `anything2mcp status` reports it with the next action:

`added` → `signed_in` → `explored` → `drafted` → `tried` → `tested` → `published`

An agent that loses its place, or a new agent picking up someone else's work, runs `status` and continues. Two steps always need a person: signing in and approving a publish.

## The guidance file

`init` writes a short `AGENTS.md` into the project, plus the small pointer files that particular agents look for. It is kept under 150 lines and contains:

- The build loop and the `status` command.
- The rules: read-only exploration, test records only, no secrets in files, stop on anything unknown, never attempt to approve.
- The command list, one line each.
- How to fetch detail on demand with `anything2mcp docs`.

Everything longer stays in the built-in reference, so the guidance costs little context and cannot drift from the installed version.

## What an agent cannot do through any face

- Approve a write or a publish.
- Read a cookie, token, or password.
- Make exploration send anything but reads.
- Write to a record that is not a declared test record before a version is published.
- Change a published version.

[Section 39](39-local-engine-and-clients.md) covers how far these hold when the agent has a shell on the same machine.

## Speed and ease targets

- A command reaching a running engine adds no more than 30 ms.
- An agent with no prior knowledge reaches a first published read tool using only the guidance file, `status`, and `docs`. This is the usability gate for Phase 5.
- Both are targets until measured.
