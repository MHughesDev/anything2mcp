# 0009. Four identity modes on a shared server

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

On a server, each run must decide who the target application sees. Organizations differ in what their identity rules allow.

## Decision

Four modes, set per target and per toolset: `per_user_token`, `per_user_session`, `per_user_device`, and `service_account`. Policy picks the identity; a caller can never name it.

## Consequences

- Each user can act with their own rights where the organization's rules allow.
- Which per-user mode works depends on Conditional Access rules outside Web2MCP's control.
- A caller's token is never forwarded to a target.

## Alternatives considered

- Service account only. Rejected: loses per-user permissions and attribution.

## Revisit if

A mode proves unworkable under common Entra configurations.

## Spec sections

- [16-identity-on-a-shared-server.md](../spec/6-running-it/602-identity-on-a-shared-server.md)
- [15-identity-sign-in-and-sessions.md](../archive/spec-2026-11/15-identity-sign-in-and-sessions.md)
