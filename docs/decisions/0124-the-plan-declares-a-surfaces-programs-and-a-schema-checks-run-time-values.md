# 0124. The plan declares a surface's programs, and a schema checks run-time values

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** three gaps the builder of the provider contract found ([decision 0089](0089-the-provider-contract-as-building-it-settled-it.md)). Changes the plan and the compiler's registration data, which are public contracts, and adds to `JsonSchema`.

## Context

The supervisor can be scoped to "the programs a published plan declares", and the engine had no place to read them: what a pack compiles about a surface is opaque to the kernel. An action declares its input with a `JsonSchema`, which could check a located YAML document and nothing a caller sends. The test pack `stand` kept its file kind and lowering private, so a crate above could not register one pack that has both a compiler side and a driver.

## Decision

**The plan declares a surface's programs.** `SurfacePlan::programs` is a list of `DeclaredProgram`: an absolute path as text and an optional digest. It is in the plan's bytes and under its hash, so a published version pins it. The kernel hands the list to the supervisor and reads nothing else in it. `Plan::check` refuses a path that is not absolute, so neither the compiler nor the engine holds a plan with one.

**The path is text, and "absolute" has one meaning on every machine.** `DeclaredProgram::is_absolute_path` accepts a path from the root, from a drive, or from a network share. The system's own test would call a drive path relative on Linux, and the same files would then compile on one machine and not on another.

**A pack says which fields name a program, as data.** `SurfaceKindSpec::programs` is a list of `ProgramField`: the key that holds the path and, where the kind has one, the key that holds the digest. Both must be fields the kind lists. The compiler copies what a surface states there into the plan. A path that is not absolute, a digest that is not a SHA-256 digest, and a digest with no program are refused under the new code `kernel.program_invalid`, at the value's place, with a fix.

**Where the path field is the address key,** as for a surface that is a program, the address an environment gives the surface is declared too, with the digest the surface states. A target that states a digest states it for the program wherever it is installed; a target whose environments run different builds states none on the surface, and its pack checks each build by its own rule.

**`ProgramRef` is made from a `DeclaredProgram`** by a `From` in `a2m-provider`. The engine passes `surface.programs.iter().map(ProgramRef::from).collect()` to `DriverHandle::supervisor`.

**One checker for a file, named values, and JSON.** `JsonSchema::check_values`, `check_value`, and `check_json` walk a run-time value by the code that walks a located document, so the rules and the words cannot drift apart. They return `Misfits`: each `Misfit` has the path of the value (`lines[2].quantity`), the code a file would get, a message, and a fix. `Misfits` converts to a `Failure` of class `input`, code `schema_invalid`. What differs at run time:

- No message quotes the value. A caller's input may hold personal data, and a failure goes to logs and traces.
- An open question is a drafter's mark in a file and means nothing in a value.
- A reference to a control, a field type, or a surface is taken as text. Only the compiler can resolve it.
- In JSON, a number with a fraction fits only where any value does, and a truth value is never one of a list of words. Both follow the JSON Schema that `to_json` publishes.
- A map's members are walked in order of name, so the order of the misfits is the same however the JSON library is built.

`JsonSchema::to_json` already gives one schema as a standard JSON Schema document; nothing was added for that.

**The test pack's parts are public** in the module `testing` of `a2m-files`: the surface kind spec, the control, the file kind, the lowering, the lint rule, and the drafting rule as values, and the names a manifest declares. Its surface kind takes an optional `helper` program, so a test can put a declared program in a plan.

**The plan format stays 1,** for the reason [decision 0121](0121-the-plan-carries-what-the-run-time-needs-from-target-files.md) gives: no release has carried a plan.

## Consequences

- Every construction of `SurfaceKindSpec` and of `SurfacePlan` names the new field. `SurfaceKindSpec::new` builds one that starts no program.
- The command pack registers its surface kind with its path key as a program field. Section 306 keeps a program's digests by platform in the `commands/` file, where the plan cannot hold them as one digest; the pack checks them itself and the plan pins the path.
- A pack whose manifest does not say `plan_programs` gains nothing: the supervisor ignores the plan's list for it.

## Alternatives considered

- **The engine decodes the surface's payload.** The kernel would then know one pack's shape.
- **A method on the file kind that returns the programs.** A surface kind is registered as data; a second route for one fact about it would split the knowledge.
- **The path as a file-system path type.** Its serialized form and its meaning of "absolute" differ by system, and the plan must not.
- **A second checker for run-time values.** Two sets of rules for one schema would disagree sooner or later.
- **Declaring an environment's address with no digest.** The surface's digest would then be dropped exactly where the program is another file.

## Revisit if

A kind of surface needs a program per environment with a digest of its own; a pack needs the plan to carry digests by platform; or an action's input needs a rule the schema subset cannot state.

## Spec sections

- [204. Supervising subsystems](../spec/2-kernel/204-supervising-subsystems.md)
- [206. The compiler and the plan](../spec/2-kernel/206-the-compiler-and-the-plan.md)
- [306. The command pack](../spec/3-packs/306-the-command-pack.md)
- [File formats](../reference/file-formats.md)
- [`a2m-core`](../architecture/crates/a2m-core.md), [`a2m-files`](../architecture/crates/a2m-files.md), [`a2m-provider`](../architecture/crates/a2m-provider.md)
