# Research: the wheel and the npm packages

**Status:** completed on 2026-11-17, hands-on on Linux x86-64, and by reading each tool's and each registry's own documentation in its source repository. Answers research brief 28 of [open research](11-open-research.md) and roadmap package F1. Feeds [spec section 605](../spec/6-running-it/605-packaging-and-release.md), [decision 0135](../decisions/0135-the-wheel-carries-the-binary-and-a-thin-client-and-npm-uses-platform-packages.md), and hypotheses H-28 and H-39. What was built is in `bindings/`, and [its README](../../bindings/README.md) says how to build and test it.

Everything was run in one Linux container (Ubuntu, glibc 2.39, x86-64) with Python 3.13 and 3.9, Node 22.23.4, and npm 10.9.9. Nothing was run on Windows or macOS, and nothing was published. Each claim says whether it was run, read, or reasoned. The registries' documentation sites could not be reached from the container; their text was read from the sources the sites are built from, on the same date.

## Question

1. Can one maturin `bin` wheel carry the binary and a pure-Python package, so that one install gives `anything2mcp`, `a2m`, and `import anything2mcp`?
2. Which platform tags to publish, and what each needs in CI.
3. Does an npm launcher with platform packages work with install scripts off and with optional dependencies off?
4. What would the release tool `dist` cover?
5. What do PyPI, npm, and crates.io require for publishing with short-lived credentials and for provenance, and what does `release.yml` lack?

## Answers

1. **Yes, with one limit.** maturin 1.16.0 builds one wheel tagged `py3-none-<platform>` that holds the binary as a script and the package `anything2mcp`; installed into a fresh virtual environment, `anything2mcp --version`, `a2m --version`, and `import anything2mcp` all worked. The limit: maturin refuses a console script beside a binary ("Defining scripts and working with a binary doesn't mix well"), so `a2m` is a small Python script shipped through the wheel's data directory. It costs about 11 ms over the binary on Linux. On Windows pip makes no `.exe` for such a script, so `a2m` from pip on Windows is not proven to work (below).
2. **Eight wheels and eight platform packages**, one per system, processor, and C library, listed in the table below. Per processor on macOS, not universal, because a universal binary doubles what every Mac downloads.
3. **Yes.** A launcher with no dependencies and no script found the platform package npm chose, ran the binary under both names normally and with `--ignore-scripts`, and with `--omit=optional` named the package to install and exited 4. Only this machine's platform package was installed. The Node launcher costs about 38 ms before the binary starts, which is over the 30 ms budget for starting a command.
4. **Not worth adopting for these routes.** `dist` 0.34.0 builds archives, shell and PowerShell installers, Homebrew formulas, and MSI installers well. Its npm package downloads the release archives instead of using platform packages, it is set up with a stored `NPM_TOKEN`, and it builds no wheels. It may still pay for the standalone installers, package 5.5.
5. **All three registries offer trusted publishing from GitHub Actions; only PyPI can create a project with it.** npm and crates.io each need the package to exist first, so the owner publishes each name once. The exact gaps in `release.yml` are listed at the end.

## 1. The wheel

**Settings** (run). In `bindings/python/pyproject.toml`:

| Setting | Value | Why |
| --- | --- | --- |
| `build-backend` | `maturin`, pinned at `maturin==1.16.0` | Released 2026-10-29; MIT OR Apache-2.0 |
| `bindings` | `bin` | The binary is packaged as a script on the user's path |
| `manifest-path` | `../../crates/anything2mcp/Cargo.toml` | The product crate in the workspace |
| `targets` | `[{ name = "anything2mcp", kind = "bin" }]` | The workspace builds two binary targets; only one is shipped ([decision 0063](../decisions/0063-the-kernels-interfaces-as-the-crate-designs-settle-them.md)) |
| `python-source`, `python-packages` | `src`, `["anything2mcp"]` | The pure-Python package |
| `data` | `wheel-data`, holding `scripts/a2m` | The short name, as a script installed beside the binary |
| `include` | `wheel-data/**/*` for the source distribution | Without it the source distribution left `a2m` out (run, then fixed) |
| `strip`, `locked` | `true` | Smaller binary; the lock file is used as it is |

**What the wheel holds** (run): `anything2mcp-0.0.1-py3-none-manylinux_2_39_x86_64.whl`, 10.5 MB compressed. Inside: the five files of the Python package, `anything2mcp-0.0.1.data/scripts/anything2mcp` (the stripped binary, 25.1 MB), `.data/scripts/a2m` (a 0.6 kB script), the metadata, `licenses/LICENSE`, and a CycloneDX bill of materials of the Rust dependencies, `sboms/anything2mcp.cyclonedx.json`, which maturin adds by itself. The tag is `py3-none`, so one wheel serves every Python version from 3.9; the tests passed on 3.9 and 3.13. Nothing of maturin is in the wheel: it is a build backend, named only in `[build-system]`.

**The tag here is not the one to publish** (run). Built directly on this host the binary needs glibc 2.39, so maturin tagged it `manylinux_2_39`. The newest symbols it needs come from the standard library and Tokio (`pidfd_spawnp`, `pidfd_getpid`); a wheel for distribution must be built in an older manylinux image. Rust's own floor for `x86_64-unknown-linux-gnu` is glibc 2.17, which `manylinux2014` matches (reasoned from Rust's platform support page; not confirmed for this toolchain).

**`a2m`** (run). Four ways were weighed:

| Way | Cost | Result |
| --- | --- | --- |
| Ship both binary targets | A second 25 MB copy; the wheel would pass the 25 MB target of H-28 | Refused |
| A console script, `[project.scripts]` | None | maturin refuses it in `bin` mode |
| A Python script in the data directory that imports the package | 78 ms median to `--version`, against 6.5 ms for the binary: the package imports `http.client`, `json`, and metadata | Too slow |
| A Python script in the data directory that imports only `os` and `sys` and replaces itself with the binary beside it | 15.6 ms median, against 4.7 ms for the binary, 30 runs each | **Chosen**: about 11 ms over the binary, inside the 30 ms budget |

pip rewrote the script's first line to the environment's interpreter and dropped any flags after `#!python`, so the script cannot ask for `-S` (run). On Windows pip makes `.exe` launchers only for entry points, not for scripts from the data directory, so `a2m` would arrive as a file Windows does not run (reasoned from what pip is known to do; not run).

**The source distribution** (run). `maturin sdist` wrote 1.9 MB holding the workspace manifest, the lock file, every crate the product crate needs, the test stands it names as development dependencies, and the Python package. It does not hold `rust-toolchain.toml`. A wheel was not built from it here; building one compiles the whole workspace.

**The client.** The Python package holds a client that reaches the local engine itself, over the Unix socket, with HTTP/1.1 from `http.client` and the standard library only: in about 150 lines with their comments: the place of the socket by the engine's rules, the client's check that the socket and its directory are the user's, and a connection class. It was proven against a fake engine and against the real binary's `serve` (run). The Windows named pipe is not built: its name carries the user's security identifier, and the client must check that the pipe's server runs as the user, which needs calls into the operating system that the standard library does not offer (reasoned).

## 2. Platform tags, and what each needs in CI

| Platform | Wheel tag | npm package | CI runner | Cross-compiled | C (bundled SQLite) |
| --- | --- | --- | --- | --- | --- |
| Linux x86-64, glibc | `manylinux2014_x86_64` (`manylinux_2_17`) | `@anything2mcp/linux-x64` | `ubuntu-latest`, inside the manylinux2014 image that `PyO3/maturin-action` starts | No | The image's GCC |
| Linux arm64, glibc | `manylinux2014_aarch64` | `@anything2mcp/linux-arm64` | `ubuntu-24.04-arm`, natively; or `ubuntu-latest` with the action's cross image | Either | The image's GCC, or its cross GCC |
| Linux x86-64, musl | `musllinux_1_2_x86_64` | `@anything2mcp/linux-x64-musl` | `ubuntu-latest`, musllinux image | Target `x86_64-unknown-linux-musl` | musl's GCC in the image; statically linked |
| Linux arm64, musl | `musllinux_1_2_aarch64` | `@anything2mcp/linux-arm64-musl` | `ubuntu-24.04-arm`, or cross | Either | As above |
| macOS x86-64 | `macosx_10_12_x86_64` | `@anything2mcp/darwin-x64` | `macos-latest` (arm64) | Yes, target `x86_64-apple-darwin` | Apple's Clang targets both processors; no extra tool |
| macOS arm64 | `macosx_11_0_arm64` | `@anything2mcp/darwin-arm64` | `macos-latest` | No | Apple's Clang |
| Windows x86-64 | `win_amd64` | `@anything2mcp/win32-x64` | `windows-latest` | No | MSVC, found by the `cc` crate |
| Windows arm64 | `win_arm64` | `@anything2mcp/win32-arm64` | `windows-11-arm`, natively; or `windows-latest` with MSVC's arm64 tools | Either | MSVC for arm64 |

The minimum macOS versions are Rust's floors for each target, 10.12 and 11.0 (reasoned; the tag comes from `MACOSX_DEPLOYMENT_TARGET`, which the CI sets). A universal2 wheel was weighed and not chosen: each Mac would download both processors' code, about 50 MB unpacked. The bundled SQLite is the only C in the tree that matters here: it rules out building every target from one Linux runner without a C cross toolchain, and is why each Linux target is built inside an image that has one (reasoned; only the Linux x86-64 build was run). The npm `libc` field kept the musl package off this glibc machine (run).

## 3. npm

**Shape** (run). `bindings/npm/anything2mcp/`: a launcher package with no dependency and no script, `bin` naming one file for both `anything2mcp` and `a2m`, and the eight platform packages as `optionalDependencies` at its own exact version. `bindings/npm/platform/` holds the template of a platform package: `os`, `cpu`, and on Linux `libc`, and the binary in `bin/`; the package has no `bin` of its own. `bindings/npm/scripts/assemble-platform.js` fills the template from a built binary.

**The proof** (run). `bindings/npm/test/install.js` assembles all eight platform packages (this machine's with the real binary, the others with a stand-in file, since npm only reads their manifests), packs them and the launcher with `npm pack`, serves them from a small registry of its own, in the same process, on `127.0.0.1`, and installs `anything2mcp@0.0.1` into a scratch project three ways. All three passed:

| Install | Platform packages installed | `anything2mcp --version` and `a2m --version` |
| --- | --- | --- |
| Normal | `linux-x64` only | `anything2mcp 0.0.1`, exit 0 |
| `--ignore-scripts` | `linux-x64` only | The same |
| `--omit=optional` | None | Exit 4, with "the package @anything2mcp/linux-x64, which holds the anything2mcp program for this machine, is not installed" and the command to install it |

`npm pack` kept the binary's executable bit (run). The registry has to answer asynchronously: a first version that ran npm with a blocking call hung, because the registry shared its event loop.

**Start time** (run). The launcher takes 42.7 ms median to `--version`; Node alone takes 32 ms, so the npm route cannot meet the 30 ms budget with any launcher written in JavaScript. A first version spent 6 ms more asking Node for its report to detect musl; reading `/lib` for the musl loader takes under 1 ms.

**Names.** The platform packages are scoped, `@anything2mcp/<platform>`, so that nobody else can take a platform package's name before the first release; that needs an npm organization named `anything2mcp`, which the owner creates. On 2026-11-17 the registry had none of the unscoped or scoped names (read from the registry).

## 4. `dist`

`dist` (formerly cargo-dist) 0.34.0, released 2026-11-02, MIT OR Apache-2.0 (read from crates.io). From its book, read in its repository:

| Need | What `dist` does | Fits |
| --- | --- | --- |
| Binaries for every platform, archives, checksums | Yes, with a generated workflow | Yes |
| Shell, PowerShell, Homebrew, MSI installers | Yes | Yes, for package 5.5 |
| GitHub artifact attestations | Yes, off by default, through `actions/attest` | Yes |
| Wheels | No | We need maturin anyway |
| npm | One package that fetches the release's archives and installs them into `node_modules`; set up with a stored `NPM_TOKEN` | No: a download at install time, and a stored token |
| Its own workflow file | It generates and owns `release.yml` | Conflicts with the hand-written file PyPI's publisher names |

Verdict: keep the hand-written workflow for crates.io, PyPI, and npm. Weigh `dist` again with package 5.5 for the standalone installers only, where it would run as a separate workflow or a generated job, and where a stored token is not needed.

## 5. Trusted publishing and provenance

| | PyPI | npm | crates.io |
| --- | --- | --- | --- |
| Short-lived credential | Yes. `id-token: write` on the job; `pypa/gh-action-pypi-publish` exchanges it | Yes. npm CLI 11.5.1 or later and Node 22.14.0 or later find the credential themselves; `id-token: write` | Yes. `rust-lang/crates-io-auth-action` exchanges it for a token that expires after 30 minutes; `id-token: write` |
| Before the first publish | A pending publisher creates the project on its first publish | The package must exist (`npm trust`: "The package you're configuring must already exist"); a new publisher must publish within 2 days or it expires | The crate must exist ("initial publish requires an API token") |
| What the publisher names | Owner, repository, workflow file, environment | Workflow file name only, optional environment; `repository.url` in `package.json` must match the repository exactly | Owner, repository, workflow file, optional environment |
| Provenance | Attestations (PEP 740) generated and uploaded by the publish action by default | Generated by default with trusted publishing from a public repository | None offered |
| Other | | Publishers made after 2026-09-03 allow `npm stage publish` only unless the owner also allows `npm publish`; a staged version waits for a maintainer's approval with two-factor authentication (CLI 11.15.0 or later). After it works, "disallow tokens" can be set | |

Sources: PyPI's [using a publisher](https://docs.pypi.org/trusted-publishers/using-a-publisher/) and [producing attestations](https://docs.pypi.org/attestations/producing-attestations/); npm's [trusted publishers](https://docs.npmjs.com/trusted-publishers), [`npm trust`](https://docs.npmjs.com/cli/v11/commands/npm-trust), and [staged publishing](https://docs.npmjs.com/staged-publishing); crates.io's [trusted publishing](https://crates.io/docs/trusted-publishing). Build provenance for the binaries and archives themselves, beyond what each registry records, comes from GitHub's `actions/attest-build-provenance`.

**A gap that is not the workflow's: the crate cannot be published.** The workspace sets `publish = false`, and the product crate depends by path on kernel and pack crates, 26 of them counting those they bring, none of which is on crates.io. `cargo publish -p anything2mcp` refuses a dependency that is not on the registry, so `cargo install anything2mcp` needs either every `a2m-*` crate published or the crates.io route dropped (reasoned from Cargo's rules; not run, since running it would publish). Spec section 605 leaves publishing the `a2m-*` crates to a decision record.

### What `release.yml` lacks

1. It builds the pip package with `python -m build` on one Linux runner. With the real project that builds a source distribution, then one wheel from it, tagged for the runner's glibc, in a job with a 10-minute limit; it must instead build eight wheels in a matrix with `PyO3/maturin-action`, and one source distribution.
2. Its pull-request trigger watches only `bindings/python/**`; the wheel now holds the binary, so a change to any crate changes it. The CI job below covers pull requests; the release workflow can keep its narrow trigger.
3. The version check reads only `pyproject.toml`. It must also compare the workspace's version in `Cargo.toml` and the launcher's `package.json`, and refuse a difference.
4. No npm job: assemble the eight platform packages from the matrix's binaries, then publish them before the launcher, each with `npm publish` (or `npm stage publish`) under `id-token: write`, Node 22.14 or later and npm 11.5.1 or later, in an environment the publishers name.
5. No crates.io job: `rust-lang/crates-io-auth-action` and `cargo publish`, blocked by the gap above.
6. No build provenance for the binaries: `actions/attest-build-provenance` with `id-token: write` and `attestations: write` in the build job. PyPI's attestations come from the publish action already, since it uses trusted publishing.
7. No release page with the binaries, checksums, and licence texts of the Rust dependencies. The wheel carries our licence and a bill of materials; it does not yet carry the third-party licence texts that spec section 605 asks for.
8. The placeholder at 0.0.1 has never been published. Once `bindings/python/` is the real project, the existing workflow can no longer publish the placeholder; publishing 0.0.1 first means tagging a commit from before this change.

## What could not be verified here

- Any build or install on Windows or macOS, any arm64 build, any musl build, and any manylinux image build. The tags above are what the tools document.
- `a2m` from pip on Windows, which is expected not to run as a command.
- The Python client on Windows, which is not built.
- A wheel built from the source distribution.
- Every registry rule above, which was read and not exercised: nothing was published.
