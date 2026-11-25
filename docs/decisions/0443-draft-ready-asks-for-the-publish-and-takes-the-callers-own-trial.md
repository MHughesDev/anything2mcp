# 0443. `draft.ready` asks for the publish, and takes the caller's own trial

**Status:** Accepted. Design decision, made by the agent that built package S3. The owner had not read this record when it was merged. It supersedes points 1 and 4 of [decision 0401](0401-the-steps-between-a-proven-read-and-a-publish-are-one-action.md); the rest of that record stands.

**Date:** 2026-11-25

**Changes:** the action `draft.ready` and the shortcut `ready`, as package S3 builds them; the action catalog; the guidance file's order of work.

## Context

Decision 0401 made the steps between a proven read and a publish one action. It took only the reads the newest scout proved, and it stopped short of the publish request, because asking a person to review is a step someone means to take.

Package 5.6a measured the first hour ([H-18's result](../hypotheses/H-18-result.md)). Three agents with no knowledge of the repository went from an empty folder to a published read. In all three runs:
- the scout proved no read, because no read stated an example of its key, and each agent tried the read itself with `draft.try`;
- the `next` after `test.run` named `publish.request` and skipped `binding.promote`;
- every agent then asked for the publish all the same, as its next step.

A `draft.ready` that takes only the scout's proven reads would have done nothing for any of the three. One that stops before the publish request leaves the agent one more step to find. Nobody decides anything in that step: the gates have passed, and the person is asked, not told. The person's press on the engine's page is the step someone means to take, and it stays a person's.

## Decision

1. **A read is tried when a trial in this project read a value through it.** That is either of these:
   - the newest finished scout of the target in this project proved it; the input is the one the report carries, or, where the scout took it from a list read's answer, the scout's own trial's stored input;
   - the caller has a completed trial of it, made from this project since that scout finished (or at any time, when there is no scout); the input is that trial's own stored input.

   A read the scout tried and failed counts only by a completed trial made after the scout. A read whose last trial waits for a sign-in, or that no trial read through, is not tried.
2. **The binding is promoted on the fixture's own trial.** `fixture.record` runs the read in the sandbox, so its trial is the newest evidence of the binding as it is now. That trial's `run_id` is handed to `binding.promote`. A promote then never rests on a try whose run has gone.
3. **When the gates pass, `draft.ready` asks for the publish.** It dispatches `publish.request` for the same caller. The answer is `needs_person`, exit code 3, with the page's link under `publish`, and `next` names the call of the published read once the person has carried the publish out. The publish itself is the person's, on the page, past the presence prompt, as before. Asked again after the person published, the request's kept answer is reported as `published`, with its version.
4. **A call that names a tool which cannot be made ready changes nothing.** Every tool is looked at before any step is taken. A named write or pipeline is refused `input`/`not_made_ready_not_a_read`, and a named read with no trial `input`/`not_made_ready_untried`. Each refusal gives its reason and the step that moves the tool on. Without `tools`, such a tool is listed under `left_alone` with the same reason and step, and the others go on.
5. **When the gates do not pass, nothing is asked of a person.** Each draft with no offline test is listed. When every one of them is a tool left alone, `next` is `draft.delete` of those files, and its `when` names the other way on.

Everything else in decision 0401 stands. Each step is the existing action, dispatched for the same caller and decided by the policy engine. The action adds no power, never touches a write or a held draft, and reports a read whose step fails with that step's failure while the others go on.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep decision 0401 as it was: the scout's proven reads alone, and no publish request | It does nothing in the first hour as 5.6a measured it, and leaves the step the agents had to find |
| Hand `binding.promote` the trial the read was made ready from | That trial may have gone from the store, or be of the binding before a file changed; the fixture's trial is made now, of the binding as it is |
| Ask for the publish even when the gates fail | `publish.request` refuses it anyway, and the person would be shown nothing |
| Remove the drafts left alone, so that the gates pass | Removing a draft is the developer's choice; `draft.ready` names the removal as the way on and does not take it |

## Proof

Package S3's test in `crates/anything2mcp/tests/ready.rs`, through the product's commands:
- from an empty folder, `init`, `add`, a named write and a named read with no completed trial each refused with its code, then `ready`, gives every proven read its fixture, test, and promoted binding, lists the write and the failed read as left alone, and names `draft.delete` of the drafts with no test;
- after that removal, a second `ready` asks for the publish with its link, the person's press publishes, and `call` returns what the test application holds;
- a read whose fixture cannot be recorded is listed under `failed` with that failure, and the next read is still made ready.

## Revisit if

- An agent is seen to take the publish request asked by `draft.ready` as a publish.
- A trial made outside the scout is found to be the wrong evidence to choose an input by.
