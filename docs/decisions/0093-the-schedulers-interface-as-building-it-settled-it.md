# 0093. The scheduler's interface, as building it settled it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

## Context

`a2m-sched` was built against the store as [decision 0080](0080-the-store-is-one-sqlite-file-and-a-crate-reaches-its-own-tables-through-the-transaction.md) settled it and the ledger as [decision 0081](0081-what-the-ledger-holds-as-data-the-token-doubt-locks-and-rule-three.md) settled it. Several points of the [crate's design](../architecture/crates/a2m-sched.md) changed or were settled. They are recorded together.

## Decision

- **Time is the store's clock, and every deadline is kept in the store.** A lease's end, a queue limit, the end of a quarantine, and a budget's level are moments of the store's clock, written in the transaction that sets them. A restart finds them again, and a job whose runner is gone is recovered by its deadline. *Instead of:* a steady clock of the scheduler's own, as the design sketched, whose readings mean nothing after a restart.
- **Nothing sleeps; `sweep` ends the waits.** A wait for a moment is a list entry. `sweep` ends every entry whose moment has come, and one task outside the crate calls it when `next_due` says so. A test moves the clock and calls `sweep`. *Instead of:* a timer inside each wait, which no test could drive without real time.
- **A lost lease is fenced at once, with a lease number nobody holds.** `sweep` calls the ledger's `new_epoch` in the transaction that marks the job lost, then stops the run under that number. The reconcile job gets a further number when it is leased. *Instead of:* leaving the run's number as it was until the reconcile job is leased: in between, the ledger would still take a call from the lost runner, which could have moved a stopped run with nothing in doubt back to `running`. The cost is that a lost lease raises the run's attempt number by two.
- **What a lost job held alone passes to its reconcile job.** *Instead of:* freeing it at the loss, which would let another job take a screen or a device link during the quarantine.
- **The ordering rule against deadlock.** Exclusive keys are ordered by their text. One call takes its keys together, and a job may ask only for keys that sort after every key it holds; a call against the order is refused at once with `busy`, `resource_order`. *Instead of:* finding circles of waiters, which needs every holder to be known to the scheduler, or refusing the younger of two jobs, which makes the outcome depend on arrival. The rule does not reach record locks, whose holders the ledger knows; their waits are bounded by a deadline.
- **A lock wait starts from a ticket.** `lock_ticket` is taken before the ledger is asked for the lock, and `wait_lock` returns at once when the lock was freed since. *Instead of:* a wait that can miss a release between the ledger's answer and the wait.
- **Round robin between callers is kept in the store.** Each dispatch gets a number, and the caller with the lowest number of its last dispatch goes first. *Instead of:* ordering by `available_at` before the caller, as the design said, which lets a flood queued earlier starve everyone.
- **A directive `pause` is added** to `continue`, `cancel`, and `drain`. A hold, which is how a kill switch reaches the scheduler, must stop a run in flight and leave it resumable; `cancel` ends it for good. Spec section 223 is changed to list it.
- **Holds are the scheduler's data, and deciding them is not.** `hold` and `release_hold` take a scope: everything, a workspace, a target, a caller. The policy engine owns kill switches; `a2m-actions` tells the scheduler when one changes.
- **Budgets are kept in the store from the start,** in whole numbers: a level in requests times the refill period. *Instead of:* buckets in memory, which a restart would refill.
- **`submit` takes no dedupe key,** because the ledger's `open_run` computes it from the run row. **`enqueue` is added** for a further job of a run that exists: the execute job after an approval, and a resume. One run has at most one job waiting or leased; an index holds that.
- **`JobEnd` carries no run state.** The engine writes the run's state through the ledger; `finish` reads it and keeps a copy on the job. *Instead of:* a state passed in, which could disagree with the ledger.
- **`JobLease` carries no `parallelism`;** the limits are in its `requirements`. It carries the mode whose slot it took and the attempt number, and gives the ledger's `Fence`.
- **A job names the person it acts as** (`acts_as`), and a runner the person whose device it is (`owner`). *Instead of:* an identity in the requirements, which the run row already holds.
- **Six failure codes are new:** `resource_in_use`, `resource_order`, `rate_limited`, `on_hold` (all `busy`), `runner_not_enrolled` (`policy`), and `runner_protocol_unsupported` (`unknown_variant`).
- **The tables.** `jobs` gains `seq`, `workspace_id`, `principal_id`, `target`, `environment`, `session_key`, `writes`, `queue_until`, `lease_mode`, `directive`, and how it ended. `resource_holds`, `caller_turns`, `scheduler_holds`, and `budgets` are new. The [ledger schema](../reference/ledger-schema.md) lists them.

## What follows

- `a2m-actions` says `hello` for its in-process loop, runs one task that waits on `next_due` and `due_watch` and calls `sweep`, and calls `lock_freed` after a commit that released a record lock.
- A waiter woken for a record lock can lose it to a run that never waited, because the ledger alone grants locks.
- Not built: the `verify_after_loss` job, a weighted share for a caller, revoking a runner, marking a silent runner offline, and everything of the cluster profile.

## What would reopen it

A second control plane on one store, which needs the waits to cross processes; a measured dispatch time over the budget of spec section 225; or a pipeline that must take exclusive resources in an order the rule refuses.
