# 0062. Words that arrive with packs are registered names, and packs share data and never code

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Extends:** [0060](0060-a-mode-is-a-registered-name-and-the-archetype-is-the-closed-type.md), from modes to every word a pack brings.

## Context

Designing five further packs on paper, and a design for each of the 24 crates, found the same problem three times. The file formats list four surface kinds as a closed list, and the command, data, file, stream, and device packs each need one more. Adding a value to a closed list in the kernel for every pack is exactly what hypothesis H-26 counts as a failure.

A second problem appeared between packs. The `page_http` mode of the web pack carries out requests described in `endpoints/`, a file kind the HTTP pack owns. The web pack's probe produces endpoint findings that the HTTP pack's drafting rule reads. Screen models are used by the web pack and the desktop pack. A pack may not depend on another pack's crates, so none of this can be shared as Rust types.

## Decision

- **A word whose values arrive with packs is a registered name.** It is checked for shape in `a2m-core` and registered by the provider that brings it. This holds for a mode, a surface kind, a step kind, a file kind, a lint code, a finding kind, and a control name. A failure code is a checked name that each crate defines for itself.
- **The kernel's enums are the words that are the same for every kind of system:** archetypes, effect classes, hazards, failure classes, evidence levels, coverage, and the states of a run, a record, a session, a binding, a provider, and a subsystem.
- **A step kind is a name within its mode.** Two modes may both have a step named `request` or `locate`; the registry keys a step kind by its mode, so no pack has to prefix its names and none can take a name from another.
- **An unregistered name does not compile.** The check moves from the Rust compiler to the target compiler, where the registry is known.
- **Packs share data shapes and never code.** A file kind or a finding kind has one owning pack, which registers its schema. Registration is keyed by the kind and the surface kind, so two packs can each register `screens/` for their own surfaces. Another pack may declare that it reads a kind; the compiler then hands its lowering the validated entries as data. If the owning pack is not enabled, a binding that needs its kind does not compile.
- **There is no kernel type for a request, a screen, a statement, or a register.** Each pack compiles what it reads into its own payload inside the plan, which the kernel stores, hashes, and never decodes.
- **`site_tools` is a mode of the web pack.** It needs a tab that can act, and the MCP pack cannot borrow one.

## Consequences

- The file formats' list of surface kinds becomes the list the first four packs register.
- A pack's manifest gains what it reads from other packs, beside what it registers.
- "Endpoints only in the endpoint catalog" still holds: there is one catalog, owned by one pack, and read by two.
- The technical reference's table of verbs by mode gains a column per pack, kept in each pack's design.

## Alternatives considered

- Closed lists in the kernel, extended by decision record for each pack: checked by the Rust compiler, and every pack changes the kernel.
- A kernel type for requests, since the request archetype is the kernel's: one shared type, and the kernel would know HTTP.
- A shared crate between packs for what they have in common: forbidden by the layering, and it would grow into a second kernel.

## Revisit if

Checking names at load time lets through a class of mistake the Rust compiler would have caught, or two packs need to share behavior and not only data.

## Spec sections

- [54-interaction-archetypes.md](../archive/spec-2026-11/54-interaction-archetypes.md)
- [55-kernel-and-packs.md](../spec/2-kernel/202-kernel-and-packs.md)
- [The architecture](../architecture/README.md)
