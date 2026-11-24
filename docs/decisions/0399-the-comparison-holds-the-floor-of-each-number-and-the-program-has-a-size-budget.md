# 0399. The comparison holds the floor of each number, and the program has a size budget

**Status:** Accepted. Design decision. The owner had not read this record when it was merged. Carries out package F4 and settles the point [decision 0363](0363-the-budgets-as-first-measured-two-are-revised-and-four-cannot-yet-be-told-from-the-systems-own-time.md) left open: which statistic `a2m-bench compare` holds, and where a baseline comes from.

**Date:** 2026-11-24

**Changes:** `a2m-bench compare` compares the floor of each budgeted number and not its median; a ninth budget, the size of the program, in the budget table of [spec section 225](../spec/2-kernel/225-performance.md) and in `bench/budgets.json`, with a scenario `program_size`; a step of CI that compares a run with a stored baseline when one is stored.

## Context

Decision 0363 held the comparison out of CI because the median of `http_operation` moved from 10 ms to 31 ms between two runs of the same build on a shared machine, while its minimum moved far less. It left two things open: the statistic to compare, and a baseline from a quiet runner.

Package F4 asks for the size of the program and of the wheel, measured with every first-party pack compiled in, against the 25 MB target of hypothesis H-28. Package 1.7 measured both once, by hand, on Linux. Nothing in CI measured either, so the next package that grew the program would not have been seen.

## Decision

1. **The comparison holds the floor.** For a time the floor is the fastest iteration of the run (`min_ns`); for an overhead it is the fastest operation less the fastest baseline operation; for a memory or a size, which are one reading, it is that reading. Load on a shared machine only adds time, so the fastest of 500 iterations (40 for a browser read, 20 for a start of the program) is the best estimate of the code's own time, and it moves little between two runs of the same build. The rule of a regression is unchanged: a rise of more than 10% and more than the noise floor. The limit of a budget is still held to the median, as section 225 says, and `standing` still shows it; the floor is what a comparison holds a build to, not what the limit means. Nothing is weakened: a median that triples with the floor unmoved passes, and a floor that rises with the median unmoved fails, and a test holds each.
2. **A baseline is a file a person stores on purpose.** It is `bench/baseline/<os>-<arch>.json`, a result file of the run on a quiet runner, committed with a line in the journal saying where it was taken. CI compares a run with it when it exists, and says there is none and passes when it does not. This is not the gate being skipped: with no stored number there is nothing to compare, and the comparison already reports a budget with no stored number as a first measurement. No baseline is stored in this change, because the one stored result (`bench/results/`, package 1.7) was taken on a machine whose load average was 7 to 37 and is not a quiet one; a floor taken then is an upper bound, so a gate against it would be loose, not wrong, but the owner of the next quiet run should replace it.
3. **The size of the program has a budget: 25 MB, of the stripped release program.** The scenario `program_size` reads the size of the file `anything2mcp` beside the harness, or the one `A2M_BENCH_PROGRAM` names. A debug build of the harness refuses to measure a size unless the program is named, because a debug program is several times larger and is not what the target is about. CI builds the release program on every operating system and stores the size; on Linux the full benchmark run already has it. The target is of the program, and the wheel that carries it is smaller (11.4 MB against 23.7 MB at package 1.7); the wheel is built by the release workflow and measured by `bindings/python/tools/try_wheel.py`, and a size taken from a compressed file would need a compressor the harness does not have, which is a dependency to verify and record, so it is not added for a number a built wheel gives directly.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Compare the median and raise the noise floor | A floor large enough to swallow a threefold move of the median lets a real regression through |
| Compare the 95th percentile | It is the most affected by load of all the numbers |
| Gate on the minimum of several separate runs | The run already holds 20 to 500 iterations; running it again multiplies the time of CI for a number that is not nearer the floor |
| Store the baseline of package 1.7 and gate on it now | It was taken on a busy machine and by a different release build; the first merge that changes the program would fail or pass by chance |
| Measure the wheel by compressing the program in the harness | A dependency for an estimate of a number that `try_wheel.py` takes from the real wheel |

## Proof

`bench/src/compare.rs` holds, in tests, that a median that moves from 10 ms to 31 ms with an unmoved floor is within, that a floor that rises 9 to 11 ms with an unmoved median is a regression, and that a program that grows by more than 10% is a regression and one over 25 MB is shown as over its target. `bench/tests/measure.rs` holds that the size is the size of the file and that a debug harness measures none. The numbers are in section 225.

## What would reopen it

A floor that proves as noisy as the median on a real quiet runner; a stored baseline that fails builds for the runner's own drift; a release whose program is over 25 MB on another operating system than Linux, which would raise H-28's bet that every first-party pack fits in one program.
