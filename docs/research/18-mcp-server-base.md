# Research: what the MCP server stands on, and which revisions it serves

**Status:** completed on 2026-11-17 by a spike and by reading sources. It answers brief 2 of [open research](11-open-research.md) and closes roadmap package 0.5. The spike is in [spikes/mcp-server](../../spikes/mcp-server/README.md). The choice it supports is [decision 0071](../decisions/0071-the-mcp-server-stands-on-the-official-sdk-behind-a-seam-and-serves-two-revisions.md). Feeds [spec section 222](../spec/2-kernel/222-front-doors-and-the-local-engine.md), [spec section 221](../spec/2-kernel/221-the-tool-surface-agents-see.md), and the design of [`a2m-mcp`](../architecture/crates/a2m-mcp.md).

## Question

Does the product's MCP server stand on the official Rust SDK crate, `rmcp`, or on a small JSON-RPC layer of our own? Which protocol revisions does it serve, and how is a revision agreed? No make of agent may be preferred: the server must work with any client that follows the standard.

## Answer in short

- One server can serve the 2026-07-28 revision and the 2025-11-25 revision together, with no session for either. The protocol text allows it and names it a "dual-era" server. The SDK does it. A hand-written server of about 400 lines does it too.
- Serving both is what keeps any client from being preferred. Of the eight client configurations tried, all eight worked against a server that serves both. Three could not use a server that serves only 2026-07-28, and two could not use a server that serves only 2025-11-25.
- The SDK passes the licence check with the closed list unchanged, builds for all three operating systems, takes its tool list from data at run time, and costs about 25 seconds of processor time in a cold release build and 13 to 17 crates the product does not have yet.
- The hand-written layer is smaller and passed every check. It was only built for stdio. The part still to write, the HTTP binding of the newer revision, is where most of the rules are.
- Since then the product has left the SDK for a layer of its own, HTTP binding included, because the advisory check refused a crate in the SDK's tree ([decision 0130](../decisions/0130-the-mcp-protocol-layer-is-our-own-because-the-sdk-brings-an-unmaintained-crate.md)).

## What was run, and what was only read

| | Run here | Only read |
| --- | --- | --- |
| The SDK crate | Built and run on Linux: stdio, and stateless Streamable HTTP, in both revisions. Type-checked for Windows and macOS targets | Tasks, round trips for input, elicitation, subscriptions, authorization: present in its source, not exercised |
| The hand-written layer | Built and run on Linux over stdio in both revisions. Type-checked for Windows and macOS targets | Nothing |
| Clients | The reference client library for Python and the reference client library for TypeScript in its two release lines, over stdio; the Python one also over HTTP. A hand-written caller with no library. `curl` for raw HTTP | No hosted agent and no desktop agent was tried |
| The protocol | | The specification's own repository, files under `docs/specification/2026-07-28/`, `docs/specification/2025-11-25/`, and `schema/2026-07-28/schema.ts`, as they stood on the main branch on 2026-11-17 |
| Not reachable from this session | | The specification's web site and the crate documentation site. Every protocol fact below was taken from the repository files; none from the site |

## Results

### The two bases, side by side

Both servers serve one tool, `example_orders_order_get`, read from a data file at start. Both were built with Rust 1.100.0 in release mode with symbols stripped, on four cores.

| | On the SDK | By hand |
| --- | --- | --- |
| Lines of server code | 201, of which about 25 are the HTTP mode | 415 |
| Outside crates | 59 in the tree for stdio; 13 of them are not in the product's lock file today. The HTTP server adds 4 more | `serde_json` and its tree, all already in the product |
| Licences | Pass against `deny.toml` with no change | Pass |
| Cold release build | 38.2 s on the clock, 78.7 s of processor time | 3.8 s on the clock, 5.6 s of processor time |
| Processor time of the crates the SDK brings | `rmcp` 11.9 s, `schemars` 4.2 s, `chrono` 2.7 s, `futures-util` 2.6 s, `tokio-util` 1.1 s, and smaller ones: about 25 s | None |
| Binary, stdio only | 2,777,640 bytes | 545,624 bytes |
| Binary, with the HTTP server and its listener | 4,254,472 bytes | Not built |
| The 50 raw checks | 47 pass | 50 pass |
| Both revisions from one process | Yes | Yes |
| Tool list from data at run time | Yes, with no macro | Yes |
| Structured result, output schema, annotations | Yes | Yes |
| Progress and cancellation | Yes | Yes |
| Stateless Streamable HTTP, both revisions | Yes, run | Not built |
| Windows and macOS | Type-checks for both targets | Type-checks for both targets |

The two binaries are not a fair pair: the SDK variant carries the Tokio runtime, which the product links anyway, and the hand-written one uses threads. The SDK's own share of the size was not measured apart.

The 13 crates new to the product for stdio are `rmcp`, `schemars`, `schemars_derive`, `serde_derive_internals`, `chrono`, `futures`, `futures-executor`, `futures-sink`, `tokio-util`, `pastey`, `ref-cast`, `ref-cast-impl`, and `dyn-clone`. The HTTP server adds `rand`, `rand_core`, `sse-stream`, and `tokio-stream`.

The three checks the SDK variant missed:

| Check | What the SDK does | Does it matter |
| --- | --- | --- |
| A line that is not JSON gets error -32700 | It skips the line and says nothing | Little. A client that sends a broken line has no request id to wait on. The product's stdio side is its own proxy in any case |
| Results name the server in `_meta` | Only `server/discover` does by default | No. The text says SHOULD, and the handler can add it |
| A server set to the handshake revision alone answers a modern probe with an error that is not a modern one | It answers -32022, a modern error, listing only 2025-11-25 | Not when both revisions are served. Both client libraries coped and fell back to the handshake |

### Clients against servers

Each row is one client configuration. Each was run against both servers, with the server set three ways. "Works" means it connected, listed the tool, called it, got the structured result, saw progress, cancelled a call, and called again.

| Client configuration | How it opens | Server serves both | Only 2026-07-28 | Only 2025-11-25 |
| --- | --- | --- | --- | --- |
| Python library 2.3.0, default mode | `server/discover` at 2026-07-28; falls back to `initialize` | Works, at 2026-07-28 | Works | Works, at 2025-11-25 |
| Python library, handshake mode | `initialize` asking 2025-11-25 | Works, at 2025-11-25 | Cannot connect | Works |
| Python library, pinned to 2026-07-28 | No opening message; every request carries its version | Works | Works | Every request refused |
| TypeScript library 1.32.2 | `initialize` asking 2025-11-25. It knows no newer revision | Works, at 2025-11-25 | Cannot connect | Works |
| TypeScript library 2.4.0, default mode | `initialize` asking 2025-11-25. The handshake is this line's default | Works, at 2025-11-25 | Cannot connect | Works |
| TypeScript library 2.4.0, mode `auto` | `server/discover` on a second, short-lived server process, then the real one | Works, at 2026-07-28 | Works | Works, at 2025-11-25 |
| TypeScript library 2.4.0, pinned | `server/discover` at 2026-07-28 | Works | Works | Cannot connect |
| Hand-written caller, no library | Both ways | 50 of 50 by hand, 47 of 50 on the SDK | | |

Every client declared empty capabilities, `{}`, because the callers set none. The full table, with what each client sent, is in `spikes/mcp-server/results/summary.md`.

The same held on both servers in every cell. The two bases cannot be told apart by any client library tried.

Over stateless HTTP, against the SDK variant: the Python library worked in its default mode at 2026-07-28 and in handshake mode at 2025-11-25, with progress and cancellation. Raw requests showed that `initialize` is answered with no session header, that a later handshake-era request with only the version header is answered, that a header that disagrees with the body is refused with status 400 and error -32020, that an unserved version is refused with status 400 and error -32022, and that a foreign `Host` header is refused with status 403.

### Four things the clients showed that the design must take in

1. **An error result must fit the output schema, or carry no structured content.** The spike's tool answers a missing order with `isError: true` and structured content holding the error's class and code. That does not fit the tool's output schema. The Python library and the TypeScript 2.4.0 library passed it through. The TypeScript 1.32.2 library refused the whole result with its own error -32602, so the caller saw neither the message nor the class. The text says a server MUST send structured results that fit the output schema and makes no exception for errors.
2. **The last progress notification can be lost.** When the last progress notification and the result arrive together, the TypeScript libraries sometimes delivered three of four notifications. It happened with both servers. Nothing may be said in a progress notification that the result does not also say.
3. **`ping` is gone from the newer revision.** A modern caller that sends it gets -32601. The TypeScript 2.4.0 library refuses to send it at all.
4. **A client's capabilities arrive with every request in the newer revision, and only once in the older one.** A stateless server that answers `initialize` from settings never learns what a handshake-era client can do. Such a client gets the fallback every client gets: the link in the result, and the run identifier to ask again with.

## Facts

### The SDK crate

| Fact | Source |
| --- | --- |
| `rmcp` 3.10.0 was published on 2026-11-12. The 3.x line has had 23 releases since 2026-07-23, about one a week. Licence Apache-2.0. Needs Rust 1.88 or newer | The crates.io registry record, read on 2026-11-17 |
| Three major versions in 2026: 1.0.0 on 2026-03-03, 2.0.0 on 2026-06-29, and the 3.0 line from 2026-07-23 | The same |
| Its README names the 2026-07-28 specification | `README.md` in the published crate |
| It knows five revisions: 2024-11-05, 2025-03-26, 2025-06-18, 2025-11-25, 2026-07-28. A server serves all of them unless it overrides `supported_protocol_versions` | `src/model.rs`, `src/handler/server.rs` in the published crate; run |
| One server serves both eras. A request with a version in `_meta` is served statelessly; `initialize` selects the handshake. On stdio the choice is made by the first message; `server/discover` leaves it open | `src/service/server.rs`; run |
| The tool list can come from data: implement `list_tools` and `call_tool` on the handler, and build each tool with `Tool::new_with_raw`, `with_raw_output_schema`, and `ToolAnnotations::from_raw`. With `default-features = false` the macro crate is not built | `src/model/tool.rs`; run |
| A structured result is `CallToolResult::structured`. By default it also writes the JSON as text; the spike replaced that with a one-line summary | `src/model.rs`; run |
| Results for a handshake-era caller have `resultType` removed by the SDK. The caching fields `ttlMs` and `cacheScope` stay on them, which is harmless | Run |
| Cancellation reaches the handler as a token on the request context. Progress is sent through the peer on the request context | `src/service.rs`; run |
| Stateless HTTP is `StreamableHttpServerConfig` with `with_legacy_session_mode(false)`, `with_json_response(true)`, and the `NeverSessionManager`. JSON answers become an event stream when the handler sends a notification first. The `Host` check allows loopback names by default | `src/transport/streamable_http_server/tower.rs`; run |
| The HTTP service is a Tower service over `http` requests, so it mounts on the listener the product already has | The same; run with `axum` 0.8.9 |
| List-changed: the capability can be declared. In the newer revision the notification travels on a `subscriptions/listen` stream that the handler accepts | `src/handler/server.rs`. Not run |
| Tasks, round trips for input, elicitation, signed request state, and OAuth are in the crate | `src/model/task.rs`, `src/model/mrtr.rs`, feature flags in `Cargo.toml`. Not run |
| A client role and an HTTP client transport over a local socket are in the crate too | Feature flags `client` and `transport-streamable-http-client-unix-socket`. Not run |
| Whether the SDK is tested against the protocol's own conformance suite | Unverified |

### The client libraries

| Fact | Source |
| --- | --- |
| The Python package `mcp` 2.3.0, MIT, speaks both eras. Its default mode probes with `server/discover` and falls back to `initialize` on any error except a modern-only refusal | The installed package, `mcp/client/_probe.py`; run |
| The npm package `@modelcontextprotocol/sdk` 1.32.2, MIT, published 2026-10-23, speaks handshake revisions only, up to 2025-11-25 | The installed package, `dist/esm/types.js`; run |
| The npm package `@modelcontextprotocol/client` 2.4.0, Apache-2.0, published 2026-10-23, implements 2026-07-28, and its default is still the handshake. The newer revision is opt-in | The installed package's type declarations; run |

### The protocol, 2026-07-28

Read from the specification's repository. Brief 2 asked for each of these to be confirmed.

| Topic | What the text says | File |
| --- | --- | --- |
| No handshake, no session | `initialize`, `notifications/initialized`, the session header, and `ping` are removed. Every request carries `io.modelcontextprotocol/protocolVersion` and `io.modelcontextprotocol/clientCapabilities` in `_meta`, and should carry `io.modelcontextprotocol/clientInfo` | [changelog](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/changelog.mdx), [schema](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/schema/2026-07-28/schema.ts) |
| Agreeing a version | There is no negotiation. A request with an unserved version gets error -32022 with `data.supported` and `data.requested`. Servers MUST implement `server/discover`, which lists `supportedVersions` and capabilities | [versioning](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/versioning.mdx) |
| Serving both | A server MAY serve both eras, and MAY do so at once on one endpoint or process. A request with modern `_meta` is served statelessly; `initialize` selects the older semantics. A server that serves only modern revisions SHOULD name them in its error to `initialize` | The same |
| How a client finds the era | On stdio it probes with `server/discover`: a result or a modern error means modern; any other error or silence means the handshake. On HTTP it sends a modern request and reads the body of a 400 | [stdio](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/transports/stdio.mdx), [Streamable HTTP](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/transports/streamable-http.mdx) |
| Result type | Every result carries `resultType`: `complete`, or `input_required` | Changelog; schema |
| Tool annotations | `title`, `readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`, unchanged. They are hints, and a client must not trust them from a server it does not trust | Schema; [tools](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/server/tools.mdx) |
| Output schemas | Optional. Where one is given, the server MUST send structured results that fit it, and clients SHOULD validate. Any JSON Schema 2020-12 keyword is allowed, and structured content may be any JSON value. A tool that returns structured content SHOULD also return the JSON as text, for older clients | Tools; changelog |
| Errors from a tool | An unknown tool and a malformed request are protocol errors, -32602. A failed call, bad input included, is a result with `isError: true` | Tools |
| Resource links | A `resource_link` content item in a tool result, with the resource's own annotations. `resources/read` results are cacheable. A missing resource is now -32602 | Tools; changelog |
| List caching | `tools/list`, `prompts/list`, `resources/list`, `resources/templates/list`, `resources/read`, and `server/discover` results MUST carry `ttlMs` and `cacheScope`, `public` or `private`. Tools SHOULD be listed in a fixed order | Changelog; schema |
| Round trips for input | A server never sends a request to a client. It answers `tools/call`, `prompts/get`, or `resources/read` with `resultType: "input_required"`, carrying `inputRequests`, `requestState`, or both. The client asks again with a new id, `inputResponses`, and the same `requestState`. The server must treat `requestState` as an attacker's input and protect it, and must not ask for input a client did not declare it supports | [round trips](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/patterns/mrtr.mdx) |
| Elicitation by link | URL mode remains: `mode`, `message`, `url`. The completion notification and the elicitation id of 2025-11-25 are removed; the client learns the outcome by asking again | Changelog; schema |
| Tasks | An extension, `io.modelcontextprotocol/tasks`. The client declares it in its per-request capabilities and the server in `server/discover`. The server may answer with `resultType: "task"`, a task id, a status, a lifetime, and a polling interval. The client polls `tasks/get`, answers input with `tasks/update`, and may send `tasks/cancel`. Statuses: `working`, `input_required`, `completed`, `failed`, `cancelled`. There is no `tasks/list` | Changelog; [tasks overview](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/extensions/tasks/overview.mdx). The extension's own repository was not read |
| List-changed | `subscriptions/listen` replaces the standing GET stream and `resources/subscribe`. The client names the kinds it wants; the server acknowledges first and tags each notification | Changelog; [subscriptions](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/patterns/subscriptions.mdx) |
| Cancellation | On stdio the client sends `notifications/cancelled`. On HTTP, closing the response stream is the signal and no message is sent. The server stops and sends nothing more for that request | [cancellation](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/patterns/cancellation.mdx) |
| HTTP headers | Every POST carries `MCP-Protocol-Version`, `Mcp-Method`, and for a tool call `Mcp-Name`. They must match the body, or the answer is status 400 with error -32020. Stream resumption is removed | Streamable HTTP; changelog |
| Error codes | -32020 to -32099 belong to the specification: -32020 header mismatch, -32021 missing client capability, -32022 unsupported version. -32000 to -32019 are an implementation's own | Changelog; schema |
| Deprecated | Roots, sampling, logging, the older HTTP transport with server-sent events, and dynamic client registration | Changelog |

In 2025-11-25 the handshake rule is: the server answers `initialize` with the client's revision if it serves it, and otherwise with a revision it does serve; the client then decides. A `ping` is allowed before `initialize` ([lifecycle](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2025-11-25/basic/lifecycle.mdx)).

## What this means for the design

1. **Serve both revisions, statelessly.** The condition in [decision 0011](../decisions/0011-stateless-streamable-http-and-an-oauth-resource-server.md) is met: the SDK serves both from one service.
2. **The stdio proxy is more than a copier.** On stdio a client cancels with a message; on HTTP the same thing is said by closing the stream. On HTTP the newer revision wants three headers that repeat the body. A proxy between the two must do both translations, and nothing else.
3. **Errors and output schemas.** Either a failed call carries no structured content, or every published output schema admits the error shape. The design of `a2m-mcp` says an error is rendered with its class, code, message, and fix; it must say where.
4. **The text item.** The specification asks for the JSON as text beside structured content, for clients that read only text. The design sends a one-line summary. A client that ignores structured content would then see only the summary. Which to send is an open point.
5. **Handshake-era callers get fallbacks only.** No elicitation and no tasks for them, because a stateless server does not remember what they declared.
6. **Declare only what is served.** `tools.listChanged` must not be declared until the notification can be sent.

## The product, run against the same client libraries

**2026-11-17.** A manual proof, not a CI test, owed since the spike ([decision 0131](../decisions/0131-the-front-doors-joined-serve-the-thin-client-and-the-bridge.md)). The real program was run on Linux: `anything2mcp serve` on a temporary data directory, with one read tool, `example_order_get`, published over the test application, and each client library was given `anything2mcp bridge` as its stdio server. The libraries were installed as for the spike, in the same versions, and a small caller of the spike's shape was written for each and thrown away. A tee between client and bridge recorded what each sent first. Every run connected, listed the eleven built-in tools of the `use` toolset, called the read by name, and read the structured result, whose `result.customer` was the value the test application holds.

| Client library | Mode | What it sent first | Revision used | Connected | Listed tools | Called the read | Read the structured result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Python, `mcp` 2.3.0 | `auto` | `server/discover`, naming 2026-07-28 | 2026-07-28 | yes | 11 | yes | yes |
| Python, `mcp` 2.3.0 | `legacy` | `initialize`, asking for 2025-11-25 | 2025-11-25 | yes | 11 | yes | yes |
| Python, `mcp` 2.3.0 | `2026-07-28` | `tools/list`, naming 2026-07-28 | 2026-07-28 | yes | 11 | yes | yes |
| TypeScript, `@modelcontextprotocol/sdk` 1.32.2 | its only one | `initialize`, asking for 2025-11-25 | 2025-11-25 | yes | 11 | yes | yes |
| TypeScript, `@modelcontextprotocol/client` 2.4.0 | `legacy` | `initialize`, asking for 2025-11-25 | 2025-11-25 | yes | 11 | yes | yes |
| TypeScript, `@modelcontextprotocol/client` 2.4.0 | `auto` | `server/discover`, naming 2026-07-28 | 2026-07-28 | yes | 11 | yes | yes |
| TypeScript, `@modelcontextprotocol/client` 2.4.0 | pinned to 2026-07-28 | `server/discover`, naming 2026-07-28 | 2026-07-28 | yes | 11 | yes | yes |

One thing the libraries showed. The Python library wrote a line to its standard error on every run: the tool it called is not in the server's listing, so it could not check the structured content against an output schema. The default listing holds the built-in tools, and a published operation is found with the search tool and called by name; a client that validates results against the listing validates nothing for such a call. The result was returned all the same. Not run: any client that is not one of these two libraries; Windows and macOS; a write; progress and cancellation through the bridge with a client library, which the product's own tests cover with raw messages only in part.

## Not verified

- Any client that is not one of the two reference libraries: no desktop agent, no hosted agent.
- The SDK's behavior under load, over long-lived streams, or on Windows and macOS at run time.
- The SDK's tasks, round trips for input, elicitation, subscriptions, and authorization code.
- The extension's own text for tasks.
- Revisions older than 2025-11-25. A client that asks for one is answered with 2025-11-25, as the handshake rule says; no client that speaks only an older revision was tried.
- The security advisories for the SDK's tree. The check needs a network fetch that this session could not make; the continuous integration run makes it.
