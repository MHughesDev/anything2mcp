# 29. Repository, crates, and packaging

One Cargo workspace holds the whole system. The engine is a set of small Rust crates behind one binary. Rust is the default for everything; another language is used where it clearly fits better.

```text
web2mcp/
  Cargo.toml              # workspace
  crates/
    w2m-core/             # shared types: targets, operations, bindings, effects, failure classes
    w2m-files/            # schemas, loader, linter, plan compiler
    w2m-expr/             # CEL evaluation; host functions for units, numbers, dates
    w2m-engine/           # step loop, mode selection, read-back, normalization
    w2m-ledger/           # run store, ledger, reconcile, audit chain (SQLite, Postgres)
    w2m-policy/           # grants, effect classes, approvals, limits
    w2m-sched/            # queues, record locks, quotas, runner dispatch
    w2m-auth/             # auth providers, auth discovery, session keeper
    w2m-vault/            # envelope encryption, key-store back ends, redactor
    w2m-browser/          # DevTools-protocol engine: locate, act, wait, network
    w2m-driver-ui/        # ui and page_http drivers, widget drivers, platform profiles
    w2m-driver-http/      # http and api drivers; OpenAPI, GraphQL, OData import
    w2m-driver-mcp/       # upstream MCP pass-through and site tools
    w2m-driver-desktop/   # Windows UI Automation, macOS Accessibility, Linux AT-SPI
    w2m-sources/          # xlsx, csv, json readers; template fingerprints; mappings
    w2m-probe/            # capability probe, traffic capture, endpoint inference
    w2m-actions/          # the action registry: one definition per authoring and admin action
    w2m-local/            # local channel: socket and named pipe, peer check, engine lifecycle
    w2m-mcp/              # MCP server: stdio, Streamable HTTP, toolsets, authoring tools
    w2m-api/              # REST API, OpenAPI generation, webhooks
    w2m-runner/           # runner role and the control-plane link
    w2m-telemetry/        # traces, metrics, OpenTelemetry export
    web2mcp/              # the binary and the embeddable library facade
  bindings/
    python/               # pip package `web2mcp`: ships the binary and a thin pure-Python SDK
    typescript/           # npm package `w2mcp`: launcher, platform packages, and a thin SDK
  web/console/            # admin console, approval page, sign-in page (TypeScript)
  plugins/                # plug-in protocol and examples
  profiles/               # platform profiles and detection rules, as data files
  targets/                # the imaginary `example` target and the built-in test application
  bench/                  # performance suite and budgets
  docs/                   # user guide, format reference, AGENTS.md
  xtask/                  # build, release, and code-generation tasks
```

**Building blocks.** The MCP SDK and desktop library below were checked in this research round; the other crate names come from general knowledge of the Rust ecosystem and are to be confirmed in phase 0.

| Need | Choice | Note |
| --- | --- | --- |
| Async runtime, HTTP server and client | Tokio, Axum, Hyper with rustls | Standard, fast, no OpenSSL dependency |
| MCP protocol | `rmcp`, the official Rust SDK | Tracks the 2026-07-28 specification ([docs.rs](https://docs.rs/crate/rmcp)) |
| Browser | Own DevTools-protocol engine | Evaluate chromiumoxide and Rustwright as a base first |
| Desktop accessibility | [xa11y](https://docs.rs/crate/xa11y/0.3.0) or per-platform crates | xa11y offers one API over UI Automation, AXUIElement, and AT-SPI2, under MIT |
| Storage | SQLite for local profiles, Postgres for servers | One schema, two back ends |
| Expressions | A CEL implementation for Rust | Pre-compiled at publish |
| Spreadsheets, decimals | A pure-Rust xlsx reader; decimal arithmetic crate | Exact comparison needs decimals, never floats |
| Key store | A cross-platform keyring crate over DPAPI, Keychain, and Secret Service | External secret managers on servers |
| OAuth and OpenID Connect | Standards-based crates; no official MSAL for Rust exists | Broker sign-in handled on user devices |
| Telemetry | OpenTelemetry | Traces and metrics in one format |

**Languages: Rust first, others where they fit.**

Rust is the default. Anything on the path of a run (the control plane, runners, drivers, ledger, policy, vault) is Rust. Another language is used where it is clearly the better tool:

| Where | Language | Why |
| --- | --- | --- |
| Admin, approval, and sign-in pages | TypeScript | Runs in a browser |
| Scripts the engine evaluates inside a target page | JavaScript, kept small and tested | The page only runs JavaScript |
| SDKs and install wrappers | Python, TypeScript, and others as callers need | Meets callers in their own language |
| In-process bindings for embedding (deferred) | Python, Node | Lets other software load the engine directly; not in the first release |
| Installers and service setup | Shell, PowerShell | Native to each operating system |
| Database migrations | SQL | Native to the stores |
| Plug-ins | Any language | They are separate processes |
| Optional Firefox and WebKit support | TypeScript, as a Playwright sidecar plug-in | Reuses a mature engine |

A language not listed here may be added when it clearly fits better than Rust for a part that is off the run path; the choice gets a decision record.

**Plug-ins.** New drivers, auth providers, source readers, and widget drivers are separate processes that speak a small documented protocol over standard input and output, so they can be written in any language. Each declares its capabilities and never receives raw vault access. Unsigned plug-ins are off by default.

**Distribution.** [Section 41](41-packaging-and-release.md) gives the package names, the shape of each package, and the release pipeline.

- One self-contained binary per platform: Windows, macOS, and Linux, on x64 and ARM.
- Installers: pip, npm, cargo, Homebrew, winget, a Windows service installer, and a container image.
- **Browser.** By default Web2MCP uses the Chrome or Edge already installed, with its own data directory, so a laptop install downloads nothing. Servers pin an exact Chromium with `web2mcp browser install` for reproducibility.
- Releases are signed, ship a software bill of materials, and are built reproducibly; dependencies are audited in CI.
- **License:** Apache-2.0, for its patent grant.
