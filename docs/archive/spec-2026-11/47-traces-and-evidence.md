# 47. Traces and evidence

> Archived on 2026-11-17. This is section 47 of the first design, kept as it was written, with the names and numbers of its time. What it says now lives in [section 217, Traces, evidence, and repair](../../spec/2-kernel/217-traces-evidence-and-repair.md).

Everything Anything2MCP claims about a run has to be checkable afterwards, and several parts of the system consume that record: the publish gates, the repair loop, shadow comparison, and people investigating a problem. This section defines what is captured, in what form, how secrets are kept out of it, and how it connects to outside monitoring. It extends [section 28](28-observability-audit-and-repair.md).

## Three layers

| Layer | What it is | Size | Where it lives |
| --- | --- | --- | --- |
| Run events | The ledger's append-only history: state changes, intents, verifications | Small | Database ([section 42](42-run-ledger-design.md)) |
| Step trace | One structured record per step: what was chosen, matched, sent, seen, and how long it took | About 2 KB per step | A blob, as JSON Lines |
| Artifacts | Snapshots, network exchanges, screenshots, desktop trees | Large | Blobs, referenced from the step trace |

## A step trace record

| Group | Fields |
| --- | --- |
| Identity | Run, attempt, step, record, operation, plan hash, engine and driver versions |
| Choice | Binding mode and the reason it was chosen |
| Timing | Queue, prepare, act, confirm, and read-back durations |
| Locating | Which locator matched, at which position in the list, and the fingerprint score |
| Acting | The widget-driver rung used; for HTTP, the method, path template, status, and sizes |
| Values | A hash of the intended value, the value read back, the normalized forms, and whether they matched |
| Outcome | Verified, or the failure class and code |
| Artifacts | References to snapshots and exchanges taken at this step |

Request and response bodies are not in the step trace. They are artifacts, captured only at the levels below.

## The trace signature

Two runs are "the same" when they did the same things, not when they took the same time. The trace signature is a hash over the parts of a step trace that should not vary: step order, bindings chosen, locators matched, normalized values, and outcomes. Timings, identifiers, and declared volatile values are left out.

- The **determinism gate** passes when five runs on the same input have the same signature.
- The **binding-equivalence gate** compares normalized results across bindings and, for an HTTP binding against a UI binding, the shape of the request each produced.
- **Shadow comparison** uses the same request-shape comparison.

## Capture levels

| Level | Captures | Used |
| --- | --- | --- |
| `minimal` | Run events and step trace | Reads in production |
| `standard` | Plus a snapshot at each checkpoint and at any failure | Writes in production; the default |
| `full` | Plus every network exchange and a screenshot per step | Building, testing, and a time-boxed window an administrator opens to investigate |

Screenshots are the riskiest evidence, because they show whatever is on the page. Outside `full` they are taken only at a failure.

## Keeping secrets out

Redaction happens on the runner, at capture, before anything is written or sent.

1. **Registered secrets.** Every secret the vault releases is registered with the redactor, which masks exact matches anywhere in text.
2. **Headers and cookies.** Only headers on an allowlist are kept. Cookie and authorization values are always dropped.
3. **Labelled fields.** Fields with a sensitivity label are masked, hashed, or left out, as policy says.
4. **Free text.** Length-limited and stripped of markup and hidden characters.
5. **Screenshots.** Password inputs and labelled fields are painted over using their positions on the page.
6. **A check on the way out.** Before evidence leaves the runner, it is scanned once more for registered secrets. A hit blocks the upload and raises an alert.

A test in CI plants a known secret in the test application's traffic and fails if it appears in any evidence.

## Formats and tools

- **Step trace:** JSON Lines, one record per step, with a published schema.
- **Network exchanges:** a subset of the HAR format, so existing viewers open them.
- **Snapshots:** the compact accessibility snapshot the authoring tools already produce.
- **Export:** `anything2mcp trace export` writes one archive with a manifest, for attaching to a bug report.
- **Viewing:** `anything2mcp trace show` in a terminal, and a page in the admin console.
- **For agents:** the trace tool returns a summary and resource links. A whole trace is never returned inline.

## Logs

The engine also writes ordinary logs for operating it: structured, one JSON object per line, with levels, rotated by size, and passed through the same redactor. Logs say what the engine did as a program. They are never the record of a run; that is the ledger and the trace. `anything2mcp engine logs` shows them.

## Connecting to outside monitoring

Export through OpenTelemetry is optional and off by default on a local machine.

- Each MCP call produces a span that follows OpenTelemetry's semantic conventions for MCP: the span is named for the method and the tool, and carries the method name and the tool name as attributes. Those conventions are still marked as in development, so the names may change ([research note 15](../../research/15-traces-and-key-stores.md)).
- Trace context sent by the calling agent in the request's metadata is honored, so the agent's trace and Anything2MCP's join into one.
- Steps appear as child spans with Anything2MCP's own attributes: mode, binding, and failure class.
- No value, snapshot, or secret is ever placed in a span. Spans carry names, timings, and outcomes only.
- Metrics: call duration per tool, plus success, mismatch, retry, and drift rates per tool version.

## What the repair loop receives

When a run or a canary reports `drift`, the repairing agent is given a fixed bundle:

- the failing step's trace record;
- the snapshot taken at the failure;
- the last snapshot of the same screen from a verified run or a fixture;
- the differences between the two;
- the screen model or endpoint file involved.

Restricted fields are masked in all of it. The agent proposes a file change; it never touches a live run.

## Fixtures from traces

`anything2mcp fixture record` turns captured evidence into a fixture for offline tests.

- Values are replaced with synthetic ones of the same field type, keeping structure and lengths.
- The fixture carries a marker stating that it was scrubbed, and a hash.
- The linter refuses a fixture without the marker.

## Overhead targets

`standard` capture adds no more than 5% to a run's time. A step trace record stays near 2 KB. Both are targets until measured.

## Open points

- The exact step-trace schema, fixed in Phase 1 with the first driver. Its unit is not settled: the glossary's step is one driver instruction, such as `locate` or `set`, yet the record above has one locator and the timings of a whole pass of the engine loop. Slice 1.2 chooses between a record per step and a record per pass, before the signature is defined over it.
- How well screenshot masking works on pages that move while being captured.
- Whether `full` capture should ever be allowed on a production write.
