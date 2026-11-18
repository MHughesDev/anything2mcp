# 0258. Every crate declares its failure codes in one table, with a meaning, a way out, and whether it passes by itself

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the [design of `a2m-core`](../architecture/crates/a2m-core.md) (its interface, its failures, and its build order), `tools/failure_codes.py`, and the generated table of failure codes in the [technical reference](../reference/technical-reference.md#failure-codes). The [designs of `a2m-expr`](../architecture/crates/a2m-expr.md) and [`a2m-sources`](../architecture/crates/a2m-sources.md), the first two crates to move to the form.

## Context

A failure carries a class, a code, a sentence, and a next step ([section 220](../spec/2-kernel/220-actions-and-the-authoring-interface.md), Failures). The next step is written only where a failure is made, so nothing could say what a code means, or what to do about it, without one in hand. Crates declared their codes in six forms: six copies of one private `codes!` macro, hand-written `ALL` lists, constants whose class was read from doc comments or from the calls that used them, and literals. `tools/failure_codes.py` read all of them as text. Nothing said whether a call that failed may pass if made again unchanged, and the class cannot say it: most `busy` codes, such as a run on hold or a store that is corrupt, never pass by themselves.

## Decision

1. **One form.** `a2m-core` exports one macro, `failure_codes!`, which every crate calls once, in a public module named `codes`. Each row is `NAME = Class, "code", meaning "...", next "..."`, then optionally `, passes by_itself` or `, passes not_by_itself`, then optionally `, then ("noun.verb", "what it is for")`. It writes the constants, with the meaning and the next step as their documentation; `ALL`, every class and code, which tests use; and `TABLE`, every row as a `CodeEntry`.
2. **Checked when the crate is compiled.** An empty meaning or next step, a code that is not a checked name, a `then` that names no action, and a `busy` row that does not say `passes` each stop the build. A class other than `busy` that does not say it takes its default: `transient` passes by itself, every other class does not.
3. **A row's next step is the general one.** It is an instruction, as section 220 asks. A failure made for a case may say more; one whose case adds nothing carries the row's, read from the crate's own table, so the sentence is written once.
4. **The book.** `CodeBook` reads the tables of many crates. It refuses one code declared with two classes, naming both crates; gives the row for a code (`explain`); and answers whether a code of a class passes by itself (`passes`), falling back to the class's default for a code it does not hold.
5. **`Failure` can carry a next action and a wait as data.** `with_next(action, args)` and `next()`; `with_retry_after(Duration)` and `retry_after_ms()`, in whole milliseconds rounded up, counted from when the failure was made. Each is written only when present, so a failure without them reads and writes as before. The text of what the system said and the two new parts sit behind one pointer, so a failure is no larger than it was and the errors that hold one stay under the lints' limit. Nothing reads them yet; the dispatcher's use of them is a later record's.
6. **The generator reads the form** and adds two columns to the reference's table: what to do, and whether the code passes by itself (empty for a crate not yet moved). A row it cannot read, an empty meaning or next step, and one code given two answers to whether it passes are faults. A list `MIGRATED` in the tool names the crates that have moved; such a crate that declares a code anywhere else is a fault, so a crate cannot slip back. `--list` names every crate still outside the form. The older forms are read until the last crate moves.
7. **First moved:** `a2m-core`, `a2m-expr`, and `a2m-sources`. `FailureCode::SCHEMA_INVALID` and the core's other associated constants are now the table's constants, so the generator counts each once.
8. **One code renamed.** `a2m-sources` gave `key_not_unique` the class `input`, for a file given with two rows of one key; the data and file packs give it `drift`, for a system that holds them. One code may have one class, and the class decides what happens next, so the source reader's code is now `source_key_not_unique`. No release had carried it.

## Alternatives weighed

- **Keep the next step at the call site only, and give the table a meaning alone.** Smaller. It leaves nothing to answer "what do I do about `queue_full`" without a failure in hand, which `failure.explain` and the dispatcher's fallback both need.
- **Two new fields held in place in `Failure`.** Simpler serialization. It makes a failure 16 bytes larger, enough to push an error that holds a failure and a list, such as a rejected source, over clippy's limit for a large error.
- **A procedural macro, or a build script that reads a table file.** Either can check more, such as that a `then` action exists. Either adds a crate and a build step to every crate. The action registry, which knows the actions, is the place to check a `then` against them.
- **Passing by itself decided by class.** Wrong for `busy`, as above. It is stated per row, and a class's default applies only to classes whose codes all answer alike.
