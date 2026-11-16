# 0015. SQLite locally, Postgres on servers, and a built-in ledger

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

Runs must be resumable without duplicates, in a system small enough to run on a laptop.

## Decision

One schema on two back ends: SQLite for local profiles, Postgres for servers. The run ledger is a simple state machine in that store, not an external workflow engine.

## Consequences

- No extra service to install for the Solo profile.
- The ledger design has no dedicated research behind it yet and must be checked against durable-execution practice in Phase 3.

## Alternatives considered

- An external durable-execution engine. Rejected for footprint.

## Revisit if

The ledger cannot give the resume guarantees under concurrency.

## Spec sections

- [10-runs-ledger-and-resume.md](../spec/10-runs-ledger-and-resume.md)
