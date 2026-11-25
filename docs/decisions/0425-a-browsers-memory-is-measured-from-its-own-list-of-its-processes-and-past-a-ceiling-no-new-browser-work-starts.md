# 0425. A browser's memory is measured from its own list of its processes, and past a ceiling no new browser work starts

**Status:** Accepted. Design decision, made by the worker on package 9.3g under the lead's brief. The owner had not read this record when it was written. It adds to the browser engine's interface, all additions: `BrowserSettings::memory_limit`, `memory_ceiling`, and `memory_retry`, with the named defaults `MEMORY_LIMIT`, `MEMORY_CEILING`, and `MEMORY_RETRY`; `LaunchReason::MemoryShort` and its `MemoryShort`; and the failure code `busy`/`memory_short`, which the web pack passes on.

**Date:** 2026-11-24

**Changes:** `a2m-pack-web-browser` (`src/memory.rs`, `src/browsers.rs`, `src/error.rs`, `src/protocol.rs`); `a2m-pack-web-ui` (its manifest and `codes::PASSED_ON` name the new code); [section 224](../spec/2-kernel/224-when-the-engine-itself-fails.md)'s row and map; the design of `a2m-pack-web-browser`.

## Context

Section 224 says that when memory runs short no new browser work is started, the call answers `busy` with a retry time, and in the web pack the browser is recycled between runs once past a memory limit. Nothing measured memory. The supervisor in `a2m-provider` starts and ends processes and counts them; it measures no memory. No document said whose memory is measured, how, where the limit is set, or how `busy` carries its wait.

A browser is many processes: itself, a renderer for each site, and its helpers. The supervisor knows the first one; its children are in its process group on Linux and macOS and in its job object on Windows, and listing either takes a different system call on each system. The browser itself answers `SystemInfo.getProcessInfo` over its pipe with every process of its own, each by number and kind. Measured on Chrome 153 on Linux: with no tab it lists itself, its GPU process, and its network and storage services; with tabs open it lists each renderer as well. Summed by proportional set size, a browser of the test application with one tab held between 330 and 380 MB.

## Decision

1. **Which processes are the browser's is the browser's word; what each holds is the operating system's.** The engine asks the browser `SystemInfo.getProcessInfo` and measures each number it names: on Linux the `Pss` of `/proc/<id>/smaps_rollup`, which shares a page among the processes that map it so that the processes of one browser add up to what it holds; on macOS the physical footprint, `ri_phys_footprint` of `proc_pid_rusage`; on Windows the private commit, `PrivateUsage` of `GetProcessMemoryInfo`. These come from `libc` and `windows-sys`, both already in the workspace under licences on the closed list; Linux needs no crate. A number that names no process holds nothing, since a process may end between the list and the measurement. A browser that does not answer within `hang_probe`, names no process, or has a process that cannot be read is **unmeasured**.
2. **Two limits, both settings of `BrowserSettings`.** `memory_limit` is the most one browser may hold: `MEMORY_LIMIT`, 2 GiB. `memory_ceiling` is the most the engine's browsers may hold together for new browser work to start: `MEMORY_CEILING`, 4 GiB, two browsers at their limit. Both are drafts in the sense of section 226, set when first measured on long runs. Neither is a key of the pack's settings group yet: no one has measured a reason for a person to change one, and a key is declared when one is.
3. **Memory is measured when an open begins browser work.** An open of a profile whose browser holds a tab is a run's work going on, and is let go on unmeasured. Every other open, of a new browser, of one none of whose tabs is held, or of a window, measures every open browser that still runs.
4. **Past its limit, a browser is recycled between runs.** At such an open, the profile's own browser, if it is past `memory_limit` or unmeasured, is closed as `Browsers::close` closes it and started afresh, as after repeated hangs (decision 0422). Since this is decided only at an open that finds no tab of it held, a browser is never replaced inside a run.
5. **Past the ceiling, no new browser work starts.** After that recycle, if the browsers measured hold `memory_ceiling` or more, or one is unmeasured, the open starts nothing and hands out no browser: it fails `LaunchFailed(MemoryShort)`, which is `busy`/`memory_short` with the retry time `memory_retry` (`MEMORY_RETRY`, 30 s) as the failure's `retry_after_ms`. Nothing was sent, so the crates above report it as not sent; the web pack passes the failure on as it is. The code passes by itself: browsers past their limit are recycled between runs, and the work in flight ends.
6. **The scheduler is not changed.** It names no pack and measures nothing (it is told the kernel's health). A run whose browser work is refused stops `busy` before anything is sent, and its caller is told when to ask again. Holding queued runs in the queue until memory frees, which section 224's "queued runs wait" says, would need the scheduler to know a job's memory-heavy subsystem and a runner to report pressure; it is not built.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A crate that lists and measures processes on every system | It reads every process of the machine to find a browser's children; the engine needs a few numbers it is told, and the two system crates it already links do that |
| The browser's own memory reporting over the protocol | `Performance.getMetrics` gives a tab's script heap, not a process; the memory histograms are sampled when the browser chooses, so a measurement would not be of now |
| The supervisor lists the browser's children | Its process group and job object would serve, with a different call on each system for each list; the browser's list costs one call and the supervisor's contract stays as it is |
| Resident set size summed | Shared pages are counted once per process: a browser's processes add up to more than it holds |
| One limit for both | With one limit a browser past it between runs is recycled and the run goes, so no run is ever refused; one browser's limit and the room all of them may take are different numbers |
| The machine's free memory as the floor | A truer "memory runs short", and the analogue of `store.free_space_floor`. Its call differs on each system and its value moves with everything else the person runs, so a test cannot set it below the use; the engine's own browsers are what the engine can recycle and refuse. Not built |
| Refuse also an open inside a run | A run's later step would stop on memory it has already been given; letting its work go on and refusing new work is what section 224 says |
| Treat an unmeasured browser as within its limits | Fails open: a system on which measuring breaks would never refuse or recycle. Unmeasured counts as past both |

## Consequences

- Every open that begins browser work costs one protocol call to each running browser and a read of a few files or system calls, off the async threads.
- On a system where measuring fails, a lone profile's browser is started afresh at every run, and a second profile's open is refused while the first runs: slow and safe, and the failure says why.
- Proven on Linux by `crates/a2m-pack-web-browser/src/tests/memory.rs`: `with_the_ceiling_below_the_browsers_use_a_new_runs_browser_work_is_not_started_and_with_it_raised_the_run_goes` (the ceiling set to half the measured use: the next open is `busy`/`memory_short` with `retry_after_ms` 2500, nothing is started or handed out, and another profile is refused too; raised, the run reads on the same browser) and `a_browser_past_its_memory_limit_is_replaced_between_two_runs_and_never_inside_one`; `src/memory.rs` measures the test's own process on each system. On macOS and Windows the system calls are type-checked here and first run in CI.
- Reopen if a long run shows the defaults wrong, if the browser stops listing its processes, or when the scheduler can hold a run back for a subsystem's pressure.
