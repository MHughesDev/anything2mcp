# 0999. An answer that did its part and waits for a person is `ok`

**Status:** Accepted. Design decision, made by the worker who built package 5.6c, under the lead's brief. The owner had not read this record when it was written. It changes a public contract, the command output contract of [section 220](../spec/2-kernel/220-actions-and-the-authoring-interface.md) and the technical reference, in one field: `ok`.

**Date:** 2026-11-25

**Changes:** [`a2m-actions`](../architecture/crates/a2m-actions.md), whose dispatcher sets `ok`, and whose `draft.ready` gives `state` and says first that it succeeded; the command output contract in [the technical reference](../reference/technical-reference.md#command-output-contract) and section 220; the guidance file. It leaves every exit code, every exit class, the REST status (`202` for a person needed), and the MCP face's `isError` as they were.

## Context

In the second measurement of the first hour ([H-18's result](../hypotheses/H-18-result.md)), all three agents ran `ready`, every gate passed, and the answer said a person was now asked to publish. It said so with `ok: false` and exit code 3. Each agent first took it for a failure.

The dispatcher set `ok` from the exit class alone: `true` only for `succeeded`. So every answer that rests on a person read `ok: false`, whether it carried an error or not: a publish asked for, a write resting at its approval, a capture or a watch waiting for its person, a promote of a write that is a person's act. The output contract named two shapes, success with `ok: true` and a result, and failure with `ok: false` and an error, and these answers were neither: `ok: false` with a result and no error. The MCP face already rendered them as no error (`isError: false`, the result as structured content), so the two faces disagreed about one answer.

## Decision

1. **`ok` is true when the answer carries no error,** and the action did its part: exit class `succeeded`, or `person_needed` with a result and no error. Exit code 3 still says a person is needed, and the result's `state` says on what (`needs_person`, `needs_approval`, `needs_login`, and the rest).
2. **A refusal for want of a person stays `ok: false`,** with its error and exit code 3: a call refused because a session must be signed in to, or a person's proof of presence is missing, did nothing.
3. **Every `needs_person` answer follows the one rule.** No action chooses its own `ok`; the dispatcher sets it from the error and the exit class, so `publish.request`, `draft.ready`, `binding.promote` of a write, `tool.call` resting at its approval, `watch.create`, and `capture.start` answer alike. A write resting at its approval is `ok: true` too: the call did its part, recording the intent and asking the person, and its result says, as before, that nothing was written.
4. **`draft.ready` says it first.** Its result gains `state`, the publish's own (`needs_person`, `published`, or `not_asked`), and when it waits for a person its text begins `Succeeded:`, then names the tools the version holds.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep `ok: false` for `draft.ready` and change only its text's first words | The agents read `ok` before any text. And the same shape would stay on every other answer that waits for a person, where the two faces disagree |
| `ok: true` for `draft.ready` alone, chosen by its handler | One action would answer a person needed one way and the rest another; an agent could not learn the rule once |
| A third value of `ok`, or a field `pending` | Every client of the contract reads `ok` as a boolean; `state` and the exit code already say what waits |
| `ok: false` for a write resting at its approval, since nothing was written | The call did what a call of a write does before the person: recorded and asked. Its result says nothing was written, and the exit code says a person is needed; the agent's next step, telling the person and stopping, is the same either way |

## Proof

`second_measurement::the_faults_of_the_second_measurement_are_met_no_more_through_the_commands` in the product's tests: `ready` of one read, with the gates passed, answers `ok: true`, exit code 3, no error, `state: needs_person`, and a text that starts `Succeeded:`. The unit tests of the dispatcher's faces hold the rest.

## Revisit if

A client is found that takes `ok: true` with exit code 3 for a finished write, or the MCP protocol gains a result kind for an answer that waits.
