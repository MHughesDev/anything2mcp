# Bindings

The packages that carry the `anything2mcp` binary to Python and Node users. Neither holds engine logic: each delivers the one Rust binary and adds a thin client of the local engine. The design is [spec section 605](../docs/spec/6-running-it/605-packaging-and-release.md) and [decision 0135](../docs/decisions/0135-the-wheel-carries-the-binary-and-a-thin-client-and-npm-uses-platform-packages.md); what was proven, and how, is [research note 21](../docs/research/21-wheel-and-npm-packaging.md).

| Folder | What it is |
| --- | --- |
| `python/` | The pip package: a maturin `bin` wheel with the binary, the short name `a2m` (a script, and `a2m.cmd` for Windows, in `wheel-data/scripts/`), and the package `anything2mcp` with its client |
| `python/tools/` | The saved OpenAPI document of the engine, the generator of the Python methods, and the script that saves the document |
| `npm/anything2mcp/` | The npm launcher: no dependency, no install script; `bin/anything2mcp.js` serves both names. `client/` is the TypeScript client |
| `npm/platform/` | The template of a platform package, `@anything2mcp/<os>-<cpu>[-musl]`, holding only the binary |
| `npm/scripts/assemble-platform.js` | Fills the template from a built binary |
| `npm/scripts/generate-actions.js` | Writes the Node client's methods and types from the saved document |
| `npm/test/` | The table test, the client tests, and the install test against a registry of its own |

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

The client reaches the engine on macOS and Linux by the socket, and on Windows by the named pipe (`_winpipe.py`, with `ctypes`; see [decision 0156](../docs/decisions/0156-the-clients-are-generated-from-the-served-document-and-reach-windows-by-the-pipe.md)). The Windows tests that call the operating system run only on Windows; the rest of that module is tested everywhere.

**What the client offers.** `anything2mcp.connect(project=..., name=...)`, `Client.call(action, input)`, which returns the engine's `Response` as it was written, and one typed method per action (`client.tool_call(...)`, `client.run_get(...)`), generated.

**How the methods are generated.** The engine serves an OpenAPI document made from the action registry (`GET /api/v1/openapi.json`). `python/tools/openapi.json` is a saved copy. `python/tools/generate_actions.py` reads it and writes `python/src/anything2mcp/_actions.py`; both generated and saved files are committed, and a test fails when the module differs from what the generator writes. When the engine's actions change, start an engine and run, from `bindings/python`:

```sh
anything2mcp serve &                          # or any way of starting an engine
python3 tools/fetch_openapi.py                # saves the document
python3 tools/generate_actions.py             # writes the Python methods
node ../npm/scripts/generate-actions.js       # writes the Node methods and types
```

`python3 tools/generate_actions.py --check` and `node ../npm/scripts/generate-actions.js --check` fail when a file differs.

**`a2m` on Windows.** pip makes no launcher for a data script there, so the wheel also carries `wheel-data/scripts/a2m.cmd`, which runs `anything2mcp.exe` beside it with every argument and returns its exit code. `python/tests/test_wheel_data.py` holds the file; `maturin sdist` was run to see that it is carried. The wheel itself needs the product built.

## The npm packages

Needs Node 18 or later and npm. From `bindings/npm`:

```sh
node --test test/platform.test.js
node --test test/client.test.js
node scripts/assemble-platform.js --platform linux-x64 --binary ../../target/release/anything2mcp --out /path/to/out
node test/install.js --binary ../../target/release/anything2mcp
```

`test/client.test.js` runs the Node client against a fake engine of its own (a Unix socket, or on Windows a named pipe) and checks that the committed `actions.js` and `actions.d.ts` are what the generator writes. One more test starts a real engine; it runs only when `A2M_TEST_BINARY` names a built binary (not on Windows). On Windows the client refuses to connect until the caller supplies a check of the pipe's server, which Node cannot make itself.

`test/install.js` assembles every platform package (this machine's with the real binary, the others with a stand-in, because npm only reads their manifests), packs them and the launcher, serves them from a registry on `127.0.0.1` in the same process, and installs `anything2mcp` into scratch projects three ways: normally, with `--ignore-scripts`, and with `--omit=optional`. The first two must run both names; the third must name the missing package and exit 4. It fetches nothing from the internet.

## Publishing

Nothing here is published by hand. Only `.github/workflows/release.yml` publishes, with short-lived credentials; see [RELEASING.md](../RELEASING.md) and [publishing setup](../docs/plan/publishing-setup.md).
