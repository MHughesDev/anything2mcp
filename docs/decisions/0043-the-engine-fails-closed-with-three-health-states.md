# 0043. The engine fails closed, with three health states

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

The design said what happens when an application or a runner fails, not when Web2MCP's own database, disk, key store, or browser does.

## Decision

No write starts unless its intent can be stored. Nothing whose hash or signature cannot be checked is run. Under pressure the engine gives up evidence detail, then capacity, then reads, and never a check. It reports one of three health states: ok, degraded (reads only), or stopped. Each failure has a defined response and a fault-injection test.

## Consequences

- An unhealthy engine refuses work instead of doing it less safely.
- A full evidence store stops writes.
- The fault tests need the test application's control interface.

## Alternatives considered

- Best effort under failure: more availability, less certainty about what was written.

## Revisit if

Never for writes.

## Spec sections

- [52-when-the-engine-itself-fails.md](../spec/52-when-the-engine-itself-fails.md)
