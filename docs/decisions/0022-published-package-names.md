# 0022. Published package names

**Status:** Superseded by [0056](0056-one-name-everywhere-anything2mcp.md) on 2026-11-17. Was: Accepted. Decided by the project owner. The owner chose the pip name; the crates.io and npm names follow from it.

**Date:** 2026-11-15

## Context

On 2026-11-15 the registries' own tools found nothing published as `web2mcp` on PyPI or crates.io, and found `web2mcp` taken on npm. `w2mcp` was free on all three. Another project's documentation tells readers to `pip install web2mcp`, so the name could be taken at any time.

## Decision

The pip package and Python import are `web2mcp`. The Rust crates are `web2mcp` for the binary and facade and `w2m-*` for the parts. The npm package is `w2mcp`. The command is `web2mcp` on every route. If PyPI or crates.io refuses `web2mcp`, the fallback is `w2mcp`.

## Consequences

- The names must be reserved with placeholder releases before any public announcement. This needs the owner's registry accounts and is the first task of Phase 0.
- The npm name differs from the project name, so install instructions must show it explicitly.
- Individual `w2m-*` crate names are still to be checked.

## Alternatives considered

- `w2mcp` everywhere: one name on all registries, but not the project's name.
- A scoped npm name: needs an npm organization, which was not checked.

## Revisit if

A registry refuses a name, or the same name becomes available on npm.

## Spec sections

- [41-packaging-and-release.md](../archive/spec-2026-11/41-packaging-and-release.md)
