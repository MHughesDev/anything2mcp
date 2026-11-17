# 0007. New bindings run in shadow before promotion

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

An HTTP binding inferred from traffic can miss logic the front end performs before saving.

## Decision

A new binding starts in `shadow`. While the verified binding does the real work, the shadow binding predicts what it would send and return without acting. After a set number of exact matches, a person may promote it to `verified`.

## Consequences

- Fast bindings are earned with evidence, not assumed.
- Promotion needs enough real traffic, so rarely used operations stay on slower bindings longer.

## Alternatives considered

- Trust a binding after its first success. Rejected for writes.

## Revisit if

Shadow comparison proves impractical for a class of endpoints.

## Spec sections

- [06-modes-and-drivers.md](../archive/spec-2026-11/06-modes-and-drivers.md)
- [26-agent-toolkit-how-tools-get-built.md](../archive/spec-2026-11/26-agent-toolkit-how-tools-get-built.md)
