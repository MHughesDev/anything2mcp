# 0021. No real application is named in the repository

**Status:** Accepted. Decided by the project owner.

**Date:** 2026-11-15

## Context

Web2MCP is a general tool. Naming a real product as an example ties the project to that product, can expose where the owner or a contributor works, and can conflict with a vendor's terms.

## Decision

No real application is used as an example target anywhere in the repository. Examples, tests, fixtures, file names, and commit messages use an imaginary application called `example` or the built-in test application. Target files for real applications are kept outside the repository.

## Consequences

- Documentation examples are generic: orders and line items.
- Platform and framework families the engine must handle are still named in the interface-flavor sections, because that is technical coverage, not an example target.
- A place for private target files must be chosen before the first real target is built.

## Alternatives considered

- Named example targets. Rejected by the owner.

## Revisit if

A vendor agrees in writing to be a public reference target.

## Spec sections

- [34-first-target-archetypes.md](../archive/spec-2026-11/34-first-target-archetypes.md)
- [07-files-and-formats.md](../spec/2-kernel/205-target-files.md)
