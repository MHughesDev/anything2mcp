# Bindings

The packages that carry the `anything2mcp` binary to Python and Node users. Neither holds engine logic: each delivers the one Rust binary, and the Python package adds a thin client of the local engine. The design is [spec section 605](../docs/spec/6-running-it/605-packaging-and-release.md) and [decision 0135](../docs/decisions/0135-the-wheel-carries-the-binary-and-a-thin-client-and-npm-uses-platform-packages.md); what was proven, and how, is [research note 21](../docs/research/21-wheel-and-npm-packaging.md).

| Folder | What it is |
| --- | --- |
| `python/` | The pip package: a maturin `bin` wheel with the binary, the short name `a2m` (a script in `wheel-data/scripts/`), and the package `anything2mcp` |
| `npm/anything2mcp/` | The npm launcher: no dependency, no install script; `bin/anything2mcp.js` serves both names |
| `npm/platform/` | The template of a platform package, `@anything2mcp/<os>-<cpu>[-musl]`, holding only the binary |
| `npm/scripts/assemble-platform.js` | Fills the template from a built binary |
| `npm/test/` | The table test, and the install test against a registry of its own |

Every package carries the workspace's version. Each folder's `LICENSE` is a copy of the repository's.

## The pip package

Needs Python 3.9 or later and the Rust toolchain. From `bindings/python`:

```sh
python3 -m venv /path/to/tools
/path/to/tools/bin/pip install maturin==1.16.0
/path/to/tools/bin/maturin build --release --out dist      # one wheel for this machine
/path/to/tools/bin/maturin sdist --out dist                # the source distribution

python3 -m venv /path/to/try
/path/to/try/bin/pip install dist/*.whl
/path/to/try/bin/anything2mcp --version
/path/to/try/bin/a2m --version
/path/to/try/bin/python -c "import anything2mcp"
```

Tests use only the standard library. From `bindings/python`:

```sh
python3 -m unittest discover -s tests
```

They run the client against a fake engine of their own. One more test starts a real engine and calls it; it runs only when `A2M_TEST_BINARY` names a built binary:

```sh
A2M_TEST_BINARY=../../target/release/anything2mcp python3 -m unittest discover -s tests
```

The client reaches the engine on macOS and Linux. On Windows it says it is not built, and those tests are skipped.

**What the client offers.** `anything2mcp.connect(project=..., name=...)` and `Client.call(action, input)`, which returns the engine's `Response` as it was written. Methods named after each action, as spec section 605 describes, are to be generated from `sdk_methods` in the `a2m-actions` crate. That needs a build step that writes those faces out as JSON, which the binary does not have; until it exists, `call` is the whole interface.

## The npm packages

Needs Node 18 or later and npm. From `bindings/npm`:

```sh
node --test test/platform.test.js
node scripts/assemble-platform.js --platform linux-x64 --binary ../../target/release/anything2mcp --out /path/to/out
node test/install.js --binary ../../target/release/anything2mcp
```

`test/install.js` assembles every platform package (this machine's with the real binary, the others with a stand-in, because npm only reads their manifests), packs them and the launcher, serves them from a registry on `127.0.0.1` in the same process, and installs `anything2mcp` into scratch projects three ways: normally, with `--ignore-scripts`, and with `--omit=optional`. The first two must run both names; the third must name the missing package and exit 4. It fetches nothing from the internet.

## Publishing

Nothing here is published by hand. Only `.github/workflows/release.yml` publishes, with short-lived credentials; see [RELEASING.md](../RELEASING.md) and [publishing setup](../docs/plan/publishing-setup.md).
