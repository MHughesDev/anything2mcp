# 605. Packaging and release

What is in the repository, in which languages, under which names, how it is packaged for each install route, how a release is produced and updated, and what the product never does unasked. Every install route delivers the same Rust binary. The pip and npm packages add a thin client on top; none of them contains engine logic. [RELEASING.md](../../../RELEASING.md) has the steps of a release, and [publishing setup](../../plan/publishing-setup.md) records where each package name stands today.

## The repository

One Cargo workspace holds the whole system. The engine is a set of small Rust crates behind one binary. Rust is the default for everything; another language is used where it clearly fits better.

```text
anything2mcp/
  Cargo.toml              # workspace
  crates/
    # The kernel: the same whatever kind of system is integrated.
    a2m-core/             # shared types: targets, operations, bindings, effects, failure classes
    a2m-files/            # schemas, loader, linter, plan compiler
    a2m-expr/             # CEL evaluation; host functions for units, numbers, dates
    a2m-vault/            # envelope encryption, key-store back ends, redactor
    a2m-telemetry/        # traces, metrics, OpenTelemetry export
    a2m-ledger/           # run store, ledger, reconcile, audit chain (SQLite, Postgres)
    a2m-policy/           # grants, effect classes, approvals, limits
    a2m-auth/             # auth providers, auth discovery, session keeper
    a2m-sources/          # xlsx, csv, json readers; template fingerprints; mappings
    a2m-provider/         # the provider contract: manifests, the registry, the supervisor, the plug-in host
    a2m-engine/           # engine loop, mode selection, read-back, normalization
    a2m-sched/            # queues, record locks, quotas, runner dispatch
    a2m-actions/          # the action registry: one definition per authoring and admin action
    a2m-local/            # local channel: socket and named pipe, peer check, engine lifecycle
    a2m-mcp/              # MCP server: stdio, Streamable HTTP, toolsets, authoring tools
    a2m-api/              # REST API, OpenAPI generation, webhooks
    a2m-runner/           # runner role and the control-plane link
    # The packs: each knows one kind of system.
    a2m-pack-web-browser/ # web pack base: DevTools-protocol engine: locate, act, wait, network; platform profiles
    a2m-pack-web-ui/      # web pack: ui, page_http, and site_tools drivers; widget drivers; sign-in in a browser
    a2m-pack-web-probe/   # web pack: capability probe, traffic capture, endpoint inference
    a2m-pack-http/        # HTTP pack: http and api drivers; OpenAPI, GraphQL, OData import
    a2m-pack-mcp/         # MCP pack: pass-through to an upstream MCP server, through a mapping file
    a2m-pack-desktop/     # desktop pack: Windows UI Automation, macOS Accessibility, Linux AT-SPI
    # The product: wires the packs into the kernel.
    anything2mcp/         # the `anything2mcp` and `a2m` commands and the embeddable library facade
  bindings/
    python/               # pip package `anything2mcp`: ships the binary and a thin pure-Python SDK
    typescript/           # npm package `anything2mcp`: launcher, platform packages, and a thin SDK
  web/console/            # admin console, approval page, sign-in page (TypeScript)
  plugins/                # plug-in protocol and examples
  profiles/               # platform profiles and detection rules, as data files
  targets/                # the imaginary `example` target; the built-in test application in example-app/
  bench/                  # performance suite and budgets
  docs/                   # the design, the architecture, the decisions, the research, the plan
  tools/                  # the documentation audit and its self-test
  xtask/                  # build, release, and code-generation tasks
```

Which crates may depend on which is set by the layering table in the [technical reference](../../reference/technical-reference.md#crate-layering), and the audit enforces it. The reasoning behind each layer, and which crate defines, implements, and joins every interface, is in the [architecture](../../architecture/README.md). The split into a kernel and packs is [section 202](../2-kernel/202-kernel-and-packs.md). Kernel crates are named `a2m-*`, pack crates `a2m-pack-<pack>` or `a2m-pack-<pack>-<part>`, and only the product crate names a pack.

The folders that are not crates (`targets/`, `bindings/`, `web/console/`, `plugins/`, `profiles/`, `bench/`, `xtask/`, `tools/`) are described, with what each may know and when it is built, in [the parts that are not crates](../../architecture/other-parts.md). `docs/` holds the specification, the architecture with one design per crate, the decision records, the research, the hypotheses, the plan, and the journal.

**Building blocks.** The MCP SDK and the desktop library below were checked in a research round; the other choices come from general knowledge of the Rust ecosystem, and each is confirmed, with its licence on the closed list in `deny.toml`, before it is depended on.

| Need | Choice | Note |
| --- | --- | --- |
| Async runtime, HTTP server and client | Tokio, Axum, Hyper with rustls | Standard, fast, no OpenSSL dependency |
| MCP protocol | `rmcp`, the official Rust SDK | Tracks the 2026-07-28 specification ([docs.rs](https://docs.rs/crate/rmcp)) |
| Browser | Own DevTools-protocol engine, in the web pack | Evaluate chromiumoxide and Rustwright as a base first; package 0.4 decides ([decision 0004](../../decisions/0004-rust-browser-engine-on-the-devtools-protocol.md)) |
| Desktop accessibility | [xa11y](https://docs.rs/crate/xa11y/0.3.0) or per-platform crates | xa11y offers one API over UI Automation, AXUIElement, and AT-SPI2, under MIT; package 8.1 decides |
| Storage | SQLite for one machine, Postgres for servers | One schema, two back ends |
| Expressions | A CEL implementation for Rust | Pre-compiled at publish |
| Spreadsheets, decimals | A pure-Rust xlsx reader; a decimal arithmetic crate | Exact comparison needs decimals, never floats |
| Key store | A cross-platform keyring crate over DPAPI, Keychain, and Secret Service | Outside secret managers on servers |
| OAuth and OpenID Connect | Standards-based crates; no official MSAL for Rust exists | Broker sign-in handled on user devices |
| Telemetry | OpenTelemetry | Traces and metrics in one format |

## Languages: Rust first, others where they fit

Rust is the default ([decision 0002](../../decisions/0002-rust-first-other-languages-where-they-fit.md)). Anything on the path of a run is Rust: the control plane, runners, drivers, the ledger, policy, and the vault. Another language is used where it is clearly the better tool.

| Where | Language | Why |
| --- | --- | --- |
| Admin, approval, and sign-in pages | TypeScript | Runs in a browser |
| Scripts the web pack evaluates inside a target page | JavaScript, kept small and tested | The page only runs JavaScript |
| Clients and install wrappers | Python, TypeScript, and others as callers need | Meets callers in their own language |
| In-process bindings for embedding (deferred) | Python, Node | Lets other software load the engine directly; not in the first release |
| Installers and service setup | Shell, PowerShell | Native to each operating system |
| Database migrations | SQL | Native to the stores |
| Plug-ins and provider packages | Any language | They are separate, supervised processes |
| Optional Firefox and WebKit support | TypeScript, as a Playwright sidecar plug-in | Reuses a mature engine |

A language not listed here may be added when it clearly fits better than Rust for a part that is off the run path; the choice gets a decision record.

**Plug-ins.** New drivers, auth providers, source readers, and discoverers can come from separate processes that speak a small documented protocol over standard input and output, so they can be written in any language. Each declares its capabilities in its manifest, is started and stopped by the supervisor, and never receives raw vault access. Unsigned plug-ins are off by default ([section 203](../2-kernel/203-the-provider-contract.md), [section 204](../2-kernel/204-supervising-subsystems.md)).

**Licence.** MIT, for the code, the documents, and every package built from it ([decision 0057](../../decisions/0057-the-licence-is-mit.md)). Releases built for people to install carry the licence texts of what they contain.

## Names

One name on every route ([decision 0056](../../decisions/0056-one-name-everywhere-anything2mcp.md)).

| Thing | Name |
| --- | --- |
| Product and repository | Anything2MCP, at `MHughesDev/anything2mcp` |
| Command | `anything2mcp`, with `a2m` as a short name for the same program. Documents use the full name |
| Python package and import | `anything2mcp` |
| Crate on crates.io | `anything2mcp`, the binary and the library facade |
| Kernel and pack crates | `a2m-*` and `a2m-pack-*` |
| npm package | `anything2mcp` |
| Built-in tool prefix | `a2m_` |
| Project file and working folder | `anything2mcp.toml` and `.anything2mcp/` |
| Environment variables | `A2M_` and a name, such as `A2M_HOME` |
| Container image | `anything2mcp` under the repository's registry namespace |

`a2m` is never a package name: it belongs to other projects on two registries. There are no alias packages under earlier names; the placeholders that exist under them stay where they are, and [publishing setup](../../plan/publishing-setup.md) says what to do with each.

**A name is held only by a published release.** A registry account, or a publisher configured in advance, reserves nothing. On the date publishing setup records, none of the three names was held: PyPI waits for the first run of the release workflow, and crates.io and npm each need one first publish from the owner's account. Until a row there says held, the name can be lost.

**One binary is shipped.** `a2m` is a link to it, or the smallest launcher a route supports, on each install route ([decision 0063](../../decisions/0063-the-kernels-interfaces-as-the-crate-designs-settle-them.md)). The workspace builds two small binary targets from one module for convenience; the package build is where the link replaces the second target, because two shipped copies would count twice against the size target. The output names the product whatever name started it, and a test runs both names with the same arguments and compares. Which link or launcher each route uses, and its cost against the 30 ms budget for starting a command, is research brief 28 and hypothesis H-39.

## What is shipped

Four layers, from what every install has to what is never bundled.

| Layer | What it carries | How it arrives |
| --- | --- | --- |
| The base | One Rust binary with every first-party pack compiled in and switched off until enabled; a thin client | Any install route below |
| What a pack needs from outside | A browser for the web pack; a database client library where one is not built in | Used where already installed. Fetched only by an explicit install action, such as `browser.install`, never during a run |
| Provider packages | Plug-ins published by this project or anyone else | A pip package of their own, found through a Python entry point; or an extra, `anything2mcp[name]`, that names one |
| Managed components | Outside programs a provider drives, such as an analysis engine or its runtime | Never bundled. Detected, identified by digest, and reported when missing or unknown (`component.list`) |

**Why first-party packs are compiled in.**

- The browser is not bundled, so the binary stays small. The target is a wheel under 25 MB with every first-party pack; hypothesis H-28 measures it.
- One binary is one version, one audit, and one thing to sign.
- A pack that is switched off is never registered, so it costs nothing at run time and cannot be reached.
- Nobody meets a missing-piece error for a common case.

What is heavy, rare, or under a licence that should not be forced on everyone stays outside as a provider package.

**The browser.** By default the web pack uses the Chrome or Edge already installed, with a data directory the engine owns, so an install on a laptop downloads nothing. Servers pin an exact Chromium with `browser.install` for reproducibility.

### Enabling a pack or provider

A pack or provider is in one of four states, and `provider.list` shows which.

| State | Meaning | To move on |
| --- | --- | --- |
| Not installed | The runtime does not have it | Install its package |
| Installed, off | Present and unreachable | Enable it in settings (`provider.enable`). On one machine, enabling a provider that starts processes or reaches the network is a protected setting |
| On, not ready | Enabled, but something it needs is missing | The listing names the missing thing and how to get it |
| Ready | Its health check passes | Nothing |

`doctor.run` checks every enabled provider and prints a fix for each problem, as it does for the browser and the key store.

### Provider packages and Python

A provider package may be written in any language. Python is the first language with a small kit for writing one, because that is what most people integrating a system reach for.

- The package declares an entry point in a group the runtime reads. The entry names the provider's manifest and the command that starts it.
- The runtime starts it as a separate process through the supervisor and speaks the plug-in protocol to it. No Python runs inside the engine, and no engine logic runs in Python.
- A provider installed with pip is listed as present and not enabled until a person enables it.
- Rule six is unchanged: everything on the path of a run is Rust, except a plug-in, which is a declared, supervised process.

## The pip package

The way in is `pip install anything2mcp`.

- **Contents.** One wheel per platform holding the compiled binary and a pure-Python package named `anything2mcp`. The build tool, maturin, supports this directly: its `bin` mode packages a Rust binary into a wheel as a script that lands on the user's path ([maturin](https://maturin.rs/bindings)).
- **No Rust needed to install.** Wheels are prebuilt for Windows, macOS, and Linux on x64 and ARM, including Linux distributions that use musl. A source distribution is published as the fallback and does need a Rust toolchain.
- **One wheel per platform, not per Python version.** The Python part is pure Python, so the wheel does not depend on the interpreter's version.
- **The browser is not included,** as above.
- **What it gives.** The `anything2mcp` command, the engine it starts, and `import anything2mcp`.
- **Today.** `bindings/python/` holds a placeholder at version 0.0.1, built repeatably with a pinned build tool, that contains no working software and says so ([decision 0046](../../decisions/0046-the-pip-name-is-held-by-a-placeholder-published-on-a-version-tag.md)). Its only purpose is to hold the name. The wheel that carries the binary is package F1 of the [roadmap](../../plan/roadmap.md).

## The clients

A client is a thin, typed client of the engine, generated and not written. It contains no engine logic, no platform-specific code, no branch on what a result means, and no credential.

```python
import anything2mcp

client = anything2mcp.connect()                       # local engine, started on demand
order = client.call("example_order_get", order_id="A-100")

run = client.run("example_order_sheet_enter", source="sheet.xlsx", order_id="A-100")
run.wait()                                       # returns when done or when a person is needed
print(run.status, run.summary)

remote = anything2mcp.connect("https://mcp.example.com", toolset="orders")   # a company server
```

- **Two transports.** Local: the client starts `anything2mcp bridge` and exchanges JSON messages with it; the bridge talks to the engine over the local channel ([section 222](../2-kernel/222-front-doors-and-the-local-engine.md)). Remote: HTTPS to a server's REST interface, with OAuth or an API key.
- **The same calls either way.** Code written against a laptop's engine runs unchanged against a company server.
- **Generated.** Models and methods come from the action registry, through a recorded catalog, and from the OpenAPI document, so a client cannot drift from the engine. CI fails when a generated file differs from what the generator produces.
- **Synchronous and asynchronous clients,** with few or no third-party dependencies.
- **Compatibility.** A client states the engine interface version it needs and gives a clear error on a mismatch.

**No in-process bindings in the first release** ([decision 0025](../../decisions/0025-pip-and-npm-packages-ship-the-binary-with-a-thin-sdk-no-in-process-bindings-yet.md)). Loading the engine inside the Python process would mean shipping it a second time as a native library; maturin's own guidance notes that shipping a binary and a library together doubles the wheel. A host that needs the engine in its own process uses the Rust crate's library facade. A separate bindings package can be added later if a real need appears.

## The npm package

- **Layout.** A small launcher package, `anything2mcp`, plus one package per platform that holds only the binary. The launcher lists the platform packages as optional dependencies, and each platform package declares its operating system and processor, so the package manager installs only the one that fits. This is the established pattern for native tools on npm ([Sentry's guide](https://blog.sentry.io/publishing-binaries-on-npm/)).
- **No install script.** Many organizations disable install scripts. If the platform package is missing, for example because optional dependencies were skipped, the launcher explains the fix instead of downloading anything.
- **What it gives.** The `anything2mcp` command and a TypeScript client with the same shape as the Python one.
- **Today.** Not started; package 5.4.

## The Rust crate

- `cargo install anything2mcp` builds the binary from source.
- Only the product crate is published. The `a2m-*` and `a2m-pack-*` crates are parts of one workspace, and are published only if embedding the engine as a library ever needs them, by a decision record.
- All crates share one version number.

## Standalone installs

- A shell installer for macOS and Linux, and a PowerShell installer for Windows.
- Homebrew and winget entries, and a Windows installer that can register the Workstation service.
- A container image for servers.
- An offline bundle for networks with no internet access: the binary, the wheels, and a pinned browser.

Every route delivers one self-contained binary per platform: Windows, macOS, and Linux, on x64 and ARM. The `dist` tool, formerly cargo-dist, generates most of these from a tagged release ([dist](https://docs.rs/crate/cargo-dist/latest)); whether to adopt it or keep hand-written workflows is decided with package 5.5.

## Versions

- **One number.** The binary, every crate, the pip package, and the npm packages carry the same version. It is set in `Cargo.toml` under `[workspace.package]` and in `bindings/python/pyproject.toml`, and the audit refuses a difference.
- **Semantic Versioning.** A patch fixes, a minor adds, a major may break. Before 1.0, a minor version may break, and the changelog says so each time.
- **What counts as breaking:** a target file that used to compile and no longer does; a tool, command, route, or client method removed or renamed; a failure code changed; a default that makes the product less safe. A change in how a run is carried out is not breaking if the result and the evidence are the same.
- **Interfaces version separately.** The REST and local interfaces are `v1`; target files carry a schema version; the MCP revisions served are listed in settings. A release can change the product version without changing any of these. A tool's own schemas are versioned as its contract ([section 604](604-management-and-operations.md)).
- **Mixed versions on one machine.** A newer client asks an older engine to finish and restart ([section 222](../2-kernel/222-front-doors-and-the-local-engine.md)).

## The release pipeline

Publishing cannot be undone. A published version can be yanked, and never replaced.

1. A version tag starts the pipeline, on a commit that is on `main`. So does a manual start of the release workflow on `main` that names the version to publish; started with no version, it builds and checks everything and publishes nothing, which is how a release is rehearsed. Either way the workflow refuses a version that is not the package's own, or a commit that is not on `main`.
2. Build the binary for every platform; run the full test and benchmark suites on each.
3. Build wheels, npm packages, installers, and the container image from those same binaries. Nothing is recompiled per package.
4. Sign every artifact, and attach a software bill of materials and build provenance. Builds are reproducible, and dependencies are audited in CI.
5. Publish only after everything has built and passed: crates.io, then the release page, then PyPI, then npm, then the container image, then Homebrew and winget.
6. Publishing to registries uses short-lived credentials issued to that one workflow, `.github/workflows/release.yml`, never stored tokens, wherever the registry supports it. Nobody publishes from their own machine, and an agent never handles a registry token.

A bad release is yanked from each registry, the changelog says why, and a fixed version with a higher number supersedes it. Published versions are never overwritten and never deleted.

**Today.** The workflow builds and checks the pip placeholder on every pull request that touches it, and publishes only from a tag or a manual start, through PyPI's trusted publishing in a GitHub environment named `pypi`. crates.io and npm offer trusted publishing only to a name that already exists, so each needs one first publish by the owner before the workflow can take over. Signing, the bill of materials, the other registries, and the standalone installers are package 5.5.

## Updates

- pip, npm, Homebrew, and winget installs are updated by those tools.
- Standalone installs update with `self.update`, which verifies the signature before replacing the binary.
- Signed updates come on stable and beta channels. Checking for them is off by default, and on servers updating is off: a server pins a version and is upgraded deliberately, one part at a time ([section 604](604-management-and-operations.md)).

## Nothing phones home

A tool that holds signed-in sessions to business systems must not talk to anyone unasked ([decision 0040](../../decisions/0040-nothing-leaves-the-machine-unless-switched-on.md)).

- No usage statistics, crash reports, or automatic update checks by default.
- The engine contacts only the targets it is told to use, the organization's identity provider, a control plane it is configured for, and a monitoring endpoint if one is set.
- Update checks and crash reports can be switched on, each separately, and say exactly what they send.
- Listing providers, checking for components, and `doctor.run` make no outside connection. An explicit install action is the only thing that fetches, and it says what it will fetch and from where before it does.
- A test in CI runs the engine with no network configuration and fails if it opens any outside connection.

## Targets and open points

- Wheel size under 25 MB with every first-party pack, and install time under 10 seconds on a normal connection. Both are targets until measured; package F4 measures them for hypothesis H-28.
- To confirm with research brief 28, before package F1: that a maturin `bin` wheel can also carry the pure-Python package; the exact platform tags to publish; whether `dist` covers the npm and installer needs; the registries' current rules for short-lived publishing credentials; which link or launcher provides `a2m` on each route.
- The exact entry-point group, and the shape of an extra that names a provider package.
- Whether a provider written in Rust, outside this repository, is loaded as a process like any other plug-in. Almost certainly yes: one way in.
- Whether the library facade publishes the `a2m-*` crates. That needs a decision record when embedding needs it.
