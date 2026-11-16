# 0026. Local approvals come from the engine, with an operating-system user-presence check

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

ADR 0012 requires approvals to be out of the calling agent's reach. A coding agent with a shell runs as the same operating-system user as the person, so any approve command would be within its reach.

## Decision

On a local machine there is no approve command, API route, or MCP tool. The engine raises the approval prompt itself and, where the operating system offers a user-presence check, requires it. Where none exists, a native dialog is used and documented as a guard against mistakes, not a security boundary. A hardened option runs the engine under its own account.

## Consequences

- The design states openly that a hostile program running as the user is outside what a single-account machine can defend against.
- User-presence checks differ per operating system and need research before Phase 3.
- Under a separate account the device's own sign-in may not carry over, so the person signs in once.

## Alternatives considered

- An approve command guarded by a typed confirmation: an agent can type it.
- No local writes at all: safe, but removes the main use on a laptop.

## Revisit if

An operating system offers no usable check and the dialog-only fallback is judged too weak.

## Spec sections

- [39-local-engine-and-clients.md](../spec/39-local-engine-and-clients.md)
- [18-safety-policy-and-approvals.md](../spec/18-safety-policy-and-approvals.md)
