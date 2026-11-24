# 0399. The budgets as first measured: two are revised, and four cannot yet be told from the system's own time

**Status:** Accepted. Design decision. The owner had not read this record when it was merged. Carries out package 1.7. Revises two rows of the budget table in [spec section 225](../spec/2-kernel/225-performance.md).

**Date:** 2026-11-24

**Changes:** the rows "Process start to ready" and "`http` and `api` operations" of the budget table in section 225 and in `bench/budgets.json`; the harness in `bench/` (six new scenarios, a resident-memory number in a result, and the memory rows of its tables); the section's account of what was measured.

## Context

Section 225 set eight budgets and said none was measured. Package 1.7 measures them. What the code can be measured against today is a call of a published tool through the product's commands in a running engine, the engine as a child process, and the browser engine of the web pack against the test application. The measurement ran on one shared machine, a release build, Linux x86-64; its load average was about 7 in the lighter run and 20 to 37 in the heavier, so a median is an upper bound and a minimum is nearer the floor.

What the numbers showed, and which assumption each contradicts:

- **Process start to ready was budgeted without the vault's key.** With the key given by passphrase, the route a machine with no key store takes, the start took 359 ms at the median (minimum 301 ms); with the vault left closed it took about 92 to 108 ms. The difference, about 250 ms, is the passphrase's key derivation, which is slow on purpose.
- **The `http` budget left out the ledger.** A call of a published tool with a verified `http` binding took 10.0 ms beyond the same request made directly (minimum total 8.4 ms), where the budget said 5 ms. With the ledger held in memory, the same overhead was 6.4 ms, so about 4 ms is the durable store ([rule 3](../../AGENTS.md): the intent is recorded before the act). The group commit of section 225 helps concurrent runs; a single caller's call pays one durable write.
- **A `ui` step cannot be told from the browser's own work.** The read through the interface binding took 206 ms beyond a request for the page; the browser starts the page, runs its scripts, and draws it, none of which the engine can take away. The product's share is not in the number.
- **Four rows cannot be measured yet.** The split of a call into the policy check, the scheduling, and the ledger write; the warm tab alone; the `page_http` mode; and the `api` mode need either the trace's own division of time (`run trace` is not built) or a benchmark project that binds the mode.
- **Three budgets held.** Listing tools took 1.5 ms (budget 5), idle memory was 37 to 40 MB (budget 50), and a cold first read through a new engine's browser took 0.94 s on the lighter run (budget 1 s for the cold start) and 1.4 s on the heavier.

The wheel and its install were also measured: a locally built wheel is 11.4 MB with a 23.7 MB program in it, installs into an empty environment in 0.6 s, and `a2m` adds about 10 ms to the program's own start (H-28 and H-39; recorded in the register).

## Decision

1. **The start-to-ready budget is split by the route the vault's key takes.** Under 100 ms with the key from the machine's key store; under 600 ms on the passphrase route. Only the second is measured, because the machine measured has no key store; the engine with the vault closed, which is the floor of both, is measured and held beside it without a budget (`start_to_ready_vault_closed`, minimum 94 ms). `bench/budgets.json` holds the 600 ms limit under the name `start_to_ready`, and the section's row says both.
2. **The `http` and `api` budget is 15 ms, measured through the command front door with the durable ledger.** 5 ms stays the aim for the engine's own loop and is written in the row as that. The limit is above the lighter run's 10 ms and the heavier run's 12.5 ms with room for a busy machine; it is a limit for the test to hold a regression against, not a promise that the loop itself is slow.
3. **The `ui` step, warm tab, and cold start budgets are not revised.** The `ui_step` number is an upper bound that includes the browser's drawing, and the budget stays as the target for the product's share until the trace splits the time. The cold start is marginal and one run is over; neither number is enough to move a limit.
4. **The harness measures what it can and says what it cannot.** Six scenarios are added (`list_tools`, `http_operation`, `ui_step`, `cold_browser_start`, `start_to_ready`, `idle_memory`) and one without a budget (`start_to_ready_vault_closed`). A result may carry resident memory, which the memory budget reads. A budget with no scenario is printed as not measured, as before; a scenario that needs the program as a process is skipped, and said to be, when the program is not beside the harness, so `cargo run -p a2m-bench` alone never reports a number it did not measure.
5. **The comparison stays out of CI until a baseline can be trusted.** `a2m-bench compare` compares medians, and the same build's median of `http_operation` moved from 10 ms to 31 ms between two runs on the shared machine while its minimum moved far less. The gate of 10% on a median of a shared runner would fail builds for noise. Nothing is weakened: the comparison is as strict as before. What is needed first is a baseline stored from a quiet runner and a rule for the statistic compared; that is recorded as open below, not decided here.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep every budget at its first value and let the table say "over" | A target the design cannot meet by its own rules (a key derivation that is slow on purpose, a ledger written before the act) is not a target; the row should say what the number is of |
| Raise every over-budget row to fit the measurements | The `ui` row's number is mostly the browser's; raising the limit to it would hide the product's share and bless a number nobody can act on |
| Measure the call overhead by making a stand that skips the ledger | Rule 3: no ledger, no write; a stand without it measures a product that does not exist |
| Run the comparison in CI now on the median | Noise would fail builds, and the way to avoid that, a larger noise floor, would let a real regression through |

## Proof

`bench/tests/measure.rs` runs each scenario against the test application and holds what it reads: the interface read reaches the application as the opening of its page and the request read as a request for its JSON (the scenario refuses to time a read that went the other way), and the program as a process starts and reports its memory when it is beside the harness. `bench/src/budgets.rs` holds the budget file to the table in section 225 (a limit the row does not state fails the test) and reads memory from a result. `bindings/python/tests/test_try_wheel.py` and `bindings/python/tools/try_wheel.py` install a built wheel into an empty environment and run both names and the client's import. The numbers are in `bench/results/a2m-bench-linux-x86_64-package-1-7.json` and in section 225.

## What would reopen it

A run on a quiet machine that puts the `http` overhead under 5 ms (the revision would then be withdrawn), a run with a key store that puts the start over 100 ms, or the trace's division of time showing the product's share of a `ui` step under or over 50 ms. Still open: which statistic the comparison holds in CI, a stored baseline from a quiet runner, the Windows and macOS numbers, the measure of the MCP `tools/list` itself, and benchmark projects that bind `page_http` and `api`.
