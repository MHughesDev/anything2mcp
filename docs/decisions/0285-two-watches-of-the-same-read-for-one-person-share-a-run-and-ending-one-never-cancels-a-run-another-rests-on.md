# 0285. Two watches of the same read for one person share a run, and ending one never cancels a run another rests on

**Status:** Accepted. A design decision by an agent, made on 2026-11-18 from the code of pull request 16; any agent may supersede it with evidence. The owner has not read this record.

**Date:** 2026-11-18

**Refines** [decision 0262](0262-watches-are-held-to-their-end-their-budget-and-their-runs-own-result.md) and [decision 0283](0283-watches-reach-an-agent-through-the-registry-and-their-person-confirms-each-on-the-page.md), on what a check does when its run is one that already exists. **Keeps** ledger rule 6 of [section 208](../spec/2-kernel/208-runs-the-ledger-and-resume.md) as it is. **Changes** the [roadmap](../plan/roadmap.md) (package W2 split, with W2b for the fix below) the watches part of the [design of `a2m-sched`](../architecture/crates/a2m-sched.md), and the watches row of [SCOPE.md](../../SCOPE.md). No code changes with this record.

## Context

A person may have two watches of one published read with the same input: two agents each asked for one, or one agent asked twice with another interval, end, or `ignore` list. What happens then was decided by no record. It follows from code written for other reasons, and was checked against it:

- **The run's identity.** `Ledger::open_run` (`crates/a2m-ledger/src/ledger.rs`) gives back the unfinished run with the same digest of workspace, principal, tool, target, plan hash, environment, and input hash, and opens none (rule 6). A watch's check is opened with the watch's person as its principal (`Kernel::submit_watch_check` in `crates/a2m-actions/src/handlers/watches.rs`), so two watches of the same read, version, environment, and input for one person give the same identity. `ignore`, the interval, the end, the phase, and who asked are not part of it.
- **Admission.** `tables::admit` (`crates/a2m-sched/src/tables.rs`) returns `Admitted::Existing` before it places a job or takes from any budget. `Scheduler::submit_check` (`crates/a2m-sched/src/watch.rs`) then begins the second watch's check on that run, with no job (`CheckStarted::job` is `None`). `Watches::begin_check` (`crates/a2m-ledger/src/watch.rs`) accepts it: the run is the watch's own read by every field it compares.
- **So:** whenever one watch's check is due while the other's run is still unfinished, one run serves both checks; the second check spends none of the surface's budget and none of the watches' share of it; both checks record that run, and each watch's `watch.changes` names it as `run_id`. The phases of two watches differ (they are made from each watch's identifier), so this happens only when a due time falls while the other check's run is unfinished. The same holds between a check and a call of the same read made by the person under their own identity, whose run has the same principal.
- **Ending each check.** `tend_watches` ends every running watch's check from its run's row and stored result, one watch at a time, so both checks of a shared run end with the same bytes. Each compares them against its own baseline: `RecordDigests::of` takes the watch's identifier and its own `ignore`, so the two watches can report differently from one read.
- **Confirmation.** `Scheduler::confirm_watch` counts each open watch of the surface at its full requests per check, so two watches that will sometimes share a run are counted as two. That errs on the side of the person's system.

What does not hold. A check's run is cancelled by its run's identifier alone, without asking whether anything else rests on it, in four places: `watch_end` cancels the run of the check it closes; `tend_watches` cancels each run in `DueWatches::cancel`, which `end_past_their_end` fills; `tend_watches` cancels a run that rests (`needs_login`, `needs_input`, `stopped`) once its check has ended; and `watches_after_publish` cancels the runs of checks under way. With a shared run:

1. Ending watch A, by its person, its asker, or its end, cancels the run watch B's check rests on. B's check then ends as a gap with `watch_check_not_completed`, because `Ledger::set_state` clears the failure of a cancelled run, and `run_end` makes a gap of a cancelled run with no failure.
2. When a shared run rests at `needs_login`, the first watch's check ends and pauses its watch for the sign-in, and the run is cancelled; the second watch's check, ended later in the same pass, reads a cancelled run and records a gap instead of pausing. Decision 0283 ordered the end before the cancel so that a check keeps the reason its run rested; with two checks the order holds for the first only.
3. Where the check joined a call the person made under their own identity, ending the watch, or the run resting, cancels the person's own call.

`Watches::check_of_run` answers one check of a run, the first row the store gives. The scheduler's grant uses it to decide whether a request is a check's and which share it spends. Two checks of one run have the same surface, so either row gives the same share; that use is right as it is.

## Decision

**Two watches of the same read for one person share a run, as two identical calls do under rule 6, and each compares the shared result against its own baseline.** Nothing refuses the second watch. What must be true, and is built except where marked Not yet:

1. Each watch's check names the run it rested on, and each watch's `watch.changes` names it as `run_id`. Built.
2. The second check spends none of the budgets the first one's run already spends. Built.
3. **A check's run is cancelled only when that check opened it and no other check under way names it.** A run a check joined is never cancelled for that check. Not yet: see the code change below.
4. **A shared run that rests ends every check on it before it is cancelled,** so that each watch pauses for the reason the run rested. Not yet: the same change.
5. **A person's own call that a check's run was handed by rule 6 is not cancelled when the watch ends or the run rests.** Not yet, and not reached by the change below: the ledger keeps no mark that a call was handed an earlier run, so nothing can tell that a person's call waits on a run a check opened. Until it does, a person whose own identical call joined a check's run can find it cancelled when that watch ends or its run rests for a sign-in.

### The code change it needs (package W2b)

- In `a2m-ledger`: `Watches::checks_of_run(run)`, every check under way that names the run; and a column of `watch_checks`, set by `Watches::begin_check`, that says whether the check opened its run, from the admission's answer (`CheckStarted::job` is `Some` exactly when it did). A migration adds the column; the [ledger schema](../reference/ledger-schema.md) gains it.
- In `a2m-actions`: `watch_end`, the cancels of `tend_watches` (those of `DueWatches::cancel` and of a resting run), and `watches_after_publish` cancel a check's run only under point 3; `tend_watches` cancels a resting run only after every check on it has ended (point 4).
- In `a2m-sched`: `end_past_their_end` may keep returning the runs of the checks it closes; the registry applies point 3 to them.
- The test, against the test application: two watches of one read and input for one person, the second due while the first's run is unfinished. One run is opened; both `watch.changes` name it; ending the first watch lets the second's check end with the run's result; a shared run that rests at `needs_login` pauses both watches for the sign-in.

## Alternatives weighed

- **(a) Keep sharing, and fix the cancels.** Taken. One read of the person's system serves both watches, which is the gentlest on that system and is what rule 6 already does for two calls. The two watches still differ wherever they were asked to: each has its own baseline, its own `ignore`, its own due times, and its own end.
- **(b) Refuse to confirm a second watch identical to one the person already has open, by name.** Not taken. `ignore` is not part of the run's identity, but it is part of what a watch reports, so two watches that differ in `ignore` can tell the person different things; so can two with another interval or end. A refusal that held only when the read, input, `ignore`, interval, and end were all the same would refuse a watch that is redundant and harmless, since the sharing above already costs the system nothing more, and it would need a sameness rule that has to change with every field a watch gains. A line on the confirmation page that names an open watch of the same read and input is a possible later step (Not yet); it informs, and refuses nothing.
- **(c) Never share: open a run of the check's own for every check.** Not taken: it breaks rule 6 for one kind of caller, and doubles the requests to the person's system for no new information.

## Consequences

- Package W2 of the roadmap is split: W2a (decision 0283) done; W2b, this record's fix, ready; W2c, what decision 0283 left (per-record comparison, removal, the MCP resource and change stream, the condition, retention).
- **The line decision 0283 left owed to the scheduler is done.** A check refused because a budget is spent now waits until that budget refills, and a check refused for any other reason a lease's length; neither waits past the watch's interval (`Scheduler::submit_check`, the `rate_limited` branch, in `crates/a2m-sched/src/watch.rs`). The test is `a_check_refused_for_its_budget_waits_until_the_share_refills_and_misses_nothing` in `crates/a2m-sched/tests/watch.rs`. The design of `a2m-sched` now says so.
- No test today covers two watches that share a run; the behaviour above is read from the code, not observed.

## Revisit if

A watch gains a field that changes what it reads, so that two watches of one read and input no longer read the same thing; or the ledger starts to record which callers were handed an earlier run, which would let point 5 be built.
