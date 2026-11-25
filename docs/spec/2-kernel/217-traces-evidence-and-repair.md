# 217. Traces, evidence, and repair

Everything Anything2MCP claims about a run has to be checkable afterwards. Every run can be replayed from its record, every change can be traced to a person's approval, and a change in a system is caught before a write meets it. Several parts of the system consume that record: the publish gates, the repair loop, shadow comparison, the audit, and people investigating a problem. This section says what is captured, in what form, how secrets are kept out of it, how long it is kept, how it connects to outside monitoring, and how drift is repaired ([decision 0037](../../decisions/0037-evidence-events-a-step-trace-and-artifacts-redacted-at-capture.md)). Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

`a2m-logs` builds the step trace and its signature, applies the capture level, redacts at capture, and emits spans, metrics, and logs. It stores nothing durable. `a2m-ledger` seals and stores what must be kept. The engine joins the two: it asks `a2m-logs` for a finished piece of evidence and hands it to the store through `ControlPort` ([crate design](../../architecture/crates/a2m-logs.md)).

## Three layers

| Layer | What it is | Size | Where it lives |
| --- | --- | --- | --- |
| Run events | The ledger's append-only history: state changes, intents, verifications | Small | The store ([section 208](208-runs-the-ledger-and-resume.md)) |
| Step trace | One structured record per step: what was chosen, matched, sent, seen, and how long it took | About 2 KB per step | A blob of kind `trace`, as JSON Lines |
| Captures | Snapshots, network exchanges, screenshots, desktop window trees | Large | Blobs, referenced from the step trace |

## A step trace record

| Group | Fields |
| --- | --- |
| Identity | Run, attempt, step, step kind, record, operation, compiled target hash, engine and driver versions |
| Choice | Binding mode and the reason it was chosen |
| Timing | How long the step took; on the first record of a pass, how long the pass queued and prepared. The act, confirm, and read-back times of a pass are the sums of their steps |
| Locating | For a mode that locates elements, which locator matched, at which position in the list, and the fingerprint score |
| Acting | What the driver did: in the web pack, the widget-driver rung used; for HTTP, the method, path template, status, and sizes |
| Values | For each field: the intended value, the value read back, the normalized forms, and whether they matched, each held as its sensitivity label allows |
| Outcome | Verified, done, or the failure class and code |
| Captures | References to snapshots and exchanges taken at this step |

Request and response bodies are not in the step trace. They are captures, kept only at the levels below. The trace has a published schema, and becomes a public format once it is fixed in Phase 1.

## The trace signature

Two runs are "the same" when they did the same things, not when they took the same time. The trace signature is a hash over the parts of a step trace that should not vary: step order, step kinds, the bindings chosen, the locators matched and their positions, normalized values, and outcomes. Timings, identifiers, run and attempt numbers, the compiled target's hash, and the values the compiled target declares volatile are left out. Each record gives one SHA-256 digest over its parts, in a fixed order, with values in their canonical form; a value that is not public enters as its keyed digest. The signature chains those digests in step order and closes with their count, and the store records it on the run. Each record's digest is written in its line, so two traces that differ show where ([decision 0076](../../decisions/0076-one-trace-record-per-step-and-a-signature-chained-over-record-digests.md); the exact rule is in the [crate design](../../architecture/crates/a2m-logs.md)).

- The **determinism gate** passes when five runs on the same input have the same signature ([section 403](../4-building-tools/403-testing-and-publish-gates.md); hypothesis H-16).
- The **binding-equivalence gate** compares normalized results across bindings and, for an HTTP binding against a UI binding, the shape of the request each produced.
- **Shadow comparison** uses the same request-shape comparison.

## Capture levels

| Level | Captures | Used |
| --- | --- | --- |
| `minimal` | Run events and step trace | Reads in production |
| `standard` | Plus a snapshot at each checkpoint and at any failure | Writes in production; the default |
| `full` | Plus every network exchange and a screenshot per step | Building, testing, and a time-boxed window an administrator opens to investigate |

The compiled target and policy decide a run's level; `a2m-logs` applies it. Screenshots are the riskiest evidence, because they show whatever was on the screen. Outside `full` they are kept only at a failure.

**When capture fails.** A failure to capture evidence never fails a read. For a write the engine decides: under `standard`, a checkpoint snapshot that cannot be kept stops the run before the next record, because the evidence the level promises is missing. A trace that passes its size limit is cut, the cut is recorded as a `truncated` limitation, and the run is not stopped for it.

## Keeping secrets out

Redaction happens on the runner, at capture, before anything is written or sent. Every text field passes the redactor when it is recorded, so nothing unredacted is held even in memory longer than the call. The redactor is the vault's ([section 216](216-the-vault-and-its-keys.md)).

1. **Registered secrets.** Every secret the vault releases is registered with the redactor, which masks exact matches anywhere in text, and the same secret where it was percent-encoded, JSON-escaped, written with character references, written out in hexadecimal, or base64-encoded on the way, also across the line breaks that mail and key files put into base64. A secret whose form is case-insensitive is masked in any letter case. The redactor sees one text at a time: a text that arrives in pieces goes through the redactor's stream, which holds back the end of what it was given, and is never redacted piece by piece. What the redactor does not cover is listed in the [vault's design](../../architecture/crates/a2m-vault.md).
2. **Headers and cookies.** In an exchange, only headers on a fixed allowlist are kept. Cookie and authorization values are always dropped, whatever the level.
3. **Labelled fields.** A value with the sensitivity label `restricted` is kept only as a keyed digest, never as a value. One labelled `internal` is kept at `standard` and above, and as a keyed digest at `minimal`. A record's key is kept only as a keyed digest. Policy may also say a labelled field is masked or left out.
4. **Free text.** Length-limited and stripped of markup and hidden characters.
5. **Images are masked by the pack that took them.** Only the pack knows where the password inputs and labelled fields were on its screen. It paints over them, and sends with the image a report of what it masked. An image that arrives without that report is dropped, and a limitation is recorded. The kernel never paints over an image.
6. **A check on the way out.** Before a finished trace or a kept capture leaves the runner, it is scanned once more for registered secrets, with `contains_secret` on the `Redact` trait of `a2m-core`. A hit drops the piece, records a `redacted` limitation, and raises an alert. The piece never reaches the store.

A test in CI plants a known secret in the test application's traffic and fails if it appears in any record, capture, span, metric, or log line.

## Evidence in the store

- **Evidence is named by a keyed digest of its content.** A blob's name is the workspace's keyed digest of its plain content, never its plain hash, so someone who can read the store cannot confirm a guessed value by hashing it. Identical evidence within a workspace is still stored once. A digest that must be the same on every machine, such as a diff hash or a compiled target hash, is never a blob name.
- **Blobs are sealed before they are written,** with the blob's name, kind, and owner as the sealing context, and synced to disk before the row that names them commits. A blob whose row never committed is collected by retention.
- Event payloads, audit entries, and failure messages pass the redactor before they are written.

## Formats and tools

- **Step trace:** JSON Lines, one record per step, with a published schema whose version is stored with each trace blob.
- **Network exchanges:** a subset of the HAR format, so existing viewers open them.
- **Snapshots:** the compact accessibility snapshot the authoring tools already produce ([steps, snapshots, and diffs](../../reference/steps-snapshots-and-diffs.md)).
- **Export:** `anything2mcp trace export` writes one file with a manifest, for attaching to a bug report: `.anything2mcp/exports/<run>.json` in the project, a JSON document of the run's events, its step trace, and its text captures. An image, a capture that can no longer be opened, and a piece that still holds a registered secret at a last check are left out and named in the manifest, and the file is read back before its digest is given. It is the auditor's, as `trace show` is ([decision 0469](../../decisions/0469-a-runs-trace-is-summarized-for-its-person-and-exported-as-one-file-in-the-working-folder.md)).
- **Viewing:** `anything2mcp trace show` in a terminal, and a page in the admin console. `run.trace` gives the run's own person a summary of its trace: each step's kind, outcome, and time, and each kept capture by its reference, never a value (decision 0469).
- **For agents:** the trace tool returns a summary and resource links. A whole trace is never returned inline ([section 221](221-the-tool-surface-agents-see.md)).

## Retention

Traces are kept 30 days by default and audit entries 1 year. Both can be set to company policy. Retention classes are set per blob, and a legal hold keeps evidence past its class. Run events and audit entries are append-only: triggers in the store refuse an update or a delete, except deletes of a run whose retention state is `purging`, which only the retention job sets.

## The audit chain

The audit log is append-only and hash-chained, one chain per workspace, so a removed, changed, or reordered entry is detectable. Each entry holds who called and, where there is one, the agent that acted for them; which tool version; the input hash; who approved; the diff hash; and the outcome. A move of the engine to a lower health state is also an audit event ([section 224](224-when-the-engine-itself-fails.md)). `audit.query` reads the log; a chain that fails its check stops the store as corrupt.

## Logs

The engine also writes ordinary logs for operating it: structured, one JSON object per line, with levels, rotated by size, and passed through the same redactor, in `logs/` in the data directory. Logs say what the engine did as a program. They are never the record of a run; that is the ledger and the trace. `anything2mcp engine logs` shows them: the command reads the files where the engine runs, whether or not an engine runs, and starts nothing and makes nothing. At the default level the log carries one line for each start of the engine, `engine.started`, with its process, its version, and whether a command started it on demand, and one for each stop, `engine.stopped`, with why it stopped and, when it was asked, what asked: a signal by its name, or `engine.stop` with the process that sent it. No line carries a call's input ([decision 0266](../../decisions/0266-the-engines-log-says-each-start-and-stop-and-what-asked-and-engine-logs-reads-it-where-the-engine-runs.md)). If the log directory cannot be written, the engine keeps running and logs to standard error, redacted.

## Metrics and outside monitoring

Nothing leaves the machine unless a person switches it on ([decision 0040](../../decisions/0040-nothing-leaves-the-machine-unless-switched-on.md)). Export through OpenTelemetry is optional and off by default on a local machine. With export off, spans and metrics are kept in memory for `engine.status` and never sent.

- Each MCP call produces a span that follows OpenTelemetry's semantic conventions for MCP: the span is named for the method and the tool, and carries the method name and the tool name as attributes. Those conventions are still marked as in development, so the names may change; their spelling lives in one table in `a2m-logs` ([research note 15](../../research/15-traces-and-key-stores.md)).
- Trace context sent by the calling agent in the request's metadata is honored, so the agent's trace and Anything2MCP's join into one.
- Steps appear as child spans with Anything2MCP's own attributes: mode, binding, and failure class.
- **No value, snapshot, or secret is ever placed in a span or a metric.** Spans carry names, timings, and outcomes only. Their attributes come from a closed set with no constructor for an arbitrary string.
- Metrics, per tool version: call duration, and the success, mismatch, retry, and drift rates. A rising retry or drift rate is the early sign of a change in the system.
- When the exporter cannot reach its endpoint, spans are dropped after a bounded buffer. A run is never slowed or stopped for it.

## Drift and the repair loop

Because locators, endpoints, and every other piece of knowledge about a system each live in one place, one repair fixes every operation and pipeline that uses them. Anything2MCP never heals a locator or any other knowledge during a run, because a wrong match on a write path silently corrupts data. Unknown means stop.

When a canary or a run reports `drift`:

1. The affected tools are marked unavailable. Reads through other tools continue. What is affected is the knowledge the drift was seen in: the pack entry the failing step refers to, a screen in the web pack, held by the digest of the section that holds it, or the binding itself when no step names one. Every binding through the same mode on the same surface that relies on that knowledge is refused before it sends anything, a write among them, until a canary's pass through it vouches for it again or a version that changed it is published. A canary is never stopped by a mark; it writes nothing, and it is the check ([decision 0380](../../decisions/0380-a-drift-marks-the-knowledge-it-was-seen-in-and-stops-every-binding-that-relies-on-it.md)). A stopped tool stays in every list, marked: `tool.search` and `tool.describe` give it `available: false` and why (the reason `drift`, the knowledge the mark names, and when it was made), and the MCP door lists it with its schema, a fixed sentence of the product's own first in its description, and the same facts under `anything2mcp/unavailable` in its `_meta`. A tool is unavailable when every binding of its operation is stopped, and a pipeline when a step's operation is ([decision 0396](../../decisions/0396-a-tool-stopped-by-a-drift-is-listed-and-marked-never-left-out.md)). Built today: the marks, the refusal, the lift, and the marked listing. A canary is a watch's check, or a canary of its own: one published read with a fixed input, asked for by an agent or a person and sent on its schedule only after the person it acts for confirms it on the engine's page, of the version published when it was asked, at a watch's limits and within the share of its surface watches take; it goes on after a drift, so that a pass lifts the mark, and it ends when another version is published ([decision 0482](../../decisions/0482-a-canary-runs-on-a-schedule-its-person-confirmed-and-sends-only-a-published-read.md)).
2. The repairing agent, or the person, is given a fixed bundle, `repair.propose`, built on asking from what is kept and kept nowhere ([decision 0492](../../decisions/0492-a-drift-yields-a-repair-bundle-and-a-person-accepts-a-repair-a-pack-proposed-by-rule.md)):
   - the drift: the tool, mode, surface, the pack's code, and the run that saw it, whose trace record `run.trace` gives;
   - the tools it stops;
   - two shapes of the screen or surface, from findings of a discovery: one from before the change, while the knowledge still fit, and one from now; a shape keeps the page's own names of its headings, controls, and sections, and every value as a type. Neither is the snapshot taken at the failure, which a driver does not keep yet, nor one kept from a verified run;
   - the file involved: in the web pack a screen model; in another pack, the file of its kind that holds the knowledge that drifted, found by the file kind of the section the mark names;
   - where the pack has one, the change its deterministic repair proposer proposes to that file, as a diff with the digest that names it, or why it proposes none. The web pack's first proposer, registered by the probe for its `screen` findings, is a rule of exact pairing rather than decision 0323's weighted score: it pairs the two shapes by role and name in each section, and proposes a locator's new name only where exactly one element of its role is gone from its section and exactly one is new, and a fingerprint's new section only where the text it names left its section for one around the field; anything else proposes nothing, and says why. A proposal applies nothing.
3. The agent proposes a file change: a new locator, a fingerprint update, a schema update, a new rule; or the person accepts the proposer's change, `repair.accept`, a person's act on the engine's page that an agent can only ask for, which writes it into the drafts exactly as the bundle showed it. Neither touches a live run or a published version. Repair is building work, done at authoring time, never at run time.
4. The change runs through the publish gates against the test environment and the fixtures ([section 403](../4-building-tools/403-testing-and-publish-gates.md)).
5. A person approves the diff, and it is published as a new version.

A repair never applies at run time, whatever its score. A false repair, a locator that matches the wrong element and passes, is the failure this loop exists to prevent: open tools that heal by similarity document exactly that failure ([research note 37](../../research/37-locator-drift-and-repair.md)). What holds it here is the fingerprint, the rule that a locator matches exactly one element, and a person's approval of the diff.

**Drifting, before drift.** A canary reports `drifting`, not `drift`, when a field or action still passes but the locator that matched it is not the first in its list. A `drifting` tool stays available; it is shown in the drift metrics and in the engine's health, and it does not stop a run. A second sign, a fingerprint score below the one it had at the last verified run, needs the matched element's reference from that run, which the screen model does not yet keep (decision 0323).

## Fixtures from traces

`fixture.record` records a fixture for offline tests from one real read, run in the sandbox for that purpose: the evidence a trace keeps holds shapes and digests, not values ([decision 0196](../../decisions/0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md)). Writes are not recorded.

- Values pass the redactor, then are replaced with synthetic ones of the same field type by fixed rules, chosen without looking at the original except for what the replay needs; names are kept only where the operation or the target's files declare them, and every other key is replaced; a value whose type has no rule stops the recording, and nothing is written ([decision 0200](../../decisions/0200-evidence-names-its-binding-and-a-recorded-fixture-says-nothing-of-the-original.md)).
- The fixture carries a marker stating that it was scrubbed, and a hash.
- The linter refuses a fixture without the marker.

## Overhead targets

`standard` capture adds no more than 5% to a run's time. A step trace record stays near 2 KB. Both are targets until measured in the benchmarks in `bench/` ([section 225](225-performance.md)).

## Open points

- A signature compares within one workspace whenever a run has a record key or a value that is not public, because both enter as keyed digests. Whether the test sandbox runs under a fixed, public key, so that a test's expected signature is the same on every machine, is for the slice that builds the test runner.
- Which of a driver's details must be the same in two runs, and so enter the signature, is each pack's choice; the HTTP pack makes it first.
- How well a pack's masking of a screenshot works on screens that move while being captured.
- Whether `full` capture should ever be allowed on a production write.
