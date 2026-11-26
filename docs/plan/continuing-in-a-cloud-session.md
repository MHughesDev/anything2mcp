# Continuing the build in a new cloud session

This page is for the agent that picks up the build in a fresh Claude Code cloud session, on any account. It says how to set up the machine, where the work stands, and how the lead of the last sessions ran builders, so the next lead can go on without anything from the old session. Read it after the newest [journal](../journal/README.md) entry.

## 1. Set up the cloud environment

A cloud session runs in an environment: its network, its variables, and a setup script that runs as root each time a container is built fresh. Make one for this repository with the environment tools (`create_environment`, then work in a new session on it), named after the repository. It needs no allowed hosts beyond the package registries, which are reachable by default: every dependency comes from crates.io, PyPI, or npm. It needs no secret: nothing here needs a token, and no agent handles a registry token.

The image of the last sessions already had what the build needs: `rustup` with cargo, Python 3.13, Node 22, Docker, and Chromium under `/opt/pw-browsers`. A setup script makes it sure, and warms the toolchain:

```bash
#!/bin/bash
set -e
# The checkout is the folder beside the script that holds rust-toolchain.toml.
repo=$(dirname "$(ls -d */rust-toolchain.toml 2>/dev/null | head -1)")
[ "$repo" != "." ] || exit 0
cd "$repo"
# The toolchain pinned in rust-toolchain.toml, with the targets the gate lints for.
rustup toolchain install
rustup target add x86_64-pc-windows-msvc aarch64-apple-darwin
# The Linux packages CI's jobs install for the presence prompt, the desktop pack, and the window tests.
apt-get update -q && apt-get install -y -q --no-install-recommends polkitd dbus bsdutils jq dbus-daemon at-spi2-core xvfb || true
cargo fetch --locked
```

The browser tests find a browser through `A2M_TEST_BROWSER`. In a cloud container set it, and `A2M_TEST_BROWSER_NO_SANDBOX=1`, as environment variables, to the Chromium the image carries (for example `/opt/pw-browsers/chromium-1243/chrome-linux64/chrome`; find it with `ls /opt/pw-browsers`). With neither set, those tests say so and skip.

**Disk.** A session's writable disk is small (about 17 GB free at best). The whole workspace's build folder takes 6 to 8 GB. Build with `CARGO_PROFILE_DEV_DEBUG=0`, give each builder its own `CARGO_TARGET_DIR` outside the repository, delete a builder's build folder as soon as its work is merged, and keep 4 GB free. Four builders at once is the most the disk holds.

## 2. Where the work stands

- **Branches.** `main` holds everything merged through pull request 27, including the prepared commit of 0.1.0 (`261eee0`). The session branch of the last session holds pull request 28, in draft, with everything since: decisions 0479 to 0493. Merge it when every check is green on Windows, macOS, and Linux; never over red. A new session works on its own branch, cut from that one or from main once 28 is merged.
- **Scope.** The first release is the whole design, the shared server with it, and nothing is cut to reach it sooner ([decision 0494](../decisions/0494-the-first-release-is-the-whole-design-the-shared-server-with-it.md)). Its first task is the sweep that record names: the build order's and the roadmap's rows once held after the release, and the spec's sentences that say so; then the server's design, as records. Before 0494: the first release has its full scope again ([decision 0493](../decisions/0493-the-first-release-has-its-full-scope-again-and-the-prepared-preview-is-not-tagged.md)). The preview's words in the README, the changelog's 0.1.0 section, and the roadmap rows 3.6, 5.6, and OS5 are rewritten to say so. When the release is near, bring the 0.1.0 section up to date with what is under Unreleased, and check `contracts/0.1.0/` against what main then holds (`python3 tools/contracts.py`).
- **What is left.** `python3 tools/progress.py` counts it from the roadmap. Of the packages left, the owner's are the release gate (SR1), the registries (0.2p), and the checks a person makes at a Windows and a macOS machine (3.6, OS5); 5.6 needs a person or an agent timed in the loop on each system. Package 8.3 is held: a start and keys of the desktop pack on Windows and macOS are not built ([decision 0490](../decisions/0490-the-desktop-pack-still-starts-nothing-on-macos-because-no-event-off-the-main-run-loop-says-an-application-answers.md)). The rest is in the [build order](build-order.md).
- **Next free decision number:** 0495.

## 3. How the lead runs builders

The last sessions ran one lead and up to four builders at once, each a subagent in its own copy of the repository. It is how the build went fastest without two agents changing one crate.

1. **A staging copy.** The lead keeps a worktree on a branch `staging`, and pushes it to the session's branch (`git push origin HEAD:<session branch>`).
2. **One builder, one package.** Each builder gets its own worktree (`git worktree add .claude/worktrees/<name> -b w/<name> staging`) and its own build folder (`/home/user/<name>-target`). Two builders never take packages that change the same crate.
3. **A brief.** Each builder's prompt carries the package, its proof from the roadmap, the crates it may change, and the common rules below.
4. **The merge.** When a builder hands back, the lead merges its branch into staging with `--no-ff`, then:
   - gives its record the next free number: `python3 tools/renumber_decision.py <file name of its 0999 record> NNNN`, then replaces every other `0999` in `crates`, `docs`, `CHANGELOG.md`, `bindings`, `tools`, `.github`, `SCOPE.md`, `targets`, `README.md`, `bench`, `RELEASING.md`, and `SECURITY.md`;
   - settles conflicts: in an index table, keep both rows; in the build order and the roadmap, take the builder's row for its own package; for a progress line, keep either, then run `python3 tools/progress.py --write`; then look for any line left starting `<<<<<<<`;
   - runs the checks: `python3 tools/docs_audit.py`, `python3 tools/progress.py --check`, `python3 tools/failure_codes.py --check`, `python3 tools/contracts.py check`, `python3 tools/check_workflows.py`, every `tools/test_*.py`, `python3 bindings/python/tools/generate_actions.py --check`, and `node bindings/npm/scripts/generate-actions.js --check`; and `python3 tools/gate.py` (or `--fast`) for the whole workspace's formatting, lints for three systems, and tests;
   - deletes the builder's build folder and worktree.
5. **CI.** Push, open or update a draft pull request, and read every failed job's log whole. The session cannot run a failed job again; each retry is a push. A run in progress is cancelled by the next push, on main too, so do not push to main's branch while its run matters.

### The common rules of a builder's brief

- Start no agents of your own. Never weaken or delete a check, and never make a test less strict. Stop and report if the brief does not cover what you need.
- First, bring staging in: `git fetch <staging copy> staging && git merge --no-edit FETCH_HEAD`.
- Read AGENTS.md's rules and conventions once, and the design of each crate you touch in `docs/architecture/crates/`.
- Use only your own build folder, with `CARGO_TARGET_DIR` set before every cargo command. Before each build check `df -h`; with less than 3 GB free, stop and report.
- Run checks scoped to the crates changed and those that depend on them: `cargo fmt --all --check`, `cargo clippy -p <crate> --all-targets --locked -- -D warnings`, and `cargo test -p <crate> --locked --no-fail-fast`, and any CI job of the crate that needs a real server. Read the whole output and list every failing test.
- After a document change, `python3 tools/docs_audit.py` reports 0 findings. Mark the package `done` in the roadmap and strike its row in the build order. Add a changelog line under `## [Unreleased]` for what a user would notice.
- A new decision record is numbered 0999; the lead renumbers it.
- Name no real application, and put no model names in the repository or in commit messages.
- Commit on your branch, one concern per commit, and do not push.
- Hand back: done, not done, unsure, gaps decided, changed files, the checks' last lines and every failure, and the commit hashes.

## 4. What the last lead knew that is written nowhere else

A new session shares no memory with the old one, even on the same account. This is what the last lead held in mind.

**The owner.**
- The owner directs in short messages, often typed fast, and wants the work to move without being asked. Per AGENTS.md: decide, record, build, merge. Ask only what is truly the owner's, and then in one question with a recommended answer first.
- The owner asked for the lead to act as an orchestrator: plan, brief builders, merge, and fix CI, with builders doing the building.
- The owner tests on a Windows 11 laptop. The CI job "Wheel and npm packages on windows-latest" stores the artifact `windows-tester-kit` for 14 days: the program, the test application, the wheel, the notices, and [the guide](trying-the-preview-on-windows.md) as its README. Give the owner a fresh link from a green run's artifacts when asked. Turn the owner's notes into entries in `docs/reviews/field-tests.md` and into the Windows halves of 3.6, 5.6, and OS5.
- Permission to run real open-source applications for H-50 and FT6 (reads only, in local containers, no real accounts) was given for that session and is done. Any new run against a real application needs the owner's go-ahead again (rule 10). Writes against a real business application are never allowed.
- The registries were put off by the owner ("later"). Never ask for a token.

**The owner's steps before the first release** (none an agent can take):
1. SR1: in the repository's settings, the environments `release`, `pypi`, and `npm`, each with the owner as a required reviewer and deployments from `main` and `v*` tags only; and a tag ruleset that lets only the owner create a `v*` tag.
2. 0.2p: the trusted publishers on PyPI and npm for `release.yml`, as [publishing setup](publishing-setup.md) says.
3. The checks at a macOS and a Windows machine (3.6, OS5), and 5.6 measured with a person.
4. Then the tag, on the commit the lead names, as [RELEASING.md](../../RELEASING.md) says. The CI-passed check of SR3 is in `release.yml` on main; a tag of a commit runs that commit's workflow.

**CI, as it behaves.**
- A new push cancels the run in progress on the same branch, main included. Cancelled jobs are not failures; read only jobs that ended `failure`.
- The session cannot re-run a job (the API answers 403). Each retry is a push.
- A log too long for the tool is saved to a file; read it with python, and read the failing test's whole output.
- The Rust tests run in three shards per system ("Tests K of 3"). A shard's summary at its end names each failing test.
- An exact count of tests in a CI step (the sign-in window job expects `6 passed; 0 failed; 5 ignored`) must be raised by a builder who adds a test to that step.
- `crates/a2m-provider/tests/paths.rs` counts each file's uses of paths; a builder that adds one updates its entry and reason.
- `crates/a2m-auth/tests/tls_rules.rs` refuses an HTTP client made by hand: a test makes its client with `client_builder` of the test application's `harness` module, or of the `tls` module of `a2m-auth`.
- A bump of the version moves the compiler's version (`crates/a2m-files/src/compile.rs`), the stream pack's manifest, and the hash of a compiled target in the web pack's test; run the whole suite and every tool test after one.

**Faults seen more than once, and their cure.**
- A test that counts what a background task did races it: wait on a named condition (for example the keeper's `idle()`), never a sleep.
- On macOS a peer that closed before the client asked who it is reads as an error of the socket; it is mapped to a lost connection, not a refusal.
- A test that starts a program on macOS must name a copy that nothing runs, or it finds the test program itself running.
- Browser tests run in one step share the runner's cores; a slow one gets a step of its own.
- When the disk fills, builds and test runs fail with misleading errors. Check `df` first, and delete build folders of merged work.

**The lead's own tools,** all in the repository: `tools/gate.py` (CI's Linux checks here, with the Windows and macOS lints through the stand-ins in `tools/cross/`), `tools/progress.py --write` (every count of packages), `tools/renumber_decision.py`, `tools/failure_codes.py`, `tools/contracts.py`, `tools/check_workflows.py`, and `tools/release_files.py`.

## 5. What no agent does

The ten rules and "What an agent does not do alone" in [AGENTS.md](../../AGENTS.md) hold in every session. In particular: no agent tags a release, starts the release workflow, or touches a registry or its token; the release goes out only through `.github/workflows/release.yml`, as [RELEASING.md](../../RELEASING.md) says, when the owner starts it.
