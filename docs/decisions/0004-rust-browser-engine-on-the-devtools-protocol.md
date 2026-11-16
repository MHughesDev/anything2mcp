# 0004. Rust browser engine on the DevTools protocol

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

Playwright has no official Rust client; its other clients pipe calls through a Node driver. Web2MCP needs only a dozen verbs over declared locators, not a general automation API.

## Decision

The `ui` and `page_http` modes run on a Rust engine that speaks the Chrome DevTools Protocol directly to Chrome, Edge, or Chromium. Firefox and WebKit are out of scope for the first release.

## Consequences

- Waiting and actionability logic must be rebuilt and tested; this is the hardest engineering in the project.
- No Node process, lower memory, and event-driven waits.
- Phase 0 runs a spike to choose a base: chromiumoxide, Rustwright's core, or a new protocol client.

## Alternatives considered

- Playwright through a Node sidecar: mature, but an extra process and slower waits.
- A Rust wrapper around Playwright's driver: same extra process.

## Revisit if

The spike shows reliable acting and waiting cannot be reached in reasonable time; then use a Playwright sidecar behind the same driver interface.

## Spec sections

- [17-browser-engine-and-runtime.md](../spec/17-browser-engine-and-runtime.md)
