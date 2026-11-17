# `a2m-telemetry`

Emitting what the engine did: the step trace and its signature, the capture of artifacts at the level a run asks for, spans and metrics with their optional OpenTelemetry export, and the engine's own logs. Everything it emits has passed a `Redact` first. It stores nothing durable: what must be kept, it hands back as redacted bytes for `a2m-ledger` to store.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 2, Foundations |
| May depend on | `a2m-core` |
| Depends on | `a2m-core`. Outside, each to be verified and recorded: a span and logging framework; an OpenTelemetry exporter, behind a cargo feature; `serde` and a JSON writer; a SHA-256 crate, shared with `a2m-core`'s `Digest` |
| Built in | Roadmap package 1.2, the step trace and its signature; package 2.1, redaction at capture and the check on the way out; the OpenTelemetry export with the server profile, Phase 6, or earlier if a slice needs it |
| Carries out | Spec sections [47](../../spec/47-traces-and-evidence.md) and [28](../../spec/28-observability-audit-and-repair.md); [decision 0037](../../decisions/0037-evidence-events-a-step-trace-and-artifacts-redacted-at-capture.md); the determinism gate's input in [section 27](../../spec/27-testing-and-publish-gates.md) |

## Owns

- The step-trace record: its fields, its schema, and the JSON Lines form a trace blob takes.
- The trace signature: which parts of a record enter it, in what order, and how they are hashed.
- Capture: given a capture level, which artifacts are kept, and in what form, before anything is written or sent.
- Redaction at capture, through the `Redact` it is given, and the check on the way out.
- Spans and metrics, with a closed set of attributes that cannot carry a value.
- The engine's logs: structured, levelled, rotated by size, redacted.
- OpenTelemetry export, off unless a person switches it on, and the incoming trace context a caller sends.

It must never: store evidence, own a table, or write a blob (`a2m-ledger` does); decide a capture level (the plan and policy do; it applies one); put a value, a snapshot, or a secret in a span or a metric; send anything off the machine unless export is switched on; paint over an image (the pack that took it does, because only it knows where the fields are).

## The split with `a2m-ledger`

| Concern | `a2m-telemetry` | `a2m-ledger` |
| --- | --- | --- |
| Run events | Nothing. They are the ledger's history | Writes them |
| Step trace | Builds each record, redacts it, computes the signature, and returns a finished trace as bytes | Seals and stores the bytes as a blob of kind `trace`, and records the signature on the run |
| Artifacts | Decides from the level whether one is kept; redacts it; returns it as bytes with its kind and limitations | Seals and stores it as a blob; keeps the reference |
| Logs | Writes them to the log files in the data directory | Nothing |
| Spans and metrics | Emits and exports them | Nothing |

The engine is the caller that joins the two: it asks this crate for a finished piece of evidence and gives it to the ledger. An `EvidenceSink` trait here, implemented by the ledger, was weighed and refused for now: it would add a join to the spine for a call the engine can make itself, and it would let telemetry decide when evidence is written. If streaming a long trace to the store proves necessary, the trait is the answer, with a decision record.

## Interface

```rust
/// Made once per run attempt by the engine. Holds the redactor, the level, and the records so far.
pub struct TraceBuilder { /* ... */ }
impl TraceBuilder {
    pub fn new(identity: TraceIdentity, level: CaptureLevel, redact: Arc<dyn Redact>) -> Self;

    /// One record per step, as section 47 defines the groups. Redacted on entry.
    pub fn record(&mut self, step: StepRecord);

    /// Offers an artifact. Returns `Kept` with redacted bytes for the ledger, or `Dropped` with a
    /// reason (the level does not keep it; an image arrived unmasked; it failed the check on the way out).
    pub fn artifact(&mut self, offered: OfferedArtifact) -> ArtifactOutcome;

    /// Ends the trace: the JSON Lines bytes, already checked, and the signature.
    pub fn finish(self) -> Result<FinishedTrace, Failure>;
}

pub struct TraceIdentity { pub run: RunId, pub attempt: u32, pub operation: OperationName, pub plan: Digest, pub engine: Version }
pub enum CaptureLevel { Minimal, Standard, Full }      // names as section 47

pub struct StepRecord {
    pub index: u32, pub kind: StepKindName, pub record: Option<RecordKey>,
    pub choice: Option<BindingChoice>,                   // mode and reason, on the first record of a pass
    pub timing: Timing,                                  // excluded from the signature
    pub locating: Option<Locating>,                      // which locator, its position, the fingerprint score
    pub acting: Option<Acting>,                          // rung used; or method, path template, status, sizes
    pub values: Vec<TracedValue>,
    pub outcome: StepOutcome,                            // verified, done, or class and code
    pub artifacts: Vec<ArtifactRef>,
}
pub struct TracedValue { pub field: String, pub intended: Option<Value>, pub read_back: Option<Value>,
                         pub normalized: Option<Value>, pub matched: Option<bool>, pub sensitivity: Sensitivity }

pub struct OfferedArtifact { pub kind: ArtifactKind, pub at: CapturePoint, pub bytes: Vec<u8>, pub masked: Option<MaskReport> }
pub enum ArtifactKind { Snapshot, Exchange, Screenshot, WindowTree }
pub enum CapturePoint { Step, Checkpoint, Failure }

pub struct FinishedTrace { pub bytes: Vec<u8>, pub signature: Digest, pub records: u32, pub limitations: Vec<Limitation> }

/// Spans. Attributes come only from this closed set: none takes free text.
pub enum Attr { Mode(Mode), Operation(OperationName), Target(TargetName), Class(FailureClass),
                Code(FailureCode), Count(u64), Effect(EffectClass), Level(CaptureLevel) }
pub fn call_span(method: McpMethod, tool: Option<&OperationName>, parent: Option<&TraceContext>) -> SpanGuard;
pub fn step_span(kind: &StepKindName, attrs: &[Attr]) -> SpanGuard;

/// Metrics, per tool version: duration, success, mismatch, retry, and drift counts.
pub fn record_call(tool: &OperationName, plan: &Digest, outcome: CallOutcome, took: Duration);

/// Incoming context from a request's metadata. Parsed and checked; anything else is ignored.
pub struct TraceContext { /* trace id and parent span id, in the standard text form */ }
impl TraceContext { pub fn parse(traceparent: &str) -> Option<TraceContext>; }

/// Started once by the role. Installs the log writer and, if switched on, the exporter.
pub fn start(settings: &TelemetrySettings, redact: Arc<dyn Redact>, logs: &Path) -> Result<Telemetry, Failure>;
```

Contracts that the signatures do not show:

- **Redaction happens on entry, not on write.** `record` and `artifact` pass every text field through the `Redact` before keeping it, so nothing unredacted is held even in memory longer than the call. The redactor is the vault's, given at start; this crate never makes its own.
- **The check on the way out runs once more.** `finish` and each kept artifact are scanned before they are returned. The scan needs a cheaper question than `redact`; until `a2m-core`'s `Redact` offers one, it is `redact(text) != text`. A hit drops the piece, records a `redacted` limitation, and raises an alert through the logs: the piece is never handed to the ledger.
- **Headers and cookies.** In an exchange, only headers on a fixed allowlist are kept; cookie and authorization values are always dropped, whatever the level.
- **Labelled fields.** A `TracedValue` whose sensitivity is `restricted` is kept as a digest, never as a value; one that is `internal` is kept at `standard` and above; free text is length-limited and stripped of markup and hidden characters.
- **Images arrive masked.** A screenshot is kept only with a `MaskReport` from the pack that took it, saying which fields were painted over. Without one it is dropped and a limitation recorded. Outside `full`, a screenshot is kept only at a failure.
- **The signature covers what should not vary.** Step order, step kinds, the binding chosen, the locator matched and its position, normalized values, and outcomes. It leaves out timings, identifiers, run and attempt numbers, and fields the plan declares volatile. Records enter in step order; within a record, fields in a fixed order; values in a canonical text form. The signature is SHA-256 over those bytes.
- **Spans carry names, timings, and outcomes only.** The `Attr` type is the enforcement: there is no constructor for an arbitrary string attribute. MCP spans follow the OpenTelemetry conventions for MCP, whose names are still in development, so their spelling lives in one table here.
- **Nothing leaves unless switched on.** With export off, spans and metrics are kept in memory for `engine.status` and never sent. Export is a setting a person changes.
- **Logs are not the record of a run.** They say what the engine did as a program. A run is the ledger and the trace.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-engine` | `TraceBuilder` for every run attempt; `step_span`; `record_call`; hands `FinishedTrace` and kept artifacts to `a2m-ledger` |
| `a2m-ledger` | Nothing at run time; the trace's schema version, to store with each trace blob |
| Packs | Nothing directly. A driver returns artifacts and a `MaskReport` in its outcome; the engine offers them |
| The front doors | `call_span` with the caller's `TraceContext`; the redactor is the vault's, not this crate's |
| `a2m-actions` | `start` at `Kernel::open`; the trace schema for `trace.show` and `trace.export`; logs for `engine.logs` |
| Every crate above it | Logging, through the framework this crate installs |

It calls only `a2m-core`, and the `Redact` it was given.

## State

| State | Where | Notes |
| --- | --- | --- |
| A trace being built | Memory, per run attempt | Dropped after `finish` returns |
| Logs | `logs/` in the data directory | Rotated by size; redacted on write |
| Spans and metrics | Memory, and the exporter's buffer when export is on | Never written to disk by this crate |

## Failures

| Class | Code | When |
| --- | --- | --- |
| `busy` | `storage_full` | The log directory cannot be written. The engine keeps running; logs go to standard error, redacted |
| `input` | `trace_too_large` | A run's trace passes its size limit. The trace is cut, the cut recorded as a `truncated` limitation, and the run is not stopped for it |
| `transient` | `export_unavailable` | The exporter cannot reach its endpoint. Spans are dropped after a bounded buffer; a run is never slowed or stopped for it |

A failure to capture evidence never fails a read. For a write, the engine decides: under `standard`, a checkpoint snapshot that cannot be kept stops the run before the next record, because the evidence the level promises is missing.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Five runs of the same read on the same input have one signature | The proof of package 1.2, on three operating systems (hypothesis H-16) |
| A planted secret appears in no record, artifact, span, metric, or log line | The planted-secret test of package 2.1 |
| No span or metric can carry a value | The `Attr` type; a test that exports spans from a run with planted values and searches them |
| A restricted value never appears in a trace | A test with a restricted field and a known value |
| With export off, nothing is sent | A test that runs with a listening socket on the default exporter address and fails on any connection |
| `standard` capture adds no more than 5% to a run, and a record stays near 2 KB | Benchmarks in `bench/`, slice 1.7; targets until measured |

## Build order

1. `StepRecord`, `TraceBuilder` with `minimal`, `finish`, and the signature, with a pass-through `Redact`. Package 1.2.
2. The JSON Lines schema, published; `standard` with snapshots at checkpoints and failures. Package 1.2.
3. Logs, with rotation. Package 1.4, when the local engine first runs for long.
4. Redaction at capture with the vault's redactor; the check on the way out; headers, cookies, labelled fields. Package 2.1.
5. Spans, metrics, and the `full` level. With the first need for them; export with the server profile.

## Open points

- **The unit of a record.** Section 47 leaves open whether a record is per step or per pass of the engine loop. The proposal: one record per step, as the glossary says, with the binding choice on the first record of each pass and timings per step. Package 1.2 decides before the signature is defined over it.
- How a restricted value enters the signature. A plain digest of a short value can be reversed by guessing; a keyed digest differs between installations. The proposal is a keyed digest, and the determinism gate compares within one installation.
- Whether the check on the way out is a method of `Redact` in `a2m-core`. Proposed, as `contains_secret`.
- The allowlist of headers, written once and shared by every pack that records exchanges.
- Whether `full` capture is ever allowed on a production write.
