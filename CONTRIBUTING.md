# Contributing to Anything2MCP

Thank you for wanting to help. This page says how the project works and what a change needs before it can merge. It applies to people and to AI agents. Agents also follow [AGENTS.md](AGENTS.md), which differs on one point: an agent does not wait for review, and plans, builds, and merges on its own once every check passes (its section "How agents work here").

## How this project works

- **The design is written down, and it is part of the product.** [docs/spec/](docs/spec/README.md) holds it, one section per file. Code and design change together: when they disagree, one of them is fixed in the same change.
- **Decisions have records.** A choice between real alternatives gets a numbered file in [docs/decisions/](docs/decisions/README.md). An old record is never rewritten; a new one supersedes it.
- **The design is a set of bets.** [docs/hypotheses/](docs/hypotheses/README.md) lists them, each with an alternative and a test. Showing that one is wrong is a welcome contribution.
- **It grows core first, in small proven steps.** The [roadmap](docs/plan/roadmap.md) gives the order. A person's change that jumps ahead of it will usually be asked to wait.

## Before you start

- For a bug, open an issue with what you did, what you expected, and what happened.
- For anything that changes the design or adds a capability, a person opens an issue first and says what problem it solves. It saves writing something that does not fit. An agent writes a decision record instead, as AGENTS.md says.
- For a security problem, do not open an issue. Follow [SECURITY.md](SECURITY.md).

## Setting up

You need the Rust toolchain named in `rust-toolchain.toml` (`rustup` installs it on first use) and Python 3.11 or newer.

```text
cargo build --workspace
cargo test --workspace
```

## What every change must pass

Continuous integration runs the three `cargo` commands on Windows, macOS, and Linux, and the two audit commands and a check of dependency licences on Linux. A change merges only when all of them pass. Run them yourself first.

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 tools/docs_audit.py
python3 tools/test_docs_audit.py
```

The documentation audit checks links, counts, names, and the rule that kernel crates never depend on pack crates. It must report no findings.

## What a good change looks like

- **Small.** One concern per commit, and every commit builds and passes its tests.
- **Proven.** New behavior comes with a test. A test of a write asks the test stand what it stored; it never trusts the product's own read path alone.
- **True to the documents.** If the change alters behavior, the spec section changes with it. If it picks between alternatives, it brings a decision record.
- **Noted.** Anything a user would notice gets a line under "Unreleased" in [CHANGELOG.md](CHANGELOG.md).
- **In the project's words.** Use the terms in the [glossary](docs/reference/glossary.md) and no synonyms for them.

## Adding a pack

A pack teaches the product one kind of system, and today it is a crate in this repository. [Writing a pack](docs/architecture/writing-a-pack.md) lists its parts as they are in the code, the eight-item entry bar of [spec section 202](docs/spec/2-kernel/202-kernel-and-packs.md) with the place the file pack clears each, and the proofs a pack owes. Open an issue naming the kind of system and its test stand before writing code. A pack outside this repository is not yet possible, because the plug-in protocol is not built.

## Rules that are never traded away

The ten rules in [AGENTS.md](AGENTS.md) bind every change. The ones contributors meet most often:

- **Nothing in a run calls a model or improvises.** If a step cannot be done as written, it stops with a named failure.
- **Every write is recorded first and proven after:** recorded before it is attempted, read back, and compared. A write that leaves nothing to read back is declared `irreversible`, approved by a person as that, and never called verified.
- **Secrets never reach a caller,** a log, a trace, a fixture, or an error message.
- **Name no real application.** Examples, tests, fixtures, file names, and commit messages use the imaginary `example` target or the test stands in `targets/`.
- **Fixtures are synthetic.** Never commit recorded data from a real system.
- **Operate openly.** Nothing that hides automation or gets past a sign-in, a licence check, a bot defence, or a device's protection.

## Dependencies

A new dependency must exist, be maintained, build on Windows, macOS, and Linux, and carry a licence on the closed list in `deny.toml`. Say in the pull request why it is needed and what was checked.

## Commit messages and pull requests

- Write the subject as a short sentence in the imperative: "Add the read-back comparison", not "added stuff".
- Say why in the body when the reason is not obvious from the change.
- Fill in the pull request template. "How it was proven" matters most.
- A pull request merges when its checks are green. Maintainers, and the agents that work for them, merge; they may ask for changes first.

## Licence of contributions

Anything2MCP is under the [MIT licence](LICENSE). By opening a pull request you agree that your contribution is offered under the same licence, and you confirm that you have the right to offer it. Nothing needs signing.

## Conduct

Everyone taking part follows the [code of conduct](CODE_OF_CONDUCT.md).
