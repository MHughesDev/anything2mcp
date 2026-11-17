# 0014. Declarative files: YAML targets, TOML settings, CEL expressions

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

Tools must be reviewable by people, writable by any coding agent, lintable, and diffable.

## Decision

Everything known about a target lives in YAML files validated against published JSON Schemas. System settings are TOML. Expressions use CEL, which always terminates and never converts types silently. Conversions are tested host functions. There is no general scripting in YAML; anything needing code is a reviewed plug-in.

## Consequences

- Files compile to a binary plan at publish time, so nothing is parsed at call time.
- Some logic will be awkward to express; the answer is a host function or plug-in, not scripting.

## Alternatives considered

- Tools defined in code. Rejected: harder to review, lint, and sandbox.

## Revisit if

Real targets repeatedly need logic the format cannot express.

## Spec sections

- [07-files-and-formats.md](../spec/2-kernel/205-target-files.md)
