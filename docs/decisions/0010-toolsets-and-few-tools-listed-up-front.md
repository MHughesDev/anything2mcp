# 0010. Toolsets, and few tools listed up front

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

Agents choose tools less accurately past roughly 30 to 50 tools. The stateless MCP revision no longer lets tool lists vary per connection. Administrators need to expose chosen tools to chosen people.

## Decision

Tools are grouped into named, versioned toolsets, each with its own MCP endpoint, members, identity mode, and approval policy. A toolset lists pipelines and control tools; single operations are found through a search tool. Per-user rights are enforced at call time.

## Consequences

- Governance and agent accuracy are served by the same mechanism.
- Clients with their own tool search can ask for the full listing.

## Alternatives considered

- One flat list of every tool. Rejected.

## Revisit if

Agents fail to find operations through search in the usability gate.

## Spec sections

- [20-access-control-toolsets-and-tenancy.md](../spec/20-access-control-toolsets-and-tenancy.md)
- [21-agent-facing-tool-surface.md](../spec/21-agent-facing-tool-surface.md)
