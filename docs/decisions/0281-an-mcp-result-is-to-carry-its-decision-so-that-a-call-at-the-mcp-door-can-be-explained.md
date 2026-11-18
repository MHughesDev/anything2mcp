# 0281. An MCP result is to carry its decision, so that a call at the MCP door can be explained

**Status:** Accepted. A design decision by an agent, made on 2026-11-18; any agent may supersede it with evidence. The owner has not read this record. What it decides is not built yet.

**Date:** 2026-11-18

**Changes, when built:** the MCP face of a response (`McpResultFace` in `crates/a2m-actions/src/faces.rs`) and its rendering in `crates/a2m-mcp/src/tools.rs`. Found while building [decision 0260](0260-the-machines-network-facts-are-served-as-reads-of-the-local-user-one-listed.md), whose product test could not explain a decision made at the MCP door.

## Context

Every call is decided once by the policy engine, and the decision is recorded whether it allows or refuses. The dispatcher's `Response` holds the recorded decision's identifier (`Response::decision`, in `crates/a2m-actions/src/dispatch.rs`), and `policy.explain` takes that identifier and says why the call was allowed or refused: the decision, the policy version, and the rules that made it.

Two faces hand the identifier to the caller. The command line's JSON output and the REST face's answer are both `Response::to_json`, which writes `decision` beside `result` or `error`; the REST answer's schema lists it (`envelope` in `crates/a2m-api/src/openapi.rs`). The MCP face does not. `McpResultFace` is made from the response's `error`, `result`, and `next` alone, and `render` in `a2m-mcp` puts only the warnings of a success into the result's `_meta`. So an agent at the MCP door, and anyone reading what that agent was answered, cannot ask why one of its calls was allowed or refused. The decision exists, recorded in the store; only its name is not given.

## Decision

1. **A tool result carries the identifier of its call's decision in its `_meta`, under `anything2mcp/decision`,** whenever the response holds one: for a success and for a failure alike, a refusal included. It is the same text the command line and the REST face give, and `policy.explain` takes it unchanged. A response with no decision (a stopped engine answers its own status with none recorded) carries no key.
2. **It goes in `_meta`, not in the structured content.** The structured content of a success must fit the tool's output schema, and a failure has none. The key is in the product's own namespace, beside `anything2mcp/warnings` and `anything2mcp/idempotency_key`.
3. **Nothing else changes.** Who may call `policy.explain` is the action's own rule, decided as for any action; the identifier says nothing a caller did not already do.

**Not yet.** It is built with item 11 of the command-line package, which gives an MCP failure the same data as every other face (its `next`, and its error as structured data in `_meta`), in the same two files. A product test then calls a tool at the MCP door through the real `bridge` and `serve` and explains its decision with `policy.explain`, for an allowed call and for a refused one.

## Consequences

- Until it is built, a decision made at the MCP door is explained only from the store, by an auditor who finds it there, and the product's tests prove that such calls are decided at the kernel and through the command line, as decision 0260 says.
- Every tool result that has a decision grows by one key and one identifier, under a hundred bytes.

## Alternatives weighed

- **Put the identifier in the result itself, as a field every output schema admits,** as `next` is. It would change every output schema and the generated clients, and a failure has no structured content to hold it. Not taken.
- **Put it in the text of a failure only.** A success could not then be explained, and a success is what an auditor most often asks about.
- **Leave it out at the MCP door, and let an agent explain only through the command line.** A caller at one door would then be given less than a caller at another, for no reason the design gives.

## Revisit if

The MCP specification gives a result's metadata a standard place for such an identifier, or a client is found that drops `_meta` from what it keeps.
