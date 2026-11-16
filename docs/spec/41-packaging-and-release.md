# 41. Packaging and release

Every install route delivers the same Rust binary. The pip and npm packages add a thin SDK on top; none of them contains engine logic. This section fixes the names, the shape of each package, and how a release is produced.

## Names

| Thing | Name | Status on 2026-11-15 |
| --- | --- | --- |
| Project and repository | Web2MCP | Decided |
| Command | `web2mcp` | Decided |
| Python package and import | `web2mcp` | No published package found on PyPI; to be reserved |
| Rust crates | `web2mcp` for the binary and library facade, `w2m-*` for the parts | No crate named `web2mcp` found on crates.io; to be reserved |
| npm package | `w2mcp` | `web2mcp` is taken on npm; `w2mcp` was free |
| MCP tool prefix | `w2m_` | Decided |
| Container image | `web2mcp` under the repository's registry namespace | Decided |

"Found" means that `pip`, `npm`, and `cargo` returned nothing for the name. A registry can still refuse a name that is reserved or too close to an existing one, so the reservation upload is the real test.

**Reserve the names first.** Another project's documentation already tells people to `pip install web2mcp`, although nothing is published under that name. Until a placeholder release is uploaded to PyPI, crates.io, and npm, any of the three names can be lost. This needs the owner's registry accounts and is task A1 of [Phase 0](../plan/phase-0.md).

## The pip package

- **Contents.** One wheel per platform holding the compiled binary and a pure-Python package named `web2mcp`. The build tool, maturin, supports this directly: its `bin` mode packages a Rust binary into a wheel as a script that lands on the user's path ([maturin](https://maturin.rs/bindings)).
- **No Rust needed to install.** Wheels are prebuilt for Windows, macOS, and Linux on x64 and ARM, including Linux distributions that use musl. A source distribution is published as the fallback and does need a Rust toolchain.
- **One wheel per platform, not per Python version.** The Python part is pure Python, so the wheel does not depend on the interpreter version.
- **The browser is not included.** The engine uses the Chrome or Edge already installed, with its own data directory, or a pinned browser fetched by `web2mcp browser install`.
- **What `pip install web2mcp` gives.** The `web2mcp` command, the engine it starts, and `import web2mcp`.

## The Python SDK

The SDK is a thin, typed client. It contains no engine logic and no platform-specific code.

```python
import web2mcp

client = web2mcp.connect()                       # local engine, started on demand
order = client.call("example_order_get", order_id="A-100")

run = client.run("example_order_sheet_enter", source="sheet.xlsx", order_id="A-100")
run.wait()                                       # returns when done or when a person is needed
print(run.status, run.summary)

remote = web2mcp.connect("https://mcp.example.com", toolset="orders")   # a company server
```

- **Two transports.** Local: the SDK starts `web2mcp bridge` and exchanges JSON messages with it; the bridge talks to the engine over the local channel in [section 39](39-local-engine-and-clients.md). Remote: HTTPS to a server's REST API, with OAuth or an API key.
- **Same calls either way.** Code written against a laptop engine runs unchanged against a company server.
- **Generated, not hand-written.** Models and methods come from the action registry and the OpenAPI document, so the SDK cannot drift from the engine.
- **Synchronous and asynchronous clients**, with few or no third-party dependencies.
- **Compatibility.** The SDK states the engine API version it needs and gives a clear error on a mismatch.

**No in-process bindings in the first release.** Loading the engine inside the Python process would mean shipping it a second time as a native library; maturin's own guidance notes that shipping a binary and a library together doubles the wheel. Hosts that need the engine in-process use the Rust crate. A separate bindings package can be added later if a real need appears.

## The npm package

- **Layout.** A small launcher package, `w2mcp`, plus one package per platform that holds only the binary. The launcher lists the platform packages as optional dependencies, and each platform package declares its operating system and processor, so the package manager installs only the one that fits. This is the established pattern for native tools on npm ([Sentry's guide](https://blog.sentry.io/publishing-binaries-on-npm/)).
- **No install script.** Many organizations disable install scripts. If the platform package is missing, for example because optional dependencies were skipped, the launcher explains the fix instead of downloading anything.
- **What it gives.** The `web2mcp` command and a TypeScript SDK with the same shape as the Python one.

## Rust crates

- `cargo install web2mcp` builds the binary from source.
- The `w2m-*` crates are published for hosts that embed the engine.
- All crates share one version number.

## Standalone installs

- A shell installer for macOS and Linux and a PowerShell installer for Windows.
- Homebrew and winget entries, and a Windows installer that can register the Workstation service.
- A container image for servers.
- An offline bundle for networks with no internet access: the binary, the wheels, and a pinned browser.

The `dist` tool, formerly cargo-dist, generates most of these from a tagged release and is under active development ([dist](https://docs.rs/crate/cargo-dist/latest)). Whether to adopt it or keep hand-written workflows is decided in Phase 0.

## Versions

- **One number.** The binary, every crate, the pip package, and the npm packages carry the same version, set in one place by the release task.
- **Interfaces version separately.** The REST and local API are `v1`; target files carry a schema version; the MCP revisions served are listed in settings. A release can change the product version without changing any of these.
- **Mixed versions on one machine.** A newer client asks an older engine to finish and restart, as in section 39.

## The release pipeline

1. A version tag starts the pipeline.
2. Build the binary for every platform; run the full test and benchmark suites on each.
3. Build wheels, npm packages, installers, and the container image from those same binaries. Nothing is recompiled per package.
4. Sign every artifact and attach a software bill of materials and build provenance.
5. Publish only after everything has built and passed: crates, then the release page, then PyPI, then npm, then the container image, then Homebrew and winget.
6. Publishing to registries uses short-lived credentials issued to the pipeline, not stored tokens, wherever the registry supports it.

A bad release is yanked from each registry and superseded; published versions are never overwritten.

## Updates

- pip, npm, Homebrew, and winget installs are updated by those tools.
- Standalone installs update with `web2mcp self update`, which verifies the signature before replacing the binary.
- Servers pin a version and upgrade deliberately; see [section 32](32-components-added-by-the-gap-review.md).

## Nothing phones home

A tool that holds signed-in sessions to business systems must not talk to anyone unasked.

- No usage statistics, crash reports, or automatic update checks by default.
- The engine contacts only the targets it is told to use, the organization's identity provider, a control plane it is configured for, and a monitoring endpoint if one is set.
- Update checks and crash reports can be switched on, each separately, and say exactly what they send.
- A test in CI runs the engine with no network configuration and fails if it opens any outside connection.

## Targets and open points

- Wheel size under 25 MB and install time under 10 seconds on a normal connection. Both are targets until measured.
- To confirm in Phase 0: that a maturin `bin` wheel can also carry the pure-Python package; the exact platform tags to publish; whether `dist` covers the npm and installer needs; the registries' current rules for short-lived publishing credentials.
