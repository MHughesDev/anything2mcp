# 0999. Debug builds keep line tables of the workspace and no debug information of the dependencies

**Status:** Accepted. Design decision, made at the lead's request that each debug and test build be smaller on disk and faster to link, here and in CI, with no check weakened. The owner had not read this record when it was merged.

**Date:** 2026-11-25

**Changes:** the root `Cargo.toml` gains `[profile.dev] debug = "line-tables-only"` and `[profile.dev.package."*"] debug = false`; the `test` profile inherits both. CI no longer sets `CARGO_PROFILE_DEV_DEBUG`, and `tools/gate.py` no longer sets it either, because the profile now says it for every build. CI's cache of compiled dependencies is keyed on the root manifest as well as the lock file and the toolchain. The release profile, the linker, and every step of CI are unchanged. Builds on [decision 0372](0372-a-push-waits-for-the-checks-of-cis-linux-job-run-here-and-ci-keeps-the-compiled-dependencies.md), whose fourth point it moves from CI's environment into the manifest.

## Context

Decision 0372 gave CI's debug and test builds line tables only, by an environment variable, so a build outside CI and the gate still carried full debug information. A full workspace test build through the gate takes about 14 GB and about 17 minutes on the machine the lead works on, and CI's Rust jobs take about 50 minutes on macOS.

## What was measured

On one Linux machine with four processors, shared with other builders (load average 6 to 12, so times are rough and sizes are exact), in a fresh build folder each time, `cargo test -p a2m-engine -p a2m-pack-web-ui --locked --no-run`: 30 test programs. Free disk was under 20 GB, so the whole workspace was not measured.

| Settings | Build folder | Fresh build |
| --- | --- | --- |
| Full debug information (Cargo's default, before this record, outside CI) | 7.5 GB | 82 s |
| Line tables everywhere (CI and the gate, before this record) | 3.3 GB | 84 s |
| **Line tables of the workspace, none of the dependencies (this record)** | **2.8 GB** | 52 to 149 s |
| The same, and split debug information (`split-debuginfo = "unpacked"`) | 2.5 GB | 62 s |
| No debug information at all | 1.8 GB | 67 to 79 s |
| No debug information, dependencies at `opt-level = 1` | 1.6 GB | 200 s |

The dependencies' archives were read after the chosen build: none of tokio's objects holds a debug section, and an engine test program holds 6.5 MB of line tables.

**The linker.** The pinned toolchain links `x86_64-unknown-linux-gnu` with its own `rust-lld` already (the programs' `.comment` section says `Linker: LLD 23.1.5`). One test program's link, saved with `-C save-temps` and run five times by each linker in turn: `rust-lld` 0.40 to 0.91 s, `mold` 2.30, installed from the distribution under the MIT licence, 0.39 to 1.48 s, and a program a quarter larger, the system's `ld.bfd` 2.65 to 3.19 s. `mold` is no faster than what is already used, so no linker is set and no job installs one.

**This machine's own settings.** A cargo configuration above the repository's folders, `/home/user/.cargo/config.toml`, outside the repository, sets `debug = 0` for the `dev` and `test` profiles and no incremental builds. It overrides the manifest's profiles, and it overrode the gate's environment variable for every test program: the gate's test build here already carried no debug information (1.8 GB on the same 30 programs). The 14 GB of the gate's folder here is code, symbols, and libraries, not debug information, so this record does not shrink it; the gate here keeps building as it did.

## Decision

1. **Line tables for the workspace's own crates,** in the `dev` profile of the manifest, which `test` inherits: a backtrace names the file and line of each frame of the workspace's code. This is what CI already built; it is now what every debug build gets.
2. **No debug information for the dependencies.** A frame in a dependency still shows its function's name, and a panic's message still names its file and line, which the panic carries in its location and not in debug information. The two crates the vault optimizes (decision 0253) take this as well, since a package's own override falls back to `"*"` for what it does not set.
3. **The cache key includes the root manifest.** The profiles say how a dependency is compiled; a cache entry stored under the old profiles and restored under the new ones would be rebuilt in every run and never stored again until the lock file changed.
4. **What each system's job gets.** Linux, macOS, and Windows: the same profiles, so line tables for the workspace and none for the dependencies, where before the dependencies had line tables too; the first run on each system builds its dependencies again and stores a smaller entry. Linux keeps the toolchain's `rust-lld`; macOS keeps its system linker and its default of unpacked debug information; Windows keeps the MSVC linker, which now writes no debug information for the dependencies into each program's database. Nothing was measured on macOS or Windows.

## Alternatives considered

- **Split debug information on Linux.** A further 0.3 GB of 2.8 GB, and a backtrace read its lines from the separate files. Not taken: `split-debuginfo = "unpacked"` is refused on Windows by the pinned compiler, so it cannot be in a profile every system reads; set by a flag for Linux alone it would also reach the release build, which is not to change; and the machine whose disk is short already builds with no debug information.
- **`mold` or `lld` as the linker on Linux.** The toolchain already links with `lld`; `mold` measured no faster.
- **Dependencies at `opt-level = 1`.** 11% smaller and about two and a half times the fresh build. It might make tests run faster, which was not measured; not taken until it is.
- **No debug information anywhere.** The smallest, and a backtrace of the workspace's own code would lose its lines.
