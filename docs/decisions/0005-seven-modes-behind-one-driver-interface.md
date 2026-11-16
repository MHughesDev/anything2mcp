# 0005. Seven modes behind one driver interface

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

Applications can be reached through their interface, their network traffic, a published API, tools they register, their own MCP server, or a desktop window. No single route fits every app.

## Decision

Seven drivers implement one interface: `ui`, `page_http`, `http`, `api`, `site_tools`, `mcp`, and `desktop`. Operations, pipelines, the ledger, and policy are the same in every mode.

## Consequences

- An operation is defined once and gains bindings per mode.
- The driver interface is public, so new modes can be added as plug-ins.
- Seven drivers is a large surface; they land across Phases 1, 4, and 8.

## Alternatives considered

- Browser only. Rejected: slow for bulk work and blind to apps with APIs or desktop clients.

## Revisit if

A driver proves unreliable enough to drop from the first release.

## Spec sections

- [06-modes-and-drivers.md](../spec/06-modes-and-drivers.md)
