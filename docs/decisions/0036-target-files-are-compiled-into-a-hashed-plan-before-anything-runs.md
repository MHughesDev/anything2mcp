# 0036. Target files are compiled into a hashed plan before anything runs

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

ADR 0014 chose declarative files and said they compile at publish time, without saying how. The ledger needs to know how many saves an operation makes, and policy needs each operation's effect and scope before a call.

## Decision

A compiler in `w2m-files` turns a target's files into one plan through fixed stages: parse, validate, resolve, type-check, analyze, lint, lower, describe, emit. It counts commit points, derives effect classes that authors can raise but not lower, and derives each operation's permission manifest. The same files and compiler version give a byte-identical plan. The engine runs only plans and depends only on the plan types in `w2m-core`. Source files are stored beside every published plan.

## Consequences

- Every file mistake is reported before publishing, with a place and a fix.
- The plan format is versioned; the engine loads the current and previous formats.
- Lint rules have stable codes and tests.

## Alternatives considered

- Interpret YAML on each call: slower, and errors surface during runs.

## Revisit if

A target needs behavior that cannot be decided at compile time.

## Spec sections

- [46-target-compiler-and-plan.md](../spec/2-kernel/206-the-compiler-and-the-plan.md)
- [07-files-and-formats.md](../spec/2-kernel/205-target-files.md)
