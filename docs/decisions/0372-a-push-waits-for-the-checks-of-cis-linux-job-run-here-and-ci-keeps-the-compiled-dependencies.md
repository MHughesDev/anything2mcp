# 0372. A push waits for the checks of CI's Linux job run here, and CI keeps the compiled dependencies

**Status:** Accepted. Design decision, made at the owner's request that CI become faster, cheaper, and green when code is ready. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** a new tool, `tools/gate.py`, runs the checks of CI's Linux job before a push, and the lead pushes only when it is green; CI restores the compiled dependencies before it builds, in the three jobs of the whole workspace and in the TLS and sign-in window jobs, and stores them when the lock file or the toolchain changed; CI's debug and test builds keep line tables only and build nothing incrementally. No check is removed, narrowed, retried, or made optional, and no job is skipped by the paths a change touches.

## Context

In one pull request on 2026-11-24, a round of CI took about thirty-five minutes from push to the last job, and a red round cost a second round. One round used about 73 minutes of Linux runners, 38 of macOS runners, and 57 of Windows runners. macOS minutes are billed at about ten times, and Windows minutes at about twice, the Linux rate, so macOS was most of the cost. Nothing was kept between runs: each of the nineteen jobs compiled its dependencies again. Windows spent fourteen minutes building and fifteen testing, and most of its build was linking programs that carry full debug information.

Each red round of that day failed on Linux for a reason a run of the Linux job's own steps would have shown before the push: a test that listed the web probe's discoverers and was not told of a new one, a fault test whose expected class an earlier package had changed, and the page-tool tests. Workers ran their checks scoped to the crates they changed, which is right while they work, and nothing ran the whole workspace before the batch went out. CI was where those faults were first found, which is the most expensive place to find them.

## Decision

1. **The gate.** `python3 tools/gate.py` runs, in order and without stopping at the first failure, the documentation audit, the failure codes check, formatting, the lints of the whole workspace, and the tests of the whole workspace, the same commands as CI's Linux job and documents job. The lead runs it on the batch before every push, in a build folder no other copy of the repository builds in, and pushes only when it is green. A failure the gate shows is fixed, or sent to the worker that owns the crate, before the push; it is never pushed to see whether CI agrees. `--fast` stops after the lints, for a check while work is in progress.
2. **The compiled dependencies are kept.** The job that builds the whole workspace restores, on each operating system, the registry and the debug build folder stored under a key made of the operating system and the digests of `rust-toolchain.toml` and `Cargo.lock`. When no entry has that exact key, it stores one at the end of its tests, after `tools/ci_cache.py` has taken out every file of the workspace's own crates, which change with every commit. The TLS job and the sign-in window job restore the same entry and never store. Every step still builds and runs all of its crates; cargo checks each restored dependency as it always does, and rebuilds one whose inputs differ.
3. **Shards and the other two systems.** `--shard K/N` splits the gate over N machines: the first runs the audits and lints, and every shard runs the tests of a share of the packages, shared out by how many tests each holds, so that the shards together run every test once. The machines are other cloud sessions of the same environment, each with its own processors and disk, started on a snapshot pushed with `[skip ci]`, which starts no run of CI. `--cross` adds the lints of the whole workspace built for Windows and for macOS from Linux: nothing is linked, so the two dependencies with C code are given stand-in tools (`tools/cross/`) that write empty objects, and the lints see the code those systems compile and Linux never does. On 2026-11-24 each took about two minutes. It runs no test of those systems; their tests stay in CI.
4. **Leaner debug builds in CI.** `CARGO_INCREMENTAL=0`, because a build in CI is never repeated in the same folder, and `CARGO_PROFILE_DEV_DEBUG=line-tables-only`, which keeps the file and line of each frame a backtrace shows and drops the rest of the debug information. The release profile is untouched, so the benchmark and the size budget measure the program that ships.

## Consequences

- A red CI round should be rare and specific to one operating system, a real server, a display, or a key store, which the gate cannot run here. What it saves is the round itself: about thirty-five minutes and the minutes of every runner.
- The first run after a change of the lock file pays the full build and the upload of the entry; later runs restore it. Entries are kept per branch and on `main`, and a run on a pull request can restore what `main` stored.
- GitHub keeps ten gigabytes of entries for a repository and removes the least used first. Three entries of the debug dependencies fit; if they do not, a run builds as it does today.
- The gate needs its own build folder and so its own disk. On the machine the lead works on, that folder is the second largest thing on the disk; the lead clears it before it fills.

## Alternatives considered

- **Running only the jobs whose paths a change touches.** Rejected: a change in one crate breaks another, as the discoverer list did, and leaving a job out by path is narrowing a check.
- **Retrying failed tests.** Rejected: a retry hides a test that fails for a reason, and every flaky test is a fault to fix.
- **Making the macOS and Windows jobs wait for the Linux one, to spend nothing on them when Linux is red.** Not taken now: with the gate, a red Linux job should be rare, and the wait would add the Linux job's time to every green run. If red Linux rounds go on, this is the next change.
- **The owner's own computers as macOS and Windows machines**, by Remote Control or as runners of CI. Asked, and not now: CI's runners keep those tests.
- **Another hosted CI service.** It needs accounts in the owner's name and adds little over the sessions and the owner's computers.
- **A community caching action for Rust.** Its licence is not on the closed list of permissive licences, so GitHub's own cache action is used, with the pruning the community action would have done written as `tools/ci_cache.py`.
- **A test runner that runs test programs in parallel processes.** It may shorten the Windows test step, but it does not run doctests, runs each test in a process of its own where some tests may share state, and is a new tool to verify. Left to be measured on its own.
