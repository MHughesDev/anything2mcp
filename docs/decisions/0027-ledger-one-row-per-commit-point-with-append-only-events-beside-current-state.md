# 0027. Ledger: one row per commit point, with append-only events beside current state

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

ADR 0015 chose a built-in ledger and ADR 0020 its guarantees, without a schema. Resume is exact only if every ledger row is either saved in the application or not.

## Decision

A ledger row corresponds to one commit point in the application: one save. Every change appends an event and updates the current-state row in one transaction, and the state can be rebuilt from events. Nine rules are enforced as tests. Resume reads each doubtful record and has three outcomes: landed, did not land, or changed by something else, which stops the run. Runners may store a small window of intents ahead to save round trips.

## Consequences

- An operation with two saves is two ledger rows, which the file linter must detect or the author declare.
- A record changed by someone else since the preview stops with `changed_since_preview` and needs a new approval.
- Values seen before each write are kept, which enables undo and costs storage.
- Durable-execution engines give no stronger guarantee for outside writes, per research note 13.

## Alternatives considered

- A step log keyed by step name only: simpler, but cannot tell a landed write from a lost one.
- Event sourcing without a current-state table: slower queries for no added safety.

## Revisit if

Phase 3 tests show a rule cannot be enforced on SQLite or Postgres as designed.

## Spec sections

- [42-run-ledger-design.md](../archive/spec-2026-11/42-run-ledger-design.md)
- [10-runs-ledger-and-resume.md](../archive/spec-2026-11/10-runs-ledger-and-resume.md)
