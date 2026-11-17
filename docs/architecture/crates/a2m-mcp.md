# `a2m-mcp`

The MCP face of the kernel. It turns the action registry and a toolset's published tools into MCP tool lists, turns a tool call into a dispatch, and turns the dispatcher's response into a structured result. It serves Streamable HTTP, which the local channel and a server both carry, and it holds the stdio proxy that an MCP client on one machine starts. It knows no make of client.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 8, Roles |
| May depend on | Layers 1 to 7 |
| Depends on | `a2m-core`, `a2m-actions`, `a2m-telemetry`. Outside: `rmcp` 3.10.0, the official Rust SDK, with default features off and the features `server` and `transport-streamable-http-server` ([decision 0071](../../decisions/0071-the-mcp-server-stands-on-the-official-sdk-behind-a-seam-and-serves-two-revisions.md), [research note 18](../../research/18-mcp-server-base.md)); Tokio. No macro of the SDK is used, and no type of it appears in this crate's public interface |
| Built in | Roadmap package 1.5 (stdio proxy, the `use` toolset, structured results); 5.1 (the `build` toolset, prompts, resources); 6.2 (Streamable HTTP on a server, as an OAuth resource server) |
| Carries out | Spec sections [221](../../spec/2-kernel/221-the-tool-surface-agents-see.md), [222](../../spec/2-kernel/222-front-doors-and-the-local-engine.md) for MCP, [220](../../spec/2-kernel/220-actions-and-the-authoring-interface.md) for the MCP face, [220](../../spec/2-kernel/220-actions-and-the-authoring-interface.md); the MCP half of [503](../../spec/5-security/503-approval-and-sign-in-surfaces.md) |

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
    pub revisions: Vec<Revision>,      // mcp.revisions; default 2026-07-28 and 2025-11-25
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

One `rmcp` service serves both, with no session for either. Spike 0.5 ran it over stdio and over stateless Streamable HTTP, with the reference client libraries for Python and TypeScript ([decision 0071](../../decisions/0071-the-mcp-server-stands-on-the-official-sdk-behind-a-seam-and-serves-two-revisions.md)). Both are served because a server with only one of them turns clients away: the handshake is still the default way a reference client library opens.

**How a revision is agreed.**

1. A request whose `_meta` carries `io.modelcontextprotocol/protocolVersion` is served under that revision. On HTTP the `MCP-Protocol-Version` header must say the same. An unserved revision is error -32022 with `data.supported` and `data.requested`, and status 400 on HTTP.
2. `server/discover` lists the revisions served and the capabilities.
3. `initialize` selects the handshake revision: 2025-11-25 if the client asks for it, and 2025-11-25 as the answer if it asks for anything else. The client then decides.
4. A handshake-era request on HTTP names its revision in the header and is served as it comes.
5. With `mcp.revisions` set to the handshake revision alone, `server/discover` must be answered as an unknown method, so that a client that speaks both falls back. The SDK answers -32022 there; the two client libraries tolerated it, and the service must answer before the SDK does. With 2026-07-28 alone, `initialize` is refused with -32022 and the list.

**What a handshake-era caller does not get.** A stateless server does not remember what a client declared in `initialize`. Such a caller gets the fallback every client gets: the link in the result in place of elicitation, the run identifier in place of a task.

**The seam.** The service is built on the SDK's handler and its Streamable HTTP service, with the session manager that never makes a session and JSON answers where no stream is needed. The tool list is built from data with the SDK's plain constructors. Everything this crate promises is in the list of messages below, which is tested with raw messages and so does not depend on the SDK. If the SDK must be left, a layer of our own takes its place behind the same list; the spike's hand-written server passed all of the stdio part in about 400 lines.

## What the server must pass, message by message

Each line is a test with raw messages. "H" is the handshake revision, "M" the newer one. The spike's caller, `spikes/mcp-server/callers/raw_conformance.py`, holds a first version of most of them.

| Message | The server must |
| --- | --- |
| A body or line that is not JSON | Answer -32700 with a null id. On stdio this is the proxy's to do |
| A message that is not a request, or has no method | Answer -32600. A batch is not a request |
| An unknown method | Answer -32601 |
| An unknown notification | Answer nothing; on HTTP, status 202 |
| A request id | Come back unchanged, text as text and number as number |
| `initialize` (H) | Answer with `protocolVersion` 2025-11-25 whatever was asked, `capabilities` that name only what is served, and `serverInfo` with a name and a version. Issue no session identifier |
| `initialize` when only M is served | Refuse with -32022 and the revisions served |
| `notifications/initialized` (H) | Answer nothing |
| `ping` (H) | Answer an empty result, before `initialize` too |
| `ping` (M) | Answer -32601 |
| `server/discover` (M) | Answer `supportedVersions`, `capabilities`, `instructions`, `resultType`, `ttlMs`, `cacheScope` |
| `server/discover` when only H is served | Answer -32601 |
| Any M request with an unserved version | Answer -32022 with `data.supported` and `data.requested`; status 400 on HTTP |
| Any M request without `io.modelcontextprotocol/clientCapabilities` | Answer -32602 |
| Any M request on HTTP whose `MCP-Protocol-Version`, `Mcp-Method`, or `Mcp-Name` header disagrees with the body | Answer status 400 with -32020 |
| Every M result | Carry `resultType`, and name the server in `_meta` under `io.modelcontextprotocol/serverInfo` |
| `tools/list` (H and M) | List each tool with its name, description, and input schema of type object; annotations from the effect class; the same tools in the same order every time; no `nextCursor` on a single page; -32602 for a cursor it did not issue |
| `tools/list` (M) | Also carry `ttlMs` and `cacheScope` |
| `tools/call` for a known tool (H and M) | Answer `structuredContent` that fits the tool's output schema, and one text item |
| `tools/call` for an unknown tool | Answer -32602, a protocol error |
| `tools/call` with input that fails the schema | Answer a result with `isError` true, not a protocol error |
| `tools/call` that fails | Answer a result with `isError` true and text; structured content only if it fits the output schema |
| `tools/call` with a `progressToken` | Send `notifications/progress` with that token and a rising count before the result, and say nothing in one that the result does not say |
| `tools/call` without a `progressToken` | Send no progress |
| `notifications/cancelled` on stdio, or the stream closed on HTTP | Stop, and send nothing more for that request. Other requests are still answered |
| `resources/read`, `prompts/list`, `prompts/get`, tasks, elicitation | Not covered by the spike. Their lines are added by the packages that build them |
| A request over HTTP with a foreign `Host` | Be refused before it is read |
| End of input on stdio | The proxy exits with status 0 |
| Standard output on stdio | Carry one JSON message per line and nothing else |
| Two unrelated client libraries, each in its default mode | Connect, list the tools, call a read, and read the structured result |

## Tool lists

A toolset's list is the same for every caller connected to it; rights are checked at call time and a refused call is a clear `policy` result ([spec section 214](../../spec/2-kernel/214-access-control-toolsets-and-tenancy.md)).

| Toolset | Lists | Reaches without listing |
| --- | --- | --- |
| `use` | Actions whose `listed` names `use`; the toolset's pipelines | Every operation, through `a2m_tool_search` and `a2m_tool_call` |
| `build` | The above, and actions whose `listed` names `build` | Every action, through `a2m_action_search`, `a2m_action_describe`, `a2m_action_run` |
| A named toolset on a server | Its members' pipelines and the `use` actions | Its operations, through search |

- **Names.** An action `noun.verb` is listed as `a2m_<noun>_<verb>`. A target's tool keeps its published name, `{target}_{module}_{object}_{verb}`; its version appears in results, never in the name.
- **Descriptions** are the published ones, or the action's summary. The first 240 characters are enough to choose.
- **Schemas.** The input schema in a list may be the compact form, provided every difference is exact or wider. Output schemas are left out of the list unless the client asks; `tool.describe` returns them.
- **Annotations.** `readOnlyHint` for `read`; `destructiveHint` for `irreversible` and for writes without a key; `idempotentHint` from the key.
- **Size.** The registry refuses to build when a built-in toolset lists more actions than its cap (spec sections 221 and 220). For target tools the cap is `tools.listed_max`; past it, pipelines beyond the cap move to search and the list says so in the server instructions. The list's size in bytes is measured and budgeted (spec section 220 leaves the budget open).
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
| `next` | A field in the structured content, as spec section 221 shows |
| `error` | `isError` with the class, code, message, fix, and whether a retry is safe |
| Text from the target | Inside fields marked untrusted, already cut and stripped by the dispatcher |

**Long jobs.** A long-running action returns its `run_id` at once. A client that supports the Tasks extension gets a task whose `tasks/get` is answered by `run.get` and whose cancel is `run.cancel`. Progress is sent only to a caller that asked, as a rising count from the run's events.

**Elicitation.** When a run rests at `needs_login` or `needs_approval`, the result carries the link from `PersonSurface`; that includes a plain link from `give_link`, which a sign-in provider such as authorization-code sign-in needs the person to open in their own browser. Where the client supports URL-mode elicitation, the same link is also sent as an elicitation. On one machine the link is the loopback page, which can only raise the operating system's prompt; on a server it is the console page behind the identity provider's sign-in. Following the link is never an approval by itself, so giving it to an agent is safe.

**Found tools.** Spec section 220 asks that a tool found by search be added to the client's list. The stateless revision has no per-connection list. Until a client can be told of a changed list without a session, the generic `a2m_tool_call` is the route, and every call is checked against the full schema either way.

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

1. From spike 0.5: one `rmcp` service over the local channel, the stdio proxy as a line copier of our own, and the `use` toolset with `a2m_tool_call` for one read. Package 1.5.
2. Structured results, resource links, `next`, annotations; the two revisions.
3. Elicitation and the link fallback, with the approval surface. Package 3.6.
4. The `build` toolset, the action search tools, prompts, resources, server instructions. Package 5.1.
5. Streamable HTTP at `/mcp/<toolset>` on a server, with token checks. Package 6.2.

## Open points

- Where a failed call's class, code, message, and fix go. Structured content must fit the tool's published output schema; one reference client library refuses the whole result otherwise. Either every output schema admits the error shape, or an error carries text alone.
- What the one text item holds. The specification asks for the JSON as text beside structured content, for clients that read only text; the design sends a one-line summary.
- How the stdio proxy stays a copier. It has to turn a `notifications/cancelled` line into the closing of that request's HTTP stream, and copy the three request headers of the newer revision out of the body. Both are transport framing; the byte-for-byte test must allow for them and nothing else.
- Whether the SDK's log events can carry a caller's input or a header. The planted-secret test must capture them at their most verbose.
- Answering `server/discover` as an unknown method when only the handshake revision is served: the SDK does not, and the step that does it in front of the SDK is not built.
- The byte budget for a tool list and for one compact schema.
- How compact schemas are derived from full ones mechanically, so that "exact or wider" is proven, not reviewed.
- How a client is told of a changed tool list with no session, if at all.
