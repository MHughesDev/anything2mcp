# 0485. An answer at a terminal names each step as its command, and every next leads the short way

**Status:** Accepted. Design decision, made by the worker who ran the design review round by lens 9, the first-time user, under the lead's brief. The owner had not read this record when it was merged. It changes no public contract: the JSON form of every answer, the action names in it, and the failure codes stay as they are; only the words of the text form, of `next`, and of a few answers change, and two answers of the engine's own commands.

**Date:** 2026-11-25

**Changes:** the text form of an answer in the command line (`anything2mcp`); the `next` and `fix` words of `a2m-actions`' handlers named below; `engine status`, `engine stop`, and `doctor`. Packages FU1 and FU2 of the roadmap build it. It refines [decision 0456](0456-the-first-release-leaves-out-what-its-first-hour-never-meets-and-a-shortcut-is-offered-only-when-built.md), point 7, which made the short way, `add` then `ready`, the first thing the guidance gives.

## Context

The [first-time user review](../reviews/first-time-user-review-2026-11-25.md) walked the README's "Try it from source" path on Linux as a person at a terminal, with the 0.1.0 build and the test application. Every command the README gives worked as written, and the stop at the person's press was said plainly on the page and by `doctor`. What lost a person was in the words between:

- The text a person reads names steps as actions (`scout.report` with `part`, `draft.delete` by `files`, `tool.search`, `test.run`), and the command line refuses those names: `anything2mcp tool.search` answers "There is no command `tool.search`". Only the `next` line gives a command.
- Three `next` lines lead three ways from one place. After `draft delete` it is `test run`; after `test run`, `publish request`; the README and the guidance say `ready`. `target status` gives `publish request` while a publish already waits for the person, and lists `tried` and `tested` as unknown after the scout tried and `ready` tested.
- The first `ready` of the path, whose only gap is the read the stand fails, answers with exit code 1 and an error line, "The target `example` is not published", after its `next`. A person takes that for the failure of the whole step.
- `init` gives no next step. The help of a shortcut shows `[argument]` where its command needs `<name>`.
- `engine status` starts an engine to answer, so it never says that none runs; it shows `runner: in_process: true`, which reads as if the engine ran inside the command. `engine stop` with no engine running answers `client_action`, "Run it as a command on the machine the engine runs on".
- `doctor` answers `state: ok` on a machine where its own `presence` check is `component_missing`, that is, where nothing can publish.
- A scout of an address that does not answer says the surface was `identified`, finds nothing, and says to "run the tool again" in a `Not looked at` line, without naming the command.

## Decision

1. **The text form names a step as its command.** Wherever an answer's words name an action a person would take next, in `fix`, `next`'s reason, a warning, or the report's text, the text form prints it as the command that takes it (`anything2mcp scout report example --part drafted.written`), as the `next` line already does. The JSON form keeps the action names; an agent reads those.
2. **One way on from each place.** `next` after `draft delete` and after `test run` names `draft ready` when a read the scout proved is still to be made ready or published; `target status` names `draft ready` before `publish request`, names the waiting act when one waits for its person, and counts a scout's trials and `ready`'s tests in its states. `tool call` of a name no published version has says, when a version of its target waits for its person, that it does and where.
3. **A ready whose only gap is a read left alone is not shown as a failure of the step.** Its text starts with what is ready and what is left alone, and gives the removal as the next step; the error line follows the result, not the `next`. Its exit code stays 1 and its JSON form is unchanged, because the publish was not asked for.
4. **Every first command says the next.** `init` gives `add` with the flags it needs. A shortcut's help shows its command's arguments by name.
5. **The engine's own commands say what is.** `engine status` answers from the engine when one runs and says that none runs when none does, starting nothing. `engine stop` with no engine running says so and succeeds. `doctor` answers `component_missing` overall when a check that a publish needs is missing, and prints no empty `fix`.
6. **A scout that met no answer says so first.** Its headline says the address did not answer, and its `next` is the scout again, `--fresh`, once the system answers.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Make the command line take dotted action names (`anything2mcp tool.search`) | Two spellings of every command, and help that must list both; the person still meets `with part` and `by files`, which are an action's inputs, not flags |
| Leave the words, and say in the README how to read them | The README already had to explain the first `ready`'s error; each new answer would need its own note, and an agent reading the text form meets the same names |
| Make the first `ready` remove the read left alone itself | It would delete a draft nobody chose to delete; the person or agent decides that, and `next` gives the call |

## Proof

FU1: a test in `crates/anything2mcp/tests/` that walks the README's path through the command face at a terminal (the text form) and holds that no answer names a dotted action, that each `next` on the path leads to `draft ready` or the call, that `target status` after a scout and a `ready` gives the right state and the waiting act, and that `init` gives `add`. FU2: a test that `engine status` with no engine running starts none and says so, that `engine stop` then succeeds, and that `doctor` with the presence check missing answers `component_missing`.

## Revisit if

An agent is found reading the text form and not the JSON form, and taking commands from it; then the text form is checked against the command tree by a test of its own.
