# 0291. A watch ends when its condition holds on a check's own result

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Builds** the condition of [decision 0234](0234-a-watch-is-a-read-repeated-on-a-schedule-a-person-confirmed-and-a-change-is-reported-never-acted-on.md), one of the parts [decision 0283](0283-watches-reach-an-agent-through-the-registry-and-their-person-confirms-each-on-the-page.md) left unbuilt, as package W2c. **Refines** [decision 0287](0287-the-last-check-under-way-on-a-run-a-check-opened-cancels-it.md): the rule it states is now answered by the ledger, and the scheduler applies it to a watch past its end in the transaction that ends the watch.

**Changes:** the saved OpenAPI copy and both generated clients; the [action catalog](../reference/action-catalog.md) row of `watch.create`; the designs of [`a2m-ledger`](../architecture/crates/a2m-ledger.md), [`a2m-sched`](../architecture/crates/a2m-sched.md), and [`a2m-actions`](../architecture/crates/a2m-actions.md); the [ledger schema](../reference/ledger-schema.md) (migration `0026`); `watch.create` gains the optional input `condition`, a watch shows it, and a change may be `condition_met`; the watches row of [SCOPE.md](../../SCOPE.md); the roadmap (W2c split).

## Plan, stated before the build

Of what decision 0283 left, every part but two needs a crate outside the registry, the scheduler, and the ledger: a record key needs `a2m-files`, completeness the provider contract and the engine, the resource `a2m-mcp`, the change stream `a2m-api`. The two inside are the condition and retention. Retention of watches waits on retention of runs, which nothing builds yet (a watch's history leaves with its runs). So the slice is the condition, proven end to end against the test application, with one known gap of the tending fixed beside it.

## Decision

**A watch may carry a condition: an expression over the read's result that gives true or false. The check on which it first holds records its changes as any check does, then `condition_met`, and the watch ends with the reason `condition_met`, all in the transaction that ends the check.**

| Part | Built | Proven by |
| --- | --- | --- |
| The ask | `watch.create` takes `condition`, text in the expression language of `a2m-expr`. It is compiled against one name, `result`, a record of the read's outputs with the types the plan gives them, as the compiler types an operation's result, and must give true or false. Nothing else may be named: no input, environment, or earlier result. A condition that does not compile is refused with the expression's own code (`expression_invalid`, `expression_type_error`) and no watch is kept. The plan's form (`PlanExpr`) is kept on the watch | `a_condition_is_checked_against_the_reads_outputs_and_ends_the_watch_when_it_holds` (actions) |
| The ledger keeps it | `NewWatch::condition` and `WatchRow::condition`, the column `condition` of `watches`, canonical JSON of the plan's form, never changed (trigger). `Watches::ask` loads it again (`Program::load`) and refuses `schema_invalid` for one that does not load or does not give true or false | `ask_refuses_a_condition_that_does_not_load_or_give_true_or_false` (ledger) |
| The person sees it | The confirmation page says the watch ends at the first check on which the condition holds, and shows its text; a watch with none says so. Every `watch.*` result shows `condition` when the watch has one | The actions test above |
| Each check | `Watches::end_check` evaluates the condition on the same decoded bytes of the run's own stored result that the comparison reads, `result` bound to them, before anything of the check is stored. True: the changes, then `condition_met` naming the check, then the end with `condition_met` (the baseline goes, as at any end). False: an ordinary check. No answer (an evaluation that stops, or anything but true or false): the check is `faulted`, a gap with the new code `watch_condition_unanswered`, and the baseline stays; the watch goes on, and is never taken as false (rule 7) | `a_condition_that_holds_on_a_checks_own_result_ends_the_watch`; `a_condition_that_cannot_be_answered_is_a_gap_and_the_baseline_stays` (ledger) |
| End to end | Through the real command (`watch create ... --condition 'result.quantity >= 9'`): the first check does not hold; the stand's own interface sets the line item to nine; the next check records `changed`, `condition_met`, and `ended`, the run it names read nine, nothing more is read, and the stand holds no write but the test's | `a_watch_ends_at_the_first_check_on_which_its_condition_holds_of_what_the_stand_changed` in `crates/anything2mcp/tests/watch.rs` |

### The store: migration `0026`

The words a reason and a change's kind may take are column checks of migration 0021, and SQLite cannot widen a check in place. Migration `0026` builds `watches` and `watch_changes` again, with `condition_met` in both and the new column `condition`, copies every row across unchanged (through temporary tables), and creates their indexes and triggers again as 0021 and 0023 made them, with `condition` among what never changes and a new trigger that a `condition_met` names a check of its watch. The other tables refer to `watches` by name; their foreign keys are deferred to the migration's commit (`PRAGMA defer_foreign_keys`). The test `building_the_watch_tables_again_keeps_every_row_and_reference` runs the migration's text once more on a store holding a watch with checks, changes, and a baseline: every row survives, `foreign_key_check` finds nothing, the triggers refuse what they refused, and the store goes on using the tables. Without the deferral, the same test fails with `FOREIGN KEY constraint failed`. The migration list now lets a crate create a table of its own a second time, for such a rebuild; two crates creating one table is still refused.

### The gap folded in: a watch's end and its run's cancel commit together

`tend_watches` took the runs in `DueWatches::cancel` from the scheduler, after the scheduler had ended their watches, and filtered them in a separate store read that ended in `unwrap_or_default()`. If that read failed, or the cancel after it, the watch was ended and its run never cancelled, and nothing offered the run again: a run queued for no watch, or one resting at `needs_login` that rule 6 would hand to a later call. Now `Watches::run_left_to_cancel(run, stopping)` answers decision 0287's rule in the ledger, and `Scheduler::due_watches` ends each watch past its end and cancels each run left to cancel in its one transaction, once every watch of the pass has ended. `DueWatches::cancel` became `DueWatches::cancelled`: the runs cancelled, or whose runner was told to cancel them, and nothing is left for the caller. A pass the store refuses commits neither, and the next does both: `a_pass_that_fails_ends_nothing_and_cancels_nothing_and_the_next_does_both` (sched) makes the store refuse the cancel, by a trigger on a second connection, and asks the store what it holds before and after.

## Alternatives weighed

- **The registry evaluates the condition and tells the ledger whether it held.** Not taken. The registry has the result's bytes too, but a check's outcome would then be the caller's word, which decision 0262 (B3) removed for records and completeness. The ledger already decodes the run's own stored result, checked against the run's row; the condition is answered from the same bytes, in the same transaction, so a crash can never leave a check compared and its condition unanswered.
- **No rebuild: record the end as `end_reached`, or keep `condition_met` in a table of its own.** Not taken. `end_reached` says the end the watch was asked with came, which is false; a second table would split one sequence of changes in two, and every reader of `watch.changes` would merge them. The rebuild is one migration, tested on a store with rows.
- **Widen the checks by editing the stored schema (`writable_schema`).** Not taken: it is SQLite's own warning-marked path, and a store edited that way cannot be told from a corrupt one.
- **For the gap: keep the registry's separate read and sweep each pass for runs a check opened that no check under way names.** It heals a failure after the fact, at a scan every pass. Not taken: deciding and cancelling in the scheduler's one transaction leaves nothing to heal.
- **A condition that cannot be answered pauses the watch.** Not taken for now: one result without the value the condition reads is not a reason to stop watching; a gap tells the agent at once, and the next check tries again. If such results turn out to be lasting rather than passing, a pause with a reason of its own is the next step.

## Not built

What decision 0283 lists, less the condition: records compared one by one, with a declared record key; the engine's completeness flag and a reported removal; the MCP resource and its subscription; the REST change stream; retention of watches. They are package W2d. The saved OpenAPI copy and both generated clients were fetched and generated again from an engine of this build, and carry `condition` and `condition_met`.

## What the work showed

- **The condition needed no new knowledge in the kernel.** An expression typed against the plan's outputs and evaluated on the run's own result is all of it; no pack and no mode is named, as hypothesis H-26 asks.
- **A word in a column check costs a rebuild.** Two of the watch tables' columns are closed lists in SQL. The next word a reason or a kind needs will cost the same; a lookup table of words, or a trigger in place of a check, would make it one statement.
