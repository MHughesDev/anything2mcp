# 0115. The MCP endpoint as built: checks in front of the SDK, and a redactor on the way out

**Status:** Accepted. Design decision. Superseded in part by [decision 0130](0130-the-mcp-protocol-layer-is-our-own-because-the-sdk-brings-an-unmaintained-crate.md): what it says of the SDK. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the interface sketch, the build order, and the open points in the design of [`a2m-mcp`](../architecture/crates/a2m-mcp.md). It builds on [decision 0071](0071-the-mcp-server-stands-on-the-official-sdk-behind-a-seam-and-serves-two-revisions.md) and changes none of it.

## Context

The design sketched `McpService::http() -> BoxedHttpService` and `stdio_proxy(link: Link, …)`, and left open where a failed call's four fields go, what the text item holds, and how the proxy stays a copier. Building the endpoint against `a2m-actions` (records [0094](0094-the-action-layers-interface-as-building-it-settled-it.md) and [0095](0095-the-settings-loader-and-the-in-process-runner-as-built.md)) and running the message checklist with raw messages settled those and a few more.

## Decision

1. **A layer of this crate's own stands in front of the SDK.** In order it checks the `Host`, the HTTP method, the caller, the size of the body, that the body is JSON, that it is one request or one notification, the revision it names, and the shape of a tool call. Each refusal is a JSON-RPC error whose `data` holds the named failure. A notification is answered here with status 202, because a stateless server has nothing to do with one. The rules for agreeing a revision that the SDK gets wrong or leaves loose are answered here: `server/discover` as an unknown method when only the handshake revision is served, a revision that is not served whether a header or the body names it, and a request with no revision when only the stateless revision is served. A request of the handshake revision that names no revision is given the header before the SDK sees it, so the proxy needs no memory of the handshake. *Instead of:* leaving all of it to the SDK. The spike had shown three misses, and each line of the checklist that the product promises should hold whatever the service stands on.
2. **Everything that leaves passes the redactor here as well.** The dispatcher redacts results. It never sees what the protocol layer writes: an error that quotes a method, a tool's name, or a value of the wrong type, and the request id and progress token that are echoed. The planted-secret test found the secret in exactly such an error. Every outgoing message is parsed, each text in it passes the kernel's redactor, and a message with nothing to redact leaves byte for byte as written. *Instead of:* never quoting input in this crate's errors, which would not cover the SDK's.
3. **The caller comes from the mount.** The endpoint takes, from the request's extensions, either a `Caller` the mount already had the kernel check, as the REST router does ([decision 0116](0116-the-rest-router-takes-its-caller-from-the-mount-and-chooses-its-status-in-one-table.md)), or an `McpPeer`: the user the local channel saw, which the endpoint hands to `Kernel::verify_caller`. With neither, a bearer token is handed to the same verifier, which refuses every token in this build. The `Authorization` header is taken out of the request before anything else reads it. The endpoint sets the door, the toolset, and that an agent is calling.
4. **The endpoint is an `axum::Router`, and the proxy takes a function.** `McpService::http()` returns a router for the product crate to nest at `mcp_path(toolset)`; `McpService::handle` answers one request without a router. `stdio_proxy` takes an `Upstream`, which any function from a request to a response is, and the two streams. The role crates do not depend on each other, so the product crate wraps the local channel's link in a closure. *Instead of:* a `Link` parameter, which is a type of another role crate.
5. **A failed call carries text alone; a success carries the result alone.** This is the one rule for results that the tool surface and the REST interface share, rendered by `McpResultFace` in `a2m-actions`. The following call and the warnings of a success go in the result's `_meta`, under `anything2mcp/next` and `anything2mcp/warnings`. They cannot go in the structured content, because every output schema refuses unknown keys. A failure's next step is its `fix`. *Instead of:* a second text item, which the rule of one text item excludes. A client that does not read `_meta` does not see the hint; if that proves to matter, the output schemas must admit `next`, which is a change in `a2m-actions`.
6. **What a name stands for.** A name of an action that the toolset lists is that action. Any other name is a published tool: the endpoint asks `tool.describe` first, and an `input` failure there is the protocol error for an unknown tool, -32602; otherwise it dispatches `tool.call`. An action the toolset does not list is an unknown tool at that endpoint: it is reached through `a2m_action_run`. A tool list is built on each request from the registry and from `tool.search` and `tool.describe`, dispatched as the caller, so the endpoint reads nothing the caller may not and keeps nothing between requests. The cost is one dispatch for each published tool listed, and two for a call by a published tool's own name.
7. **A dispatch is never cut off.** A call's dispatch runs on its own task. When the caller cancels or the stream closes, the endpoint stops waiting and sends nothing more; the run goes on and is stopped with `a2m_run_cancel`. Progress is two steps, handed to the dispatcher and answered, because the kernel offers no events to follow yet.
8. **The proxy is not a pure copier in exactly the two ways [decision 0071](0071-the-mcp-server-stands-on-the-official-sdk-behind-a-seam-and-serves-two-revisions.md) names,** and answers three things itself: a line that is not JSON with -32700, and a request the engine did not answer with an MCP message, or could not be sent, with -32000, so that no caller is left waiting. At the end of its input it answers the requests still open and then ends.
9. **One outside crate beside the SDK:** `futures-core` 0.3.34, MIT or Apache-2.0, no features, for the stream trait of the body that is redacted as it passes. It was already in the tree of `axum` and of the SDK. *Instead of:* a task and a channel for each stream, which would not close the SDK's stream when the caller goes.

## Consequences

- The SDK logs the caller's own message, input included, at the debug and trace levels. A test of its own program shows it. The log of the process must pass the redactor, which the subscriber of `a2m-telemetry` does, or stay above those levels.
- A request id or a progress token that is a registered secret comes back redacted, so its caller cannot match the answer. A caller that uses a secret as an id is not served.
- A toolset other than `use` and `build`, token checks for a server, elicitation, resource links, prompts, resources, tasks, client profiles, and an idempotency key from the request's metadata are not built. The design's build order names them.

## Revisit if

- Listing costs too much. The kernel could then offer one call that returns a toolset's published tools with their schemas.
- A client in the conformance set ignores `_meta` and needs the following call.
- The SDK's HTTP service starts answering a check before this crate's layer can, or changes what it logs.

## Spec sections

- [221. The tool surface agents see](../spec/2-kernel/221-the-tool-surface-agents-see.md)
- [222. Front doors and the local engine](../spec/2-kernel/222-front-doors-and-the-local-engine.md)
