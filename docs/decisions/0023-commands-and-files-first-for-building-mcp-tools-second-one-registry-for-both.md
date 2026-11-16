# 0023. Commands and files first for building, MCP tools second, one registry for both

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

A coding agent needs an interface for wrapping an application. Tool definitions listed over MCP cost context on every turn; commands cost nothing until used. Agents without a shell can only use MCP.

## Decision

Commands plus target files are the primary building interface. MCP authoring tools are the second door. Every action is defined once in an action registry, and its command, MCP tool, REST route, and SDK method are generated from that definition. SDKs and REST are for software that uses finished tools, not for building.

## Consequences

- A new crate, `w2m-actions`, and a CI check that no face is hand-written.
- The command line needs a strict machine-readable output contract.
- Large results are written to files or returned as resource links, never inline.
- The build toolset over MCP lists at most 15 tools; the rest are reached through search.

## Alternatives considered

- MCP tools only: works everywhere, but costs context and suits shell agents less.
- A Python API for defining tools: replaces reviewable files with code.

## Revisit if

Measured agent runs show the MCP face succeeding more often or faster than commands.

## Spec sections

- [40-authoring-interface.md](../spec/40-authoring-interface.md)
- [26-agent-toolkit-how-tools-get-built.md](../spec/26-agent-toolkit-how-tools-get-built.md)
