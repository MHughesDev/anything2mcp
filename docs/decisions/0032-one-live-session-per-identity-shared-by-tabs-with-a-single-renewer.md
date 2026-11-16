# 0032. One live session per identity, shared by tabs, with a single renewer

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

Many runs share one sign-in. Copying cookies into a context per run lets copies go stale when a token rotates, and device-bound sessions cannot be copied. Concurrent refreshes with rotating tokens invalidate each other. Repeated automatic sign-ins lock accounts.

## Decision

Runs on one session work in separate tabs of that session's browser profile and share its live state. Only the session keeper signs in or renews, once per session however many runs noticed an expiry. A stored credential is tried once. Sessions on a user's device, under a managed-device rule, or bound to hardware are not movable. Sessions are kept alive only for a window after their last use. The keeper never works around the identity provider's maximum session age.

## Consequences

- Runs on one session share local storage, as a person's tabs do.
- Losing the runner that holds a non-movable session means signing in again.
- The keeper needs per-target expiry signals and a probe, declared in the auth file or learned.

## Alternatives considered

- An isolated context per run with copied cookies: cleaner isolation, stale copies.
- Let each driver refresh its own tokens: races and lockouts.

## Revisit if

A target proves to misbehave with several tabs under one sign-in.

## Spec sections

- [44-session-keeper.md](../spec/44-session-keeper.md)
- [11-concurrency-many-agents-at-once.md](../spec/11-concurrency-many-agents-at-once.md)
