# 0001. Name: Web2MCP

**Status:** Superseded by [0048](0048-the-product-becomes-anything2mcp.md) on 2026-11-16. Was: Accepted. Decided by the project owner.

**Date:** 2026-11-15

## Context

Other projects already use this name or one close to it: a pip package named `web2mcp` that generates MCP servers from HTTP endpoints, a TypeScript project called Web2MCP, and the W3C community-group proposal WebMCP, in which sites register their own tools.

## Decision

The project is named Web2MCP. The binary is `web2mcp`, crates are prefixed `w2m-`, and built-in MCP tools are prefixed `w2m_`.

## Consequences

- The published package names are set in ADR 0022: the name is free on pip and crates.io as far as could be checked, and taken on npm.
- Documentation must explain how Web2MCP differs from the WebMCP proposal, which it uses as one mode.

## Alternatives considered

- A unique name. Rejected by the owner.

## Revisit if

A registry or trademark conflict blocks distribution.

## Spec sections

- [01-summary.md](../archive/spec-2026-11/01-summary.md)
- [35-prior-art.md](../spec/7-project/701-prior-art.md)
