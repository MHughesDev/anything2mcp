# 0039. Local approval: a diff page for the moment, then an operating-system prompt

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

ADR 0026 says the engine raises local approvals with a user-presence check, but not what the person sees. A diff of many records does not fit a system dialog, and a page alone could be approved by an agent with a shell.

## Decision

On one machine the engine notifies the person and serves the diff on a loopback listener that exists only while something is pending, at an address with a one-time code, carrying no API. Pressing Approve there triggers the operating system's own user-presence prompt, and only its success records the approval. Sign-in happens in a visible window of the engine's own browser. No tray application ships until user-device runners need one.

## Consequences

- The engine briefly listens on loopback, which narrows the earlier no-port statement.
- The diff page is the first piece of the web console and is needed in Phase 3.
- Where no user-presence prompt is usable, the fallback is a native confirmation dialog.

## Alternatives considered

- A system dialog alone: cannot show a real diff.
- A tray application from the start: more to build and install for one machine.

## Revisit if

A platform offers a better native way to show a large confirmation.

## Spec sections

- [49-approval-and-sign-in-surfaces.md](../spec/49-approval-and-sign-in-surfaces.md)
- [39-local-engine-and-clients.md](../spec/39-local-engine-and-clients.md)
