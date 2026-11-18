# 0262. Watches are held to their end, their budget, and their run's own result

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Refines** [decision 0247](0247-a-watch-keeps-only-digests-and-its-due-times-come-from-the-stores-clock.md), which built the ledger's and the scheduler's halves of [decision 0234](0234-a-watch-is-a-read-repeated-on-a-schedule-a-person-confirmed-and-a-change-is-reported-never-acted-on.md), and [decision 0251](0251-the-evidence-for-promoting-a-shadow-binding-says-what-it-covers-and-a-shadow-costs-what-it-says.md) on the purpose of a shadow's input. It answers findings B1 to B9 of the sixth safety audit (watches), before any action can ask for a watch.

**Changes:** `a2m-core` (two digest purposes); the designs of [`a2m-ledger`](../architecture/crates/a2m-ledger.md) and [`a2m-sched`](../architecture/crates/a2m-sched.md); the [ledger schema](../reference/ledger-schema.md). Migration `0023-ledger-watch-shape-and-shadow-input-purpose.sql`, the next free number. No store holds a watch yet: no action could ask for one, so no row moves.

## Built now

| Finding | Verified against the code | Fix | Test (shown to fail before the fix) |
| --- | --- | --- | --- |
| B1. An end applied only to confirmed watches | Yes: `plan` returns `None` unless `confirmed`, and nothing else ended a watch | `due_watches` first ends every watch not ended whose end has come, `asked`, `paused`, and `running` included, with `end_reached`. A running check is closed and its run returned in `DueWatches::cancel`. `next_watch_due` answers the end of a running, paused, or asked watch | `a_watch_past_its_end_ends_whatever_its_state` (sched) |
| B2. The budget share was advisory | Yes: `fits_budget` was pure and nothing called it; only `reserve_percent` held a check at run time | `confirm_watch(id, by, current, limit)` checks the share in its own transaction, against the other open watches of the same workspace, target, environment, and surface as the store holds them. At run time a check also spends `watch_share:<surface key>`, a budget whose limit is `budget_share_percent` of the surface budget's stored limit, or of `default_rate`, and which only checks spend | `two_confirmations_racing_on_one_surface_admit_one`; `a_check_of_a_paged_read_waits_on_the_watch_share_of_the_surface` (sched) |
| B3. Records and completeness came from the caller | Yes: `end_check` took `Observed { records, complete }` | `end_check(id, seq, result: Option<&[u8]>)` takes the bytes of the run's result, refuses `watch_run_not_its_own` unless their keyed name under `blob_name` is the run's stored `result_ref`, decodes them, and works the records out itself from the watch's stored `record_shape`. A result not of that shape is a gap with the new code `watch_result_not_its_shape`. No check is complete until the engine records completeness, so no removal is reported. `Observed` left the public interface | `a_check_whose_result_is_not_its_runs_own_is_refused`; `a_completed_check_never_reports_a_removal_while_completeness_is_not_recorded` (ledger) |
| B4. An asked watch escaped a version change | Yes: `version_changed` looked at `confirmed` and `running` only | Both remedies the audit named: `confirm` takes the current version and refuses `watch_needs_confirmation` for a watch asked under another; `version_changed` also ends asked watches of another version with `version_changed` | `an_asked_watch_confirmed_after_a_publish_is_refused_for_its_old_version`; `a_version_change_ends_an_asked_watch_of_another_version` (ledger) |
| B5. A check could begin at or after the end | Yes | `begin_check` refuses `watch_ended` when now or the due time is at or after `ends_at`; `confirm` refuses at or after it too | `a_check_is_not_begun_at_or_after_the_end` (ledger) |
| B6. The floor and the longest lived only in the scheduler | Yes | `Watches::ask` refuses an interval below 60 seconds and an end more than 366 days away, a check of no request, and a shape with no list or key, as `input`/`schema_invalid`; a trigger of migration 0023 refuses the same rows whoever sends them. `SHORTEST_INTERVAL_FLOOR` is the ledger's `WATCH_INTERVAL_FLOOR`, and the scheduler's `longest` is capped at `WATCH_LONGEST` | `ask_refuses_an_interval_below_the_floor` (ledger) |
| B7. Equal values shared one digest across records and watches | Yes | Each value's digest and each record's digest frame the watch's identifier and the record's key; the domain labels end in `2` | `equal_values_of_two_records_have_different_digests` (ledger) |
| B8. Ending a running watch wrote no change for its check | Yes | `end` writes a `gap` with `watch_ended` for the check it closes. `begin_check` and `defer` write no change: the check rows and `not_before` are the record of running | `ending_is_always_possible_and_leaves_the_history` |
| B9. An empty complete check would report a mass removal | Yes, in the comparison | Moot while no check is complete. Owed: when the engine records completeness, a check that sees no record after a baseline that held some is held as `unseen` unless its result proves the emptiness | The empty check in `a_completed_check_never_reports_a_removal_while_completeness_is_not_recorded` |

The purposes. `a2m-core` gains `watch_value`, under which a watch's values are digested (it was `restricted_value`), and `shadow_input`, under which a shadow comparison's input is digested (it was `caller_key`, with a label). Shadow comparisons exist in stores. One input digested under both purposes would be counted as two distinct inputs, so `shadow_comparisons` gains `input_purpose`, `caller_key` for every older row, and only rows of `shadow_input` count. Older comparisons count as no distinct input, which can only understate the evidence (`comparisons_kept_under_the_older_purpose_count_no_distinct_input`).

## Choices

- **`end_reached` for an asked watch past its end, not a new `expired`.** The reasons are a column check of migration 0021 in two tables; a new word needs both tables rebuilt with their triggers and foreign keys. `end_reached` is true of an asked watch whose end came; `declined` would say its person declined, which nobody did.
- **The new columns have defaults.** SQLite adds a `NOT NULL` column only with a default. The trigger refuses the defaults in every new row, so no watch is asked without a surface, a cost, and a shape.
- **The ledger refuses with `schema_invalid`, the scheduler with its own codes.** The scheduler checks the installation's limits first with `watch_interval_too_short` and `watch_too_long`; the ledger's refusal is the backstop no caller passes, and no code is in both crates' tables.
- **Admission spends one request of the share, not `requests_per_check`.** Admission takes one from every budget a submission names, as for any run; the check's requests are spent through `rate` as they are sent. Taking `requests_per_check` at admission as well would count each request twice. `requests_per_check` is what the confirmation's share check counts.
- **`cut_short` now also means "not proven complete".** Its column check cannot gain a word without a rebuild, and while completeness is not recorded every completed check is one whose result does not prove it saw every record.

## Waits for the registry half

The ledger and the scheduler cannot hold these; `a2m-actions` must, when it adds the `watch.*` actions:

1. Ask only through `Scheduler::ask_watch`, with `surface`, `requests_per_check`, and `shape` taken from the plan's first verified binding and declared list output and key, never from the caller's input.
2. Confirm only as the person's act on the page, passing the target's current plan hash and the surface's `RateLimit` to `Scheduler::confirm_watch`.
3. Cancel every run in `DueWatches::cancel`.
4. Call `end_check` for every check run that ends, with the bytes of that run's own `result` blob, or `None` when it did not complete; cancel a resting check run first.
5. After a publish, call `version_changed` and cancel the runs it answers once their checks end.

Not built after this: the engine's completeness flag (a provider-contract field), the condition, the MCP resource, the REST change stream, and retention.
