# 0998. A runner that starts again holds nothing, and its earlier leases are lost at once

**Status:** Accepted. Design decision, made by the worker who fixed the faults packages 9.3h and 9.3i found, under the lead's brief. The owner had not read this record when it was merged. Numbered 0998 as a placeholder beside 0999, which the same brief wrote; both are renumbered on merge.

**Date:** 2026-11-24

**Changes:** [`a2m-sched`](../architecture/crates/a2m-sched.md), which gains `Scheduler::runner_ended`; [`a2m-actions`](../architecture/crates/a2m-actions.md), whose `start_runner_loop` calls it once per process; the map of proofs in [spec section 224](../spec/2-kernel/224-when-the-engine-itself-fails.md). It leaves the lease length, the heartbeat, the sweep, and the quarantine of a lease that runs out ([decision 0029](0029-lost-leases-stop-early-quarantine-reconcile-verify-again.md), [decision 0360](0360-a-lease-is-measured-on-the-steady-clock-and-another-persons-run-is-not-found.md)) as they are.

## Context

In one process the runner's lease is at least ten minutes, because a lost runner there is a lost process. Nothing at the next start declared the ended process's lease lost, so the new engine answered `running` for its run until the lease ran out, and a person who resumed it waited ten minutes. `crates/anything2mcp/tests/start_on_demand.rs`, `the_engine_after_a_killed_one_resumes_its_run_at_once`, found it.

The runner of one machine is the installation's one runner: its identifier is a fact of the store, the same in every process. The store holds an advisory lock on its data folder, and the product's engine also holds the engine lock, so at most one process at a time is that runner. A lease the runner holds when its loop first starts was therefore given to a process that has ended.

## Decision

1. **`Scheduler::runner_ended(runner)` declares lost at once every lease `runner` holds,** each in one transaction, exactly as the sweep loses a lapsed one: the job is lost, the run is fenced by a new lease number and stops as `possible_commit`, `runner_lost`, and a reconcile is queued for a write or for a run with a record in doubt. Rule 3 holds: the run is reconciled before it is written again.
2. **Its quarantine is the job's longest step alone.** The setting's sixty seconds are for a runner that may only be frozen, and wake and act ([section 223](../spec/2-kernel/223-the-runner-protocol.md), "The gap that cannot be closed"); a process that ended cannot. A step it sent before it ended can still be in flight at the system, and the longest step from the moment of the loss outlasts it, since it was sent before then. A resume and a wake still read the quarantine from the lost job with the setting; that end is later than the reconcile's own time, so neither brings the reconcile earlier, and the time a resume reports is the reconcile's own.
3. **The kernel calls it once per process, at the first start of the runner loop, before `hello` and before anything is claimed.** A loop started again in the same process may still have work in hand, which would be fenced in the middle of a step.
4. **A runner of another identifier is not touched.** On one machine there is none; on a server a runner says it ended only of itself.

## What was rejected

- **A shorter lease in one process.** The ten minutes are there so that a binding of nineteen steps fits between two heartbeats (decision 0095); a shorter lease would stop long writes as `lease_too_short` to win time only after a crash.
- **Declaring lost every leased job at the start, whoever holds it.** It is the same on one machine today, and wrong the day a second runner is enrolled.
- **Leaving the quarantine as for a lapsed lease.** The resume's wait of sixty seconds would end as the quarantine did, and the person would be told the run is still reconciling, for a hazard that a process that ended does not carry.

## Consequences

- `the_engine_after_a_killed_one_resumes_its_run_at_once` runs, with no `ignore`: the command after the killed engine resumes the run, the reconcile finds the write, the run completes, and the test application holds one write.
- `a2m-sched` `the_leases_of_a_runner_whose_process_ended_are_lost_at_once_and_reconciled_after_its_longest_step` holds the scheduler's half: lost inside the lease, the run fenced and stopped, the ended process's heartbeat refused, another runner's lease kept, the reconcile handed out after the longest step and not before.
- A run the killed engine held is reconciled at the next start of an engine whose kernel is not degraded; one whose kernel is degraded starts no runner loop, and its runs wait for the next start that does.

## Revisit if

A runner on another machine is built: it then needs its own way to say that it started again, such as a field of `hello`, and the scheduler must not take its word for a process it cannot see.
