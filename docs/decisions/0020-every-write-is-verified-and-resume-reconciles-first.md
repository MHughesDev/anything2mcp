# 0020. Every write is verified, and resume reconciles first

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

Applications offer no transactions to an outside caller. A crash between sending a write and recording it is unavoidable.

## Decision

A write counts only after the value is read back from the application and matches after normalization. The ledger records intent before acting. On resume, the engine reads the record's current state by its key and writes only if it does not already match. Writes are keyed upserts wherever a key exists; a create with no natural key is classed irreversible.

## Consequences

- Rerunning or resuming never duplicates a record that has a key.
- Every write costs a read-back, minimized by reusing the save response where it carries stored values.
- Field types must define exact normalization for comparison.

## Alternatives considered

- Trust the response status. Rejected: silent wrong data.

## Revisit if

Never.

## Spec sections

- [10-runs-ledger-and-resume.md](../archive/spec-2026-11/10-runs-ledger-and-resume.md)
- [08-execution-engine.md](../spec/2-kernel/207-the-engine-loop.md)
- [12-data-sources-and-value-normalization.md](../spec/2-kernel/210-sources-and-value-normalization.md)
