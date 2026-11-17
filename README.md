# Anything2MCP

**Turn software and systems into safe, deterministic, agent-accessible capabilities.**

Rust-powered core. Python-first distribution. MCP-native interfaces.

[![CI](https://github.com/MHughesDev/anything2mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/MHughesDev/anything2mcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Anything2MCP latches onto a system that already exists and lays verified tools over it for AI agents to call. The system does not have to cooperate, publish an interface, or know that it is being wrapped. You point the toolkit at something you are entitled to use; it finds out what the system can do, helps you turn that into tool definitions, proves each one, and serves them over the Model Context Protocol to any agent that speaks it.

> **Status: pre-alpha.** The design is written and the foundations are in place: a Rust workspace that builds and tests on Windows, macOS, and Linux, a test application, and continuous checks. The engine itself is not built yet, and there is nothing to install. See [what works today](#what-works-today).

## Why

An agent that clicks through an application, or calls an interface it has guessed at, gives a different result on a different day, and cannot tell you whether a save was saved. That is acceptable for a demonstration and not for work that matters.

Anything2MCP takes the opposite position. An agent, or a person, does the thinking once, at build time. What comes out is a set of plain files that describe exactly how each tool works. From then on a run is a fixed replay with no model in it, and every write is read back from the system before it is called done.

## What it promises

| | |
| --- | --- |
| **Deterministic** | No model runs inside a tool call. The same input against the same system takes the same steps |
| **Verified** | A write is recorded before it is attempted, read back afterwards, and compared. A run that is interrupted resumes without writing twice |
| **Honest about evidence** | Everything it learns about a system carries a level, from `inferred` to `approved`. A guess never passes as a fact, and only proven tools are listed |
| **Safe with secrets** | Sessions, tokens, and passwords stay in the engine. They never appear in a tool's input, its result, a log, or a trace |
| **Open in how it operates** | It identifies itself as automation, keeps to rate limits, and stops at a block. It does not get past sign-in, licence, or bot protections |
| **Neutral** | Any MCP client can use the tools, whoever made it. Nothing phones home |

## How it works

```text
   discover          model            verify           publish            run
  ──────────►     ──────────►     ──────────►      ──────────►      ──────────►
  what the        plain files     gates, on a       a person         fixed replay,
  system can      describing      test record:      reads the        every write
  do, with        each tool       round trip,       summary and      read back
  evidence                        resume, faults    says yes
```

1. **Discover.** The scout identifies each surface of the system, reads what the system publishes about itself, watches it in use, and compares the two. It writes nothing to the system.
2. **Model.** What follows by rule is drafted for you. You, or your agent, supply the judgment: what things mean, which capabilities matter, and everything about writes.
3. **Verify.** Each tool runs against test records through a fixed set of gates. A tool that cannot be proven is not published.
4. **Publish.** A person reads a summary ordered by risk and publishes a version. An agent cannot do this step.
5. **Run.** Agents call the tools. The engine picks the fastest way that has been proven correct, and stops with a named failure when anything is not as described.

[The scout, and who does what](docs/spec/4-building-tools/402-the-scout-and-who-does-what.md) draws the line between what the toolkit automates and what it leaves to you.

## What it can latch onto

A neutral kernel does the work that is the same everywhere. A pack knows one kind of system. Packs arrive one at a time, and each must clear the same bar before it is accepted.

| Kind of system | Reached through | State |
| --- | --- | --- |
| Web applications | The interface, as a person uses it; the requests the page itself makes; tools a site publishes | Designed; built first |
| HTTP services | Direct requests; published descriptions such as OpenAPI, GraphQL, and OData | Designed |
| Existing MCP servers | Pass-through to named tools only, with verification added | Designed |
| Desktop applications | The operating system's accessibility interfaces | Designed in outline |
| Command-line programs | Declared commands with typed arguments, never a free-form shell | Designed; built second |
| Databases | Declared statements with typed parameters; read-only first | Designed |
| Files | Named places in a file; a write replaces the file in one step | Designed |
| Streams and queues | Bounded waits and keyed publishes; never consuming what others expect | Designed |
| Devices and firmware | The device's own interface, with a range and a hazard on every writable point; firmware updated only by the device's own route | Designed |

[What it latches onto](docs/spec/1-product/106-what-it-latches-onto.md) has the full catalog, with what each kind needs and what is refused.

## Configurable, up to the point of safety

An empty settings file is a complete configuration: the defaults are the safe single-user installation. From there nearly everything can be set, in layers: a deployment profile, a preset such as `cautious` or one for continuous integration, the installation's file, your own, the project's, environment variables, and flags. Each pack brings its own settings. The layers closest to a project can only make things stricter, and the rules that make the product safe have no setting at all. [Configuration](docs/spec/2-kernel/226-configuration.md) is the full design.

## What works today

- A Cargo workspace of 24 crates with the kernel, the packs, and the product in enforced layers. The first kernel crate holds the shared vocabulary and a secret type that cannot be printed; the product crate's `anything2mcp` command reports its version; the rest are empty.
- A test application with a web interface, a JSON interface, a control interface that reports ground truth, and injectable faults.
- Continuous checks on three operating systems: formatting, lints, build, tests, a closed list of dependency licences, and an audit that keeps the documents and the crates consistent with each other.
- The design: a specification of 59 sections in seven parts, the decisions behind them, the research they rest on, and a register of the bets still to be tested.
- Findings, the first piece of discovery: what was learned about a system, how strongly, and on whose word, with rules that stop a guess passing as a fact.
- The architecture: a design for every crate, written before its code, and one page that says who defines and joins each interface.

What comes next is in the [roadmap](docs/plan/roadmap.md). Progress is in the [changelog](CHANGELOG.md) and, session by session, in the [journal](docs/journal/README.md).

## Installing

Not yet. The first releases will be placeholders that hold the name. When there is something to use, it will be one install by whichever route suits you, and each delivers the same binary:

```text
pip install anything2mcp
cargo install anything2mcp
npm install -g anything2mcp
```

## Documentation

| You want to | Read |
| --- | --- |
| Understand what this is | [Summary](docs/spec/1-product/101-summary.md) and [what it latches onto](docs/spec/1-product/106-what-it-latches-onto.md) |
| Know what "best" is being held to | [The bar](docs/spec/1-product/107-the-bar.md) |
| Configure it | [Configuration](docs/spec/2-kernel/226-configuration.md) |
| See the whole design | [The specification](docs/spec/README.md) |
| See what was decided and why | [Decision records](docs/decisions/README.md) |
| Check what is researched and what is a guess | [Research notes](docs/research/README.md) and [the hypotheses](docs/hypotheses/README.md) |
| See how the code fits together | [The architecture](docs/architecture/README.md), with one design per crate |
| Know the plan | [The roadmap](docs/plan/roadmap.md): every work package to version 1.0 |
| See what it should feel like when finished | [The walkthrough](docs/reference/walkthrough.md) |
| Build it | [AGENTS.md](AGENTS.md) and [CONTRIBUTING.md](CONTRIBUTING.md) |

## Building from source

You need the Rust toolchain named in `rust-toolchain.toml`; `rustup` installs it on first use. Python 3.11 or newer runs the documentation audit.

```text
cargo build --workspace
cargo test --workspace
python3 tools/docs_audit.py
```

## Use it responsibly

Anything2MCP works through access you already have. Wrapping a system you do not own may be restricted by its terms, by law where you are, or by your organization's policy. Inspecting software, a device, or its firmware can be restricted in the same ways. Get agreement from the system's owner before using it on real work. The toolkit will not help you get past a protection, and it records who agreed to a system being wrapped.

## Contributing

Contributions are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) explains how the project works and what a change needs. Please report a vulnerability privately, as [SECURITY.md](SECURITY.md) describes. Everyone taking part is expected to follow the [code of conduct](CODE_OF_CONDUCT.md).

## License

MIT. See [LICENSE](LICENSE).
