# 0044. A published version becomes current in stages

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

A target version that passes its gates can still behave differently in real use. Switching every caller to it at once makes a bad version an outage.

## Decision

A published version first becomes a candidate: reads are answered by the current version while the candidate runs in shadow and results are compared, and named callers can opt in. It becomes current after enough matching reads or on a publisher's say. Writes switch only then. Toolsets pin exact versions, and one action moves the pin back.

## Consequences

- Shadowed reads double the read load on the application for a while, within its rate budget.
- Write-only changes get no shadow evidence and rely on the gates and on opt-in callers.

## Alternatives considered

- Switch on publish: simple, and a bad version reaches everyone.

## Revisit if

Shadowed reads prove too costly for an application.

## Spec sections

- [52-when-the-engine-itself-fails.md](../spec/52-when-the-engine-itself-fails.md)
- [46-target-compiler-and-plan.md](../spec/46-target-compiler-and-plan.md)
