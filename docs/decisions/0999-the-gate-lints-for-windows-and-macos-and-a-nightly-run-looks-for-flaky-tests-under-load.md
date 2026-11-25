# 0999. The gate lints for Windows and macOS, and a nightly run looks for flaky tests under load

**Status:** Accepted. Design decision, made at the lead's request that the lints of the other two systems and the search for races under load, both run by hand on 2026-11-25, be kept tools. The owner had not read this record when it was merged.

**Date:** 2026-11-25

**Changes:** `tools/gate.py` runs the lints for Windows and macOS in every run on Linux, where [decision 0372](0372-a-push-waits-for-the-checks-of-cis-linux-job-run-here-and-ci-keeps-the-compiled-dependencies.md) ran them only with `--cross`, which is still accepted and adds nothing. The stand-ins in `tools/cross/` gain a guard, `tools/cross/lintonly.py`. A new tool, `tools/flaky.py`, and a new workflow, `.github/workflows/flaky.yml`, look for flaky tests every night. `tools/test_gate.py` proves all three. No crate, no test, and no job of `ci.yml` changes; the shards of [decision 0449](0449-each-systems-rust-tests-run-in-three-shards-proven-to-be-one-run.md) run `--tests-only` and so lint nothing, as before.

## Context

Clippy built for another system sees the code that system compiles and Linux never does: the `cfg(windows)` and `cfg(target_os = "macos")` paths of the key store, the desktop pack, the pipes, and the network readers. Before this, a lint failure there was found by CI's Windows or macOS job, twenty minutes or more after a push. Clippy links nothing, so its analysis is real without a linker; only the build scripts of the C dependencies (`libsqlite3-sys`, `ring`) need a C compiler and an archiver for the target, which a Linux machine has not got. Builders ran the two lints by hand with `CC_<target>` and `AR_<target>` pointed at stand-ins that write empty objects.

Races between tests that start engines, browsers, and child processes, or take locks and ports, pass on an idle runner and fail on a busy one. On 2026-11-25 builders found some by running the tests beside several `yes > /dev/null`.

## Decision

1. **The cross lints are in the default gate.** On a Linux machine, every run of the gate, `--fast` and the first shard of `--shard` included, runs after the Linux lints: `rustup target add x86_64-pc-windows-msvc aarch64-apple-darwin`, a no-op when both are present, and then `cargo clippy --workspace --all-targets --locked --keep-going --target <target> -- -D warnings` for each. The Windows target is `msvc`, not `gnu`, because CI's Windows job and the release build compile for it, and `cfg(target_env)` differs between the two. On another system the gate skips the two and says so.
2. **Measured here,** on a machine of four processors with the toolchain of `rust-toolchain.toml`: from an empty build folder, the two lints took 52 and 67 seconds in one run and 102 and 55 in another (about two to two and a half minutes together, about 490 MB of folder); with nothing changed, 0.7 seconds together; after a change to `a2m-core`, which every crate depends on, 32 and 33 seconds. The tests of the gate take tens of minutes, so the two lints are a small share of a full run and about a minute of a `--fast` run after a change, which is why they are default and not behind a flag.
3. **The stand-ins are for these lints alone.** `nocc.py` writes an empty object with the host's compiler and `noar.py` an empty archive; code built with them is not what it claims to be. Two guards hold them to a lint. The gate gives them, with `A2M_LINT_ONLY_STUBS=1`, only to a step whose command is `cargo clippy`, and refuses any other step that names them. Each stand-in, before it writes anything, refuses with exit code 3 unless `A2M_LINT_ONLY_STUBS` is 1 and the nearest cargo among the processes that started it runs `cargo check`, the command `cargo clippy` runs, which writes metadata and never links. It reads the processes from `/proc`, and refuses where it cannot. So `cargo build`, `cargo test`, `cargo run`, and `cargo install` with the stand-ins fail in the C dependency's build script, and no workflow names them. The archiver also no longer takes a path that starts with a slash for a `lib.exe` option: on Linux every path does, and the archive was never written.
4. **A nightly run looks for flaky tests.** `.github/workflows/flaky.yml` runs at 02:41 UTC and by hand, on Linux alone, for at most sixty minutes; no pull request or push starts it, so it is never a required check. It runs `tools/flaky.py --repeat 4 --minutes 45`, which builds the tests as `cargo test --workspace` builds them, starts one busy process per processor, and then runs, from each package's folder as the gate does: the test programs of the focus packages once (`a2m-engine`, `a2m-ledger`, `a2m-local`, `a2m-mcp`, `a2m-pack-web-browser`, `a2m-pack-web-ui`, `a2m-provider`, `a2m-runner`, `a2m-sched`, and `anything2mcp`, whose tests start engines, browsers, stands, and child processes, or take locks); every other program once, starting from a different one each day, while the 45 minutes allow; and the focus programs four more times, each repeat started only when the time it took before still fits. A program that runs longer than twenty minutes is stopped as hung. Every failure is reported with its pass, its program, and each test it names, with its output, and fails the run. A failing program is never run again to pass: each program runs as many times as the schedule says, whether it failed or not. The doctests are not run.

## Expected time

CI's Linux job ran the workspace's tests in about 36 minutes, unloaded, on 2026-11-25. Under load they will take longer, so a night may not reach every program outside the focus packages; those not reached are named in the log and are not failures, and the daily rotation reaches them on later nights. The first scheduled run measures the night; if it never reaches the repeats, the focus list or the load is cut, by a new record.

## Alternatives considered

- **The cross lints behind `--cross`, as before.** A flag no one passes finds nothing; two minutes from cold and seconds after are cheap enough for every run.
- **A real cross compiler (`mingw-w64`, `zig cc`, or `cargo-zigbuild`).** It would build real objects, which a lint does not need, and adds a tool of some hundred megabytes to every builder's machine. Not taken.
- **The guard as the environment variable alone.** A variable is copied with the rest of an environment into a build that links; the cargo command above the stand-in is what says whether anything is linked.
- **Load and repeats on every pull request.** Slower, and it would make a race a red check on a change that did not cause it. A nightly run names it without holding anyone's merge.
- **`cargo-nextest` with retries off and a repeat count.** It runs each test in a process of its own, which is not how `cargo test` runs them, so a race between tests of one program would not be seen ([decision 0449](0449-each-systems-rust-tests-run-in-three-shards-proven-to-be-one-run.md) gives the rest).
- **Every program repeated.** Over the hour; most programs start nothing and hold no lock.

## Consequences

- `gate.py` now gives each test program the name of its package in `CARGO_PKG_NAME`: it gave the version, `0.0.1`, for a package whose folder is its name, because cargo's package id leaves the name out then. The records of decision 0449 key on the program's file name and do not change.
- A failure in the nightly run is a finding: a fix, or a record of the race, like any other.
