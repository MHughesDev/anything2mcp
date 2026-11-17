# 0013. Writes go through the interface until a faster binding is proven

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

Applications validate input in their interface and sometimes compute values in the browser before saving. A direct request can skip both.

## Decision

By default a write uses the `ui` binding, so the application's own validation runs. A `page_http`, `http`, or `api` write binding is used only after shadow verification and promotion.

## Consequences

- First versions of write tools are slower.
- Data written through Web2MCP matches what the interface would have produced.

## Alternatives considered

- Direct HTTP writes from the start. Rejected as the default.

## Revisit if

A target has a documented, validated API; then `api` can be the first binding.

## Spec sections

- [06-modes-and-drivers.md](../archive/spec-2026-11/06-modes-and-drivers.md)
