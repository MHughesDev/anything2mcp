# 0457. A publish of named tools compiles only their files

**Status:** Accepted. Design decision, made by the worker who built package 5.6c, under the lead's brief. The owner had not read this record when it was written. It changes the public inputs of `publish.request`, `version.publish`, and `test.run` by addition, and what `draft.ready` with `tools` publishes.

**Date:** 2026-11-25

**Changes:** [`a2m-actions`](../architecture/crates/a2m-actions.md): `chosen_sources` and `compile_chosen_in` in `handlers/build.rs`, the publish gates' check of the files tested in `offline.rs`, the person's act of a publish in `acts.rs`, and `draft.ready`. It leaves the compiler, the loader, the compiled target, and the holding folder as they were.

## Context

In the second measurement of the first hour ([H-18's result](../hypotheses/H-18-result.md)), two of three agents wanted one read and got a version of three: `ready` publishes every read it proved, and `ready --tools` named the reads to make ready but still asked for a publish of every draft, so the gates failed on the others. Deleting the other drafts first was the only way to publish fewer, and it throws away proven work.

## Decision

1. **`publish.request`, `version.publish`, and `test.run` take `tools`,** a list of drafted tools by name. With it, the drafts are loaded whole and compiled without each operation and pipeline file that defines no tool named, each test of a tool not named, and each fixture that only those tests name. Every other file, the target, its surfaces, screens, endpoints, and field types, is compiled whole, so what a named tool needs is there as the drafts hold it. A screen only a tool not named used is compiled with the rest and reached by no tool.
2. **A file is never cut.** A file that defines a tool named and one not named refuses the call, `input`/`schema_invalid`, naming both; a name no operation or pipeline file defines is `input`/`not_found`. A named pipeline whose operation is not named fails to compile, as any unresolved name does.
3. **The same input from the ask to the press.** The person's page shows the compiled target of the tools named; the press compiles it again from the same input, and the hash pinned on the page holds them together, as it does for a whole publish. The gates check that the bytes tested are the bytes compiled by loading the files again and taking the same files.
4. **`draft.ready` with `tools` publishes only those.** It runs `test.run` and `publish.request` with the same `tools`, and its `publish` gives `tools`, the tools the version holds; with no `tools`, every drafted tool. The other drafts stay in the folder, untouched, and a later publish without `tools` holds them, compared with the version in force as any publish is (decision 0454).

## Alternatives

| Alternative | Why not |
| --- | --- |
| Move the drafts not named into the holding folder | The holding folder is for drafts waiting on an open question, each listed with its question (decision 0298); these have none, and the person would have to move them back |
| Delete the drafts not named, as `next` already offered | It throws away proven reads and their trials; the agent asked to publish fewer, not to remove |
| Drop the tools not named from the compiled target after compiling | The compiled target is indexed throughout, tests by tool and steps by operation; cutting it is the compiler's work, and the publish page would show what the files do not say |
| Compile a subset in the compiler, with a new mode | The loader already gives files to choose from, and `Sources::from_files` makes the choice a set of sources like any other; nothing in the compiler needs to know |

## Proof

`second_measurement::the_faults_of_the_second_measurement_are_met_no_more_through_the_commands`: from the web scout of the test application, which proves three reads, `ready --tools example_orders_get` asks for a version that holds that read alone, the person's press publishes it, `tool search` finds only it, and the other two drafts are still in the folder.

## Revisit if

A pack's files come to define a tool across kinds the selection does not know, or people want a published version to drop knowledge no tool of it uses.
