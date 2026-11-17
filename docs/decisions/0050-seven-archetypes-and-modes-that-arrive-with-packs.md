# 0050. Seven interaction archetypes, and modes that arrive with packs

**Status:** Accepted by the agent, under the owner's instruction of 2026-11-17 to design the whole system. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Extends:** [0005](0005-seven-modes-behind-one-driver-interface.md). The seven modes it names are unchanged.

## Context

Decision 0005 fixed seven modes behind one driver interface. A universal runtime needs more ways to reach a system: commands, files, streams, devices, code inside a host. Adding modes one by one, each with its own special handling, would grow the engine without limit.

## Decision

- Every mode belongs to one of seven archetypes: interface, request, command, bridge, artifact, stream, device. An archetype fixes how a step is acted, confirmed, and read back.
- The seven modes of decision 0005 stay as they are: `ui` and `desktop` are interface modes; `page_http`, `http`, `api`, `site_tools`, and `mcp` are request modes.
- A further mode arrives only with a pack that clears the bar of spec section 55, and each gets a decision record.
- A proposed mode that fits no archetype is a reason to examine the kernel, not to add a special case.
- A write that cannot be read back, in any archetype, is `irreversible`.

## Consequences

- The driver interface must suit all seven archetypes. The sketch in the technical reference is to be checked against each when slice 1.1 settles it.
- Each new archetype adds a small set of step kinds, fixed with its first pack.
- "Seven modes" remains true of the original set and stops being a count of everything.

## Alternatives considered

- Modes with no grouping: simple to state, and every mode would be designed from nothing.
- Fewer archetypes, folding command into request and device into stream: the read-back and confirmation rules differ too much to share.

## Revisit if

A system of real interest fits none of the seven. That is hypothesis H-29.

## Spec sections

- [54-interaction-archetypes.md](../archive/spec-2026-11/54-interaction-archetypes.md)
- [62-catalog-of-surface-kinds.md](../archive/spec-2026-11/62-catalog-of-surface-kinds.md)
