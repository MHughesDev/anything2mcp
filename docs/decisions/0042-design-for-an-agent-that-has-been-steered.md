# 0042. Design for an agent that has been steered

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

Agents read application text that can contain instructions. The calling agent supplies inputs, and the building agent writes target files.

## Decision

Request templates are typed by position so no input changes where a request goes. File inputs are blobs on a server and paths inside allowed roots on one machine. Downloads are returned as data. A draft cannot add reach beyond what a person registered. Publishing and importing show a review summary ordered by risk. Imported versions start with writes disabled. On one machine, settings that affect safety change only with the user-presence prompt.

## Consequences

- A manipulation test suite runs against every release.
- Some legitimate uses need a person to add a folder or a destination first.
- What an agent does with data it was allowed to read stays outside Web2MCP's control.

## Alternatives considered

- Rely on approvals alone: reads and file inputs would be unguarded.

## Revisit if

Never for the template and file rules.

## Spec sections

- [51-defending-against-a-manipulated-agent.md](../spec/51-defending-against-a-manipulated-agent.md)
- [19-security-model.md](../spec/19-security-model.md)
