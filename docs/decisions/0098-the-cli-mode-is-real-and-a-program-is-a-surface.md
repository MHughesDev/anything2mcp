# 0098. The `cli` mode is real, and a program is a surface

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Carries out:** item 8 of the bar in [spec section 202](../spec/2-kernel/202-kernel-and-packs.md) for the mode `cli`, which [section 306](../spec/3-packs/306-the-command-pack.md) proposed, and package D3 of the [roadmap](../plan/roadmap.md).

## Decision

The mode `cli` exists. It belongs to the command archetype, the pack `command` brings it, and the crate `a2m-pack-command` drives it. It has passed the bar of section 202 as far as one pack can pass it before the engine loop runs it:

| Bar item | How it is met |
| --- | --- |
| 1. A driver with no retries inside | One start of a program for one `spawn` step. The test stand's record of every start shows it |
| 2. A read-back rule for every write | A write binding names a `read` command; a binding without one does not compile, by the kernel's own rule. A command with no read is `irreversible` |
| 3. An answer for each identity strategy | `natural` and `composite`: the key's parts are arguments. `stamped`: the command and its lookup both carry `{flag: ..., stamp: true}`, checked at compile time. `find`: a `lookup` command that must match one record. `none`: `irreversible` |
| 4. A test stand with a control interface | `targets/example-cli` |
| 5. A discoverer whose findings claim no more than was observed | `help`: a command is `discovered` as listed, with `not_exercised`; nothing is run but the declared help and version forms |
| 6. Failures mapped to the existing classes | The table in the [crate's design](../architecture/crates/a2m-pack-command.md). No class was added |
| 7. Effects and hazards declared | The manifest: programs of a published plan only, one write root, no network |
| 8. A decision record | This one |

**A program is a surface.** The surface kind `program` has the address key `path`, the program's absolute path, and an optional `digest`. The pack declares both as the surface's program, so the plan lists the program at its path in every environment, and the supervisor starts nothing else for the pack ([decision 0124](0124-the-plan-declares-a-surfaces-programs-and-a-schema-checks-run-time-values.md)).

*Instead of:* naming the program in the `commands/` file and leaving the surface an empty name. The path differs by environment, and environments override a surface's address, so the path is the address.

**The digests of each platform stay in `commands/`.** The plan holds one optional digest for a program. A target that runs on three systems lists a build, or several, for each, under `program.digests`. The pack takes the file's digest in `prepare`, refuses a file that is not listed for the platform it runs on, and hands the digest it matched to the supervisor with every start, which checks it again at the moment of starting. The surface's own `digest` is for a target whose program is one file everywhere; the supervisor then holds it without the pack.

## What is not yet shown

The mode has not run inside the engine loop: its tests drive the driver through the handle. Step 4 of section 306's build order, a read through the engine with one trace signature, is the engine's package to prove. On Windows and macOS the tests have not run.

## Consequences

- Section 306 no longer calls the mode proposed, and its failure codes are no longer marked as proposals.
- The count for hypothesis H-26 is in [decision 0099](0099-the-command-pack-as-building-it-settled-it.md).
