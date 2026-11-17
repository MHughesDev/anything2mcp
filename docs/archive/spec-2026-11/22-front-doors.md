# 22. Front doors

> Archived on 2026-11-17. This is section 22 of the first design, kept as it was written, with the names and numbers of its time. What it says now lives in [section 222, Front doors and the local engine](../../spec/2-kernel/222-front-doors-and-the-local-engine.md).

The same operations and pipelines are reachable six ways, all backed by one engine, one policy gate, and one audit log.

| Front door | For | Details |
| --- | --- | --- |
| MCP over stdio | An agent on the same machine | Started by the MCP client; a small proxy to the local engine ([section 39](39-local-engine-and-clients.md)); no network, no setup |
| MCP over Streamable HTTP | Remote and hosted agents, many at once | Stateless, one endpoint per toolset, behind TLS |
| REST API | Existing software, scripts, schedulers, non-MCP agent platforms | Versioned under `/api/v1`, described by an OpenAPI 3.1 document generated from the registry |
| SDKs | Python and TypeScript applications | Thin typed clients generated from the action registry and the OpenAPI document; local through a bridge, remote over HTTPS ([section 41](41-packaging-and-release.md)) |
| CLI | People, scripts, CI | `anything2mcp call`, `anything2mcp run`, and every admin and authoring command |
| Embedded library | Applications that want the engine in-process | The `anything2mcp` Rust crate; Python and Node bindings are deferred |

**MCP details.**

- **Protocol versions.** The server speaks the stateless 2026-07-28 revision, which removes the handshake and session header ([changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog)), and keeps serving the 2025-11-25 revision for older clients for as long as the Rust SDK supports both. The official Rust SDK, `rmcp`, provides stdio and Streamable HTTP server transports, OAuth support, and elicitation ([docs.rs](https://docs.rs/crate/rmcp)).
- **Explicit handles.** Because the protocol has no sessions, all continuity is carried by `run_id`, which matches the ledger design exactly.
- **Long jobs.** Pipelines are exposed through the official Tasks extension, where clients poll with `tasks/get` and can cancel, and equally through the `a2m_run_get` tool for clients without the extension.
- **Sign-in and approval prompts** use URL-mode elicitation where the client supports it, and a link in the result where it does not.
- **Resources.** Traces, diffs, screenshots, and downloaded files are MCP resources.

**Authorization for remote MCP.**

- Anything2MCP is an OAuth 2.1 resource server. It publishes protected resource metadata, which the MCP specification requires, and rejects any token not issued for it ([MCP authorization](https://modelcontextprotocol.io/specification/draft/basic/authorization)).
- The authorization server is the organization's identity provider, such as Entra ID or Okta. Anything2MCP never issues user credentials.
- Client registration supports Client ID Metadata Documents, which the specification now prefers, and pre-registered clients. Dynamic Client Registration is deprecated in the specification but still used by some clients, so it is supported where the identity provider or a gateway offers it ([client registration](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/client-registration)).
- **API keys** are available for clients that cannot do OAuth. A key is tied to one principal and one toolset, stored hashed, and rotatable.
- A caller's token is never forwarded to a target. Reaching a target as the user goes through the identity modes above.

**REST details.** `POST /api/v1/operations/{name}/calls` starts a call and returns a result or a `run_id`; `GET /api/v1/runs/{id}` returns status; run events are available as a server-sent stream and as signed webhooks. Idempotency keys are accepted on every write call.

**Toolset export.** Any toolset can be exported as an OpenAPI document, so platforms that consume OpenAPI actions instead of MCP can use the same tools.
