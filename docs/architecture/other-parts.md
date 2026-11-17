# The parts that are not crates

The Cargo workspace is most of the product and not all of it. This page covers every other folder in the [repository layout](../spec/6-running-it/605-packaging-and-release.md): what it holds, what language it is in, what it may know, and when it is built. Each follows the same rule as a thin client: it holds no engine logic.

| Folder | Holds | Language | State |
| --- | --- | --- | --- |
| `targets/` | The imaginary `example` target, and the test stands | Rust | The test application and all six stands exist |
| `bindings/python/` | The pip package: the binary and a thin client | Python | A placeholder exists |
| `bindings/typescript/` | The npm package: a launcher, per-platform packages, a thin client | TypeScript | Not started |
| `web/console/` | The admin console, the approval page, the sign-in page | TypeScript | Not started; milestone 9 |
| `profiles/` | Platform profiles and detection rules | Data files | Not started; milestone 10 |
| `plugins/` | The plug-in protocol's examples, and a sample plug-in in another language | Any | Not started; with package F3 |
| `bench/` | The benchmark harness and the budgets | Rust | The harness and one sample scenario exist; package 0.7 |
| `spikes/` | Throwaway experiments | Any | Empty; never part of the product |
| `xtask/` | Build, release, and code-generation tasks | Rust | Not started; with package 1.5 |
| `tools/` | The documentation audit and its self-test | Python | Exists |

## Test stands: `targets/`

A test stand imitates the part of one kind of system that makes the kind hard, and reports ground truth through a control interface. A test proves a write by asking the stand what it stored, never by the product's own read path. The rules are in the [test application plan](../plan/test-application.md).

| Stand | Imitates | Control interface reports | Designed in | Package |
| --- | --- | --- | --- | --- |
| [`targets/example-app`](../../targets/example-app/README.md) | A web application with a JSON interface | What is stored, how many times each record was written, which faults are set | The [test application plan](../plan/test-application.md) | 0.3, done |
| [`targets/example-cli`](../../targets/example-cli/README.md) | A command-line tool: a program with typed arguments, several output forms, a prompt, slow output | What it stored, how many times each record was written, every invocation it received | [Section 306](../spec/3-packs/306-the-command-pack.md) | D2, done |
| [`targets/example-db`](../../targets/example-db/README.md) | A local database: a relational store with keys and constraints | The rows as stored, a log of every row written, the catalog | [Section 307](../spec/3-packs/307-the-data-pack.md) | G1, done |
| [`targets/example-files`](../../targets/example-files/README.md) | Files in a temporary folder, in each supported format | The bytes on disk and their digests | [Section 308](../spec/3-packs/308-the-file-pack.md) | H1, done |
| [`targets/example-broker`](../../targets/example-broker/README.md) | A small local broker that speaks its own line-delimited JSON protocol, with channels of four shapes: a queue, a topic, a retained channel, and a log | Every message received and delivered, and to whom | [Section 309](../spec/3-packs/309-the-stream-pack.md) | J1, done |
| [`targets/example-device`](../../targets/example-device/README.md) | A simulated device with readable and writable points and its own update mechanism. It speaks a subset of the Modbus application protocol over TCP | Every frame received, and the state of its simulated outputs. The control interface is in the test's own process | [Section 310](../spec/3-packs/310-the-device-pack.md) | K1, done |
| [`targets/example-mcp`](../../targets/example-mcp/README.md) | A small MCP server: an upstream server whose tools can change, and whose connection can drop after a commit | Every call it received | The design of [`a2m-pack-mcp`](crates/a2m-pack-mcp.md) | 8.2; the stand is done |

Each stand's README lists its cases.

Rules for every stand:

- It is a workspace member under `targets/`, and a product crate may depend on it only in its tests. The benchmark harness in `bench/` is not a product crate and uses the stands directly. The audit enforces this for every crate under `targets/`.
- It starts with one call from a test, on a port or path it chooses, and stops when the test ends.
- Every fault it can show is a setting, so a failure is reproduced on demand and never waited for.
- It is as real as is cheap. A stand does not reimplement a product; it shows the hard part.
- It names no real product.

## The `example` target

The target files used in every document, test, and fixture describe an imaginary order-entry system named `example`. They live under `targets/example/` once package B4 needs them, and they are written to exercise the file formats, not to be complete. A new pack adds its file kinds to the same target, so that one target shows several surfaces.

## The clients: `bindings/`

| | Python | TypeScript |
| --- | --- | --- |
| Package | `anything2mcp` on PyPI | `anything2mcp` on npm |
| Carries | One wheel per platform holding the compiled binary and a pure-Python package | A launcher package, with one optional package per platform holding only the binary |
| The client | Generated from the action registry: one method per action, typed inputs and results | The same |
| Talks to | The local engine over the local channel, or a server over HTTPS | The same |
| Must never | Contain a branch on what a result means; hold a credential; start a browser | The same |

A client is generated, not written. The generator reads the action registry ([`a2m-actions`](crates/a2m-actions.md)) and is run by `xtask`; CI fails when a generated file differs from what the generator produces. [Spec section 605](../spec/6-running-it/605-packaging-and-release.md) and [section 605](../spec/6-running-it/605-packaging-and-release.md) give the packaging. Provider packages installed with pip are found through an entry point and are listed as present and not enabled until a person enables them ([`a2m-provider`](crates/a2m-provider.md)).

## The pages: `web/console/`

Three small sets of pages, served by [`a2m-api`](crates/a2m-api.md) on a server and, for the approval and sign-in pages only, by [`a2m-local`](crates/a2m-local.md) on one machine.

| Pages | Shown to | Rule |
| --- | --- | --- |
| Approval | The person who must approve, signed in as themselves | The page shows the exact diff by its hash. Nothing the caller controls reaches it but the diff's reference |
| Sign-in | The person whose session is being made | On a server, it shows the runner's browser; the person's credentials go to the system, never to the product |
| Console | Administrators | Every change it makes is an action from the registry. It has no route of its own into the kernel |

The pages are static files built ahead of time and embedded in the binary. They call the REST interface and nothing else. [Spec section 503](../spec/5-security/503-approval-and-sign-in-surfaces.md) designs the first two.

## Data, not code: `profiles/`

A platform profile says how one family of web interface behaves: how to detect it, how to know a page is ready, which locators to trust. Profiles are data files read by the web pack ([`a2m-pack-web-ui`](crates/a2m-pack-web-ui.md)), validated against a schema at build time, and embedded in the binary. Adding a profile changes no Rust. The schema is in the [technical reference](../spec/3-packs/301-the-web-pack.md#platform-profiles).

## Plug-ins: `plugins/`

Holds the written protocol a plug-in speaks, a conformance check any plug-in can be run against, and one sample plug-in in a language other than Rust, to prove the protocol does not depend on ours. Real plug-ins live in their own repositories. [`a2m-provider`](crates/a2m-provider.md) designs the host side.

## Measurement: `bench/`

The harness measures the product's overhead beyond the test stand's own time, for each scenario in the [benchmark matrix](../reference/technical-reference.md#benchmark-matrix). It runs in CI, stores its results, and fails the build on a regression over ten percent on a budgeted path. Budgets are targets until measured ([spec section 225](../spec/2-kernel/225-performance.md)).

## Tasks: `xtask/`

One Rust program for everything a developer of the product runs that is not `cargo build`: generating the faces and the clients from the action registry, building the pages, producing release artifacts, and regenerating the documents' source index. It is a workspace member that is never published and never part of the binary.

## Checks: `tools/`

`docs_audit.py` holds the documents and the crates together: links, counts, names, the layering, and a design for every crate. `test_docs_audit.py` plants each kind of mistake and shows the audit catches it. A new kind of mistake that a script could catch becomes a check there, with its planted case.
