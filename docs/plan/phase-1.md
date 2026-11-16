# Phase 1: Solo read path

A draft task list, written before Phase 0 has run. Revise it at the end of Phase 0 (task G3) to reflect what the spikes decided, then get the owner's agreement before starting.

**Outcome.** On one machine, an agent calls a read tool on the test application through MCP or a command, and gets the same verified result every time, with a trace. No sign-in and no writes yet.

**Exit test.** A read operation on the test application gives five runs with the same trace signature; two clients share one engine; a locally built wheel installs and runs with `pip`; every budget in spec section 9 that this phase touches is measured and confirmed or revised by decision record.

**Read first.** The Phase 1 entry in the [phase map](phase-map.md). Run its research briefs before the group they affect.

## Task groups

| # | Group | Tasks | Done when |
| --- | --- | --- | --- |
| A | Core and plan | Finish the shared types in `w2m-core`. In `w2m-files`: the loader, and compiler stages 1 to 9 for read operations; the lint rules for determinism, locating, contracts, naming, and layering; the plan encoding | The `example` target's read files compile to a byte-identical plan twice; every lint rule has a passing and a failing test |
| B | Browser engine | From the Phase 0 decision: launch with an engine-owned data directory; locate by every locator kind; every wait condition; read values; produce snapshots | The conformance suite's launching, locating, waiting, and snapshot groups pass on three operating systems |
| C | Engine loop for reads | The step loop of spec section 8 for the `ui` driver's `read`, `list`, and `search` verbs; binding choice with its reason; every failure class a read can raise | Test-application cases 1, 4, 6, 8, 9, 11, and 12 pass as reads; an undeclared dialog stops with `unknown_variant` |
| D | Step trace | The step-trace record and its schema; the trace signature; capture levels `minimal` and `standard`; redaction of labelled fields | The determinism gate compares signatures; a restricted value never appears in a trace |
| E | Local engine | `w2m-local`: the socket and named pipe, the check of the connecting user, start on demand under a lock, the version check, idle stop, in-process mode; the on-disk layout | Two commands started at the same moment produce one engine; a different user is refused; `--in-process` works in CI |
| F | Action registry | `w2m-actions`: definitions, and generation of the command and MCP faces; the output contract and exit codes; the Phase 1 actions in the [action catalog](../reference/action-catalog.md) | The CI check fails when a face is written by hand or missing; non-terminal output is one JSON object |
| G | MCP | The stdio proxy to the local engine; the `use` toolset; structured results with output schemas | Two different MCP clients list the tools and call a read |
| H | Store | SQLite with the run, event, and blob tables for reads; migrations numbered and forward-only | A read leaves a run row, its events, and a trace; `run.get` and `run.trace` return them |
| I | Packaging check | Build a wheel that carries the binary and an empty Python package; install it in a clean environment | `pip install` of the local wheel puts `web2mcp` on the path on three operating systems |
| J | Measure | Benchmarks for every budget this phase touches | Numbers recorded; budgets confirmed or revised by decision record |

## Not in this phase

Sign-in, the vault, writes, the ledger's record rows, approvals, HTTP modes, authoring beyond `draft.lint` and `screen.snapshot`, and anything on a server. A target that needs sign-in waits for Phase 2.

## Questions for the owner

- Which two MCP clients to test with.
- Whether the measured numbers justify keeping the Phase 0 choice of browser engine base.
