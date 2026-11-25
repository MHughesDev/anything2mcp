# 0999. The first hour's faults: a list on the command line, a key off a list, and a project held to one store

**Status:** Accepted. Design decision, made by the agent that built package 5.6b. The owner had not read this record when it was merged.

**Date:** 2026-11-25

**Changes:** how the command line reads a list flag; `init`, which makes the working folder `.anything2mcp/`; the scout's rule for the key of a read that states no example; which project folder an engine answers for, and the failure `input`/`project_of_another_store`; the guidance file's step 3; the crate designs of `a2m-actions` and `anything2mcp`.

## Context

Package 5.6a measured the first hour ([H-18's result](../hypotheses/H-18-result.md)). Three agents with no knowledge of the repository reached a published read in about two minutes each, and met the same faults on the way. Package 5.6b closes them. Four of the fixes have one obvious form: the `next` after `test.run` names `binding.promote` while the binding that would act is a draft; a promotion from a run the store no longer holds says so and names the `draft.try` to run again; the answer to a publish asked again after the person published says `published`, with its version, and that nothing waits; and the web scout looks at a page it was redirected to once, under the address it ended at, so one page seen twice is never taken for two pages that differ in a key. Three fixes chose between real designs, and are recorded here.

## Decision

1. **A list flag takes a JSON array as well as the repeated flag.** On the command line, `--files a --files b` stays the form. A value of a list flag that reads as a non-empty JSON array of text is taken as its items, in order, so `--files '["a", "b"]'` is the same list. Any other value is one item; a comma is part of an item, since a name may hold one. The help of every list flag names both forms, and the `next` line of a result writes a list as the repeated flag. `draft.delete` refuses a name that holds a comma or begins with `[` with a fix that says how a list is written.
2. **A key may be read off a list's column of the record's name.** A read that states no example of its one key was tried with the first item of the one list read, proven earlier in the same scout on the same surface, that answers an output of exactly the key's name. When no list answers one, the output of exactly that name without its `_id` counts as well: a key `order_id` from a list's `order`. Exactly one list must answer, under either rule; the value is kept only in the trial's own stored call, as before. A wrong key fails its trial and the read is reported tried and not proven: the trial only reads, in the sandbox.
3. **A project folder is held to one engine store.** The first call that names a project folder to an engine whose store is kept on disk writes the mark `.anything2mcp/engine` in the folder, naming the store: the workspace that store made at its first open. A later call that names the folder to an engine whose store the mark does not name is refused whole, `input`/`project_of_another_store`, whatever the action, with the fix: run with the engine home the project was worked from, or remove the mark to work the project from this engine from now on. The mark is written only in the working folder `.anything2mcp/`, which `init` now makes: the engine writes in no folder of a project it was not given, so a folder `init` did not make, such as a file stand's, is left unmarked and is not held. A mark that cannot be written leaves the folder unmarked and the call goes on. A store kept in memory names no folder, since it is gone when the engine closes. `init` already keeps `.anything2mcp/` out of version control, so each checkout names its own machine's store.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Only say the repeated form in the help and the refusal | The agents wrote a JSON array because a result's own `next` line wrote a list that way; refusing the form the product prints is a trap |
| Take a comma-separated string as a list | A file name may hold a comma; the refusal names the form instead |
| Let the web drafter state an example key from the pages it saw | It would write a value of the application into a drafted file, which the scout keeps out of files and reports; and the drafter's crate is not this package's |
| Match the key to any list output that resembles its name | Rule 7: no guess by resemblance. One fixed suffix rule, stated here, is not a guess, and a wrong key only fails a read in the sandbox |
| Let `--project` choose the engine home too | A store serves every project on the machine (decisions 0192 and 0194); a project choosing the store would let a project folder move a person's runs and vault |
| Warn when the project and the store differ, and go on | `tool.search` and `tool.call` then answer from a store that is not the project's, which is the contradiction the agents met |

## Proof

`crates/anything2mcp/tests/first_hour_faults.rs`, through the product's commands; both of its tests fail on the code before this package:
- `draft delete --help` names both list forms; a comma-separated string is refused with the form and nothing is removed; a JSON array removes every file it names;
- after `fixture record`, `test run` and `target status` name `binding.promote` of the read; after the promote, `test run` names `publish.request`;
- a promote naming a run this store does not hold is refused `promotion_unproven`, saying so, with the `draft.try` to run again;
- the publish asked for waits with `next` `publish.request`; after the person's press, the same request answers `state: published` with its version, no "did nothing more", and `next` `tool.search`;
- `tool search`, `tool call`, and `tool describe` with another engine home in the same project are each refused `project_of_another_store`;
- the web scout of the test application drafts its list read at `/orders`, never `/{key}`, and proves all three reads, the two by key on a key from the list's `order` column.

## Revisit if

- A person is seen to need one project worked from two engine homes.
- A key read off a list by the suffix rule is seen to be the key of another record.
