# 0481. A run's trace ends with the time each part of the control plane took, and the two budgets it splits are measured

**Status:** Accepted. Design decision. The owner had not read this record when it was merged. Carries out roadmap package 1.9b. Changes a public contract of `a2m-logs` (the trace's end line and `TraceBuilder`) by an addition. Confirms one row of the budget table in [spec section 225](../spec/2-kernel/225-performance.md) and keeps another, not met.

**Date:** 2026-11-25

**Changes:** the end line of the step trace, schema 1, which gains the optional entry `marks`; `TraceBuilder::mark`, `TimeMarks`, and `Mark` in `a2m-logs`; the engine's guard around `ControlPort`, which counts each call against the part that answers it; the scenarios `call_overhead`, `call_outside_run`, and `warm_tab` in `bench/`; the rows "Policy check, scheduling, and ledger write per call" and "Getting a tab on a warm, signed-in browser" of section 225.

## Context

[Decision 0363](0363-the-budgets-as-first-measured-two-are-revised-and-four-cannot-yet-be-told-from-the-systems-own-time.md) left two budgets unmeasured because the harness's clock sees a whole call and nothing inside it: the policy check, the scheduling, and the ledger write of a call (3 ms), and getting a tab on a warm, signed-in browser (100 ms). The `http_operation` scenario bounds the first at about 9 ms and `ui_step` bounds the second at about 200 ms, and neither bound says how much of itself is the part budgeted.

The engine already passes every call to the control plane through one guard, which gives up a call that never answers. The trace already marks each pass's `queue_us`, the wait for a session's lease, and `prepare_us`, the preparing of the driver, which for the `ui` mode is placing the session's state in the browser and opening the observing tab the read looks through. What was missing was the control plane's share of the attempt, and a way for the harness to read either mark.

## Decision

1. **The end line of a trace carries the attempt's time marks.** A new optional entry, `marks`, holds `run_us`, the time from the engine's start of the job to the end of its trace, and for each part of the control plane the calls made to it and their time together: `policy` (the guard's decision and an approval's check), `scheduling` (rate budgets and record locks), `session` (the keeper's leases and a sign of expiry), `ledger` (the run's events, each intent, act, read-back, and settling, the preview, and the evidence), and `loading` (the compiled target). A part with no call is left out. The marks never enter the signature, as no timing does.
2. **The schema stays at version 1.** The entry is optional and added at the end of the line, which the schema's own rule allows without raising the version: a reader of version 1 that does not know it skips it, and a trace written before it reads with no marks. `EndLine::marks` is `None` for such a trace.
3. **The engine's guard marks the time.** The guard that gives up a call that never answers also counts each call and its time against the part that answers it, from the call's name; every call not named for another part is the ledger's. Before the trace is finished the engine hands it the marks (`TraceBuilder::mark`). The change to the engine is that guard and one line where the trace ends.
4. **What the marks cannot see is said, not guessed.** The command front door, the scheduler's claim of the job and its finish, and the storing of the trace happen outside the engine's run of the job, so no mark holds them. The harness gives that time as its own scenario, `call_outside_run`: a call's time by the harness's clock less the run's `run_us`. It has no budget.
5. **The harness reads the marks from the stored trace.** `call_overhead` and `warm_tab` call the tool through the product's commands, as `http_operation` and `ui_step` do, then export the run's trace with `trace export` and take their number from it: the sum of `policy`, `scheduling`, and `ledger` for the first, and the `prepare_us` of the one pass that read through the `ui` binding for the second. A trace whose marks do not hold together (no policy decision, no ledger record, parts longer than the run, a run longer than the call) fails the scenario rather than giving a number.
6. **The per-call budget of 3 ms is confirmed.** On one Linux x86-64 machine, a release build with the ledger on disk, its load average 5.6 to 6.8 on four processors, `call_overhead` gave a median of 1.09 ms in one run of 500 calls (minimum 0.54 ms, 95th percentile 1.91 ms) and 1.07 ms in a second (minimum 0.76 ms). In the second, the medians by part were: policy 0.18 ms, ledger 0.90 ms, scheduling and session nothing (the read's surface has no rate and no sign-in), and the engine's whole run 1.69 ms.
7. **The warm tab's budget of 100 ms is kept, and is not met.** `warm_tab` gave a median of 143.6 ms in 40 reads (minimum 122.8 ms) and 142.3 ms in a second run (minimum 110.9 ms), on a browser the setup had already started. The number is the product's own choice and not the system's: each call opens a new observing tab, and the browser makes a page and its process for it. A pool of observing tabs kept warm per profile, which section 225's "warm pools" already names for sessions and browsers, is the design that would meet the budget; it belongs to `a2m-pack-web-ui` and is not built here. The test project's surface has no sign-in, so there is no session state to place; a signed-in surface adds that placing, which is not measured.

## What the numbers showed

The `http_operation` overhead in the same run was 8.59 ms at the median. Of that, the engine's run took about 1.7 ms and the call outside the run, `call_outside_run`, took 6.27 ms at the median (minimum 4.55 ms) and 6.48 ms in the second run. The policy check, the scheduling, and the ledger writes inside the run are within their budget with room; most of a request call's overhead is outside the run, in the front door, the scheduler's claim and finish, and the trace's storing, which section 225 already said hold the scheduler's synced commits. That time is now measured as a whole and not yet split: splitting it needs marks in `a2m-sched` and the front door, which are not this package's crates.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Time the parts in the harness by a stand-in control plane that counts calls | It measures a stand-in, not the product; the trace is what the product keeps, and section 225 says each run's trace records its own division of time |
| A new trace line per call to the control plane | Dozens of lines per run, and a run's trace grows with its writes; the budget asks for totals per call, and an end-line entry gives them without changing what a step line is |
| Raise the schema to version 2 | A reader of version 1 would read every new trace correctly without the marks; the schema's own rule keeps the version for an optional entry |
| Put the marks in spans and metrics instead | Those are kept in memory and lost with the engine; the trace is stored with the run and can be read back by `trace.export`, which is how the harness reads it |
| Revise the warm tab to 150 ms | The number is of a choice the product makes, opening a new tab for each call, and a design within the product's reach would meet the target; a revision would bless the choice instead |

## Proof

`crates/a2m-logs/tests/trace.rs`, `the_time_marks_are_in_the_end_line_and_not_in_the_signature`: the end line byte for byte, the marks read back, the signature unchanged by marks, a check of the chain still whole, and a trace with no marks still read. `crates/a2m-engine/tests/read.rs`, `the_step_trace_is_sent_with_its_signature`: a read's trace marks one policy decision, one load, the ledger's records, and nothing for the scheduler or the keeper. `bench/src/marks.rs`, `marks_that_do_not_hold_together_are_never_a_number`; `bench/tests/measure.rs`, `the_marked_scenarios_take_their_numbers_from_each_runs_trace`. The numbers are in `bench/results/a2m-bench-linux-x86_64-package-1-9b.json` and in section 225.

## What would reopen it

A run on a quiet machine, or on Windows or macOS, that puts `call_overhead` over 3 ms; a pool of observing tabs, which would be measured by the same scenario against the same 100 ms; marks in the scheduler and the front door that split `call_outside_run`, which would say whether "scheduling" in the budget's sense should include the scheduler's claim.
