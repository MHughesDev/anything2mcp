# 0347. The check after a lost runner is the last part of its reconcile

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Builds the check after a lost runner of package 3.7, which [spec section 223](../spec/2-kernel/223-the-runner-protocol.md) names as a `verify_after_loss` job.

**Date:** 2026-11-24

**Changes:** `a2m-sched`: the field `verify_after_loss` of `JobRequirements`, set on the reconcile queued after a lost lease and kept by each reconcile queued after it. `a2m-actions`: the runner runs the engine's `VerifyAfterLoss` job at the end of such a reconcile, in its lease, and notes what it came to as one event of the run. Nothing in `a2m-engine` or `a2m-ledger`.

## Context

Section 223 says that after a lost lease a `verify_after_loss` job reads the records once more, later, and alerts if any differ from what the ledger says was verified. The engine has the job kind (`JobKind::VerifyAfterLoss`, which reads every record its snapshot holds as verified, sends `VerifiedDiffers` for one that differs, and ends `Settled` or `Left` without changing state), and the product's control keeps a `VerifiedDiffers` as late evidence. Nothing queued the job. As a job of its own it would be a second live job of the run, and the scheduler holds one live job per run: a person's `run.resume` would be refused, or would wait for it, for a check that changes nothing.

## Decision

1. **The check is the reconcile's own last part.** The scheduler marks the reconcile it queues after a lost lease with `verify_after_loss`. A reconcile queued after that one, because it was lost too or ended with a record still in doubt, keeps the mark, because it is cloned from it; a job a caller makes leaves it `false`, and a job stored before the field existed reads as `false`. No other job is queued.
2. **When a marked reconcile settles**, the product's runner runs the engine's `VerifyAfterLoss` job before it gives the lease back: with the same lease number, the same control, and a snapshot read again after the reconcile, while the heartbeats go on. The run has one live job throughout; a resume that finds the reconcile waits for it, check included, and then queues its own job as before. A reconcile that did not settle makes no check, and the reconcile that follows it carries the mark.
3. **The check changes no state.** A record that no longer reads as it was written is kept as late evidence of the run, as the control already keeps a `VerifiedDiffers`. The run's state, its records, and the reconcile's own end are what they would have been without the check.
4. **What the check came to is one event of the run**: a `need` event whose detail is `{"verify_after_loss": "read"}`, or `"not_finished"` with the class, code, and message, redacted, of what stopped it. A note that cannot be stored is dropped, since the check changed nothing.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A `verify_after_loss` job of its own, queued when the reconcile ends | A second live job of the run: a resume would be held back by a check that changes nothing, or the one-live-job rule would need an exception |
| The check inside the resume's execute job | The check would wait for a person, and a run nobody resumes would never be checked |
| Decide from the run's stored failure (`runner_lost`) whether a reconcile follows a loss | A stop on an already stopped run leaves the failure as it was, so every later reconcile of the run would check again; the mark says it once |

## Proof

`crates/anything2mcp/tests/verify_after_loss.rs`: the real program is killed after the test application stored its write and before the answer came. On a machine opened again later, the reconcile reads the record and finds it landed, the check follows it under the same lease and is noted as `read`, and the resume completes the run under a later lease with no late evidence. When someone changes the record at the application between the reconcile's read and the check's, the difference is kept as late evidence, the check is still noted as `read`, the resume completes the run, and nothing is written again. `crates/a2m-sched/tests/sched.rs`: the reconcile after a lost lease carries the mark, and one after a stopped job does not.

## What this leaves open

Section 223 says the check is made "later". Here it is made at the end of the reconcile, which is after the quarantine; a further wait is not built. An alert beyond the run's late evidence, to the destinations the setting `alerts.destinations` names, is not built. A record that the reconcile has just found landed is read twice in a row, once by the reconcile and once by the check.
