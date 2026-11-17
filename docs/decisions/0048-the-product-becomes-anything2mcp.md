# 0048. The product becomes Anything2MCP, a universal runtime built on the same core

**Status:** Accepted. Owner decision. The direction is the owner's; the architecture that carries it out is proposed in the [long-term plan](../archive/plans-2026-11/anything2mcp-master-plan.md) and is not accepted in detail by this record.

**Date:** 2026-11-16

**Supersedes:** [0001](0001-name-web2mcp.md), the name.

## Context

Web2MCP was designed to wrap applications. The owner first considered a second product, to be built later, that would take all of Web2MCP and extend it to every kind of software, and wanted the separation prepared now so that nothing would have to be undone. The owner then decided to change the present system itself instead, and told the building agent to design it. The owner also asked for the REA project to be studied as a possible source of capabilities.

## Decision

- The product is Anything2MCP: it turns software and systems into safe, deterministic, agent-accessible capabilities.
- It is a universal runtime with four jobs: discover and understand, integrate and execute, verify, and expose. MCP is its first interface and not its limit.
- Rust core, Python-first distribution, MCP-native interfaces.
- It is modular: a neutral kernel, and separate parts for each kind of system. What was Web2MCP is kept and becomes the first of those parts.
- Anything2MCP owns its abstractions. Outside projects supply capabilities through them and are never required.
- The existing design, its rules, and its decisions stand unless a later record changes one.

## Consequences

- The name changes everywhere: documents, crates, the binary, commands, packages. The plan's Stage A does this.
- [Decision 0022](0022-published-package-names.md), the published package names, stands until the owner confirms new names. The plan proposes them.
- A boundary between kernel and packs has to be drawn and enforced before more code is written.
- The first usable product is unchanged in substance: reads and writes on one machine through the web part.

## Alternatives considered

- Keep Web2MCP as it is and build the universal product later, in a second repository that extends it: two release trains, and a migration once people depend on the first. The owner considered this and chose otherwise.
- Wrap an existing reverse-engineering project and call the result universal: it would discover and never verifiably execute.

## Revisit if

The first second kind of system cannot be added without reshaping the kernel. The plan's Stage D tests exactly that.

## Spec sections

- [01-summary.md](../archive/spec-2026-11/01-summary.md)
- [29-repository-crates-and-packaging.md](../archive/spec-2026-11/29-repository-crates-and-packaging.md)
- [The long-term plan](../archive/plans-2026-11/anything2mcp-master-plan.md)
