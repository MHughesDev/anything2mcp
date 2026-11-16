# 21. Agent-facing tool surface

Every tool is generated from published files, so names, descriptions, and results are consistent across targets, and an agent needs no prior knowledge of the app.

**Two built-in toolsets.** `use` holds the control tools for everyday agents; `build` adds the authoring tools. A caller sees only what its role allows. Every built-in tool is an action in the [action catalog](../reference/action-catalog.md).

**Toolsets shape the list.** On a shared server each toolset has its own endpoint and its own list, so an agent connected to `/mcp/orders` sees only that toolset's tools. Per-user rights are checked at call time.

**What is listed up front.** Agents choose tools less accurately past roughly 30 to 50 ([Anthropic](https://platform.claude.com/docs/en/agents-and-tools/tool-use/tool-search-tool)), and OpenAI's soft target is under 20 at the start of a turn ([OpenAI](https://developers.openai.com/api/docs/guides/function-calling)). So:

- **Listed:** pipelines and the control tools, kept under 20 in total, configurable per caller.
- **Found on demand:** single operations, through `w2m_tool_search` and `w2m_tool_call`; each operation's own schema is still enforced.
- **Full listing:** a setting for clients with built-in tool search.

| Control tool | Purpose |
| --- | --- |
| `w2m_tool_search` | Search operations and pipelines by plain words or module; returns names, descriptions, and schemas |
| `w2m_tool_call` | Run one operation or pipeline by name |
| `w2m_run_get` | Status, progress, result, and trace link of a run |
| `w2m_run_resume` | Continue a stopped run from its checkpoint |
| `w2m_run_cancel` | Stop a running or waiting run |
| `w2m_session_status` | Whether each target is signed in, without exposing anything |
| `w2m_session_login` | Ask the person to sign in to a target |

**Names.** `{target}_{module}_{object}_{verb}` in lowercase with underscores, at most about 60 characters; pipelines are named for their outcome. The build fails on duplicates or near-duplicates.

**Descriptions** are the biggest factor in correct use ([Anthropic](https://platform.claude.com/docs/en/agents-and-tools/tool-use/define-tools)) and are generated from a required template: purpose in the user's words; when to use it and what to prefer instead; preconditions; side effects; what it returns; limits.

**Schemas.** Flat, about 10 parameters or fewer, enums from declared options, defaults, no extra properties, and one to three examples from recorded tests. Example inputs raised accuracy on complex parameters from 72% to 90% in Anthropic's testing ([Anthropic](https://www.anthropic.com/engineering/advanced-tool-use)). Values the system already knows, such as the session or mode, are never asked for.

**Annotations.** `readOnlyHint`, `destructiveHint`, and `idempotentHint` are set from the effect class and key.

**Results.** Structured content with a published output schema, plus a one-line summary. Concise by default, paginated by cursor, size-capped with a note when truncated. Every non-success names the next call:

```json
{
  "run_id": "r_8f21",
  "tool": "example_order_sheet_enter@4",
  "status": "needs_approval",
  "summary": "12 line items: 9 to create, 3 to update. Waiting for approval.",
  "mode": "ui",
  "diff": {"create": 9, "update": 3, "unchanged": 0, "detail": "resource://runs/r_8f21/diff"},
  "next": {"tool": "w2m_run_get", "args": {"run_id": "r_8f21"}, "when": "after the person approves"}
}
```

`status` is the run's state from [section 42](42-run-ledger-design.md): `queued`, `validating`, `rejected`, `needs_approval`, `expired`, `running`, `needs_input`, `needs_login`, `stopped`, `completed`, `failed`, or `cancelled`. A call that could not be queued returns `busy` with a retry time and no run. Long jobs return a `run_id` at once; clients that support MCP progress notifications or long-running tasks also receive progress. Traces, screenshots, files, and diffs are MCP resources linked from results, never inlined.
