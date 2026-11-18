# Anything2MCP

**Turn software and systems into safe, deterministic, agent-accessible capabilities.**

Rust-powered core. Python-first distribution. MCP-native interfaces.

[![CI](https://github.com/MHughesDev/anything2mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/MHughesDev/anything2mcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Anything2MCP is a toolkit that wraps a system you are already entitled to use (a web application, an HTTP service, a command-line program, a database, files, a message stream, a device) in tools that any MCP client can call, and that do the same thing every time.

- **No model runs inside a tool call.** A tool is a compiled plan. A step that cannot be done as written stops with a named failure.
- **A write is recorded first, then read back and compared.** A run whose answer was lost is reconciled before anything is sent again.
- **A person at the machine approves a write and publishes a tool.** An agent asks; it cannot approve or publish, and no command, tool, or route does it for them.
- **Secrets never reach the caller.** Tokens and passwords stay in the engine's vault; the tests plant them and find them in no result, log, trace, or file.
- **Nothing phones home.** Usage data, crash reports, and update checks are off unless a person switches them on.

## Status

**Pre-alpha. Not yet published to PyPI, npm, or crates.io**: those names are not held yet, so `pip install anything2mcp` does not work today. You build it from source.

What exists and is tested in CI on Windows, macOS, and Linux: the kernel, nine packs, the local engine with its command line and stdio bridge, the tool-building flow (scout, drafts, recorded fixtures, gates, promotion, publish on a page by a person), and verified writes with reconciliation, all against the built-in test stands. Separate CI jobs run the data pack against a real PostgreSQL server, the stream pack against a real NATS server, the desktop pack on Linux against its stand, the Linux presence prompt against a real polkit, and the wheel and npm packages on the three systems.

Not built yet: plug-ins, MCP over HTTP for a shared server, remote runners, ten of the twelve publish gates (the schema and lint gate and the offline replay run; `test` lists the rest as not run), and drafting of writes (the scout drafts reads only). The presence prompts of macOS and Windows are written and not yet run by a person. The [roadmap](docs/plan/roadmap.md) has every package and its state.

## Try it from source

You need the Rust toolchain (`rustup` installs the pinned one) and a Unix shell. This builds the program and the test application, an imaginary order system, and turns one of its reads into a tool. The engine gets a home of its own beside the project, not inside it, on a short path: its socket lives there, and Unix limits a socket's path to about 100 bytes. On Linux this build has no operating-system key store, so the vault takes its root key from a passphrase by default: set it in the shell (16 bytes or more, and keep it), and a command hands it to the engine it starts.

```sh
git clone https://github.com/MHughesDev/anything2mcp && cd anything2mcp
cargo build -p anything2mcp -p example-app
export PATH="$PWD/target/debug:$PATH"
example-app &                    # example-app is listening on http://127.0.0.1:4010

mkdir ~/a2m-try && cd ~/a2m-try
export A2M_HOME="$PWD/home"
export A2M_VAULT_PASSPHRASE='a passphrase of your own'
mkdir first-project && cd first-project

# Make a project, point it at the test application, and let the scout
# draft and try its reads. Nothing here writes to the application.
anything2mcp init
anything2mcp target add example --address http://127.0.0.1:4010/ --kind http
anything2mcp scout run example        # prints its summary; `scout report example --json` has the whole report
anything2mcp try example --tool example_api_orders_get --input '{"order_id": "A-100"}'
```

```text
Tried: `example_api_orders_get`: proven, 5 of 5 completed.
Tried: `example_api_stock_get`: failed, 2 of 3 completed.   (the stand fails every third call)
result:
  customer: Example Customer One
  order_id: A-100
  status: draft
status: completed      (and a warning: this was a trial of a draft; nothing was published)
```

To make that read a published tool: keep it, record its offline test from one real read (every value replaced by a made-up one), run the gates, promote its binding on that evidence, and ask for the publish.

```sh
for op in api_orders_list api_stock_get orders_delivery_get orders_items_get orders_totals_get; do
  anything2mcp draft delete example --file operations/$op.yaml
done
anything2mcp fixture record example --tool example_api_orders_get --input '{"order_id": "A-100"}'
anything2mcp test example             # passed: true; ten gates listed as not run
anything2mcp binding promote example --tool example_api_orders_get --mode http
anything2mcp publish example --version 1.0.0   # state: needs_person, and a link
```

**This is where a run by hand stops on most machines.** The link opens a page that shows exactly what will be published and which gates ran. Pressing Carry out raises the operating system's presence prompt, and only a person passing it publishes. On a Linux desktop the prompt is polkit's, once an administrator has copied `crates/a2m-local/polkit/io.github.mhughesdev.anything2mcp.presence.policy` into `/usr/share/polkit-1/actions`. In a container or with no desktop, the page says that nothing on the machine can approve, and nothing is published: that is the design, not a fault. The macOS and Windows prompts are written and not yet run by a person, so a publish by hand there is untried. The whole flow, through the publish and `anything2mcp call example_api_orders_get`, is the test `crates/anything2mcp/tests/first_hour.rs`, which stands in for the prompt and runs on all three systems in CI.

`anything2mcp engine stop` stops the engine, `anything2mcp doctor` says what this machine lacks and how to fix it, and `anything2mcp config show` lists every setting and the layer that set it.

## Connect an agent

An MCP client starts `anything2mcp bridge`, which copies MCP messages over standard input and output to the local engine, and starts the engine if none runs. Most clients take a block of this shape in their configuration file; use full paths:

```json
{ "mcpServers": { "anything2mcp": {
    "command": "/full/path/to/anything2mcp",
    "args": ["bridge", "--project", "/full/path/to/first-project"],
    "env": { "A2M_HOME": "/full/path/to/a2m-try/home" }
} } }
```

The default toolset, `use`, serves the published tools and the calls around them (`a2m_tool_search`, `a2m_tool_call`, `a2m_run_resume`, and others). `["bridge", "--toolset", "build", ...]` serves the authoring actions instead, so an agent can do the steps above itself; `a2m_action_run` reaches any action by name. Start the engine from a terminal first (any command does it, and it stops by itself after `engine.idle_stop_minutes` idle), so that the passphrase is in its environment and not in a client's file. Any client that speaks MCP over stdio can use it; none is preferred. `anything2mcp connect` prints this block for your project; a client profile you keep as data writes it into that client's file.

## How it works

```text
   discover          model            verify           publish            run
  ──────────►     ──────────►     ──────────►      ──────────►      ──────────►
  what the        plain files     offline tests    a person         compiled plan,
  system can      describing      and trials;      reads the        every write
  do, with        each tool       the gates        page and         read back
  evidence                                         says yes
```

1. **Discover.** The scout identifies each surface, reads what the system publishes about itself, and looks at it live. It writes nothing to the system.
2. **Model.** Reads that follow by rule are drafted as plain files. You, or your agent, supply the judgment: names, meaning, which reads matter, and every write, which you write yourself.
3. **Verify.** A read is tried five times; an offline test replays a scrubbed fixture; the gates run. A drafted binding cannot act until it is promoted on that evidence.
4. **Publish.** A person reads the page, ordered by risk, and passes the machine's presence prompt. An agent cannot do this step.
5. **Run.** Agents call the tools. By default a write rests at `needs_approval` until a person approves it, and is then recorded, made, and read back. Anything not as described is a named stop. [The scout, and who does what](docs/spec/4-building-tools/402-the-scout-and-who-does-what.md) draws the line between what the toolkit automates and what it leaves to you.

## What it can latch onto

A neutral kernel does the work that is the same everywhere; a pack knows one kind of system, and each must clear [the same bar](docs/spec/2-kernel/202-kernel-and-packs.md). "Built" means proven against the pack's test stand. [What it latches onto](docs/spec/1-product/106-what-it-latches-onto.md) has the full catalog, with what each kind needs and what is refused.

| Kind of system | Reached through | State |
| --- | --- | --- |
| Web applications | The page, as a person uses it, in a browser the engine owns; requests the page itself makes | Built: reads and verified writes. Needs Chrome, Edge, or Chromium. Tools a site publishes: designed |
| HTTP services | Direct requests; an imported OpenAPI description | Built: reads and verified writes. GraphQL and OData: designed |
| Existing MCP servers | Pass-through to the tools a mapping names, with verification added | Built: reads and verified writes, over HTTP, and a local program on macOS and Linux |
| Command-line programs | Declared commands with typed arguments, never a shell | Built: reads and verified writes |
| Databases | Declared statements with bound parameters | Built: reads and verified writes on an embedded database file; PostgreSQL in a build with the `postgres` feature, off by default |
| Files | Named places in delimited text, JSON, and workbooks | Built: reads; verified writes for delimited text and JSON |
| Streams and queues | Bounded reads and keyed publishes, NATS with JetStream | Built: reads and keyed publishes. A publish with no way to read it back ends as acknowledged, not verified |
| Devices | The device's own link, with a range on every writable point | Built: reads and verified writes, against its simulator. Firmware: designed |
| Desktop applications | The operating system's accessibility interface | Built: reads, on Linux only. Windows is being built; macOS is not built |

## What it does not do

Each limit carries its kind, as in [SCOPE.md](SCOPE.md): a rule (one of the ten rules in AGENTS.md, which only the owner changes), a choice (a design choice in force today), or not yet.

- **It does not improvise.** No model runs at run time, so when a page or a response changes, a tool stops with a named failure instead of finding its way around. Rule 1.
- **It does not use your everyday browser profile.** The browser runs with a data directory the engine owns. Rule 5; whether that may ever change is a question open for the owner ([decision 0211](docs/decisions/0211-whether-the-engine-may-ever-act-in-a-session-the-person-already-has.md)).
- **It does not click by pixel or read a screenshot.** It reaches controls through the page or the accessibility tree, or not at all. Rules 1, 3, and 7; whether a fixed, non-learned recognizer may ever read a screen is a question open for the owner ([decision 0212](docs/decisions/0212-whether-an-application-that-offers-only-pixels-is-ever-reached.md)).
- **It does not get past a sign-in, a licence check, or bot protection.** A block is a named stop. Rule 5.
- **It does not loosen its rules by setting.** An empty settings file is the safe single-user installation; layers closer to a project can only make things stricter ([configuration](docs/spec/2-kernel/226-configuration.md)). Choice ([decision 0066](docs/decisions/0066-configuration-in-layers-that-may-only-tighten-and-a-bar-that-is-measured.md)).

## Installing

Today, only from source, as above. A release will bring one binary by three routes: a pip wheel for eight platforms, an npm launcher with eight platform packages, and later crates.io. The release workflow builds and checks the wheels and the npm packages, and has never published; nothing is on a registry yet. [Publishing setup](docs/plan/publishing-setup.md) says what remains, and the [changelog](CHANGELOG.md) will say when it changes.

## Documentation

| You want to | Read |
| --- | --- |
| See the scope, both sides | [SCOPE.md](SCOPE.md): what it does and answers for, and what it does not, with each limit marked as a rule, a choice, or not built yet |
| Understand what this is | [Summary](docs/spec/1-product/101-summary.md) and [what it latches onto](docs/spec/1-product/106-what-it-latches-onto.md) |
| Know what "best" is being held to | [The bar](docs/spec/1-product/107-the-bar.md) |
| Configure it | [Configuration](docs/spec/2-kernel/226-configuration.md) |
| See the whole design | [The specification](docs/spec/README.md) |
| See what was decided and why | [Decision records](docs/decisions/README.md) |
| Check what is researched and what is a guess | [Research notes](docs/research/README.md) and [the hypotheses](docs/hypotheses/README.md) |
| See how the code fits together | [The architecture](docs/architecture/README.md), with one design per crate |
| Know the plan | [The roadmap](docs/plan/roadmap.md) and [the adoption plan](docs/plan/adoption.md) |
| See what it should feel like when finished | [The walkthrough](docs/reference/walkthrough.md) |
| Build it | [AGENTS.md](AGENTS.md) and [CONTRIBUTING.md](CONTRIBUTING.md) |

## Building from source

You need the Rust toolchain named in `rust-toolchain.toml`; `rustup` installs it on first use. Python 3.11 or newer runs the documentation audit.

```text
cargo build --workspace
cargo test --workspace
python3 tools/docs_audit.py
```

The web pack's tests need Chrome, Edge, or Chromium in a usual place; `A2M_TEST_BROWSER` names one elsewhere by its full path, and `A2M_TEST_BROWSER_NO_SANDBOX=1` lets a test run it as the superuser. Tests that need a PostgreSQL server, a NATS server, or a desktop session say they were skipped unless the environment asks for them (`A2M_TEST_POSTGRES` or `A2M_REQUIRE_POSTGRES=1`, `A2M_TEST_NATS`, `A2M_REQUIRE_DESKTOP=1`).

## Use it responsibly

Anything2MCP works through access you already have. Wrapping a system you do not own may be restricted by its terms, by law where you are, or by your organization's policy. Inspecting software, a device, or its firmware can be restricted in the same ways. Get agreement from the system's owner before using it on real work. The toolkit will not help you get past a protection. The design records who agreed to a system being wrapped; that record is not built yet, and the scout's report says so.

## Contributing

Contributions are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) explains how the project works and what a change needs. Please report a vulnerability privately, as [SECURITY.md](SECURITY.md) describes. Everyone taking part is expected to follow the [code of conduct](CODE_OF_CONDUCT.md).

## License

MIT. See [LICENSE](LICENSE).
