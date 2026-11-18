# 0283. Watches reach an agent through the registry, and their person confirms each on the page

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Builds** the registry's half of [decision 0262](0262-watches-are-held-to-their-end-their-budget-and-their-runs-own-result.md), whose ledger and scheduler halves were built and proven, and so accepts the actions of [decision 0234](0234-a-watch-is-a-read-repeated-on-a-schedule-a-person-confirmed-and-a-change-is-reported-never-acted-on.md) as [decision 0247](0247-a-watch-keeps-only-digests-and-its-due-times-come-from-the-stores-clock.md) and 0262 refined them. **Keeps** [decision 0280](0280-the-use-toolset-lists-at-most-twelve-tools.md): no `watch.*` action is listed; an agent reaches each through `a2m_action_run`.

**Changes:** the [action catalog](../reference/action-catalog.md) (eight actions); the design of [`a2m-actions`](../architecture/crates/a2m-actions.md); [section 226](../spec/2-kernel/226-configuration.md) (five keys); the product crate's idle holds; the saved OpenAPI copy and both generated clients.

## Plan, stated before the build

The slice: eight actions over the built halves, the kernel's tending of watches, five protected settings, and an engine that is not idle while a watch has checks to make. It tests hypothesis H-47 against the test application for the first time. The alternative weighed first was the plan's: `watch.create` answered with the person-act shape of `publish.request`. It was not taken, because that shape names no watch, and an agent needs the watch's identifier to ask for its changes.

## Decision

| Part | Built | Proven by |
| --- | --- | --- |
| The actions | `watch.create`, `watch.confirm`, `watch.get`, `watch.list`, `watch.changes`, `watch.pause`, `watch.resume`, `watch.end`, phase 5, none listed. Who: `Viewer` for each but `watch.confirm`, which is `The person` with `PersonNeed::Presence`. Every result shows a watch as its state, read, plan hash, interval, end, checks, surface, requests per check, and what is compared, and never its input or a value it read | `src/tests/watches.rs`; the catalog test |
| An agent asks; its person confirms | `watch.create` asks through `Scheduler::ask_watch` and hands `watch.confirm` with the watch's identifier to `Kernel::ask_person`, as `publish.request` hands a publish. The page shows the read, its input, the interval and the end, how many checks that is, the share of the surface's budget, and what is kept (`Kernel::watch_confirmation_lines`). An agent that calls `watch.confirm` is sent to the same page by the dispatcher; with no page it is refused `person_required` | `an_agent_asks_for_a_watch_and_nothing_runs_until_its_person_confirms_on_the_page`; `an_agent_calling_watch_confirm_is_sent_to_the_page` |
| Whose watch | `acts_for` is `Principal::person` of the caller, never input; a caller acting for no person is refused `person_required`. Every action but `watch.create` answers another person's watch as `input`/`not_found`; `watch.end` also serves the caller that asked for it, as `ended_by_caller` | `watch_changes_never_names_a_record_from_another_persons_run` |
| What a watch reads | The tool must be a published operation whose effect is `read`. Its surface is that of its first verified binding; its requests per check, the binding's act and confirm steps, at least one; its shape, the whole result (below). No input names any of the three. The input's names are checked against the operation's inputs and the required ones must be given; `ignore` names only outputs | `src/tests/watches.rs` (surface, cost, and shape come from the plan) |
| The confirmation | `watch.confirm` requires a proof of presence, reads the target's current plan hash and the surface's rate limit (its declared one, or the scheduler's default), and calls `Scheduler::confirm_watch`, which holds the version, the end, and the budget share in one transaction | The first test above |
| The tending | `Kernel::tend` runs `tend_watches` after the sweep: it ends every watch past its end and cancels the runs `DueWatches::cancel` names; admits each due check as a `canary` read of the watch's own person with the watch's stored input and the watch's identifier as its asker label; ends each check whose run ended with that run's own stored result bytes, or `None`; and resumes a watch paused `needs_login` once the keeper reports its person's session for the target and environment `valid`. The tending task waits on the earlier of `next_due` and `next_watch_due`. The runner wakes it when a check's run finishes | The product tests below |
| After a publish | `version.publish` calls `Watches::version_changed` for the new plan hash and cancels the runs of the checks under way that it answers | Not tested through a publish in this slice |
| The settings | `watch.shortest_interval_seconds` (60, 60 to 86400, higher is safer), `watch.longest_days` (7, 1 to 366, lower), `watch.most` (20, 1 to 1000, lower), `watch.budget_share_percent` (10, 1 to 50, lower), `watch.reserve_percent` (50, 0 to 90, higher). Each is built, of scope U, protected, and read into `SchedSettings::watch` | `a_settings_file_of_the_person_cannot_lower_the_watch_floor` |
| Idle | The solo engine's `Holds::busy` is true while any watch is confirmed or running and its end has not come (`Kernel::watches_hold_the_engine`); a paused watch holds nothing | `a_watch_past_its_end_lets_an_idle_engine_stop` |

The product's tests, `crates/anything2mcp/tests/watch.rs`, run through the real command on a clock the test moves, against the test application: `a_watch_reports_the_record_the_stand_changed_and_no_other` (the stand's own interface changes one line item, outside the product; the next check reports one `changed` with the one output, the other watch reports nothing, and the stand holds no write but the test's); `quiet_checks_record_no_change` (ten checks, nothing but the confirmation); `a_missed_due_time_is_a_gap_and_is_not_made_up` (an engine started three and a half intervals later records one gap of two missed due times and makes one check); `a_list_cut_short_never_reports_a_removal` (the half decision 0262 allows today); `a_watch_past_its_end_lets_an_idle_engine_stop`.

## Choices

- **Every watch compares its read whole.** A plan declares no list of records with a key: an operation's output is a value of one field type, or a list of such values (`a2m-files`, the operation's `outputs`). So there are no records to compare one by one, the shape is always `whole`, and no check can report `added`, `removed`, or `unseen` of a record. Taking a key from the names of another operation's inputs was weighed and not taken: it is a guess (rule 7). Per-record changes arrive with a declared record key for a list output, a change to the file formats that needs its own record. `watch.changes` resolves a record's key only from a run of the caller's own person, so that code is in place for it.
- **The field is `tool`, as in `tool.call`,** not the plan's `operation`: an agent passes the name `tool.search` gave it. `watch.create` also takes `environment`, which every run needs, the target's default when left out.
- **`watch.create` answers with the watch,** its state `asked`, and, when a page was made, the act, its link, the person's line, and when it stops waiting; the exit is `person_needed`. With no page the watch stays asked, the result says so in a warning, and it ends at its end unconfirmed.
- **A check whose run rests is ended before its run is cancelled,** the reverse of the plan's order. The ledger reads how a check ended from its run's row: a run resting at `needs_login` pauses the watch for a sign-in, and one cancelled first would end as a gap and lose that reason.
- **The surface's declared budget is set before a check is admitted,** as a run's control sets it before its first request. Admission and each request then count the watches' share of the same limit. Without it, admission counted a share of the scheduler's default and the check's requests a share of the declared limit, and a check could wait without end.
- **An admission refused for a reason the scheduler does not wait out** defers the watch for a minute, so that the tending task never runs again at once for the same due time.
- **The input is checked by name and by presence.** Its values are checked by type when each check runs, as for any call; a value of the wrong type is a gap with the engine's `input_invalid`. Checking types at the ask needs the engine's normalizers in the registry.
- **No new failure code.** The handlers reuse `not_found`, `schema_invalid`, `person_required`, and the ledger's `watch_not_a_read` and `watch_needs_confirmation`.

## Lines proposed for decision 0247's glue that were wrong against the code

1. "`watch.confirm` refuses an agent with `person_required` and needs no presence prompt": the dispatcher sends an agent's person act to the page, and a press carries it out only with a proof of presence.
2. "First `fits_budget`, then `confirm_watch`": two transactions race; `confirm_watch` now checks the share inside its own.
3. A check's records and completeness given by the caller: they come from the run's own result.
4. `watch.create` and `watch.changes` listed in `use`: the toolset is full (decision 0280).
5. On a publish, `version_changed` for confirmed watches only: asked watches of another version end too.
6. The settings keys unprotected: each is protected.
7. "Not idle while any watch is open": a paused watch would hold the engine; only confirmed and running watches do.
8. `NewWatch` with no surface, request count, or shape: nothing could hold the budget or work out the records.

## Not built

The condition (`a2m-expr` in the registry); the MCP resource and its subscription; the REST change stream; the engine's completeness flag, a field of the provider contract, and so any `removed`; a declared record key, and so any record compared by itself; retention of watches; a kill switch as a pause with `on_hold` (a check refused for a hold is deferred, and its due times become gaps); a test of `version_changed` through a publish; the served engine's idle stop with a watch, through the real program (a minute of real time and a presence prompt the test machine does not have; the hold it reads is tested).

## What the work showed

- **H-47, against the test application.** Ten quiet checks of a line item recorded nothing, and one change made through the stand's interface was reported as one `changed` naming the one output, on that watch alone. The half about records and removal is not tested: every read is compared whole.
- **A check refused for its budget loses its due time.** The scheduler defers it for the smaller of a lease and the interval, so a check that waits for the share's next request becomes a gap at the next due time. Two watches due together on a surface that declares no rate collide: the share of the default is one request, and in this slice's first run of the product test the second check was deferred a whole interval, past its due time. The product tests declare a rate on their surface for that reason. Owed to `a2m-sched`: defer a check refused for its budget until the budget's refill, not a whole interval.
