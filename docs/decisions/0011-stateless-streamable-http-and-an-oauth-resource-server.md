# 0011. Stateless Streamable HTTP and an OAuth resource server

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

The 2026-07-28 MCP revision removed protocol sessions and the handshake. Hosted agents such as Copilot Studio accept only the Streamable transport.

## Decision

Remote MCP is served as stateless Streamable HTTP. Web2MCP is an OAuth 2.1 resource server that publishes protected resource metadata; the organization's identity provider issues tokens. All continuity is carried by `run_id`. The 2025-11-25 revision is also served while the SDK allows. API keys are available for clients that cannot do OAuth.

## Consequences

- Any control-plane replica can answer any request; no sticky routing.
- Web2MCP never issues user credentials.
- Serving two protocol revisions must be confirmed feasible with `rmcp`.

## Alternatives considered

- Sessions and custom authentication. Rejected: against the current specification.

## Revisit if

`rmcp` cannot serve both revisions; then choose based on which clients matter most.

## Spec sections

- [22-front-doors.md](../archive/spec-2026-11/22-front-doors.md)
- [24-networking.md](../spec/6-running-it/603-networking.md)
