# Traces and key stores

**Status:** Partial. Built from search-result summaries on 2026-11-15; no primary page was opened in full. It answers part of open briefs 5 and 22.

Feeds spec sections [217](../spec/2-kernel/217-traces-evidence-and-repair.md) and [216](../spec/2-kernel/216-the-vault-and-its-keys.md).

## OpenTelemetry has conventions for MCP

- OpenTelemetry publishes semantic conventions for MCP, and recommends them over its generic RPC conventions because they record MCP-specific detail. Their status is Development ([OpenTelemetry](https://opentelemetry.io/docs/specs/semconv/gen-ai/mcp/index.md)).
- A span is named with the method and its target, so a tool call reads as the method followed by the tool name ([Elastic](https://www.elastic.co/observability-labs/blog/mcp-tracing-opentelemetry-elastic-apm)).
- The method name attribute is required; the tool name is carried in the shared generative-AI attribute, not an MCP-specific one ([Last9](https://last9.io/blog/mcp-server-monitoring/), [OpenObserve](https://openobserve.ai/blog/mcp-server-observability.md)).
- Trace context travels in the request's `_meta` field under the standard key names, which lets an agent's trace and a server's trace join ([MCP Playground](https://mcpplaygroundonline.com/blog/mcp-observability-opentelemetry)).
- The official Python SDK for MCP already emits spans this way ([MCP Python SDK](https://py.sdk.modelcontextprotocol.io/run/opentelemetry/)).

**Consequence.** Anything2MCP follows these conventions for its MCP spans and accepts incoming trace context, while treating the attribute names as liable to change. Whether the Rust MCP SDK emits such spans itself was not checked.

## One Rust crate family covers the three desktop key stores

- The `keyring` crate gives one interface over the macOS Keychain, the Windows Credential Manager, and the Secret Service on Linux and similar systems ([docs.rs](https://docs.rs/crate/keyring/=3.6.3)).
- Stores are selected by feature flags, and access is synchronous ([same page](https://docs.rs/crate/keyring/=3.6.3)).
- It can be built without Secret Service support, in which case the Linux kernel keyring, which works on headless machines, becomes the default there ([docs.rs](https://docs.rs/crate/keyring/2.0.0-rc.2)).
- A package index lists a 4.x line under the Apache 2.0 license, restructured around a core crate with separate store crates ([openapps.pro](https://openapps.pro/packages/keyring-rs)).
- The `uv` project vendors this crate for its own credential storage ([uv source mirror](https://git.joshthomas.dev/mirrors/uv/src/branch/main/crates/uv-keyring)).

**Consequence.** The local root key can be held through this crate on all three desktop systems. Synchronous access means calls go on a blocking thread. The headless-Linux case needs its own answer, which spec section 216 gives.

## Not verified

- The current major version, license, and maintenance state of the key-store crate, read from its own repository.
- How the Windows Credential Manager behaves for a service account with no interactive sign-in.
- Whether the Linux kernel keyring's contents survive a restart, and what that means for a server.
- Which authenticated-encryption crate to use and whether it has had an independent review.
