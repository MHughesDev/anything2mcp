# 0458. An answer that did its part and waits for a person is `ok`

**Status:** Accepted. Design decision, made by the worker who built package 5.6c, under the lead's brief, and narrowed by the lead before its merge: a run that rests on a person stays `ok: false`. The owner had not read this record when it was written. It changes a public contract, the command output contract of [section 220](../spec/2-kernel/220-actions-and-the-authoring-interface.md) and the technical reference, in one field: `ok`.

**Date:** 2026-11-25

**Changes:** [`a2m-actions`](../architecture/crates/a2m-actions.md), whose dispatcher sets `ok`, and whose `draft.ready` gives `state` and says first that it succeeded; the command output contract in [the technical reference](../reference/technical-reference.md#command-output-contract) and section 220; the guidance file. It leaves every exit code, every exit class, the REST status (`202` for a person needed), the MCP face's `isError`, and the answer of a run that rests on a person as they were.

## Context

In the second measurement of the first hour ([H-18's result](../hypotheses/H-18-result.md)), all three agents ran `ready`, every gate passed, and the answer said a person was now asked to publish. It said so with `ok: false` and exit code 3. Each agent first took it for a failure.

The dispatcher set `ok` from the exit class alone: `true` only for `succeeded`. So every answer that rests on a person read `ok: false`, whether it carried an error or not: a publish asked for, a write resting at its approval, a capture or a watch waiting for its person, a promote of a write that is a person's act. The output contract named two shapes, success with `ok: true` and a result, and failure with `ok: false` and an error, and these answers were neither: `ok: false` with a result and no error. The MCP face already rendered them as no error (`isError: false`, the result as structured content), so the two faces disagreed about one answer. They still disagree for a run that rests on a person, which the MCP face shows as no error with its state; there the command face's `ok` is the stricter of the two, on purpose.

## Decision

1. **One closed test.** An answer is `ok` when it carries no error, and either its exit class is `succeeded`, or its exit class is `person_needed` and it is not the answer of a run that rests on a person. Nothing else is `ok`. The dispatcher applies the test; no action chooses its own `ok`.
2. **`ok: true`, exit code 3, `state` saying what waits:** an authoring or admin action whose own part is done and whose remaining step is a person's act on a page. These are `draft.ready` and `publish.request` waiting for the publish, `binding.promote` of a write, `watch.create` and `capture.start` waiting for their person, and `scout.run` stopped for a sign-in before it drafted.
3. **`ok: false`, exit code 3, as before:** the answer of a run that rests on a person. That covers a `tool.call` whose write waits for its approval, and any run that waits for a sign-in or a person's answer, whether `tool.call`, `run.get`, `run.resume`, or `action.run` of one of them gives it. Nothing was written, and an agent that reads only `ok` must never take the write as done. Safety comes before ease (rule 2). The answer carries the run's result and no error, as before. The handler that renders a run (`run_output`) marks its answer as a run that rests (`Output::run_rests`), and `action.run` keeps the mark of the action it ran.
4. **`ok: false`, exit code 3, with its error:** a call refused for want of a person, such as a session that must be signed in to first. It did nothing.
5. **`draft.ready` says it first.** Its result gains `state`, the publish's own (`needs_person`, `published`, or `not_asked`), and when it waits for a person its text begins `Succeeded:`, then names the tools the version holds.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep `ok: false` for `draft.ready` and change only its text's first words | The agents read `ok` before any text |
| `ok: true` for every answer with no error, a waiting write's too | A waiting write has written nothing; an agent that reads only `ok` would take it as written. Safety comes before ease |
| `ok: true` for `draft.ready` alone, chosen by its handler | One authoring action would answer a person's act one way and the rest another; an agent could not learn the rule once |
| A third value of `ok`, or a field `pending` | Every client of the contract reads `ok` as a boolean; `state` and the exit code already say what waits |

## Proof

`second_measurement::the_faults_of_the_second_measurement_are_met_no_more_through_the_commands` in the product's tests: `ready` of one read, with the gates passed, answers `ok: true`, exit code 3, no error, `state: needs_person`, and a text that starts `Succeeded:`. A write that waits for its approval stays `ok: false`: through the product in `verified_write::a_write_rests_is_approved_by_a_person_and_is_written_and_verified_exactly_once`, and in `a_write_rests_until_a_person_approves_at_the_desk_and_is_written_exactly_once` in `a2m-actions`, for `tool.call`, `run.get`, and `action.run` of it.

## Revisit if

A client is found that takes `ok: true` with exit code 3 for a finished write, or the MCP protocol gains a result kind for an answer that waits.
