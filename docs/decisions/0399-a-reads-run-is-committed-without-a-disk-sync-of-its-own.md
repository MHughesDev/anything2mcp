# 0399. A read's run is committed without a disk sync of its own

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** The commits that record a run of a tool whose effect is `read`, and nothing else, use `Durability::Relaxed`: the kept call, the state moves, the decision about the call at the front door and in the run, the result, the evidence blob's row, and the final state. Everything else is synced as before. `Policy::durability_for` is the one rule for a decision: relaxed only when asked for, for a request whose effect is `read` and that is bound to no diff. `Blobs::put_as` keeps a blob with a named durability for its row; `Blobs::put` is synced as before. Separately, an open engine makes its command tree once instead of on every command line. Spec section 208's rules, the intent token, and every check of a call are unchanged.

## Context

The exit test of Phase 4 asks a request read to run at least ten times faster than the interface read of the same item. On a CI runner it measured 9.0, with the request read at 17.9 ms by the median, for a read whose system answers in about 0.1 ms. One read through the product, timed stage by stage and traced for disk syncs on this machine, made 15 synced commits and about 16 fsyncs, with no new connection: the HTTP pack's warm client was reused, the plan was compiled once, and the request itself took about 0.45 ms. The commits were the time. Eight synced commits were the read's own, made by `a2m-actions` and `a2m-policy`; six were the scheduler's claims and the job's finish; one was the trace blob, with two more fsyncs for its file and folder. The command tree of the whole registry was rebuilt for each command line, about 0.3 ms.

Spec section 208 already said that a read has no record rows and is "recorded for audit without forcing a disk sync on every call", and `Durability::Relaxed` is documented as being for the run rows of reads. The code synced them all the same.

## Decision

A run of a tool that only reads commits its own rows relaxed. Each such commit is a committed transaction before the engine goes on: a crash of the process loses nothing, and rules 6 (one run per identical call), 8 (final means final), and 9 (history is append-only) hold in the transaction exactly as before. What a crash of the whole machine can lose is the tail of commits made since the last synced one. The store is SQLite in write-ahead-log mode, and a synced commit syncs every frame before it in the log, so no later commit is ever on disk without the relaxed ones before it: a read's job is claimed and finished by synced commits of the scheduler, which carry the read's rows to disk at the latest when the job ends. Rule 1 (intent before action), rule 2 (proof before verified), rule 3 (approval bound to the diff), rule 5, and rule 7 concern runs that write, and a write run's commits are all still synced. A decision bound to a diff, a decision about anything but a read (the dispatcher's decision about an action other than `tool.call` included, whose effect it treats as `read`), an approval, a key, the audit chain, and a caller's idempotency key are synced as before. A blob's file is still written and synced before its row commits, as section 208 says.

Measured on one Linux machine, release build, in the same session, the read alone through the product (the `http_operation` setup, 300 calls, three runs of each build interleaved): median 11.1, 11.4, and 10.8 ms before; 8.6, 8.6, and 8.8 ms with the relaxed commits; and about 0.3 ms less again with the command tree kept. The promotion exit test, five runs with both changes: median factor 16.0, 15.3, 14.8, 14.3, 17.6, and per-pair median factor 16.4, 15.7, 14.6, 13.9, 16.7, with the request read at 10.9 to 13.3 ms by the median; five runs with the relaxed commits alone: 15.4, 15.2, 12.1, 15.7, 13.9 and 14.0, 15.2, 12.1, 15.0, 13.2, at 12.0 to 15.7 ms; four runs before, the same day: 13.7, 11.7, 12.0, 10.9 and 12.9, 11.9, 11.6, 11.5, at 16.7 to 18.7 ms (an earlier run that day: 10.8 and 12.6, at 22.1 ms). The bar of the test and the test are unchanged.

## Rejected

- **Batch the read's commits into fewer transactions.** The state moves, events, result, and trace are separate calls across the `Control` contract, which is public; merging them would change that contract for about 0.1 ms a commit, now that none of them syncs.
- **Skip the sync of a read's evidence file.** Section 208 holds a blob synced before the row that names it commits; the two fsyncs of the trace (about 0.9 ms) stay.
- **Relax every decision whose request says `read`.** The dispatcher gives an action that is not a tool call the effect `read` for its decision, `killswitch.set` among them; only the decision about calling a tool that reads is relaxed.

## Not done

The scheduler's commits are the largest part of a read that is left: a claim is a synced commit on every look, leasing or not, and two runner loops look on every change, so a read makes five claims and one finish, all synced. Relaxing the claims that lease only reads or nothing, and the finish of a read's job, measured about 1 ms more in a trial that was not kept. That is `a2m-sched`'s to decide, with the question of hypothesis H-21.

## Would reopen it

A crash of a machine that loses a read's run while a later synced commit survived, which would show the log does not keep its order; or a rule of section 208 that is found to read a read's rows as durable.
