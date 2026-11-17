# Research: the MCP protocol and authorization for a shared server

**Status:** gathered from web searches on 2026-11-15. The pages on modelcontextprotocol.io are primary; several findings come from secondary write-ups and are marked. The full specification was not read end to end, so re-check each point against the specification before building [front doors](../spec/22-front-doors.md).

## Findings

### The 2026-07-28 revision made MCP stateless

- The revision removes protocol-level sessions and the `Mcp-Session-Id` header from the Streamable HTTP transport. List endpoints no longer vary per connection. Servers that need cross-call state use explicit, server-minted handles passed as ordinary tool arguments. [MCP changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog)
- The `initialize` handshake is removed. Every request carries its protocol version and client capabilities in `_meta`. [MCP changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog)
- Tasks moved out of the core protocol into an official extension, `io.modelcontextprotocol/tasks`. [MCP changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog). Secondary sources describe it as polling based: `tasks/get`, `tasks/update`, `tasks/cancel`. [Flavio Copes](https://flaviocopes.com/mcp-2026-07-28-stateless/)
- Secondary sources report: Multi Round-Trip Requests carry mid-call input such as elicitation without a held-open stream; Streamable HTTP POST requests carry `Mcp-Method` and `Mcp-Name` headers for routing; list responses are cacheable; Roots, Sampling, Logging, and the legacy HTTP+SSE transport are deprecated; the revision is wire-incompatible with earlier ones. [Appwrite](https://appwrite.io/blog/post/mcp-goes-stateless-in-the-2026-07-28-specification), [Stacktree](https://stacktr.ee/blog/mcp-2026-spec-changes), [Traversaal](https://traversaal.ai/blog/mcp-spec-update-2026-07-28-breaking-changes-stateless-transport-migrate)

**Design consequence.** Anything2MCP's `run_id` is exactly the kind of explicit handle the revision expects, so any control-plane replica can answer any request. Per-user tool lists are not possible on one endpoint, which is why toolsets have their own endpoints and per-user rights are enforced at call time.

### Authorization

- MCP servers must implement OAuth 2.0 Protected Resource Metadata (RFC 9728), and clients must use it to find the authorization server. [MCP authorization, draft](https://modelcontextprotocol.io/specification/draft/basic/authorization)
- Three client registration mechanisms exist, in priority order: Client ID Metadata Documents, pre-registration, and Dynamic Client Registration. Dynamic Client Registration is deprecated and kept for authorization servers that do not support metadata documents. [MCP client registration](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/client-registration)
- With a Client ID Metadata Document, the client uses an HTTPS URL as its client id and hosts a JSON document there. [MCP authorization tutorial](https://modelcontextprotocol.io/docs/2026-07-28/tutorials/security/authorization)
- Secondary sources report: clients must send the RFC 8707 `resource` parameter; the server must reject tokens not issued for it; token passthrough to downstream APIs is forbidden; clients validate the issuer of authorization responses (RFC 9207). [Scalekit](https://www.scalekit.com/blog/mcp-authorization-guide), [SSOJet](https://ssojet.com/blog/mcp-authentication-oauth-tokens-security-ai-connections)
- One secondary source reports an "Enterprise-Managed Authorization" extension shipped in June 2026. Not verified. [rohitraj.tech](https://rohitraj.tech/notes/mcp-server-authentication-oauth-guide-2026)

**Design consequence.** Anything2MCP is a resource server only. The organization's identity provider issues tokens. A caller's token is never forwarded to a target; reaching a target as the user goes through the identity modes in [spec section 16](../spec/16-identity-on-a-shared-server.md).

### Elicitation

- Elicitation has two modes. Form mode collects structured data in-band, and the data is exposed to the client. URL mode sends the user to a URL for an out-of-band interaction whose data is not exposed to the client. [MCP elicitation](https://modelcontextprotocol.io/specification/2026-07-28/client/elicitation)
- URL mode was introduced in the 2025-11-25 revision for sensitive interactions such as credentials, third-party authorization, and payments. It is not for authorizing the client to the server. [MCP elicitation, latest](https://modelcontextprotocol.io/specification/latest/client/elicitation), [SEP-1036](https://modelcontextprotocol.io/seps/1036-url-mode-elicitation-for-secure-out-of-band-intera)
- Clients declare which modes they support, and servers must not send a mode the client does not support. [MCP elicitation](https://modelcontextprotocol.io/specification/2026-07-28/client/elicitation)

**Design consequence.** Approvals and target sign-in use URL mode where the client supports it, and a link in the tool result where it does not.

### Rust SDK

- `rmcp` is the official Rust SDK. Its README for the 3.x line targets the 2026-07-28 specification. Feature flags include stdio and Streamable HTTP server transports, `auth` (OAuth 2.0), `elicitation`, and `auth-enterprise-managed`. [docs.rs rmcp](https://docs.rs/crate/rmcp)

## Not yet verified

- Tool annotations (`readOnlyHint`, `destructiveHint`, `idempotentHint`), `outputSchema`, resources, and list-changed notifications were not re-checked against the 2026-07-28 text.
- Whether `rmcp` 3.x can serve both the 2026-07-28 and 2025-11-25 revisions from one server.
- The exact Tasks extension lifecycle and how clients negotiate it.
- Recommended mitigations for local HTTP servers (Origin and Host checks, DNS rebinding) were not researched; the spec's local-HTTP rules rest on general practice.
