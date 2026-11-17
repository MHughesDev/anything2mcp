# 0041. Identity strategies make creates and dependent records resumable

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

ADR 0020 made keyed upserts the basis of safe resume and classed anything without a key as irreversible. Many applications assign identifiers themselves, and the commonest pipeline, a header followed by its lines, needs the header's new identifier.

## Decision

Every write operation declares how its record is found again: natural, composite, stamped, find, or none. An identifier the application assigns is stored in the ledger as soon as it is known. A lookup that matches more than one record stops the run. Each record's result is stored so later steps use it without repeating the earlier step. Create or update is decided by reading during the preview and is part of the approved diff.

## Consequences

- Creates become resumable wherever a stamp field or a specific enough lookup exists.
- The compiler must check the strategy in every binding and reject dependency cycles.
- Operations with no way to find their record stay irreversible.

## Alternatives considered

- Treat every create as irreversible: safe, and useless for data entry.
- Guess the newest matching record: fast, and wrong when two people work at once.

## Revisit if

A common class of application defeats all four strategies.

## Spec sections

- [50-record-identity-and-dependent-records.md](../spec/2-kernel/209-record-identity-and-dependent-records.md)
- [42-run-ledger-design.md](../archive/spec-2026-11/42-run-ledger-design.md)
