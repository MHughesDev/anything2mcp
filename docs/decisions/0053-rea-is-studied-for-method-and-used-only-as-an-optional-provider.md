# 0053. REA is studied for its method, and used only as an optional provider

**Status:** Accepted by the agent, under the owner's instruction of 2026-11-17 to design the whole system. The owner had not read this record when it was merged.

**Date:** 2026-11-17

## Context

The owner asked for the REA project to be researched heavily as a reference for controlling subsystems and for laying tools over an existing application, and said it need not be a dependency. It was read from source, in five studies, at one commit ([research note 16](../research/16-rea-methods.md)).

## Decision

- No REA code enters this repository, and nothing depends on it.
- Its methods are adopted where they fit, designed afresh in our terms: declared capability apart from observed availability; no silent fallback between providers; the private launch handshake; one framing; every failure saying whether the request ran; ownership proven before stopping a process; bridges inside a host; components identified by digest; a tagged subject for every finding; level apart from source; coverage apart from content; described against observed; an unknowns ledger; scenarios with named steps; a check that must be able to fail; hints derived only from declared effects; and its doctrine for tools.
- Where REA and this design differ on purpose, this design holds: effects are enforced; every write is verified; few tools are listed; an agent cannot raise a protected limit; missing fields take the weakest value.
- REA itself may be attached as an optional discovery provider, as an upstream MCP server behind the `mcp` mode, with a mapping file naming the tools exposed. It is off by default, used at build time only, can only read, and its findings are never above `discovered`.

## Consequences

- Spec sections 56 to 61, 63, and 64 carry the adopted ideas, each credited in the research note.
- The optional provider is work package E3 of the long-term plan, and hypothesis H-27 decides whether it is worth its requirements.
- Nothing breaks if REA changes or disappears.

## Alternatives considered

- Vendor or port its code: its value is the engines behind it and its discipline, neither of which is code a Rust core could reuse.
- Wrap all of it as tools: roughly two hundred tools would bury an agent's list, and none of them verifies a write.
- Ignore it: its treatment of evidence, of process ownership, and of tool design is better worked out than anything else found.

## Revisit if

H-27 is refuted, or REA's licence or direction changes.

## Spec sections

- [57-supervising-subsystems.md](../spec/2-kernel/204-supervising-subsystems.md)
- [58-discovery-and-findings.md](../spec/2-kernel/218-discovery-and-findings.md)
- [64-rules-for-tools-and-capability-discovery.md](../archive/spec-2026-11/64-rules-for-tools-and-capability-discovery.md)
