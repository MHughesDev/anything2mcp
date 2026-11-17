# Spike: what the MCP server stands on

Roadmap package 0.5, with research brief 2. Throwaway code. Nothing here is promoted into the product; `a2m-mcp` is written fresh.

**The question.** Does the product's MCP server stand on the official Rust SDK crate or on a small JSON-RPC layer of our own, and which protocol revisions does it serve?

**The answer** is in [decision 0071](../../docs/decisions/0071-the-mcp-server-stands-on-the-official-sdk-behind-a-seam-and-serves-two-revisions.md). The evidence is in [research note 18](../../docs/research/18-mcp-server-base.md).

## What is here

| Path | What it is |
| --- | --- |
| `tools.json` | The one tool, as data: name, description, input schema, output schema, annotations, and two records of the imaginary `example` target. Both servers read it at start |
| `on-sdk/` | The server on the official Rust SDK, with no macros. Feature `http` adds a stateless Streamable HTTP mode |
| `by-hand/` | The same server by hand: JSON-RPC 2.0 over stdio with `serde_json` and the standard library |
| `callers/raw_conformance.py` | A hand-written caller with no client library: 50 message-level checks |
| `callers/py_caller.py` | A caller on the reference client library for Python |
| `callers/ts_caller.mjs` | A caller on the reference client library for TypeScript, in its two release lines |
| `callers/tee.py` | Sits between a client and a server and writes down every line each sends |
| `callers/http_probe.sh` | Raw HTTP requests against the SDK variant's HTTP mode |
| `callers/summarize.py` | Turns the reports into `results/summary.md` |
| `results/` | `summary.md`, the two conformance listings, the HTTP probe, and three sample wire logs |

Both servers take `<tools.json> [revisions]`. The revisions are a comma-separated list; the default is `2025-11-25,2026-07-28`.

## How to run it

```
cd spikes/mcp-server
python3 -m venv clients/py/venv && clients/py/venv/bin/pip install mcp
(mkdir -p clients/ts && cd clients/ts && npm init -y && npm install @modelcontextprotocol/sdk)
(mkdir -p clients/ts2 && cd clients/ts2 && npm init -y && npm install @modelcontextprotocol/client)
./run_all.sh
```

The client libraries are installed under `clients/`, which is not kept. The build output goes to `target/` here, never to the product's.

The versions this was run with: the SDK crate 3.10.0; the Python package `mcp` 2.3.0; the npm packages `@modelcontextprotocol/sdk` 1.32.2 and `@modelcontextprotocol/client` 2.4.0; Rust 1.100.0; Python 3.13; Node 22.

## What the spike does not do

- The tool's extra input `wait_ms` exists only so that progress and cancellation can be seen. It sleeps. The product never waits on a fixed time.
- Both servers declare `tools.listChanged` and never send the notification. It was not exercised.
- The hand-written server has no HTTP mode. Only the SDK variant was run over HTTP.
- No resources, prompts, elicitation, round trips for input, tasks, or authorization.
- Run on Linux only. Both variants were type-checked for Windows and macOS targets; nothing was run there.
