# 0019. Priority order: safe, correct, fast, easy

**Status:** Accepted. Decided by the project owner. The ordering among the four was set in the system design.

**Date:** 2026-11-15

## Context

The owner asked for maximum breadth and depth, ease of use, and speed as a core decision metric, in a system that writes to business applications.

## Decision

When goals conflict, the order is safe, then correct, then fast, then easy. Speed has budgets enforced in CI. Every feature must work with its defaults before it offers a setting.

## Consequences

- A faster path is never taken before it is proven correct.
- A change that slows a budgeted path by more than 10% fails the build.
- Budgets are targets until measured in Phase 1.

## Alternatives considered

- Speed first. Rejected for a system that writes.

## Revisit if

Measured budgets turn out unrealistic; revise the numbers, not the order.

## Spec sections

- [03-design-principles.md](../spec/03-design-principles.md)
- [09-performance.md](../spec/09-performance.md)
