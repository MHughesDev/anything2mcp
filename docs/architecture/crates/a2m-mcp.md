# `a2m-mcp`

The MCP face of the kernel. It turns the action registry and a toolset's published tools into MCP tool lists, turns a tool call into a dispatch, and turns the dispatcher's response into a structured result. It serves Streamable HTTP, which the local channel and a server both carry, and it holds the stdio proxy that an MCP client on one machine starts. It knows no make of client.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 8, Roles |
| May depend on | Layers 1 to 7 |
| Depends on | `a2m-core`, `a2m-actions`, `a2m-telemetry`. Outside: `rmcp`, the official Rust SDK named in spec section 29, its dual-revision support to be confirmed by roadmap package 0.5 and research brief 2; Tokio; Hyper |
| Built in | Roadmap package 1.5 (stdio proxy, the `use` toolset, structured results); 5.1 (the `build` toolset, prompts, resources); 6.2 (Streamable HTTP on a server, as an OAuth resource server) |
| Carries out | Spec sections [21](../../spec/21-agent-facing-tool-surface.md), [22](../../spec/22-front-doors.md) for MCP, [40](../../spec/40-authoring-interface.md) for the MCP face, [64](../../spec/64-rules-for-tools-and-capability-discovery.md); the MCP half of [49](../../spec/49-approval-and-sign-in-surfaces.md) |

## Owns

- The MCP endpoint, one per toolset, as an HTTP service that a listener above it serves.
- The revisions served, and the difference between them, kept inside this crate.
- Tool lists: which actions and which target tools each toolset lists, and the cap on their number and size.
- The mapping from an MCP tool call to a `Request`, and from a `Response` to an MCP result: structured content, a one-line summary, resource links, annotations.
- Elicitation for sign-in and approval links, and the link in the result where a client has no elicitation.
- Progress and long jobs: progress notifications to a caller that asked, and the Tasks extension mapped onto runs.
- Server instructions, prompts, and resources generated from the same sources as the guidance file and `docs.get`.
- The stdio proxy: a thin client that copies MCP messages between standard input and output and the engine's MCP endpoint.
- Client profiles, as data: a quirk of one client, where one must be handled.

It must never: hold run state between requests; decide whether a call is allowed; validate a call against anything but the full schema the dispatcher holds; let text from a target into a tool's name or description; branch on which client is calling except through a profile.

## Interface

```rust
/// One endpoint per toolset. A pure service: the product crate mounts it on the local
/// channel and, on a server, on the network listener.
pub struct McpService { /* Kernel, toolset, revisions, limits, profiles */ }
impl McpService {
    pub fn new(kernel: Kernel, toolset: ToolsetName, cfg: McpSettings) -> Result<Self, Failure>;
    pub fn http(self) -> BoxedHttpService;            // Streamable HTTP, stateless
}

pub struct McpSettings {
    pub revisions: Vec<Revision>,      // server.mcp_revisions; default 2026-07-28 and 2025-11-25
    pub listing: Listing,              // Default (pipelines and control tools), Full
    pub listed_max: u32,               // tools.listed_max; a fixed ceiling above it
    pub result_cap: ByteSize,          // tools.result_size_cap
    pub client_profiles: Vec<ClientProfile>,
}

/// The stdio proxy. It reads and writes MCP messages and nothing else; it has no tools of its own.
pub async fn stdio_proxy(link: Link, toolset: ToolsetName) -> Result<(), Failure>;

/// Data, loaded from a profile file. Matched on what the client says of itself; changes
/// presentation only, never what is allowed or which tools exist.
pub struct ClientProfile { pub matches: ClientMatch, pub adjust: Vec<Adjustment> }
```

The proxy is started by `bridge`, a process command of the product crate, which connects through `a2m-local` and hands the `Link` in. The two role crates do not depend on each other.

## Revisions

| Revision | How it is served |
| --- | --- |
| 2026-07-28 | Stateless. No handshake, no session header. Every request carries everything; continuity is the `run_id` |
| 2025-11-25 | The initialize exchange is answered from settings. No session identifier is issued, which that revision allows, so the server stays stateless |

Whether one `rmcp` server can do both is the question of package 0.5. If it cannot, the older revision is dropped before statefulness is added ([decision 0011](../../decisions/0011-stateless-streamable-http-and-an-oauth-resource-server.md) says how to choose).

## Tool lists

A toolset's list is the same for every caller connected to it; rights are checked at call time and a refused call is a clear `policy` result ([spec section 20](../../spec/20-access-control-toolsets-and-tenancy.md)).

| Toolset | Lists | Reaches without listing |
| --- | --- | --- |
| `use` | Actions whose `listed` names `use`; the toolset's pipelines | Every operation, through `a2m_tool_search` and `a2m_tool_call` |
| `build` | The above, and actions whose `listed` names `build` | Every action, through `a2m_action_search`, `a2m_action_describe`, `a2m_action_run` |
| A named toolset on a server | Its members' pipelines and the `use` actions | Its operations, through search |

- **Names.** An action `noun.verb` is listed as `a2m_<noun>_<verb>`. A target's tool keeps its published name, `{target}_{module}_{object}_{verb}`; its version appears in results, never in the name.
- **Descriptions** are the published ones, or the action's summary. The first 240 characters are enough to choose.
- **Schemas.** The input schema in a list may be the compact form, provided every difference is exact or wider. Output schemas are left out of the list unless the client asks; `tool.describe` returns them.
- **Annotations.** `readOnlyHint` for `read`; `destructiveHint` for `irreversible` and for writes without a key; `idempotentHint` from the key.
- **Size.** The registry refuses to build when a built-in toolset lists more actions than its cap (spec sections 21 and 40). For target tools the cap is `tools.listed_max`; past it, pipelines beyond the cap move to search and the list says so in the server instructions. The list's size in bytes is measured and budgeted (spec section 64 leaves the budget open).
- **Full listing** is a setting for clients with their own tool search. It lists every operation, and changes nothing about how calls are checked.

## Calls and results

1. `tools/call` arrives with a name and arguments. A name of the form `a2m_<noun>_<verb>` is the action `noun.verb`. Any other name is `tool.call` with the name as its `tool` input; `a2m_tool_call` is the same action called by name.
2. The front door builds a `Request`: the caller (on the local channel, the operating-system user; on a server, from the validated token), the toolset, the client label, and an idempotency key if the client sent one in the request's metadata.
3. `Kernel::dispatch` does the rest. This crate only renders the `Response`:

| Part of the response | MCP result |
| --- | --- |
| `result` | `structuredContent`, matching the published output schema |
| A one-line summary | One text content item |
| Kept blobs: traces, diffs, snapshots, files | Resource links, never inlined; read through `resources/read` |
| `next` | A field in the structured content, as spec section 21 shows |
| `error` | `isError` with the class, code, message, fix, and whether a retry is safe |
| Text from the target | Inside fields marked untrusted, already cut and stripped by the dispatcher |

**Long jobs.** A long-running action returns its `run_id` at once. A client that supports the Tasks extension gets a task whose `tasks/get` is answered by `run.get` and whose cancel is `run.cancel`. Progress is sent only to a caller that asked, as a rising count from the run's events.

**Elicitation.** When a run rests at `needs_login` or `needs_approval`, the result carries the link from `PersonSurface`; that includes a plain link from `give_link`, which a sign-in provider such as authorization-code sign-in needs the person to open in their own browser. Where the client supports URL-mode elicitation, the same link is also sent as an elicitation. On one machine the link is the loopback page, which can only raise the operating system's prompt; on a server it is the console page behind the identity provider's sign-in. Following the link is never an approval by itself, so giving it to an agent is safe.

**Found tools.** Spec section 64 asks that a tool found by search be added to the client's list. The stateless revision has no per-connection list. Until a client can be told of a changed list without a session, the generic `a2m_tool_call` is the route, and every call is checked against the full schema either way.

## No client is special

| What differs between clients | Where it is handled |
| --- | --- |
| Capabilities: elicitation, tasks, progress, resource links | What the client declares; each has a fallback that every client gets |
| Which revision | The revision the request names |
| A quirk with no declared capability | A `ClientProfile` row in a data file, with a test, and a note of why |
| The client's own name | The client label, for the audit log and fair queuing only |

The kernel never sees a client's name. Conformance is tested against the protocol with at least two unrelated clients ([decision 0058](../../decisions/0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md)).

## On a server

The same `McpService` is mounted at `/mcp/<toolset>` on the network listener. The listener authenticates: it is an OAuth 2.1 resource server, publishes protected resource metadata, and rejects a token not issued for it. Token checks use the verifier the kernel holds, so `a2m-api` and this crate check tokens in one place. A caller's token is never forwarded to a target.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `anything2mcp` | `McpService` mounted on the local channel and the network listener; `stdio_proxy` for `bridge` |

It calls `a2m-actions` for the registry, `dispatch`, `events`, and the token verifier.

## State

None between requests. Client profiles and settings are read at start.

## Failures

Every failure is the dispatcher's, rendered. This crate adds one: `input` / `schema_invalid` for a malformed MCP message, before any dispatch.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Two unrelated clients list the tools and call a read | The Phase 1 exit test |
| A built-in toolset never lists more than its cap | The registry check; a test with one action too many |
| A call is validated against the full schema, whatever the list showed | A test with an input valid for the compact schema and not the full one |
| No secret appears in any MCP message | The planted-secret test, through this face |
| No client name reaches the kernel | A test that dispatches with two client labels and compares the decisions recorded |
| The proxy copies messages and adds nothing | A test that compares proxied and direct exchanges byte for byte, apart from transport framing |

## Build order

1. From spike 0.5: one `rmcp` service over the local channel, the stdio proxy, and the `use` toolset with `a2m_tool_call` for one read. Package 1.5.
2. Structured results, resource links, `next`, annotations; the two revisions.
3. Elicitation and the link fallback, with the approval surface. Package 3.6.
4. The `build` toolset, the action search tools, prompts, resources, server instructions. Package 5.1.
5. Streamable HTTP at `/mcp/<toolset>` on a server, with token checks. Package 6.2.

## Open points

- Whether `rmcp` can serve both revisions from one service: package 0.5.
- The byte budget for a tool list and for one compact schema.
- How compact schemas are derived from full ones mechanically, so that "exact or wider" is proven, not reviewed.
- How a client is told of a changed tool list with no session, if at all.
