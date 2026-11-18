# 0287. The last check under way on a run a check opened cancels it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Refines** point 3 of [decision 0285](0285-two-watches-of-the-same-read-for-one-person-share-a-run-and-ending-one-never-cancels-a-run-another-rests-on.md), on which check may cancel a shared run. The rest of that record stands, and package W2b builds it.

## Context

Decision 0285, point 3, says a check's run is cancelled only when that check opened it and no other check under way names it. Building it showed one case the words leave open. Watch A's check opens a run; watch B's check is handed the same run. Watch A ends first: its check closes, and the run is left, because B's check still rests on it. Then watch B ends, or the run rests at `needs_login`. B's check did not open the run, so by the words of point 3 nothing ever cancels it. A run left queued is read once more for no watch. A run left resting at `needs_login` stays unfinished, so rule 6 hands it to B's next check after the person resumes the watch, and that check pauses again on the same run.

## Decision

**A check's run is cancelled when a check of a watch opened it and no check under way names it any more.** The opener need not be the check that stops last. In code: `Watches::run_opened_by_a_check` asks whether any check, ended or not, has `opened_run` set for the run, and `Watches::checks_of_run` gives the checks still under way on it. The registry's `run_left_to_cancel` in `crates/a2m-actions/src/handlers/watches.rs` cancels a run only when the first answer is yes and every check the second gives is one that is stopping now. All four places that cancel a check's run apply it: `watch.end`, the runs in `DueWatches::cancel`, a resting run after its check ended, and `watches_after_publish`, where every check of the old version stops at once, and each run is cancelled once.

What decision 0285 protects stays protected. A run no check opened, such as the person's own call that a check joined, has no `opened_run` mark and is never cancelled for a watch. A run another check still rests on is not cancelled. The person's own call that was handed a run a check opened (point 5) is still not protected, as before.

Two smaller choices go with it:

- **A check that opens a run opens one no check names yet.** A trigger of migration 0025 refuses a check marked as the opener of a run that some check names already, and `begin_check` refuses it with `schema_invalid`. The mark never changes.
- **A row from before migration 0025 holds `opened_run = 1`.** Before, every check was treated as the opener of its run, so a check under way at the upgrade ends as it would have. A run two checks shared before the upgrade still gets the second guard, since it is cancelled only once no check under way names it.

## Alternatives weighed

- **Point 3 as written: only the check that opened the run cancels it.** Not taken, for the run left behind above.
- **Keep the opener's identity on the run itself, as a column of `runs`.** Not taken: the ledger's run rows know nothing of watches, and `watch_checks` already holds one row per check and run.

## Consequences

- Tests: `ending_one_watch_leaves_the_run_another_rests_on_and_it_ends_with_its_result`, `ending_the_last_watch_on_a_run_its_check_opened_cancels_the_run`, `a_shared_run_that_waits_for_a_sign_in_pauses_every_watch_resting_on_it`, and `a_check_that_joined_a_run_it_did_not_open_never_cancels_it` in `crates/a2m-actions/src/tests/watches.rs`; `two_watches_of_one_read_share_a_run_and_only_the_opener_is_marked` in `crates/a2m-ledger/tests/watch.rs`.
- `Watches::begin_check` takes a fourth argument, `opened`; `Scheduler::submit_check` passes whether admission placed a job.

## Revisit if

The ledger starts to record which callers were handed an earlier run (point 5 of decision 0285): the person's own call would then be one more thing that rests on a run, and `run_left_to_cancel` would ask about it too.
