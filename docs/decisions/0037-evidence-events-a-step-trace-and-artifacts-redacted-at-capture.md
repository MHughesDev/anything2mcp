# 0037. Evidence: events, a step trace, and artifacts, redacted at capture

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

Gates, shadow comparison, repair, and audit all need a record of what a run did. Evidence is also the easiest place for a secret or business data to leak.

## Decision

Evidence has three layers: ledger events, one structured step-trace record per step, and large artifacts stored as blobs. A trace signature hashes the parts that should not vary and is what the determinism and equivalence gates compare. Redaction runs on the runner at capture, with a second scan before upload. Three capture levels exist; screenshots outside the fullest level are taken only at a failure. MCP spans follow OpenTelemetry's MCP conventions and never carry values.

## Consequences

- A CI test plants a secret and fails if it reaches any evidence.
- OpenTelemetry's MCP attribute names are still in development and may change.
- The step-trace schema becomes a public format once fixed in Phase 1.

## Alternatives considered

- Free-form logs: cannot be compared or safely redacted.
- Adopt another tool's trace archive: tied to that tool's engine.

## Revisit if

The step-trace schema proves too rigid for a new driver.

## Spec sections

- [47-traces-and-evidence.md](../archive/spec-2026-11/47-traces-and-evidence.md)
- [28-observability-audit-and-repair.md](../archive/spec-2026-11/28-observability-audit-and-repair.md)
