# 221. The tool surface agents see

Every tool is generated from published files, so names, descriptions, and results are consistent across targets, and an agent needs no prior knowledge of the system. This section designs that surface. Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

**No client is special.** Any client that speaks the protocol is served alike, whoever made it, a home-built runtime included. Nothing in the kernel knows which make of agent is calling, and conformance is tested against the protocol with at least two unrelated clients. A client's quirk, where one must be handled, is data in a profile ([decision 0058](../../decisions/0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md)). The guidance on list sizes below is cited from client makers' published advice and applies to every client.

**Two built-in toolsets.** `use` holds the control tools for everyday agents; `build` adds the authoring tools. A caller sees only what its role allows. Every built-in tool is an action in the [action catalog](../../reference/action-catalog.md).

**Toolsets shape the list.** On a shared server each toolset has its own endpoint and its own list, so an agent connected to `/mcp/orders` sees only that toolset's tools. Per-user rights are checked at call time.

**What is listed up front.** Agents choose tools less accurately past roughly 30 to 50 ([Anthropic](https://platform.claude.com/docs/en/agents-and-tools/tool-use/tool-search-tool)), and OpenAI's soft target is under 20 at the start of a turn ([OpenAI](https://developers.openai.com/api/docs/guides/function-calling)). So:

- **Listed:** pipelines and the control tools, kept under 20 in total, configurable per caller.
- **Found on demand:** single operations, through `a2m_tool_search` and `a2m_tool_call`; each operation's own schema is still enforced.
- **Full listing:** a setting, or a client profile, for clients with built-in tool search.

| Control tool | Purpose |
| --- | --- |
| `a2m_tool_search` | Search operations and pipelines by plain words or module; returns names, descriptions, and schemas |
| `a2m_tool_call` | Run one operation or pipeline by name |
| `a2m_run_get` | Status, progress, result, and trace link of a run |
| `a2m_run_resume` | Continue a stopped run from its checkpoint |
| `a2m_run_cancel` | Stop a running or waiting run |
| `a2m_session_status` | Whether each target is signed in, without exposing anything |
| `a2m_session_login` | Ask the person to sign in to a target |
| `a2m_net_standing` | What kind of place each of this machine's addresses is, and which interface carries each address family's preferred default route, read from the operating system; it sends nothing ([decision 0260](../../decisions/0260-the-machines-network-facts-are-served-as-reads-of-the-local-user-one-listed.md)) |

The `use` toolset lists these eight, `a2m_tool_describe`, and the three `a2m_action_*` tools: twelve, which is its cap. Every other action is reached through `a2m_action_run`, and the list's limit `tools.listed_limit` (19 by default) leaves the rest of its places to a target's pipelines ([decision 0280](../../decisions/0280-the-use-toolset-lists-at-most-twelve-tools.md)).

**Names.** `{target}_{module}_{object}_{verb}` in lowercase with underscores, at most 64 characters (`OperationName` in `a2m-core`); pipelines are named for their outcome. The build fails on duplicates or near-duplicates, and on two names that differ only by a suffix such as `_v2`. A name never carries a version: it stays the tool's name in every version of its target, and a breaking change is shown to the person who publishes it ([decision 0999](../../decisions/0999-a-tools-name-carries-no-version-and-a-breaking-change-is-named-at-publish.md)).

**Descriptions** are the biggest factor in correct use ([Anthropic](https://platform.claude.com/docs/en/agents-and-tools/tool-use/define-tools)) and are generated from a required template: purpose in the user's words; when to use it and what to prefer instead; preconditions; side effects; what it returns; limits. A tool's whole definition is bounded text: the description and every free-text field of its schemas, at most 1,024 characters each and 8,192 in all, with no character that can reorder, hide, or restyle text ([decision 0325](../../decisions/0325-what-a-browser-can-reach-and-what-a-tools-definition-may-hold.md)).

**Schemas.** Flat, about 10 parameters or fewer, enums from declared options, defaults, no extra properties, and one to three examples from recorded tests. Example inputs raised accuracy on complex parameters from 72% to 90% in Anthropic's testing ([Anthropic](https://www.anthropic.com/engineering/advanced-tool-use)). Values the system already knows, such as the session or mode, are never asked for.

**Annotations.** `readOnlyHint`, `destructiveHint`, and `idempotentHint` are set from the effect class and key.

**A repeated call.** A call that may write can carry an idempotency key of the caller's own, in its `_meta` as `anything2mcp/idempotency_key` or as the `idempotency_key` argument of `a2m_tool_call`. Made again with the same key, it gives back the run the first call started and starts no other; this is what an agent does after a lost answer. Without a key, it lists its runs since the call was sent with `run.list` first ([section 222](222-front-doors-and-the-local-engine.md)).

**Results.** Structured content with a published output schema, plus a one-line summary. Concise by default. A result larger than the setting `tools.result_cap_bytes` (262,144 bytes by default) is not returned: in its place the caller gets a result that says it was truncated, which limit cut it, and its size, and a warning says to ask for less. Lists are paged by `limit` and `offset` (`tool.search`) or by `after` (`run.list`); there is no cursor, and the MCP tool list is one page; a target's own result is neither paged nor cut to a part: it is returned whole or not at all. A cursor over a target's results, and a cut that keeps part of a result, are not built yet. Every non-success names the next call:

```json
{
  "run_id": "r_8f21",
  "tool": "example_order_sheet_enter",
  "status": "needs_approval",
  "summary": "12 line items: 9 to create, 3 to update. Waiting for approval.",
  "mode": "ui",
  "diff": {"create": 9, "update": 3, "unchanged": 0, "detail": "resource://runs/r_8f21/diff"},
  "next": {"tool": "a2m_run_get", "args": {"run_id": "r_8f21"}, "when": "after the person approves"}
}
```

Over MCP the `next` hint of a success is carried inside the structured content, as above, with the action's name beside the tool's: every output schema admits `next`. Warnings go in the result's `_meta`, under `anything2mcp/warnings`. Each published tool in a list carries its target and version in its own `_meta`, under `anything2mcp/target` and `anything2mcp/version`, and who wrote its description under `anything2mcp/description_by`. The engine adds nothing to the description, with one exception: a tool a drift stopped is listed with one fixed sentence of the product's own before its description, and `anything2mcp/unavailable` in its `_meta` ([decision 0396](../../decisions/0396-a-tool-stopped-by-a-drift-is-listed-and-marked-never-left-out.md)). Two more marks are designed and not built (package YL2): under `anything2mcp/digest`, the SHA-256 of the tool's name, description, input schema, output schema, and annotations in canonical JSON, so that a client can pin a tool and notice when it changes ([decision 0325](../../decisions/0325-what-a-browser-can-reach-and-what-a-tools-definition-may-hold.md), [decision 0999](../../decisions/0999-a-tools-name-carries-no-version-and-a-breaking-change-is-named-at-publish.md)); and in the output schema, `x-anything2mcp-untrusted: true` on each field that holds the system's words, so that a client can treat those fields as data ([section 502](../5-security/502-defending-against-a-manipulated-agent.md)). The compiled target already marks each such field. These keys are a contract with clients: a key is added in any release, and removed or renamed only as decision 0997 allows a break. A failure carries its next step as `fix` ([decision 0132](../../decisions/0132-the-action-registrys-second-pass-a-decline-ends-the-run-and-a-wait-reads-the-schedulers-own-table.md), [decision 0151](../../decisions/0151-the-doors-take-up-the-action-registrys-second-pass.md)).

`status` is the run's state from [section 208](208-runs-the-ledger-and-resume.md): `queued`, `validating`, `rejected`, `needs_approval`, `expired`, `running`, `needs_input`, `needs_login`, `stopped`, `completed`, `failed`, or `cancelled`. A call that could not be queued returns `busy` with a retry time and no run. Long jobs return a `run_id` at once; clients that support MCP progress notifications or long-running tasks also receive progress: a rising count and a sentence of the server's own for each event of the run that matters, never text a system gave. Traces, screenshots, files, and diffs are MCP resources linked from results, never inlined.
