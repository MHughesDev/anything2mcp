# 0006. The runtime picks the fastest verified binding

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

Speed is a core metric, but a faster route is only useful if it produces the same result.

## Decision

At call time the runtime takes the first verified binding whose driver is available, whose session is valid, and whose surface has not drifted, preferring `api`, then `http`, then `page_http`, then `ui`. The choice and reason are recorded in the trace. Reads fall back automatically; writes fall back only before anything was sent or after reconciliation.

## Consequences

- Callers never choose a mode unless policy lets them pin one.
- Results must be identical across bindings, which the binding-equivalence gate enforces.

## Alternatives considered

- A fixed mode per tool. Rejected: wastes speed and gives no fallback.

## Revisit if

Mode switching causes confusing differences in practice.

## Spec sections

- [06-modes-and-drivers.md](../spec/06-modes-and-drivers.md)
- [09-performance.md](../spec/09-performance.md)
