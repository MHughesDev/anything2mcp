# 0135. The wheel carries the binary and a thin client, and npm uses platform packages

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the open points of spec section 605 that research brief 28 was to confirm, for the pip and npm routes; roadmap package F1. Ends the placeholder of [decision 0046](0046-the-pip-name-is-held-by-a-placeholder-published-on-a-version-tag.md) in `bindings/python/`.

## Context

[Decision 0025](0025-pip-and-npm-packages-ship-the-binary-with-a-thin-sdk-no-in-process-bindings-yet.md) said the pip and npm packages ship the binary and a thin client. How, and how `a2m` arrives on each route, waited for brief 28. [Research note 21](../research/21-wheel-and-npm-packaging.md) built both on Linux and installed them.

## Decision

**The wheel.** One maturin `bin` wheel per platform, tagged `py3-none-<platform>`, holding the stripped `anything2mcp` binary as a script and the pure-Python package `anything2mcp`. Only the binary target with the product's name is packaged. maturin is pinned in `[build-system]` and is not in the wheel. *Instead of:* a Python extension module, which would put the engine inside the Python process, ship it a second time as a library, and need one build per Python version; decision 0025 already keeps in-process bindings out of the first release, and the measurement adds that the `py3-none` wheel served Python 3.9 and 3.13 alike.

**`a2m` on pip** is a script of a few lines in the wheel's data directory that imports only `os` and `sys` and replaces itself with the binary beside it: 15.6 ms to `--version` against 4.7 ms for the binary, inside the 30 ms budget. *Instead of:* a second binary, which doubles the wheel; a console script, which maturin refuses beside a binary; or a script that imports the package, which took 78 ms. On Windows the script is not a runnable command; there `a2m` from pip is not provided until a fix is chosen (an `a2m.cmd` beside it is the likely one).

**The Python client** reaches the local engine itself: HTTP/1.1 over the Unix socket, the hello, one POST to the action's REST route, with only the standard library, and it checks that the socket and its directory belong to the user before it sends anything. It offers one generic `call(action, input)`; methods generated from `sdk_methods` in the `a2m-actions` crate wait for a step that writes them out, which the binary does not have. It does not start the engine. Windows, where the channel is a named pipe that needs operating-system calls to find and to check, is not built. *Instead of:* going through `anything2mcp bridge` as spec section 222 describes, which needs a JSON framing on the bridge that is not built, and a process per client. The cost of the choice is that the client restates the engine's rules for where the socket is; a test holds it to the real engine when a built binary is given.

**npm.** A launcher package `anything2mcp` with no dependency and no script, whose `bin` gives both names through one JavaScript file, and eight platform packages `@anything2mcp/<os>-<cpu>[-musl]`, listed as optional dependencies at the same exact version, each with `os`, `cpu`, and on Linux `libc`, holding only the binary. When the platform package is missing the launcher names it and exits 4. *Instead of:* a postinstall script that downloads the binary, which fails where install scripts are disabled, fetches from the network at install, and is what the `dist` tool's npm package does. The platform packages are scoped so that nobody else can take one of their names; the owner creates the npm organization.

**The start time of the npm route is over budget.** The launcher takes about 43 ms, of which Node itself is 32. This is recorded against H-39 and not fixed here.

**Other languages (rule 6).** Python for the client and the wheel's `a2m` script, and JavaScript for the npm launcher and its build and test scripts, are the "clients and install wrappers" that spec section 605 already lists. Nothing here is a new use of another language.

## Consequences

- `release.yml` must change in the same merge as `bindings/python/`: as it stands it builds one wheel for one Linux runner, and could publish it. Research note 21 lists its gaps.
- The 0.0.1 placeholder is no longer in the tree. Holding the PyPI name with it means tagging a commit from before this change.
- The audit checks the pip version against the workspace and not the npm launcher's; it should check both.
- The crates.io route needs the `a2m-*` crates published, by a decision of its own, or `cargo install anything2mcp` cannot work.
