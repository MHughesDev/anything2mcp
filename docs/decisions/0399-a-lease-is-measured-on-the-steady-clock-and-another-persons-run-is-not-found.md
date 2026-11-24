# 0399. A lease is measured on the steady clock, and another person's run is not found

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md), in package 3.10. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** the design of [`a2m-sched`](../architecture/crates/a2m-sched.md) ("Time is the store's clock"); the [roadmap](../plan/roadmap.md) row 3.10.

## Context

Package 3.10 had two rows of spec sections 224 and 502 left open: an input that names another person's run or workspace, and the machine's clock jumping. Section 224 says that leases are unaffected by a jump, because they are measured on a steady clock. The scheduler did not do that: it wrote each lease's end as a moment of the store's clock, which is the time of day, and judged every lease by it. A clock set forward a day would have declared every lease lost at the next sweep, stopped every run that held one as `runner_lost`, and queued a reconcile for each; a clock set back would have kept a dead runner's lease for as long as it was set back.

## Decision

1. **Every lease is measured on the scheduler's count of lease time:** the time of day when the scheduler was made, carried forward by the machine's steady clock and by nothing else. A lease's end is written as a moment of that count, and the grant, the heartbeat, the check of a call against its lease, a resource waiter's lease, and the sweep that declares a lease lost all read it. A deadline a timer on the store's clock waits for (the end of a wait inside a lease, the moment the sweeping task is told) is turned into the store's clock at the moment it is set. Every other moment the scheduler keeps, a queue limit, a job's availability, a budget's refill, a watch's due time, stays a moment of the store's clock. *Instead of* finding a jump by comparing the two clocks at each sweep and moving every lease's end by it, which a transaction that rolls back would apply twice or not at all.
2. **A lease written before a restart is judged by the count the new scheduler starts.** That count starts at the time of day of the start, so a lease the old process wrote after a jump forward looks older than it is, and may be declared lost early. The run is fenced and reconciled, as after any lost lease, and nothing is written twice. Keeping the count across a restart needs a row the store does not have.
3. **Another person's run is not found.** With a grant that lets a second person of the workspace look and change, `run.get`, `run.resume`, and `run.cancel` naming the first person's run answer `not_found` and say nothing of it; `trace.show` is an auditor's (section 214) and answers `not_granted` to anyone else. A caller of another workspace is refused the same way. An input that names a workspace, a person, or a principal to act as is not in any action's schema and is refused as `schema_invalid`; inside a tool's own input it is refused by the tool's schema. The run stays as it was and the person's own resume completes it once.

## Proven

- `a2m-sched` `a_jump_in_the_machines_clock_does_not_move_leases`: a jump of a day forward loses no lease and keeps the heartbeat taken; two days back, the lease is lost after its own length and grace, and the sweeping task is told that moment on the store's clock.
- `a2m-actions` `an_input_naming_another_persons_run_or_workspace_is_refused_and_changes_nothing`, against the test application's store.

## Left

`run.undo`, `run.trace`, and `trace.export` are not built and answer `action_not_built` to everyone; each is held to the same rule when it is built.

## Revisit if

A runner is found whose lease must outlive a restart of the engine on its own count, or an action is built that takes a run or a workspace another caller started.
