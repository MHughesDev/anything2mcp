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

**When no engine runs** ([decision 0192](../docs/decisions/0192-a-served-engine-hands-no-root-of-the-project-that-started-it-and-clients-start-and-compare.md)). Both clients start the engine through the installed program's `engine start`, and then connect. Pass `start=False` in Python, or `{ start: false }` in Node, to start nothing.

**What the client offers.** `anything2mcp.connect(project=..., name=...)`, `Client.call(action, input)`, which returns the engine's `Response` as it was written, and one typed method per action (`client.tool_call(...)`, `client.run_get(...)`), generated.

**When the engine was not reached, or its answer was lost** (both clients; [decision 0159](../docs/decisions/0159-a-lost-answer-is-a-write-in-doubt-at-the-doors-and-in-the-clients.md)). The client raises (Python) or rejects with (Node) a `ChannelError` with a code and `retry_safe` (`retrySafe` in Node). Before anything was sent, `retry_safe` is true: `engine_unreachable`, `peer_not_allowed`, or `connection_lost` for a connection that had closed. Once the request may have reached the engine, the error is `answer_lost` and `retry_safe` is false: the call may have started a run that writes, and a second call may write a second time. A broken connection, an answer that breaks off, runs too long, or cannot be read, and the caller's own `timeout` all count. The fix depends on whether the call carried an idempotency key ([decision 0160](../docs/decisions/0160-an-idempotency-key-gives-back-its-run-and-run-list-shows-a-callers-own-runs.md)). With a key (`idempotency_key=` in Python, `{ idempotencyKey }` in the options in Node, sent as `Idempotency-Key`), it says to make the same call again with the same key: a `tool.call` made again gives back the run the first one started, finished or not, and starts no other; the same key with another input is refused with `idempotency_key_reused`. Without a key, it says not to call again, to find the run with the run list (`run_list(since=<ms>, tool=<name>)` in Python, `runList({ since, tool })` in Node) from one minute before the call, to do what that run's `next` says, and to pass a key on a call that may write. A `tool.call` input field of the same name is not generated: the per-call key stands for it. A connection is opened once and never again behind the caller's back: after it closes, make a new client. An action's name must be `noun.verb`; anything else is refused before a byte is sent.

**How the methods are generated.** The engine serves an OpenAPI document made from the action registry (`GET /api/v1/openapi.json`). `python/tools/openapi.json` is a saved copy. `python/tools/generate_actions.py` reads it and writes `python/src/anything2mcp/_actions.py`, and `npm/scripts/generate-actions.js` writes the Node methods and types; the generated and saved files are committed, and a test fails when a generated file differs from what its generator writes. A method is named by the action's words: joined with underscores in Python (`policy.tools_allowed` is `policy_tools_allowed`) and in camel case in Node (`policyToolsAllowed`); an action named in any other form stops both generators ([decision 0405](../docs/decisions/0405-a-method-name-is-the-actions-words-joined-in-each-languages-way.md)). When the engine's actions change, build the program and run, from `bindings/python`:

```sh
python3 tools/fetch_openapi.py --binary <the built anything2mcp>   # saves the document
python3 tools/generate_actions.py                                 # writes the Python methods
node ../npm/scripts/generate-actions.js                           # writes the Node methods and types
```

With `--binary` the document is taken from an engine started in a scratch home and stopped after; without it, from the engine running as you. `python3 tools/generate_actions.py --check` and `node ../npm/scripts/generate-actions.js --check` fail when a generated file differs from the saved document, and `python3 tools/fetch_openapi.py --binary <program> --check` fails when the saved document differs from the one that program serves. CI runs all three, the last on Linux and macOS.

**Which program `a2m` starts.** Only the one installed beside it, by its full path. The script finds its folder from its own resolved path and never from the search path or the working folder, and replaces itself with the binary (`execv`) on macOS and Linux.

**`a2m` on Windows.** pip makes no launcher for a data script there, so the wheel also carries `wheel-data/scripts/a2m.cmd`, which runs `anything2mcp.exe` beside it with every argument and returns its exit code. It first checks that its folder holds `a2m.cmd`: cmd can give the working folder as `%~dp0` when the file is named in quotes and found on the search path, and a program planted in the working folder is then not run. What remains is cmd's own: a person who types `a2m` in a folder that holds an `a2m.cmd` runs that one, whatever is installed. `python/tests/test_wheel_data.py` holds the file; `maturin sdist` was run to see that it is carried. The wheel itself needs the product built.

## The npm packages

Needs Node 18 or later and npm. From `bindings/npm`:

```sh
node --test test/platform.test.js
node --test test/client.test.js
node scripts/assemble-platform.js --platform linux-x64 --binary ../../target/release/anything2mcp --out /path/to/out
node test/install.js --binary ../../target/release/anything2mcp
```

`test/client.test.js` runs the Node client against a fake engine of its own (a Unix socket, or on Windows a named pipe) and checks that the committed `actions.js` and `actions.d.ts` are what the generator writes. Tests against a stand-in for the program's `anything2mcp channel` command, run by Node itself, cover the Windows path on every system. Three tests start a real engine; they run only when `A2M_TEST_BINARY` names a built binary. Two of them are not run on Windows. The third, `the client reaches a real engine through the program's channel command`, runs on every system, and on Windows CI runs it alone and fails if it was skipped. On Windows Node cannot tell who serves a pipe, so the client runs the program's `anything2mcp channel` command, which opens the pipe after the check every command makes and carries the client's bytes without reading them ([decision 0466](../docs/decisions/0466-the-node-client-on-windows-reaches-the-engine-through-the-channel-command.md)).

**Which program the launcher runs.** It looks for the platform package only where the launcher's own dependencies are installed: inside its own folder, and beside it in the `node_modules` folder that holds it (under Yarn's Plug'n'Play, Node's resolution, which reads only the declared dependencies). A package of the same name in a folder above is not looked at. The package's `name` and `version` must be the launcher's, and the program's resolved path must lie inside it; otherwise the launcher stops with exit 4. What remains: a `node_modules` folder that others can write to can still be written to. **Signals.** The launcher passes `SIGINT`, `SIGTERM`, and `SIGHUP` on to the program and ends as it ends, so a process manager or an MCP client that stops the launcher stops `serve` and `stdio`. On Windows the console sends Ctrl-C to the program itself, and the launcher only waits.

`test/install.js` assembles every platform package (this machine's with the real binary, the others with a stand-in, because npm only reads their manifests), packs them and the launcher, serves them from a registry on `127.0.0.1` in the same process, and installs `anything2mcp` into scratch projects three ways: normally, with `--ignore-scripts`, and with `--omit=optional`. The first two must run both names; the third must name the missing package and exit 4. It fetches nothing from the internet.

## Publishing

Nothing here is published by hand. Only `.github/workflows/release.yml` publishes, with short-lived credentials; see [RELEASING.md](../RELEASING.md) and [publishing setup](../docs/plan/publishing-setup.md).
