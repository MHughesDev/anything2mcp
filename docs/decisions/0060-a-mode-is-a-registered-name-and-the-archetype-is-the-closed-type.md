# 0060. A mode is a registered name, and the archetype is the closed type

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Refines:** [0050](0050-seven-archetypes-and-modes-that-arrive-with-packs.md) and the convention in AGENTS.md that modes are an enum.

## Context

AGENTS.md said that modes, like every other fixed word, are an enum in the core types. That was written when there were seven modes and no packs. Since decision 0050, modes arrive with packs, and a plug-in can bring a driver for a kind of system this repository has never seen. An enum of modes in the kernel would mean that every new pack changes a kernel crate, and that a plug-in could never add a mode at all. Hypothesis H-26 counts exactly such changes.

## Decision

- In the kernel, a mode is a checked name: lowercase letters, digits, and underscores, starting with a letter, at most 32 characters. The type is `Mode` in `a2m-core`.
- The kernel does not hold the list of modes. A provider registers each mode it brings, with its archetype, through its manifest. A binding that names a mode nobody registered does not compile: unknown means stop.
- The archetype is the closed type. `Archetype` is an enum of seven values, and the engine's handling differs by archetype and never by mode.
- Every other fixed word stays an enum: effect classes, hazards, failure classes, evidence levels, coverage, and the states of a run, a record, a session, and a binding.
- Failure codes are checked names as well. The classes are closed; each crate names the codes it can produce, and a code never changes once a release has carried it.
- A first-party pack's new mode still needs a decision record, as 0050 says. A plug-in's mode needs its manifest approved by an administrator, as [section 56](../spec/56-the-provider-contract.md) says.

## Consequences

- Adding the command pack's `cli` mode should change no kernel crate. That is what H-26 will measure.
- The kernel cannot check a mode's spelling at compile time. The check moves to load time, where the registry is known; a misspelled mode is an `input` failure with the list of registered modes.
- Kernel code cannot branch on a particular mode, because it cannot name one. That is the point.

## Alternatives considered

- An enum of every first-party mode in the kernel, as documented: checked by the Rust compiler, and it couples the kernel to every pack and shuts plug-ins out.
- An enum with a catch-all value holding a string: two ways to write the same mode, and code that treats the listed ones specially.

## Revisit if

Load-time checking of modes lets a class of mistake through that the Rust compiler would have caught.

## Spec sections

- [54-interaction-archetypes.md](../spec/54-interaction-archetypes.md)
- [55-kernel-and-packs.md](../spec/55-kernel-and-packs.md)
- [The technical reference](../reference/technical-reference.md#core-types)
