# 0003. No AI at run time

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

Most comparable products replay generated code and fall back to a live agent when replay fails. That makes results vary between runs and makes writes hard to account for.

## Decision

AI explores applications and drafts or repairs files at build time only. A run replays fixed, reviewed steps. When a step cannot be carried out as written, the run stops with a named cause.

## Consequences

- Runs are deterministic and auditable.
- A changed application stops tools until a repair is reviewed and published; nothing self-heals mid-run.
- The quality of drift detection and the repair loop decides how much downtime that causes.

## Alternatives considered

- Agent fallback on failure. Rejected: it breaks determinism and can silently mis-write.

## Revisit if

Never for write paths. A read-only, opt-in exploratory mode could be considered separately.

## Spec sections

- [03-design-principles.md](../spec/1-product/103-design-principles.md)
- [28-observability-audit-and-repair.md](../archive/spec-2026-11/28-observability-audit-and-repair.md)
