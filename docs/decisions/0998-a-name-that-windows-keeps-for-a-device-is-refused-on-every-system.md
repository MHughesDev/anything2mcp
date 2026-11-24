# 0998. A name that Windows keeps for a device is refused on every system

**Status:** Accepted. Design decision, made in the design review round by lens 13, the other operating systems ([report](../reviews/other-operating-systems-review-2026-11-24.md), finding 2). The owner had not read this record when it was written. It narrows the registered name's shape in `a2m-core`, which every kind of name uses. No public type changes. Built by roadmap package OS2.

**Date:** 2026-11-24

**Changes:** `a2m-core` (`src/name.rs`); the glossary's and the file formats reference's statement of a name's shape, once OS2 is built.

## Context

A registered name is `[a-z][a-z0-9_]*`, at most 32 or 64 characters (`is_name` in `a2m-core`). Names become parts of paths: a browser's data folder is `profiles/<target>/<environment>/<kind>-<digest>` (`ProfileKey::place`), a client's file is `<name>.toml`, and a project keeps its target under the target's name.

Windows keeps ten names, and the same names with any extension, for devices: `con`, `prn`, `aux`, `nul`, `com0` to `com9`, and `lpt0` to `lpt9`, in any letter case. On Windows 10, which the first release supports (section 503 names Windows 10 and 11 for the presence prompt), a path whose last part is one of them names the device, not a file or a folder, whatever folder it is in; Windows 11 narrowed this for paths with a folder before the name and kept it for others. Every one of these names has the registered shape. A target called `con`, or an environment called `aux`, compiles and runs on Linux and macOS, and on Windows its browser's data folder cannot be made, or is made as something no other program can open. Nothing in the design says so; the stand and the `example` target never meet it.

## Decision

1. **A registered name that is one of Windows's device names is not a name,** on every system: `is_name` and the name types of `a2m-core` refuse `con`, `prn`, `aux`, `nul`, and `com` or `lpt` followed by one digit, with `InvalidName` saying that Windows keeps the name for a device. A dotted name is refused when either part is one.
2. **On every system, not only on Windows.** A project is a folder of text a person may carry from one machine to another, and a name that compiles on one system and fails on another is the kind of difference rule 7 forbids being found late.
3. **Nothing else about the shape changes.** Names are already lowercase, so two names that differ only in letter case, which Windows and macOS keep in one folder, cannot both exist.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Refuse the names on Windows alone | The project then compiles on Linux and fails on the person's other machine, with nothing said where it was written |
| Make every path part from a digest of the name | Ends the fault for paths and makes every folder unreadable to a person who opens the data folder; the profile already uses a digest for the identity, which can be any text, and keeps the target and environment readable on purpose |
| Write Windows's verbatim form of a path for these parts | Makes a folder that the person's own tools and the browser's cannot open or delete |

## Proof

Roadmap package OS2: a test in `crates/a2m-core/src/name.rs` that each of the device names, and `com0`, `lpt9`, and a dotted name with one as a part, is refused with the reason, while `console`, `nulls`, and `com10` are names; a target named `con` fails to compile on every system with the name's diagnostic.

## What would reopen it

A Windows version the first release supports that no longer keeps these names in any path.
