# 0024. One local engine per user, reached over a socket or named pipe

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

Building through commands means many short processes that must share one browser, one session, one capture, and one set of record locks. Several agents may also use one target at once. A local TCP port would be reachable by web pages and other programs.

## Decision

On one machine a single engine process runs per user. Commands, the stdio MCP proxy, the SDK bridge, and the tray application are thin clients. They reach the engine over a Unix domain socket or a Windows named pipe restricted to that user; no TCP port is opened by default. The engine starts on demand and stops when idle. An in-process mode exists for single-command use in CI.

## Consequences

- Solo and Workstation service become one architecture with different lifetimes.
- A new crate, `w2m-local`, and a client-to-engine version check.
- SDKs need no platform-specific code, because the bridge is the Rust binary.
- A background process is one more thing that can be left running or out of date.

## Alternatives considered

- An engine per MCP client or per command: no shared state; sessions and locks collide.
- A local HTTP port with a token: simpler, but exposed to local web pages.

## Revisit if

The on-demand engine proves unreliable on an operating system.

## Spec sections

- [39-local-engine-and-clients.md](../archive/spec-2026-11/39-local-engine-and-clients.md)
- [23-deployment-configurations.md](../spec/6-running-it/601-deployment-configurations.md)
