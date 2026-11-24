# 0950. The steps between a proven read and a publish are one action

**Status:** Accepted. Design decision, made by the design architect. The owner had not read this record when it was merged. Nothing in it is built.

**Date:** 2026-11-24

**Changes:** the action catalog (one new action, `draft.ready`, and a shortcut, `ready`); a new roadmap package, S3. It follows from the [review of the first release's effort](../reviews/first-release-effort-review-2026-11-24.md), finding 1.

## Context

[Decision 0306](0306-the-short-path-is-add-then-a-trial-and-only-a-built-action-is-listed-up-front.md) made the way to a first value short: `add`, then a trial. The way from there to a published tool is not. Read from `crates/anything2mcp/tests/first_hour.rs`, each read a scout has already proven takes `fixture record` with the tool and an input, `binding promote` with the tool and a mode, and then `test` and `publish` for the target: two commands for each read and two more. A target with twenty proven reads takes forty-two commands before a person is shown anything. Nobody decides anything in any of them: the input is the one the scout's report already carries, and the mode is the first binding's.

## Decision

1. **`draft.ready` does those steps for every read the scout proved.** For each read the target's latest scout report lists as proven, with the input the report carries: it records the offline test and fixture as `fixture.record` does, promotes the read's first binding as `binding.promote` does, and then runs the publish gates as `test.run` does. It takes an optional list of tools, and without one takes every proven read.
2. **It adds no power.** Each step is the existing action's own handler, with its own checks. An agent may already run each. A read whose step fails is left as it was and reported with the step's failure; the others go on.
3. **It never touches a write, a held draft, or a read the scout did not prove.** Those are reported as left alone, each with its reason.
4. **It does not ask for the publish.** `publish` stays its own command, because asking a person to review is a step someone means to take.
5. **The shortcut `ready <name>` runs it.** A read-only tool is then `add`, `ready`, `publish`, and a person's press.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A shortcut that is a fixed sequence, as the others are | The sequence is per read and takes its inputs from the report; a shortcut holds no logic |
| Fold these steps into `add` | `add` would then write fixtures and change files' statuses before anyone had read the report |
| Leave it to the agent | It works, at two commands a read, and every one is a place for an agent to go wrong; package 5.6a will count them |

## Proof

Package S3's row: in `crates/anything2mcp/tests/`, from an empty folder, `init`, `add`, `ready`, `publish`, the press of the prompt that stands for a person, and `call` give a published read that returns what the test application holds; a target with one proven read, one read that waits for a sign-in, and one drafted write gets the first made ready and the other two reported as left alone; a read whose fixture cannot be recorded is reported with that failure and the next read is still made ready.

## Revisit if

Package 5.6a shows agents do not go wrong in the steps this replaces.
