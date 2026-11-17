# 0035. A user's session is used only by that user and agents acting for them

**Status:** Accepted. Decided by the project owner.

**Date:** 2026-11-15

## Context

Web2MCP acts in a person's own account on an application. On a shared server many people and agents connect to the same system, and a person's signed-in session is as powerful as the person.

## Decision

A session signed in as a person can be used only by that person or an agent acting for them. An agent acts for a person when it runs as that person on their own machine, when it connects with a token the identity provider issued to that person, or when the person has explicitly delegated to it with a revocable, scoped, expiring record. No other user, service account, or administrator can run through the session. Administrators can end it.

## Consequences

- This is a built-in forbid in the policy engine and cannot be configured away.
- Saved session state is encrypted under a key for its owner.
- An agent with its own identity cannot use a person's account until that person delegates to it.
- Support staff cannot reproduce a problem by running as the affected user.

## Alternatives considered

- Let administrators act as a user for support: convenient, and removes the guarantee.

## Revisit if

Never without the owner's decision.

## Spec sections

- [44-session-keeper.md](../archive/spec-2026-11/44-session-keeper.md)
- [45-policy-engine.md](../archive/spec-2026-11/45-policy-engine.md)
- [16-identity-on-a-shared-server.md](../spec/6-running-it/602-identity-on-a-shared-server.md)
