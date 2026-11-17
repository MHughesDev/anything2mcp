# 0071. The MCP server stands on the official SDK behind a seam, and serves two revisions

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the open point "Whether `rmcp` can serve both revisions from one service" in the design of `a2m-mcp` and in spec section 222; the condition left open by [decision 0011](0011-stateless-streamable-http-and-an-oauth-resource-server.md); roadmap package 0.5 and research brief 2.

## Context

The design named the official Rust SDK crate, `rmcp`, without having run it, and asked one question before depending on it: can one server serve the 2026-07-28 revision and the 2025-11-25 revision together? The spike for package 0.5 built the same small server twice, once on the SDK and once by hand, and called both from the reference client libraries for Python and TypeScript and from a hand-written caller. [Research note 18](../research/18-mcp-server-base.md) has the numbers.

What the spike showed:

- Both bases serve both revisions from one process, with no session. No client library could tell them apart.
- Of eight client configurations, all eight worked against a server that serves both revisions. Three could not use one that serves only 2026-07-28: the TypeScript library still opens with the handshake by default, in both of its release lines. Two could not use one that serves only 2025-11-25.
- The SDK takes its tool list from data at run time with no macro, passes the licence check with `deny.toml` unchanged, type-checks for Windows and macOS, and serves stateless Streamable HTTP in both revisions. It adds 13 crates the product does not have for stdio and 4 more for HTTP, and about 25 seconds of processor time to a cold release build.
- The SDK has had three major versions in 2026 and releases about once a week.
- The hand-written server is about 400 lines over stdio and passed all 50 raw checks; the SDK passed 47, missing nothing a client depends on.

## Decision

**The base.** `a2m-mcp` builds its MCP service on `rmcp`, pinned to one exact version, with default features off and the features `server` and `transport-streamable-http-server`. No macro is used: tools, prompts, and resources are listed from the action registry and from published plans at run time.

**The seam.** No type from `rmcp` appears in the public interface of `a2m-mcp`. The rest of the kernel sees `McpService`, `McpSettings`, and the proxy, as the crate design sketches them. What the server must do is written as a list of messages, in the design of `a2m-mcp`, and tested with raw messages and with two unrelated client libraries. Those tests do not know what the server stands on.

**The stdio proxy is our own code.** It copies lines and does not parse them into the SDK's types, because the design asks that it add nothing. It does two translations that the two transports force: a `notifications/cancelled` line becomes the closing of that request's HTTP stream, and the three request headers of the newer revision are copied out of the body.

**The revisions served** are 2026-07-28 and 2025-11-25, both statelessly, set by `mcp.revisions`. The server claims no older revision.

**How a revision is agreed.**

1. A request whose `_meta` carries `io.modelcontextprotocol/protocolVersion` is served under that revision, with no memory of earlier requests. On HTTP the `MCP-Protocol-Version` header must say the same. If the revision is not served, the answer is error -32022 with the list of revisions served and the one asked for; on HTTP with status 400.
2. `server/discover` lists the revisions served and the capabilities. A client may call it first; none has to.
3. `initialize` selects the handshake revision. If the client asks for 2025-11-25 it gets it. If it asks for anything else it is answered with 2025-11-25, and decides for itself whether to go on. The answer comes from settings. No session identifier is issued.
4. A handshake-era request on HTTP names its revision in the `MCP-Protocol-Version` header and is served as it comes, with no session.
5. If `mcp.revisions` holds only the handshake revision, `server/discover` is answered as an unknown method, -32601, so that a client that speaks both falls back to the handshake. If it holds only 2026-07-28, `initialize` is refused with -32022 and the list of revisions served.

*Instead of the SDK:* a JSON-RPC layer of our own. It is smaller, it builds in a tenth of the time, and over stdio it was the more exact of the two. It was not chosen because the larger part is still unwritten: the HTTP binding of the newer revision, round trips for input, tasks, and subscriptions, each with rules that the protocol's own project keeps changing and keeps the SDK current with. Rule 2 puts correct before easy, and for a protocol that is still moving, correct is likelier on the base its authors maintain. The hand-written layer stays as the named way out.

*Instead of two revisions:* only 2026-07-28. It would shut out every client that opens with the handshake, which today includes the default mode of a reference client library. That prefers some clients over others, which [decision 0058](0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md) forbids.

*Instead of two revisions:* every revision the SDK knows. Nothing older than 2025-11-25 was tested, and the server should not claim what is not tested.

## Consequences

- A caller in the handshake era gets the fallback every client gets: the link in the result in place of elicitation, and the run identifier in place of a task. A stateless server does not remember what such a client declared in `initialize`.
- A failed call's structured content must fit the tool's published output schema, or be left out. One reference client library refuses the whole result otherwise. The design of `a2m-mcp` must say which.
- A capability is declared only when it is served. `tools.listChanged` waits for `subscriptions/listen`.
- The SDK logs through the `tracing` crate. The planted-secret test must run through this face with those events captured at their most verbose.
- An upgrade of the SDK is a change like any other: the raw checks and the two client libraries must pass before it merges. A new major version gets a line in the journal.
- The MCP pack, which speaks to existing MCP servers as a client, may use the same crate's client role. That was not tested here.

## Revisit if

- The SDK stops being released, or a release breaks a raw check and is not fixed within a few weeks.
- Keeping up with its major versions costs more than the roughly one thousand lines a layer of our own would be.
- The SDK cannot do something the design needs behind the seam, such as rendering a result byte for byte the same for both transports.
- A client in the conformance set speaks only a revision older than 2025-11-25.
- The 2025-11-25 revision falls out of use by the clients people run. Then it is dropped, by a new record.
- A third revision is published. Whether to serve three is a new decision.

## Spec sections

- [222. Front doors and the local engine](../spec/2-kernel/222-front-doors-and-the-local-engine.md)
- [221. The tool surface agents see](../spec/2-kernel/221-the-tool-surface-agents-see.md)
- [605. Packaging and release](../spec/6-running-it/605-packaging-and-release.md)
